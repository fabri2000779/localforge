//! Remote [`NodeBackend`] implementation — talks to a `localforge-agent`
//! over HTTPS + WebSocket.
//!
//! TLS strategy:
//!   - If a SHA-256 fingerprint is configured we pin it (the agent's
//!     self-signed default).
//!   - Otherwise we fall back to the system WebPKI roots (the
//!     "agent terminates Let's Encrypt at a real domain" case).

mod pinning;

pub use pinning::{FingerprintVerifier, FingerprintError, PinnedFingerprint};

use async_trait::async_trait;
use futures_util::stream::{self, BoxStream, StreamExt};
use localforge_core::backend::{BackendError, LogLine, LogStream, NodeBackend, Result};
use localforge_core::types::{
    ContainerStats, CreateServerRequest, DirectoryContents, DockerInfo, FileEntry, GameConfig,
    Server, ServerStatus,
};
use rustls::ClientConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use url::Url;

/// Configuration for a remote agent connection. Persisted on disk and
/// pasted by the user into the "Add Node" form.
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
    // Ensure the default crypto provider is installed for this process.
    // It's idempotent — repeated installs after the first are no-ops.
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

// ===========================================================================
// NodeBackend impl — each method is a one-shot HTTP call mapping to the
// agent route of the matching name.
// ===========================================================================

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
struct LogsResponseBody {
    logs: Vec<String>,
}

#[derive(Deserialize)]
struct WsLogFrame {
    server_id: Option<String>,
    line: Option<String>,
    error: Option<String>,
}

#[async_trait]
impl NodeBackend for RemoteAgentBackend {
    async fn ping(&self) -> Result<()> {
        // /v1/health is public — no token needed, but we still send it so
        // the server can log who connected.
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

    async fn stream_logs(&self, id: &str) -> Result<LogStream> {
        ws_log_stream(self.base_url.clone(), self.token.clone(), self.tls.clone(), id).await
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
}

/// Minimal URL encoding for the path query parameter (we just escape the
/// characters that would actually break the URL; query parsers accept the
/// rest as-is).
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

// ===========================================================================
// WebSocket log streaming
// ===========================================================================

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

    // Build the wss:// URL from the agent base URL.
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
