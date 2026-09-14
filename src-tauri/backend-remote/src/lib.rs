//! Remote [`NodeBackend`] talking to a `localforge-agent` over HTTPS + WebSocket. TLS pins the
//! agent's self-signed cert by SHA-256 fingerprint, or falls back to the WebPKI roots.

mod pinning;

pub use pinning::{FingerprintVerifier, FingerprintError, PinnedFingerprint};

use async_trait::async_trait;
use futures_util::sink::SinkExt;
use futures_util::stream::{self, BoxStream, StreamExt};
use localforge_core::backend::{
    BackendError, ByteStream, InstallStream, LogLine, LogStream, NodeBackend, Result,
};
use localforge_core::types::{
    BackupEntry, BackupTarget, ContainerStats, CreateServerRequest, DirectoryContents, DockerInfo,
    FileEntry, GameConfig, InstallEvent, MetricPoint, NodeStats, OrgBackupTarget, Player,
    PlayerAction, Schedule, Server, ServerStatus,
};
use rustls::ClientConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use url::Url;

/// Remote agent connection settings, persisted on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteAgentConfig {
    /// Base URL, e.g. `https://1.2.3.4:7878`.
    pub url: String,
    /// Bearer token (the `lf_agent_…` string printed by the installer).
    pub token: String,
    /// SHA-256 cert fingerprint. `None` ⇒ verify against system roots.
    pub fingerprint: Option<String>,
}

pub struct RemoteAgentBackend {
    base_url: Url,
    token: String,
    http: reqwest::Client,
    tls: Arc<ClientConfig>,
}

impl RemoteAgentBackend {
    pub async fn connect(cfg: RemoteAgentConfig) -> Result<Self> {
        let base_url = Url::parse(&cfg.url)
            .map_err(|e| BackendError::invalid(format!("bad agent URL: {}", e)))?;

        let tls = build_tls_config(cfg.fingerprint.as_deref())?;

        let http = reqwest::Client::builder()
            .use_preconfigured_tls((*tls).clone())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| BackendError::Transport(e.to_string()))?;

        let backend = Self {
            base_url,
            token: cfg.token,
            http,
            tls: tls.clone(),
        };
        backend.ping().await?;
        Ok(backend)
    }

    fn endpoint(&self, path: &str) -> Result<Url> {
        self.base_url
            .join(path)
            .map_err(|e| BackendError::invalid(format!("bad path: {}", e)))
    }

    async fn get<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T> {
        let url = self.endpoint(path)?;
        let resp = self
            .http
            .get(url)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(transport)?;
        decode_json(resp).await
    }

    async fn post_json<B: Serialize, T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let url = self.endpoint(path)?;
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await
            .map_err(transport)?;
        decode_json(resp).await
    }

    async fn patch_json<B: Serialize, T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let url = self.endpoint(path)?;
        let resp = self
            .http
            .patch(url)
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await
            .map_err(transport)?;
        decode_json(resp).await
    }

    async fn delete(&self, path: &str) -> Result<()> {
        let url = self.endpoint(path)?;
        let resp = self
            .http
            .delete(url)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(transport)?;
        ensure_ok(resp).await
    }
}

fn build_tls_config(fingerprint: Option<&str>) -> Result<Arc<ClientConfig>> {
    // Idempotent; harmless if another crate already installed a provider.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let cfg = if let Some(fp) = fingerprint {
        let pinned = PinnedFingerprint::parse(fp)
            .map_err(|e| BackendError::invalid(format!("bad fingerprint: {}", e)))?;
        let verifier = FingerprintVerifier::new(pinned);
        ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth()
    } else {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth()
    };
    Ok(Arc::new(cfg))
}

fn transport(e: reqwest::Error) -> BackendError {
    if e.is_timeout() {
        BackendError::Transport(format!("timed out: {}", e))
    } else if e.is_connect() {
        BackendError::NotConnected(e.to_string())
    } else {
        BackendError::Transport(e.to_string())
    }
}

async fn ensure_ok(resp: reqwest::Response) -> Result<()> {
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    let body = resp.text().await.unwrap_or_default();
    Err(status_to_backend_error(status, &body))
}

async fn decode_json<T: for<'de> Deserialize<'de>>(resp: reqwest::Response) -> Result<T> {
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(status_to_backend_error(status, &body));
    }
    resp.json::<T>()
        .await
        .map_err(|e| BackendError::Transport(format!("invalid JSON from agent: {}", e)))
}

fn status_to_backend_error(status: reqwest::StatusCode, body: &str) -> BackendError {
    let msg = parse_error_body(body).unwrap_or_else(|| body.to_string());
    match status.as_u16() {
        401 | 403 => BackendError::Unauthorized,
        404 => BackendError::NotFound(msg),
        400 => BackendError::InvalidInput(msg),
        _ => BackendError::Other(format!("agent {} — {}", status, msg)),
    }
}

#[derive(Deserialize)]
struct ErrorBody {
    error: String,
}

fn parse_error_body(body: &str) -> Option<String> {
    serde_json::from_str::<ErrorBody>(body).ok().map(|e| e.error)
}

// NodeBackend impl: one HTTP call per agent route.

#[derive(Serialize)]
struct CreateBody<'a> {
    request: &'a CreateServerRequest,
    game: &'a GameConfig,
}

#[derive(Serialize)]
struct ConfigBody<'a> {
    config: &'a HashMap<String, String>,
}

#[derive(Serialize)]
struct CommandBody<'a> {
    command: &'a str,
}

#[derive(Serialize)]
struct PathBody<'a> {
    path: &'a str,
}

#[derive(Serialize)]
struct WriteBody<'a> {
    path: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct FromToBody<'a> {
    from: &'a str,
    to: &'a str,
}

#[derive(Deserialize)]
struct ReadResponse {
    content: String,
}

#[derive(Deserialize)]
struct DiskResponse {
    bytes: u64,
}

#[derive(Deserialize)]
struct KeyResp {
    key: String,
}

#[derive(Serialize)]
struct RestoreReq<'a> {
    target: &'a BackupTarget,
    key: &'a str,
}

#[derive(Deserialize)]
struct LogsResponseBody {
    logs: Vec<String>,
}

/// `GET /v1/health` body; the version gates features older agents mis-handle.
#[derive(Deserialize)]
struct AgentHealth {
    #[serde(default)]
    version: String,
}

/// First agent release whose `DELETE /v1/servers/{id}` honours `?keep_data`; older agents
/// ignore the flag and wipe the data dir.
const KEEP_DATA_MIN_AGENT: (u64, u64, u64) = (0, 1, 58);

fn agent_supports_keep_data(version: &str) -> bool {
    let mut parts = version.trim().split('.').map(|p| {
        // Tolerate pre-release / build suffixes ("0.1.58-rc1").
        p.split(|c: char| !c.is_ascii_digit())
            .next()
            .unwrap_or("")
            .parse::<u64>()
            .unwrap_or(0)
    });
    let v = (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    );
    v >= KEEP_DATA_MIN_AGENT
}

/// Budget for requests that move a whole world (backups, file transfers).
const LONG_OP_TIMEOUT: Duration = Duration::from_secs(6 * 3600);

#[derive(Deserialize)]
struct WsLogFrame {
    server_id: Option<String>,
    line: Option<String>,
    error: Option<String>,
}

#[async_trait]
impl NodeBackend for RemoteAgentBackend {
    async fn ping(&self) -> Result<()> {
        // Public route; the token is sent anyway so the agent can log who connected.
        let url = self.endpoint("/v1/health")?;
        let resp = self
            .http
            .get(url)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(transport)?;
        ensure_ok(resp).await
    }

    async fn docker_info(&self) -> Result<DockerInfo> {
        self.get("/v1/info").await
    }

    async fn node_stats(&self) -> Result<NodeStats> {
        self.get("/v1/node/stats").await
    }

    async fn list_servers(&self) -> Result<Vec<Server>> {
        self.get("/v1/servers").await
    }

    async fn get_server(&self, id: &str) -> Result<Option<Server>> {
        self.get(&format!("/v1/servers/{}", id)).await
    }

    async fn server_status(&self, id: &str) -> Result<ServerStatus> {
        self.get(&format!("/v1/servers/{}/status", id)).await
    }

    async fn get_stats(&self, id: &str) -> Result<ContainerStats> {
        self.get(&format!("/v1/servers/{}/stats", id)).await
    }

    async fn get_disk_usage(&self, id: &str) -> Result<u64> {
        let r: DiskResponse = self.get(&format!("/v1/servers/{}/disk", id)).await?;
        Ok(r.bytes)
    }

    async fn get_logs(&self, id: &str, lines: usize) -> Result<Vec<String>> {
        let r: LogsResponseBody = self
            .get(&format!("/v1/servers/{}/logs?lines={}", id, lines))
            .await?;
        Ok(r.logs)
    }

    async fn create_server(
        &self,
        request: CreateServerRequest,
        game: GameConfig,
    ) -> Result<Server> {
        self.post_json(
            "/v1/servers",
            &CreateBody {
                request: &request,
                game: &game,
            },
        )
        .await
    }

    async fn update_server_config(
        &self,
        id: &str,
        config: HashMap<String, String>,
    ) -> Result<Server> {
        self.patch_json(
            &format!("/v1/servers/{}/config", id),
            &ConfigBody { config: &config },
        )
        .await
    }

    async fn delete_server(&self, id: &str) -> Result<()> {
        self.delete(&format!("/v1/servers/{}", id)).await
    }

    async fn delete_server_keep_data(&self, id: &str) -> Result<()> {
        // Older agents ignore the flag and do a FULL delete, so refuse unless the version is known-good.
        let health: AgentHealth = self.get("/v1/health").await?;
        if !agent_supports_keep_data(&health.version) {
            return Err(BackendError::Other(format!(
                "this agent (v{}) doesn't support keeping world data on delete — update it, or choose \"Delete everything\"",
                health.version
            )));
        }
        self.delete(&format!("/v1/servers/{}?keep_data=true", id)).await
    }

    async fn start_server(&self, id: &str) -> Result<ServerStatus> {
        let url = self.endpoint(&format!("/v1/servers/{}/start", id))?;
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(transport)?;
        decode_json(resp).await
    }

    async fn stop_server(&self, id: &str) -> Result<ServerStatus> {
        let url = self.endpoint(&format!("/v1/servers/{}/stop", id))?;
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(transport)?;
        decode_json(resp).await
    }

    async fn send_command(&self, id: &str, command: &str) -> Result<()> {
        let url = self.endpoint(&format!("/v1/servers/{}/command", id))?;
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .json(&CommandBody { command })
            .send()
            .await
            .map_err(transport)?;
        ensure_ok(resp).await
    }

    // ----- backups -------------------------------------------------------

    async fn create_backup(&self, id: &str, target: &BackupTarget) -> Result<String> {
        let url = self.endpoint(&format!("/v1/servers/{}/backup", id))?;
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .timeout(LONG_OP_TIMEOUT)
            .json(target)
            .send()
            .await
            .map_err(transport)?;
        let r: KeyResp = decode_json(resp).await?;
        Ok(r.key)
    }

    async fn list_backups(&self, id: &str, target: &BackupTarget) -> Result<Vec<BackupEntry>> {
        self.post_json(&format!("/v1/servers/{}/backups/list", id), target)
            .await
    }

    async fn restore_backup(&self, id: &str, target: &BackupTarget, key: &str) -> Result<()> {
        let url = self.endpoint(&format!("/v1/servers/{}/restore", id))?;
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .timeout(LONG_OP_TIMEOUT)
            .json(&RestoreReq { target, key })
            .send()
            .await
            .map_err(transport)?;
        ensure_ok(resp).await
    }

    async fn delete_backup(&self, id: &str, target: &BackupTarget, key: &str) -> Result<()> {
        // Server-scoped so the agent can confine the delete to this server's prefix.
        let url = self.endpoint(&format!("/v1/servers/{}/backups/delete", id))?;
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .json(&RestoreReq { target, key })
            .send()
            .await
            .map_err(transport)?;
        ensure_ok(resp).await
    }

    /// Replace the agent's backup-target list over direct HTTPS (never the relay).
    async fn set_backup_targets(&self, targets: &[OrgBackupTarget]) -> Result<()> {
        let url = self.endpoint("/v1/backup-targets")?;
        let resp = self
            .http
            .put(url)
            .bearer_auth(&self.token)
            .json(targets)
            .send()
            .await
            .map_err(transport)?;
        ensure_ok(resp).await
    }

    // ----- scheduled actions ---------------------------------------------

    async fn list_schedules(&self, server_id: &str) -> Result<Vec<Schedule>> {
        self.get(&format!("/v1/servers/{}/schedules", server_id)).await
    }

    async fn upsert_schedule(&self, schedule: Schedule) -> Result<()> {
        let url = self.endpoint(&format!("/v1/servers/{}/schedules", schedule.server_id))?;
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .json(&schedule)
            .send()
            .await
            .map_err(transport)?;
        ensure_ok(resp).await
    }

    async fn delete_schedule(&self, id: &str) -> Result<()> {
        let url = self.endpoint(&format!("/v1/schedules/{}", id))?;
        let resp = self
            .http
            .delete(url)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(transport)?;
        ensure_ok(resp).await
    }

    async fn query_metrics(&self, server_id: &str, since_ms: i64) -> Result<Vec<MetricPoint>> {
        self.get(&format!("/v1/servers/{}/metrics?since={}", server_id, since_ms))
            .await
    }

    async fn list_players(&self, server_id: &str) -> Result<Vec<Player>> {
        self.get(&format!("/v1/servers/{}/players", server_id)).await
    }

    async fn player_action(&self, server_id: &str, action: PlayerAction) -> Result<()> {
        let url = self.endpoint(&format!("/v1/servers/{}/players/action", server_id))?;
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .json(&action)
            .send()
            .await
            .map_err(transport)?;
        ensure_ok(resp).await
    }

    async fn stream_logs(&self, id: &str) -> Result<LogStream> {
        ws_log_stream(self.base_url.clone(), self.token.clone(), self.tls.clone(), id).await
    }

    async fn reset_server_data(&self, id: &str) -> Result<()> {
        let url = self.endpoint(&format!("/v1/servers/{}/reset-data", id))?;
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(transport)?;
        ensure_ok(resp).await
    }

    async fn run_install(&self, id: &str, game: GameConfig) -> Result<InstallStream> {
        ws_install_stream(
            self.base_url.clone(),
            self.token.clone(),
            self.tls.clone(),
            id,
            game,
        )
        .await
    }

    // ---- file ops --------------------------------------------------------

    async fn list_files(&self, path: &str) -> Result<DirectoryContents> {
        self.get(&format!("/v1/fs?path={}", urlencoding(path))).await
    }

    async fn read_file_text(&self, path: &str) -> Result<String> {
        let r: ReadResponse = self.post_json("/v1/fs/read", &PathBody { path }).await?;
        Ok(r.content)
    }

    async fn write_file_text(&self, path: &str, content: &str) -> Result<()> {
        let url = self.endpoint("/v1/fs/write")?;
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .json(&WriteBody { path, content })
            .send()
            .await
            .map_err(transport)?;
        ensure_ok(resp).await
    }

    async fn create_file(&self, path: &str) -> Result<()> {
        let url = self.endpoint("/v1/fs/create-file")?;
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .json(&PathBody { path })
            .send()
            .await
            .map_err(transport)?;
        ensure_ok(resp).await
    }

    async fn create_directory(&self, path: &str) -> Result<()> {
        let url = self.endpoint("/v1/fs/create-dir")?;
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .json(&PathBody { path })
            .send()
            .await
            .map_err(transport)?;
        ensure_ok(resp).await
    }

    async fn delete_path(&self, path: &str) -> Result<()> {
        self.delete(&format!("/v1/fs?path={}", urlencoding(path)))
            .await
    }

    async fn rename_path(&self, from: &str, to: &str) -> Result<()> {
        let url = self.endpoint("/v1/fs/rename")?;
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .json(&FromToBody { from, to })
            .send()
            .await
            .map_err(transport)?;
        ensure_ok(resp).await
    }

    async fn move_path(&self, from: &str, to: &str) -> Result<()> {
        let url = self.endpoint("/v1/fs/move")?;
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .json(&FromToBody { from, to })
            .send()
            .await
            .map_err(transport)?;
        ensure_ok(resp).await
    }

    async fn copy_path(&self, from: &str, to: &str) -> Result<()> {
        let url = self.endpoint("/v1/fs/copy")?;
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .json(&FromToBody { from, to })
            .send()
            .await
            .map_err(transport)?;
        ensure_ok(resp).await
    }

    async fn file_info(&self, path: &str) -> Result<FileEntry> {
        self.get(&format!("/v1/fs/info?path={}", urlencoding(path)))
            .await
    }

    async fn download_file(&self, path: &str) -> Result<ByteStream> {
        let url = self.endpoint(&format!("/v1/fs/download?path={}", urlencoding(path)))?;
        let resp = self
            .http
            .get(url)
            .bearer_auth(&self.token)
            .timeout(LONG_OP_TIMEOUT)
            .send()
            .await
            .map_err(transport)?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(status_to_backend_error(status, &body));
        }
        let stream = resp
            .bytes_stream()
            .map(|r| r.map_err(|e| BackendError::Transport(e.to_string())));
        Ok(Box::pin(stream))
    }

    async fn upload_file(&self, path: &str, body: ByteStream) -> Result<()> {
        let url = self.endpoint(&format!("/v1/fs/upload?path={}", urlencoding(path)))?;
        // reqwest's streaming body wants io::Error items.
        let mapped =
            body.map(|r| r.map_err(|e| std::io::Error::other(e.to_string())));
        let resp = self
            .http
            .put(url)
            .bearer_auth(&self.token)
            .timeout(LONG_OP_TIMEOUT)
            .header("content-type", "application/octet-stream")
            .body(reqwest::Body::wrap_stream(mapped))
            .send()
            .await
            .map_err(transport)?;
        ensure_ok(resp).await
    }
}

/// Minimal percent-encoding for the `path` query parameter.
fn urlencoding(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' | '/' => c.to_string(),
            _ => {
                let mut buf = [0u8; 4];
                c.encode_utf8(&mut buf)
                    .as_bytes()
                    .iter()
                    .map(|b| format!("%{:02X}", b))
                    .collect()
            }
        })
        .collect()
}

// WebSocket streams

async fn ws_install_stream(
    base_url: Url,
    token: String,
    tls: Arc<ClientConfig>,
    server_id: &str,
    game: GameConfig,
) -> Result<InstallStream> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::handshake::client::Request;
    use tokio_tungstenite::tungstenite::http::HeaderValue;
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::{connect_async_tls_with_config, Connector};

    let mut ws_url = base_url.clone();
    let scheme = match base_url.scheme() {
        "https" => "wss",
        "http" => "ws",
        other => {
            return Err(BackendError::invalid(format!(
                "unsupported scheme: {}",
                other
            )))
        }
    };
    ws_url
        .set_scheme(scheme)
        .map_err(|_| BackendError::Other("failed to swap URL scheme".into()))?;
    ws_url.set_path(&format!("/v1/servers/{}/install/stream", server_id));

    let mut request: Request = ws_url
        .as_str()
        .into_client_request()
        .map_err(|e| BackendError::Transport(e.to_string()))?;
    request.headers_mut().insert(
        "Authorization",
        HeaderValue::try_from(format!("Bearer {}", token))
            .map_err(|e| BackendError::Other(e.to_string()))?,
    );

    let connector = Connector::Rustls(tls);
    let (mut ws_stream, _resp) =
        connect_async_tls_with_config(request, None, false, Some(connector))
            .await
            .map_err(|e| BackendError::Transport(e.to_string()))?;

    // Send the first frame with the game config.
    let init = serde_json::json!({ "game": game }).to_string();
    ws_stream
        .send(Message::Text(init.into()))
        .await
        .map_err(|e| BackendError::Transport(e.to_string()))?;

    let mapped: BoxStream<'static, Result<InstallEvent>> = ws_stream
        .filter_map(|item| async move {
            match item {
                Ok(Message::Text(text)) => {
                    // An error frame bubbles up as Err; anything else parses as InstallEvent.
                    #[derive(serde::Deserialize)]
                    struct ErrFrame {
                        kind: String,
                        message: Option<String>,
                    }
                    if let Ok(err) = serde_json::from_str::<ErrFrame>(&text) {
                        if err.kind == "error" {
                            return Some(Err(BackendError::Other(
                                err.message.unwrap_or_else(|| "agent error".to_string()),
                            )));
                        }
                    }
                    match serde_json::from_str::<InstallEvent>(&text) {
                        Ok(ev) => Some(Ok(ev)),
                        Err(e) => Some(Err(BackendError::Transport(format!(
                            "bad install frame: {}",
                            e
                        )))),
                    }
                }
                Ok(Message::Close(_)) => None,
                Ok(_) => None,
                Err(e) => Some(Err(BackendError::Transport(e.to_string()))),
            }
        })
        .boxed();

    Ok(mapped)
}

async fn ws_log_stream(
    base_url: Url,
    token: String,
    tls: Arc<ClientConfig>,
    server_id: &str,
) -> Result<LogStream> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::handshake::client::Request;
    use tokio_tungstenite::tungstenite::http::HeaderValue;
    use tokio_tungstenite::{connect_async_tls_with_config, Connector};

    let mut ws_url = base_url.clone();
    let new_scheme = match base_url.scheme() {
        "https" => "wss",
        "http" => "ws",
        other => return Err(BackendError::invalid(format!("unsupported scheme: {}", other))),
    };
    ws_url
        .set_scheme(new_scheme)
        .map_err(|_| BackendError::Other("failed to swap URL scheme".into()))?;
    ws_url.set_path(&format!("/v1/servers/{}/stream", server_id));

    let mut request: Request = ws_url
        .as_str()
        .into_client_request()
        .map_err(|e| BackendError::Transport(e.to_string()))?;
    request.headers_mut().insert(
        "Authorization",
        HeaderValue::try_from(format!("Bearer {}", token))
            .map_err(|e| BackendError::Other(e.to_string()))?,
    );

    let connector = Connector::Rustls(tls);
    let (ws_stream, _resp) = connect_async_tls_with_config(request, None, false, Some(connector))
        .await
        .map_err(|e| BackendError::Transport(e.to_string()))?;

    let server_id_owned = server_id.to_string();
    let mapped: BoxStream<'static, Result<LogLine>> = ws_stream
        .flat_map(move |item| {
            let server_id = server_id_owned.clone();
            let items: Vec<Result<LogLine>> = match item {
                Ok(msg) => match msg {
                    tokio_tungstenite::tungstenite::Message::Text(text) => {
                        match serde_json::from_str::<WsLogFrame>(&text) {
                            Ok(frame) => {
                                if let Some(line) = frame.line {
                                    vec![Ok(LogLine {
                                        server_id: frame.server_id.unwrap_or(server_id.clone()),
                                        line,
                                    })]
                                } else if let Some(err) = frame.error {
                                    vec![Err(BackendError::Other(err))]
                                } else {
                                    vec![]
                                }
                            }
                            Err(e) => vec![Err(BackendError::Transport(format!(
                                "invalid WS frame: {}",
                                e
                            )))],
                        }
                    }
                    tokio_tungstenite::tungstenite::Message::Close(_) => vec![],
                    _ => vec![],
                },
                Err(e) => vec![Err(BackendError::Transport(e.to_string()))],
            };
            stream::iter(items)
        })
        .boxed();

    Ok(mapped)
}

#[cfg(test)]
mod tests {
    use super::agent_supports_keep_data;

    #[test]
    fn keep_data_gate_by_agent_version() {
        assert!(!agent_supports_keep_data("0.1.50"));
        assert!(!agent_supports_keep_data("0.1.57"));
        assert!(agent_supports_keep_data("0.1.58"));
        assert!(agent_supports_keep_data("0.1.58-rc1"));
        assert!(agent_supports_keep_data("0.2.0"));
        assert!(agent_supports_keep_data("1.0.0"));
        // Unparseable → treated as too old (refuse, never wipe).
        assert!(!agent_supports_keep_data(""));
        assert!(!agent_supports_keep_data("dev"));
    }
}
