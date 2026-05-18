//! Cloud sync of server configs. Push + pull only — no local DB of
//! remote-only servers yet, the UI just surfaces them as a list.
//!
//! Privacy contract: we send the cloud (a) the server `id` and `name`
//! (plain, used for UI listing) and (b) an AES-256-GCM-sealed blob of
//! the rest of the config. The blob key never leaves the user's device.
//!
//! Triggered manually right now ("Sync now" button); Tier 2 wires the
//! WS relay to auto-pull on `sync_changed`.

use serde::{Deserialize, Serialize};

use super::{api, auth, vault};
use crate::backend::NodeRegistry;
use crate::backend::registry::RemoteNodeForSync;
use localforge_backend_remote::RemoteAgentConfig;
use localforge_core::types::{GameType, Server};
use localforge_core::NodeId;

/// What we actually serialise + encrypt per server. NOT the full Server
/// — we drop container_id, data_path, install state, all of which are
/// machine-specific. The user's container ID on laptop != desktop.
///
/// `node_id` tracks WHICH node this server lives on. For sub-users
/// this is the routing key — when Bob clicks Start, the relay cmd
/// includes node_id so Alice's RelayCommandExecutor knows whether to
/// hit local Docker or her VPS agent. Defaults to "local" on
/// deserialise for backwards-compat with v0.1.12 blobs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudServerConfig {
    pub id: String,
    pub name: String,
    pub game_type: GameType,
    pub port: u16,
    pub memory_mb: u32,
    pub config: std::collections::HashMap<String, String>,
    #[serde(default = "default_node_id")]
    pub node_id: String,
}

fn default_node_id() -> String {
    "local".to_string()
}

impl CloudServerConfig {
    pub fn from_server(s: &Server, node_id: String) -> Self {
        Self {
            id: s.id.clone(),
            name: s.name.clone(),
            game_type: s.game_type.clone(),
            port: s.port,
            memory_mb: s.memory_mb,
            config: s.config.clone(),
            node_id,
        }
    }
}

#[derive(Debug, Serialize)]
struct SyncedServer<'a> {
    id: &'a str,
    name: &'a str,
    #[serde(rename = "encryptedBlob")]
    encrypted_blob: String,
    #[serde(rename = "updatedAt")]
    updated_at: i64,
}

#[derive(Debug, Serialize)]
struct PutBody<'a> {
    servers: Vec<SyncedServer<'a>>,
}

// PullEntry / PullResponse are inlined in `pull()` where we declare the
// camelCase rename via field renames — keeping them inline lets us
// avoid a duplicate type that drifts.

/// One row of pull result handed to the UI. `decrypted` is None when
/// the blob couldn't be decrypted (different vault key on this device,
/// or v2 format we don't recognise yet).
#[derive(Debug, Serialize)]
pub struct RemoteServer {
    pub id: String,
    pub name: String,
    /// Unix ms.
    pub updated_at: i64,
    pub decrypted: Option<CloudServerConfig>,
    /// Whether a server with this id exists locally.
    pub exists_locally: bool,
    /// Set when decryption failed — surfaces "different vault key on
    /// this device" etc.
    pub decrypt_error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SyncResult {
    pub pushed: usize,
    pub conflicts: Vec<String>,
    pub remote: Vec<RemoteServer>,
}

// ---------------------------------------------------------------------------
// Push
// ---------------------------------------------------------------------------

async fn list_local_servers(
    state: &tauri::State<'_, NodeRegistry>,
) -> Result<Vec<Server>, String> {
    // Only the LOCAL node syncs — remote nodes have their own desktop
    // somewhere else. Plus, "the local Docker backend" is the canonical
    // ground truth for the user's own configs.
    let Some(backend) = state.backend(&NodeId::local()).await else {
        return Ok(vec![]);
    };
    backend.list_servers().await.map_err(|e| e.to_string())
}

async fn push(
    token: &str,
    key: &[u8; 32],
    local: &[Server],
) -> Result<(usize, Vec<String>), api::ApiError> {
    if local.is_empty() {
        return Ok((0, vec![]));
    }
    let now = chrono::Utc::now().timestamp_millis();
    let mut payload: Vec<(String, String, String)> = Vec::with_capacity(local.len());
    for s in local {
        // For now we only sync servers from the LOCAL node — multi-node
        // sync (iterating all NodeRegistry entries) is a follow-up.
        // Tag everything as "local" so the receiving side knows to
        // route commands at the owner's local Docker.
        let cfg = CloudServerConfig::from_server(s, "local".to_string());
        let plaintext = serde_json::to_vec(&cfg)
            .map_err(|e| api::ApiError::Decode(format!("serialize: {e}")))?;
        let envelope = vault::encrypt(key, &plaintext)
            .map_err(|e| api::ApiError::Decode(format!("encrypt: {e}")))?;
        payload.push((s.id.clone(), s.name.clone(), envelope));
    }
    let body = PutBody {
        servers: payload
            .iter()
            .map(|(id, name, env)| SyncedServer {
                id,
                name,
                encrypted_blob: env.clone(),
                updated_at: now,
            })
            .collect(),
    };

    // The cloud accepts the batch under /v1/sync/servers (PUT). We
    // don't use If-Match yet — the desktop is the only writer for now.
    // When sub-users start editing too (Phase 4.5) we'll wire it.
    #[derive(Deserialize)]
    struct Resp {
        ok: bool,
        count: usize,
    }
    let res: Result<Resp, api::ApiError> = put_servers(token, &body).await;
    match res {
        Ok(r) if r.ok => Ok((r.count, vec![])),
        Ok(_) => Ok((0, vec![])),
        Err(api::ApiError::Server { status: 409, .. }) => {
            // For now the only writer is this device, so 409 means the
            // other side already had this state — record + continue.
            Ok((0, local.iter().map(|s| s.id.clone()).collect()))
        }
        Err(e) => Err(e),
    }
}

/// PUT wrapper. Custom because api::post is JSON but our shape is fine
/// with serde so we just lean on the same helper indirectly through
/// reqwest's `.json()`.
async fn put_servers<R: serde::de::DeserializeOwned>(
    token: &str,
    body: &PutBody<'_>,
) -> Result<R, api::ApiError> {
    let url = format!("{}/v1/sync/servers", super::api_origin());
    let res = api::client()
        .put(&url)
        .bearer_auth(token)
        .json(body)
        .send()
        .await
        .map_err(api::ApiError::Network)?;
    let status = res.status();
    if status.is_success() {
        res.json::<R>()
            .await
            .map_err(|e| api::ApiError::Decode(e.to_string()))
    } else {
        let code_num = status.as_u16();
        let body = res.json::<api::ApiErrorBody>().await.ok();
        Err(api::ApiError::Server {
            status: code_num,
            code: body
                .as_ref()
                .map(|b| b.error.clone())
                .unwrap_or_else(|| format!("http_{}", code_num)),
            message: body.and_then(|b| b.message),
        })
    }
}

// ---------------------------------------------------------------------------
// Pull
// ---------------------------------------------------------------------------
async fn pull(
    token: &str,
    key: &[u8; 32],
    local_ids: &std::collections::HashSet<String>,
) -> Result<Vec<RemoteServer>, api::ApiError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct PullEntryRaw {
        id: String,
        name: String,
        encrypted_blob: String,
        updated_at: i64,
    }
    #[derive(Deserialize)]
    struct PullResp {
        servers: Vec<PullEntryRaw>,
    }
    let resp: PullResp = api::get("/v1/sync/servers", Some(token)).await?;
    Ok(resp
        .servers
        .into_iter()
        .map(|e| match vault::decrypt(key, &e.encrypted_blob) {
            Ok(plain) => match serde_json::from_slice::<CloudServerConfig>(&plain) {
                Ok(cfg) => RemoteServer {
                    id: e.id.clone(),
                    name: e.name,
                    updated_at: e.updated_at,
                    decrypted: Some(cfg),
                    exists_locally: local_ids.contains(&e.id),
                    decrypt_error: None,
                },
                Err(parse_err) => RemoteServer {
                    id: e.id.clone(),
                    name: e.name,
                    updated_at: e.updated_at,
                    decrypted: None,
                    exists_locally: local_ids.contains(&e.id),
                    decrypt_error: Some(format!("parse: {parse_err}")),
                },
            },
            Err(decrypt_err) => RemoteServer {
                id: e.id.clone(),
                name: e.name,
                updated_at: e.updated_at,
                decrypted: None,
                exists_locally: local_ids.contains(&e.id),
                decrypt_error: Some(decrypt_err),
            },
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Public command — "Sync now"
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn cloud_sync_now(
    state: tauri::State<'_, NodeRegistry>,
) -> Result<SyncResult, api::ApiError> {
    let token = auth::current_token().ok_or_else(|| api::ApiError::Server {
        status: 401,
        code: "unauthenticated".into(),
        message: None,
    })?;
    let key = vault::ensure_key().map_err(|e| api::ApiError::Decode(format!("vault: {e}")))?;

    let local = list_local_servers(&state)
        .await
        .map_err(|e| api::ApiError::Decode(format!("local list: {e}")))?;
    let local_ids: std::collections::HashSet<String> =
        local.iter().map(|s| s.id.clone()).collect();

    let (pushed, conflicts) = push(&token, &key, &local).await?;
    let remote = pull(&token, &key, &local_ids).await?;

    Ok(SyncResult {
        pushed,
        conflicts,
        remote,
    })
}

/// Light pull-only used by the relay listener when it gets a
/// sync_changed event. Doesn't push.
#[tauri::command]
pub async fn cloud_sync_pull(
    state: tauri::State<'_, NodeRegistry>,
) -> Result<Vec<RemoteServer>, api::ApiError> {
    let token = auth::current_token().ok_or_else(|| api::ApiError::Server {
        status: 401,
        code: "unauthenticated".into(),
        message: None,
    })?;
    let key = vault::ensure_key().map_err(|e| api::ApiError::Decode(format!("vault: {e}")))?;
    let local = list_local_servers(&state)
        .await
        .map_err(|e| api::ApiError::Decode(format!("local list: {e}")))?;
    let local_ids: std::collections::HashSet<String> =
        local.iter().map(|s| s.id.clone()).collect();
    pull(&token, &key, &local_ids).await
}

// ===========================================================================
// Node sync — owner-only.
//
// `cloud_sync_nodes_now` pushes every remote node (id, label, url,
// fingerprint, token — all encrypted with the same vault key) up to
// /v1/sync/nodes, then pulls back the cloud list and (if any of them
// don't exist locally) reconstitutes them in NodeRegistry so the user's
// other devices recover the same setup with zero manual re-adding.
//
// The owner-side cloud route 403s sub-users, so this command is safe
// to call from any signed-in client — sub-users just get a no-op.
// ===========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudNodeConfig {
    pub url: String,
    pub fingerprint: Option<String>,
    pub token: String,
}

#[derive(Debug, Serialize)]
struct SyncedNode<'a> {
    id: &'a str,
    label: &'a str,
    #[serde(rename = "encryptedBlob")]
    encrypted_blob: String,
    #[serde(rename = "updatedAt")]
    updated_at: i64,
}

#[derive(Debug, Serialize)]
struct NodesPutBody<'a> {
    nodes: Vec<SyncedNode<'a>>,
}

#[derive(Debug, Serialize)]
pub struct RemoteNodeSummary {
    pub id: String,
    pub label: String,
    pub url: Option<String>,
    pub updated_at: i64,
    pub exists_locally: bool,
    pub imported: bool,
    pub decrypt_error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NodeSyncResult {
    pub pushed: usize,
    pub remote: Vec<RemoteNodeSummary>,
}

#[tauri::command]
pub async fn cloud_sync_nodes_now(
    state: tauri::State<'_, NodeRegistry>,
) -> Result<NodeSyncResult, api::ApiError> {
    let token = auth::current_token().ok_or_else(|| api::ApiError::Server {
        status: 401,
        code: "unauthenticated".into(),
        message: None,
    })?;
    let key = vault::ensure_key().map_err(|e| api::ApiError::Decode(format!("vault: {e}")))?;

    let local: Vec<RemoteNodeForSync> = state
        .list_remote_for_sync()
        .map_err(|e| api::ApiError::Decode(format!("list nodes: {e}")))?;

    let pushed = push_nodes(&token, &key, &local).await?;
    let remote = pull_nodes(&token, &key, &state, &local).await?;

    Ok(NodeSyncResult { pushed, remote })
}

async fn push_nodes(
    token: &str,
    key: &[u8; 32],
    local: &[RemoteNodeForSync],
) -> Result<usize, api::ApiError> {
    if local.is_empty() {
        return Ok(0);
    }
    let now = chrono::Utc::now().timestamp_millis();
    let mut payload: Vec<(String, String, String)> = Vec::with_capacity(local.len());
    for n in local {
        let cfg = CloudNodeConfig {
            url: n.url.clone(),
            fingerprint: n.fingerprint.clone(),
            token: n.token.clone(),
        };
        let plaintext = serde_json::to_vec(&cfg)
            .map_err(|e| api::ApiError::Decode(format!("serialize node: {e}")))?;
        let envelope = vault::encrypt(key, &plaintext)
            .map_err(|e| api::ApiError::Decode(format!("encrypt node: {e}")))?;
        payload.push((n.id.clone(), n.label.clone(), envelope));
    }

    let body = NodesPutBody {
        nodes: payload
            .iter()
            .map(|(id, label, env)| SyncedNode {
                id,
                label,
                encrypted_blob: env.clone(),
                updated_at: now,
            })
            .collect(),
    };

    #[derive(Deserialize)]
    struct Resp { ok: bool, count: usize }
    let url = format!("{}/v1/sync/nodes", super::api_origin());
    let res = api::client()
        .put(&url)
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .map_err(api::ApiError::Network)?;
    if !res.status().is_success() {
        // 403 from sub-users is expected; surface gently as "0 pushed".
        if res.status().as_u16() == 403 {
            return Ok(0);
        }
        let status = res.status().as_u16();
        let body = res.json::<api::ApiErrorBody>().await.ok();
        return Err(api::ApiError::Server {
            status,
            code: body
                .as_ref()
                .map(|b| b.error.clone())
                .unwrap_or_else(|| format!("http_{}", status)),
            message: body.and_then(|b| b.message),
        });
    }
    let r: Resp = res.json().await.map_err(|e| api::ApiError::Decode(e.to_string()))?;
    Ok(if r.ok { r.count } else { 0 })
}

async fn pull_nodes(
    token: &str,
    key: &[u8; 32],
    state: &tauri::State<'_, NodeRegistry>,
    local: &[RemoteNodeForSync],
) -> Result<Vec<RemoteNodeSummary>, api::ApiError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct PullEntryRaw {
        id: String,
        label: String,
        encrypted_blob: String,
        updated_at: i64,
    }
    #[derive(Deserialize)]
    struct PullResp { nodes: Vec<PullEntryRaw> }

    let resp: Result<PullResp, api::ApiError> = api::get("/v1/sync/nodes", Some(token)).await;
    let entries = match resp {
        Ok(r) => r.nodes,
        // Sub-users get 403 here — that's fine, they shouldn't receive nodes.
        Err(api::ApiError::Server { status: 403, .. }) => return Ok(vec![]),
        Err(e) => return Err(e),
    };

    let local_ids: std::collections::HashSet<String> =
        local.iter().map(|n| n.id.clone()).collect();

    let mut out = Vec::with_capacity(entries.len());
    for e in entries {
        let exists = local_ids.contains(&e.id);
        let mut summary = RemoteNodeSummary {
            id: e.id.clone(),
            label: e.label.clone(),
            url: None,
            updated_at: e.updated_at,
            exists_locally: exists,
            imported: false,
            decrypt_error: None,
        };
        // Skip decrypt-and-import when this node already exists locally;
        // the user already has it. Auto-importing would risk silently
        // replacing locally-edited credentials.
        if exists {
            out.push(summary);
            continue;
        }
        match vault::decrypt(key, &e.encrypted_blob) {
            Ok(plain) => match serde_json::from_slice::<CloudNodeConfig>(&plain) {
                Ok(cfg) => {
                    summary.url = Some(cfg.url.clone());
                    // Auto-restore: same human, same vault key, same
                    // org → re-add the node so Tauri commands can hit it.
                    let res = state
                        .add_remote(
                            e.id.clone(),
                            e.label.clone(),
                            RemoteAgentConfig {
                                url: cfg.url,
                                token: cfg.token,
                                fingerprint: cfg.fingerprint,
                            },
                        )
                        .await;
                    summary.imported = res.is_ok();
                    if let Err(err) = res {
                        summary.decrypt_error = Some(format!("import: {err}"));
                    }
                }
                Err(err) => summary.decrypt_error = Some(format!("parse: {err}")),
            },
            Err(err) => summary.decrypt_error = Some(err),
        }
        out.push(summary);
    }
    Ok(out)
}

