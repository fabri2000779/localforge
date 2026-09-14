//! Cloud sync of server configs: the cloud stores each server's id + name in plain and the rest as an
//! AES-256-GCM blob sealed with the org DEK, which never leaves the device.

use base64::Engine;
use serde::{Deserialize, Serialize};

use super::{api, auth, vault};
use crate::backend::NodeRegistry;
use crate::backend::registry::RemoteNodeForSync;
use localforge_backend_remote::RemoteAgentConfig;
use localforge_core::types::{BackupTarget, GameType, OrgBackupTarget, Server};

/// The per-server payload we encrypt: NOT the full Server (container id, data path and install state are
/// machine-specific). `node_id` says which node hosts it, so sub-users route relay commands to the right
/// executor; defaults to "local" for pre-v0.1.12 blobs.
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

/// One pull row for the UI; `decrypted` is None when the blob can't be read with this device's key.
#[derive(Debug, Serialize)]
pub struct RemoteServer {
    pub id: String,
    pub name: String,
    /// Unix ms.
    pub updated_at: i64,
    pub decrypted: Option<CloudServerConfig>,
    /// Whether a server with this id exists locally.
    pub exists_locally: bool,
    /// Set when decryption failed (e.g. a different vault key on this device).
    pub decrypt_error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SyncResult {
    pub pushed: usize,
    pub conflicts: Vec<String>,
    pub remote: Vec<RemoteServer>,
}

// Push

/// Every server across this desktop's nodes, tagged with the node id to sync under (the local node uses
/// this machine's global device id so the cloud + relay can address it).
async fn list_servers_for_sync(
    state: &tauri::State<'_, NodeRegistry>,
) -> Vec<(Server, String)> {
    state.list_servers_for_sync().await
}

async fn push(
    token: &str,
    key: &[u8; 32],
    servers: &[(Server, String)],
) -> Result<(usize, Vec<String>), api::ApiError> {
    if servers.is_empty() {
        return Ok((0, vec![]));
    }
    let now = chrono::Utc::now().timestamp_millis();
    let mut payload: Vec<(String, String, String)> = Vec::with_capacity(servers.len());
    for (s, node_id) in servers {
        let cfg = CloudServerConfig::from_server(s, node_id.clone());
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
            // 409: the other side already had this state (this device is the only writer) — record + continue.
            Ok((0, servers.iter().map(|(s, _)| s.id.clone()).collect()))
        }
        Err(e) => Err(e),
    }
}

/// PUT the batch to /v1/sync/servers, mapping non-2xx to `ApiError::Server`.
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

// Pull
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

// "Sync now"

#[tauri::command]
pub async fn cloud_sync_now(
    state: tauri::State<'_, NodeRegistry>,
) -> Result<SyncResult, api::ApiError> {
    let token = auth::current_token().ok_or_else(|| api::ApiError::Server {
        status: 401,
        code: "unauthenticated".into(),
        message: None,
    })?;
    let tagged = list_servers_for_sync(&state).await;
    let local_ids: std::collections::HashSet<String> =
        tagged.iter().map(|(s, _)| s.id.clone()).collect();

    // Push only to our OWN org with our OWN DEK: as a sub-user viewing someone else's org, pushing our
    // servers (sealed with our key) would leave undecryptable blobs in their vault and brick their rotation.
    // Both signals are needed: a member without a grant yet has no borrowed-DEK override installed.
    let (pushed, conflicts) = if !super::orgs::active_org_owned() || vault::has_active_override() {
        (0, vec![])
    } else {
        let own = vault::ensure_key().map_err(|e| api::ApiError::Decode(format!("vault: {e}")))?;
        push(&token, &own, &tagged).await?
    };

    // Pull/decrypt with the ACTIVE org's DEK (the borrowed override if set) so a sub-user sees the owner's servers.
    let key = vault::active_dek().map_err(|e| api::ApiError::Decode(format!("vault: {e}")))?;
    let remote = pull(&token, &key, &local_ids).await?;

    Ok(SyncResult {
        pushed,
        conflicts,
        remote,
    })
}

/// Tombstone a server in the cloud after a local delete; the push path only UPSERTs, so without this
/// every other device kept a ghost server. Skipped when the active org isn't ours; 404 counts as success.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_sync_delete_server(server_id: String) -> Result<bool, api::ApiError> {
    let token = auth::current_token().ok_or_else(|| api::ApiError::Server {
        status: 401,
        code: "unauthenticated".into(),
        message: None,
    })?;
    if !super::orgs::active_org_owned() || vault::has_active_override() {
        return Ok(false);
    }
    match api::delete::<serde_json::Value>(
        &format!("/v1/sync/servers/{}", urlencode_path(&server_id)),
        Some(&token),
    )
    .await
    {
        Ok(_) => Ok(true),
        Err(api::ApiError::Server { status: 404, .. }) => Ok(false),
        // Free plan / not signed up for sync — nothing to tombstone.
        Err(api::ApiError::Server { status: 402, .. }) => Ok(false),
        Err(e) => Err(e),
    }
}

/// Percent-encode a path segment so an odd id can't alter the route.
fn urlencode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Pull-only sync used by the relay listener on `sync_changed`.
#[tauri::command]
pub async fn cloud_sync_pull(
    state: tauri::State<'_, NodeRegistry>,
) -> Result<Vec<RemoteServer>, api::ApiError> {
    let token = auth::current_token().ok_or_else(|| api::ApiError::Server {
        status: 401,
        code: "unauthenticated".into(),
        message: None,
    })?;
    // Decrypt with the ACTIVE org's DEK (our own, or the grant we opened as a sub-user); pushes use our own.
    let key = vault::active_dek().map_err(|e| api::ApiError::Decode(format!("vault: {e}")))?;
    let tagged = list_servers_for_sync(&state).await;
    let local_ids: std::collections::HashSet<String> =
        tagged.iter().map(|(s, _)| s.id.clone()).collect();
    pull(&token, &key, &local_ids).await
}

// DEK rotation

/// Rotate the org's DEK: re-encrypt every server blob under a FRESH key and re-seal it to the remaining
/// members, so a removed member's cached key decrypts nothing new. Owner-only; needs the sync passphrase
/// (the KEK isn't cached). The cloud commits wrap + blobs + grant wipe in ONE transaction. Returns re-grants.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_rotate_org_dek(
    org_id: String,
    passphrase: String,
    state: tauri::State<'_, NodeRegistry>,
) -> Result<usize, api::ApiError> {
    let token = auth::current_token().ok_or_else(|| api::ApiError::Server {
        status: 401,
        code: "unauthenticated".into(),
        message: None,
    })?;
    let current_dek =
        vault::ensure_key().map_err(|e| api::ApiError::Decode(format!("vault: {e}")))?;

    // Backup targets and node configs are encrypted with the SAME DEK as the server blobs: read them now,
    // while current_dek is still active, so they can be re-pushed under the new key once it's adopted below.
    let backup_targets_plain = pull_backup_targets().await.unwrap_or_default();
    let local_nodes: Vec<RemoteNodeForSync> = state.list_remote_for_sync().unwrap_or_default();

    // Re-derive the KEK and verify it unwraps the current DEK (a typo must not mint an unopenable wrap).
    let me = localforge_cloud_client::auth::fetch_me(&token).await?;
    let sk = me.sync_key.ok_or_else(|| api::ApiError::Server {
        status: 412,
        code: "sync_key_not_set".into(),
        message: None,
    })?;
    let salt = base64::engine::general_purpose::STANDARD
        .decode(&sk.kek_salt)
        .map_err(|e| api::ApiError::Decode(format!("bad salt: {e}")))?;
    let kek = vault::derive_kek(&passphrase, &salt).map_err(api::ApiError::Decode)?;
    let check = vault::unwrap_dek(&kek, &sk.wrapped_dek).map_err(|_| api::ApiError::Server {
        status: 400,
        code: "wrong_secret".into(),
        message: Some("passphrase doesn't match".into()),
    })?;
    if check != current_dek {
        return Err(api::ApiError::Server {
            status: 400,
            code: "wrong_secret".into(),
            message: Some("passphrase doesn't match this device's key".into()),
        });
    }

    // Pull + decrypt every blob with the current DEK.
    let empty: std::collections::HashSet<String> = std::collections::HashSet::new();
    let remote = pull(&token, &current_dek, &empty).await?;

    // Fresh DEK; verify each re-encryption round-trips before trusting the new blob.
    let new_dek = vault::generate_key();

    #[derive(Serialize)]
    struct RotateServer {
        id: String,
        #[serde(rename = "encryptedBlob")]
        encrypted_blob: String,
        #[serde(rename = "updatedAt")]
        updated_at: i64,
    }
    let mut servers = Vec::with_capacity(remote.len());
    for r in remote {
        let Some(cfg) = r.decrypted else {
            // A blob we can't read → abort rather than orphan it under the new key.
            return Err(api::ApiError::Server {
                status: 409,
                code: "undecryptable_blob".into(),
                message: Some(format!("can't rotate: server {} didn't decrypt", r.id)),
            });
        };
        let plaintext = serde_json::to_vec(&cfg)
            .map_err(|e| api::ApiError::Decode(format!("serialize: {e}")))?;
        let blob = vault::encrypt(&new_dek, &plaintext).map_err(api::ApiError::Decode)?;
        match vault::decrypt(&new_dek, &blob) {
            Ok(back) if back == plaintext => {}
            _ => {
                return Err(api::ApiError::Decode(format!(
                    "rotation self-check failed for {}",
                    r.id
                )));
            }
        }
        servers.push(RotateServer {
            id: r.id,
            encrypted_blob: blob,
            // Send the PULLED version: the cloud aborts the rotation (409) if any server changed since, so no
            // edit is clobbered and no blob is left under the discarded key.
            updated_at: r.updated_at,
        });
    }

    // KEK-wrap the new DEK + commit atomically on the cloud.
    let new_wrapped = vault::wrap_dek(&kek, &new_dek).map_err(api::ApiError::Decode)?;
    #[derive(Serialize)]
    struct Body {
        #[serde(rename = "wrappedDek")]
        wrapped_dek: String,
        servers: Vec<RotateServer>,
    }
    let _: serde_json::Value = api::post(
        "/v1/sync/rotate",
        &Body {
            wrapped_dek: new_wrapped,
            servers,
        },
        Some(&token),
    )
    .await?;

    // Adopt the new DEK locally, then re-seal it to the remaining members.
    vault::save_key(&new_dek).map_err(|e| api::ApiError::Decode(format!("vault: {e}")))?;

    // Re-encrypt backup targets + node configs under the new DEK (best-effort: a failure leaves them on
    // the old key, recoverable by a manual re-push).
    for t in &backup_targets_plain {
        if let Err(e) = push_backup_target(t).await {
            tracing::warn!("[rotate] re-push backup target {} failed: {:?}", t.id, e);
        }
    }
    if !local_nodes.is_empty() {
        if let Err(e) = push_nodes(&token, &new_dek, &local_nodes).await {
            tracing::warn!("[rotate] re-push nodes failed: {:?}", e);
        }
    }

    let granted = vault::process_grants(&org_id, &token).await?;
    Ok(granted)
}

// Org backup targets: BYO S3 credentials encrypted with the org DEK; the owner's devices pull + decrypt,
// agents are provisioned over direct HTTPS. Paid-only: a free org gets 402 (callers treat it as "stays local").

fn require_token() -> Result<String, api::ApiError> {
    auth::current_token().ok_or_else(|| api::ApiError::Server {
        status: 401,
        code: "unauthenticated".into(),
        message: None,
    })
}

fn get_dek() -> Result<[u8; 32], api::ApiError> {
    vault::active_dek().map_err(|e| api::ApiError::Decode(format!("vault: {e}")))
}

/// Push one named target (matched by id), encrypting the credentials with the org DEK.
pub async fn push_backup_target(t: &OrgBackupTarget) -> Result<(), api::ApiError> {
    let token = require_token()?;
    let dek = get_dek()?;
    let plain = serde_json::to_vec(&t.credentials)
        .map_err(|e| api::ApiError::Decode(format!("serialize: {e}")))?;
    let blob =
        vault::encrypt(&dek, &plain).map_err(|e| api::ApiError::Decode(format!("encrypt: {e}")))?;
    #[derive(Serialize)]
    struct Body<'a> {
        id: &'a str,
        name: &'a str,
        #[serde(rename = "encryptedBlob")]
        encrypted_blob: &'a str,
    }
    let _: serde_json::Value = api::post(
        "/v1/sync/backup-targets",
        &Body { id: &t.id, name: &t.name, encrypted_blob: &blob },
        Some(&token),
    )
    .await?;
    Ok(())
}

/// Pull + decrypt all org backup targets; empty when the org has none or the DEK is unavailable.
pub async fn pull_backup_targets() -> Result<Vec<OrgBackupTarget>, api::ApiError> {
    let token = require_token()?;
    let dek = get_dek()?;
    #[derive(Deserialize)]
    struct Row {
        id: String,
        name: String,
        #[serde(rename = "encryptedBlob")]
        encrypted_blob: String,
    }
    #[derive(Deserialize)]
    struct Resp {
        targets: Vec<Row>,
    }
    let r: Resp = api::get("/v1/sync/backup-targets", Some(&token)).await?;
    let mut out = Vec::with_capacity(r.targets.len());
    for row in r.targets {
        // Skip (and log) a row this device can't read instead of failing the WHOLE list, e.g. a target left
        // under the old key after a rotation whose best-effort re-push failed.
        let plain = match vault::decrypt(&dek, &row.encrypted_blob) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("[backup-targets] skipping undecryptable target {} ({}): {e}", row.id, row.name);
                continue;
            }
        };
        let credentials: BackupTarget = match serde_json::from_slice(&plain) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("[backup-targets] skipping unparseable target {} ({}): {e}", row.id, row.name);
                continue;
            }
        };
        out.push(OrgBackupTarget { id: row.id, name: row.name, credentials });
    }
    Ok(out)
}

/// Remove one org backup target from the cloud by id.
pub async fn delete_backup_target_remote(id: &str) -> Result<(), api::ApiError> {
    let token = require_token()?;
    let _: serde_json::Value =
        api::delete(&format!("/v1/sync/backup-targets/{}", id), Some(&token)).await?;
    Ok(())
}

// Node configs (url, fingerprint, token) are E2E-encrypted with the org DEK: pushed on add and
// sign-in, tombstoned on remove, and pulled on sign-in so another desktop of the owner restores
// the fleet without re-pairing. Re-pushed under the new key by `cloud_rotate_org_dek`.

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

/// Import every cloud node missing locally; returns how many were restored. 403 (sub-user) → 0.
async fn pull_nodes(
    token: &str,
    key: &[u8; 32],
    state: &NodeRegistry,
    local: &[RemoteNodeForSync],
) -> Result<usize, api::ApiError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Entry {
        id: String,
        label: String,
        encrypted_blob: String,
    }
    #[derive(Deserialize)]
    struct Resp {
        nodes: Vec<Entry>,
    }
    let resp: Result<Resp, api::ApiError> = api::get("/v1/sync/nodes", Some(token)).await;
    let entries = match resp {
        Ok(r) => r.nodes,
        Err(api::ApiError::Server { status: 403, .. }) => return Ok(0),
        Err(e) => return Err(e),
    };

    let mut imported = 0;
    for e in entries {
        // Never overwrite a node the user already has (its credentials may have been edited locally).
        if local.iter().any(|n| n.id == e.id) {
            continue;
        }
        let cfg = vault::decrypt(key, &e.encrypted_blob).and_then(|plain| {
            serde_json::from_slice::<CloudNodeConfig>(&plain).map_err(|err| err.to_string())
        });
        let cfg = match cfg {
            Ok(c) => c,
            Err(err) => {
                tracing::warn!("[node-sync] skipping undecryptable node {} ({}): {err}", e.id, e.label);
                continue;
            }
        };
        let import = state
            .import_remote(
                e.id.clone(),
                e.label,
                RemoteAgentConfig {
                    url: cfg.url,
                    token: cfg.token,
                    fingerprint: cfg.fingerprint,
                },
            )
            .await;
        match import {
            Ok(()) => imported += 1,
            Err(err) => tracing::warn!("[node-sync] import of node {} failed: {err}", e.id),
        }
    }
    Ok(imported)
}

#[derive(Debug, Serialize)]
pub struct NodeSyncResult {
    pub imported: usize,
}

/// Owner-side node sync: restore cloud nodes missing locally, then push the local set.
#[tauri::command]
pub async fn cloud_sync_nodes_now(
    state: tauri::State<'_, NodeRegistry>,
) -> Result<NodeSyncResult, api::ApiError> {
    let token = require_token()?;
    let key = vault::ensure_key().map_err(|e| api::ApiError::Decode(format!("vault: {e}")))?;
    let local = state
        .list_remote_for_sync()
        .map_err(|e| api::ApiError::Decode(format!("list nodes: {e}")))?;
    let imported = pull_nodes(&token, &key, &state, &local).await?;
    push_nodes(&token, &key, &local).await?;
    Ok(NodeSyncResult { imported })
}

/// Tombstone a node in the cloud after a local remove so other desktops stop restoring it.
/// Same skip rules as `cloud_sync_delete_server`; `false` when nothing was tombstoned.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_sync_delete_node(node_id: String) -> Result<bool, api::ApiError> {
    let token = require_token()?;
    if !super::orgs::active_org_owned() || vault::has_active_override() {
        return Ok(false);
    }
    match api::delete::<serde_json::Value>(
        &format!("/v1/sync/nodes/{}", urlencode_path(&node_id)),
        Some(&token),
    )
    .await
    {
        Ok(_) => Ok(true),
        Err(api::ApiError::Server { status: 402..=404, .. }) => Ok(false),
        Err(e) => Err(e),
    }
}
