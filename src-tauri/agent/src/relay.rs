//! Relay client — connects a *linked* agent to the LocalForge cloud relay so
//! it executes commands without the owner's desktop being online.
//!
//! Only runs when `config.cloud` is set (an enrolled agent). The standalone
//! HTTPS surface is unaffected. Mirrors the mobile relay's staged-connect TLS
//! (webpki roots + ring) — the approach that cured the connect-hang — and
//! dispatches the relay cmd vocabulary to the SAME `NodeBackend` the agent
//! already serves over HTTPS. See the cloud repo's
//! docs/adr/0001-agent-direct-relay.md.
//!
//! Inbound  : `cmd` frames {type:'cmd', cmd, target, request_id, args}.
//! Outbound : `event` frames the cloud broadcasts to the mobile/desktop —
//!            cmd_result, logs_snapshot, stats_snapshot, console_line,
//!            server.state_changed, state_snapshot.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use localforge_core::types::{Schedule, ServerStatus};
use localforge_core::{BackendError, NodeBackend};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::config::CloudLink;

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;
type Outbound = mpsc::UnboundedSender<String>;
/// Per-server live-log stream tasks, so `server.detach` can abort them.
type AttachMap = Arc<Mutex<HashMap<String, JoinHandle<()>>>>;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// App-level keepalive. The cloud DO auto-responds to {"type":"ping"} via
/// setWebSocketAutoResponse without waking, keeping the socket alive.
const PING_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, Deserialize)]
struct RelayFrame {
    #[serde(rename = "type")]
    ty: String,
    cmd: Option<String>,
    target: Option<String>,
    request_id: Option<String>,
    args: Option<Value>,
}

/// Spawn the relay client. Returns immediately; the loop runs for the life of
/// the process, reconnecting with backoff (250ms → 30s). `data_root` is where
/// the provisioned S3 backup target lives (for relay-triggered backups).
pub fn spawn(backend: Arc<dyn NodeBackend>, link: CloudLink, data_root: std::path::PathBuf) {
    tokio::spawn(async move { run(backend, link, data_root).await });
}

async fn run(backend: Arc<dyn NodeBackend>, link: CloudLink, data_root: std::path::PathBuf) {
    let connector = relay_tls_connector();
    let host = link
        .api_origin
        .strip_prefix("https://")
        .or_else(|| link.api_origin.strip_prefix("http://"))
        .unwrap_or("api.localforge.gg")
        .to_string();
    // node_token is base64url (URL-safe) so it needs no escaping.
    let url = format!(
        "wss://{}/v1/relay/{}?node_token={}",
        host, link.org_id, link.node_token,
    );

    let mut backoff_ms: u64 = 250;
    loop {
        tracing::debug!("[relay] connecting to {}", host);
        match connect(&url, &host, &connector).await {
            Ok(ws) => {
                tracing::info!("[relay] connected as node {}", link.node_id);
                backoff_ms = 250;
                serve_connection(ws, backend.clone(), data_root.clone()).await;
                tracing::info!("[relay] disconnected; reconnecting");
            }
            Err(e) => tracing::warn!("[relay] connect failed: {}", e),
        }
        tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
        backoff_ms = (backoff_ms.saturating_mul(2)).min(30_000);
    }
}

/// Staged, time-boxed connect: DNS → TCP → TLS+WS upgrade.
async fn connect(
    url: &str,
    host: &str,
    connector: &tokio_tungstenite::Connector,
) -> Result<Ws, String> {
    use tokio::time::timeout;
    let addrs: Vec<std::net::SocketAddr> =
        timeout(CONNECT_TIMEOUT, tokio::net::lookup_host((host, 443u16)))
            .await
            .map_err(|_| format!("dns timeout for {host}"))?
            .map_err(|e| format!("dns: {e}"))?
            .collect();
    let addr = addrs.into_iter().next().ok_or_else(|| format!("no address for {host}"))?;
    let tcp = timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
        .await
        .map_err(|_| "tcp timeout".to_string())?
        .map_err(|e| format!("tcp: {e}"))?;
    let _ = tcp.set_nodelay(true);
    let (ws, _resp) = timeout(
        CONNECT_TIMEOUT,
        tokio_tungstenite::client_async_tls_with_config(url, tcp, None, Some(connector.clone())),
    )
    .await
    .map_err(|_| "tls/ws timeout".to_string())?
    .map_err(|e| format!("tls/ws upgrade: {e}"))?;
    Ok(ws)
}

async fn serve_connection(mut ws: Ws, backend: Arc<dyn NodeBackend>, data_root: std::path::PathBuf) {
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
    let attach: AttachMap = Arc::new(Mutex::new(HashMap::new()));
    let mut ping = tokio::time::interval(PING_INTERVAL);
    ping.tick().await; // swallow the immediate first tick

    loop {
        tokio::select! {
            biased;
            out = out_rx.recv() => match out {
                Some(text) => {
                    if ws.send(Message::Text(text.into())).await.is_err() { break; }
                }
                None => break,
            },
            _ = ping.tick() => {
                let ping = json!({ "type": "ping" }).to_string();
                if ws.send(Message::Text(ping.into())).await.is_err() { break; }
            }
            frame = ws.next() => match frame {
                Some(Ok(Message::Text(txt))) => {
                    if let Ok(f) = serde_json::from_str::<RelayFrame>(&txt) {
                        if f.ty == "cmd" {
                            tokio::spawn(handle_cmd(f, backend.clone(), out_tx.clone(), attach.clone(), data_root.clone()));
                        }
                        // hello / presence / pong / our own echoed events: ignore
                    }
                }
                Some(Ok(Message::Ping(p))) => { let _ = ws.send(Message::Pong(p)).await; }
                Some(Ok(_)) => { /* binary / pong */ }
                Some(Err(e)) => { tracing::warn!("[relay] frame err: {}", e); break; }
                None => break,
            }
        }
    }

    // Tear down any live log streams on disconnect.
    let mut guard = attach.lock().await;
    for (_, h) in guard.drain() {
        h.abort();
    }
}

async fn handle_cmd(
    f: RelayFrame,
    backend: Arc<dyn NodeBackend>,
    out: Outbound,
    attach: AttachMap,
    data_root: std::path::PathBuf,
) {
    let cmd = f.cmd.unwrap_or_default();
    let target = f.target.unwrap_or_default();
    let rid = f.request_id;
    let args = f.args.unwrap_or(Value::Null);

    // Validate the server id (relay `target`) before passing it to the backend.
    // The backend constructs filesystem paths from this value, so reject anything
    // that could escape the data directory. Valid ids are UUIDs (hex + dashes);
    // we allow underscores for forward-compat but nothing else.
    if !target.is_empty()
        && !target.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        cmd_result(&out, &rid, &cmd, &target, false, Some("invalid_server_id".into()));
        return;
    }

    match cmd.as_str() {
        "state.snapshot" => match backend.list_servers().await {
            Ok(servers) => {
                // Include name + game_type so a relay client that doesn't
                // already know this server (e.g. the mobile app discovering
                // an agent's servers) can render a full row, not just a
                // status badge. Extra fields are ignored by older consumers.
                let arr: Vec<Value> = servers
                    .iter()
                    .map(|s| {
                        json!({
                            "id": s.id,
                            "status": s.status,
                            "container_id": s.container_id,
                            "name": s.name,
                            "game_type": s.game_type,
                        })
                    })
                    .collect();
                let _ = out.send(
                    json!({ "type": "event", "kind": "state_snapshot", "request_id": rid, "servers": arr })
                        .to_string(),
                );
            }
            Err(e) => cmd_result(&out, &rid, &cmd, &target, false, Some(e.to_string())),
        },

        "server.logs" => {
            let lines = args.get("lines").and_then(Value::as_u64).unwrap_or(200) as usize;
            match backend.get_logs(&target, lines).await {
                Ok(logs) => {
                    let _ = out.send(
                        json!({ "type": "event", "kind": "logs_snapshot", "request_id": rid, "target": target, "lines": logs })
                            .to_string(),
                    );
                }
                Err(e) => cmd_result(&out, &rid, &cmd, &target, false, Some(e.to_string())),
            }
        }

        "server.stats" => match backend.get_stats(&target).await {
            Ok(stats) => {
                let _ = out.send(
                    json!({ "type": "event", "kind": "stats_snapshot", "request_id": rid, "target": target, "stats": stats })
                        .to_string(),
                );
            }
            Err(e) => cmd_result(&out, &rid, &cmd, &target, false, Some(e.to_string())),
        },

        "server.start" => lifecycle(backend.start_server(&target).await, &out, &rid, &cmd, &target),
        "server.stop" => lifecycle(backend.stop_server(&target).await, &out, &rid, &cmd, &target),
        "server.restart" => {
            // A real restart: stop, then start (the desktop maps restart to
            // stop-only today; the agent does it properly).
            if let Err(e) = backend.stop_server(&target).await {
                cmd_result(&out, &rid, &cmd, &target, false, Some(e.to_string()));
                return;
            }
            lifecycle(backend.start_server(&target).await, &out, &rid, &cmd, &target);
        }

        "server.send_command" => {
            let command = args.get("command").and_then(Value::as_str).unwrap_or("");
            match backend.send_command(&target, command).await {
                Ok(()) => cmd_result(&out, &rid, &cmd, &target, true, None),
                Err(e) => cmd_result(&out, &rid, &cmd, &target, false, Some(e.to_string())),
            }
        }

        "server.update_config" => {
            let config: HashMap<String, String> = args
                .get("config")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            match backend.update_server_config(&target, config).await {
                Ok(_) => cmd_result(&out, &rid, &cmd, &target, true, None),
                Err(e) => cmd_result(&out, &rid, &cmd, &target, false, Some(e.to_string())),
            }
        }

        "server.delete" => match backend.delete_server(&target).await {
            Ok(()) => cmd_result(&out, &rid, &cmd, &target, true, None),
            Err(e) => cmd_result(&out, &rid, &cmd, &target, false, Some(e.to_string())),
        },

        "server.attach" => {
            let out2 = out.clone();
            let backend2 = backend.clone();
            let tgt = target.clone();
            let handle = tokio::spawn(async move {
                match backend2.stream_logs(&tgt).await {
                    Ok(mut stream) => {
                        while let Some(item) = stream.next().await {
                            match item {
                                Ok(line) => {
                                    let ts = chrono::Utc::now().timestamp_millis();
                                    let frame = json!({
                                        "type": "event", "kind": "console_line",
                                        "target": line.server_id, "line": line.line, "ts": ts,
                                    });
                                    if out2.send(frame.to_string()).is_err() {
                                        break;
                                    }
                                }
                                Err(_) => break,
                            }
                        }
                    }
                    Err(e) => tracing::warn!("[relay] attach {}: {}", tgt, e),
                }
            });
            if let Some(old) = attach.lock().await.insert(target.clone(), handle) {
                old.abort();
            }
            cmd_result(&out, &rid, &cmd, &target, true, None);
        }

        "server.detach" => {
            if let Some(h) = attach.lock().await.remove(&target) {
                h.abort();
            }
            cmd_result(&out, &rid, &cmd, &target, true, None);
        }

        // ----- backups over relay (uses this node's provisioned target) ------
        // The S3 secret never travels the relay: the desktop provisioned it to
        // this agent over direct HTTPS, and we resolve it locally here.
        "server.backups_list" => match crate::backup_target::find(&data_root, args.get("targetId").and_then(|v| v.as_str())) {
            Some(t) => match backend.list_backups(&target, &t).await {
                Ok(list) => {
                    let _ = out.send(
                        json!({ "type": "event", "kind": "backups_snapshot", "request_id": rid, "target": target, "backups": list })
                            .to_string(),
                    );
                }
                Err(e) => cmd_result(&out, &rid, &cmd, &target, false, Some(e.to_string())),
            },
            None => cmd_result(&out, &rid, &cmd, &target, false, Some(NO_TARGET.into())),
        },

        "server.backup_now" => match crate::backup_target::find(&data_root, args.get("targetId").and_then(|v| v.as_str())) {
            Some(t) => match backend.create_backup(&target, &t).await {
                Ok(_key) => cmd_result(&out, &rid, &cmd, &target, true, None),
                Err(e) => cmd_result(&out, &rid, &cmd, &target, false, Some(e.to_string())),
            },
            None => cmd_result(&out, &rid, &cmd, &target, false, Some(NO_TARGET.into())),
        },

        "server.restore_backup" => {
            let key = args.get("key").and_then(Value::as_str).unwrap_or("").to_string();
            match crate::backup_target::find(&data_root, args.get("targetId").and_then(|v| v.as_str())) {
                Some(t) => match backend.restore_backup(&target, &t, &key).await {
                    Ok(()) => cmd_result(&out, &rid, &cmd, &target, true, None),
                    Err(e) => cmd_result(&out, &rid, &cmd, &target, false, Some(e.to_string())),
                },
                None => cmd_result(&out, &rid, &cmd, &target, false, Some(NO_TARGET.into())),
            }
        }

        "server.delete_backup" => {
            let key = args.get("key").and_then(Value::as_str).unwrap_or("").to_string();
            match crate::backup_target::find(&data_root, args.get("targetId").and_then(|v| v.as_str())) {
                Some(t) => match backend.delete_backup(&target, &t, &key).await {
                    Ok(()) => cmd_result(&out, &rid, &cmd, &target, true, None),
                    Err(e) => cmd_result(&out, &rid, &cmd, &target, false, Some(e.to_string())),
                },
                None => cmd_result(&out, &rid, &cmd, &target, false, Some(NO_TARGET.into())),
            }
        }

        // ----- schedules over relay (no secret; stored host-side) ------------
        "server.schedules_list" => match backend.list_schedules(&target).await {
            Ok(list) => {
                let _ = out.send(
                    json!({ "type": "event", "kind": "schedules_snapshot", "request_id": rid, "target": target, "schedules": list })
                        .to_string(),
                );
            }
            Err(e) => cmd_result(&out, &rid, &cmd, &target, false, Some(e.to_string())),
        },

        "server.upsert_schedule" => {
            match args
                .get("schedule")
                .and_then(|v| serde_json::from_value::<Schedule>(v.clone()).ok())
            {
                Some(s) => match backend.upsert_schedule(s).await {
                    Ok(()) => cmd_result(&out, &rid, &cmd, &target, true, None),
                    Err(e) => cmd_result(&out, &rid, &cmd, &target, false, Some(e.to_string())),
                },
                None => cmd_result(&out, &rid, &cmd, &target, false, Some("invalid schedule".into())),
            }
        }

        "server.delete_schedule" => {
            let sid = args.get("schedule_id").and_then(Value::as_str).unwrap_or("");
            match backend.delete_schedule(sid).await {
                Ok(()) => cmd_result(&out, &rid, &cmd, &target, true, None),
                Err(e) => cmd_result(&out, &rid, &cmd, &target, false, Some(e.to_string())),
            }
        }

        other => cmd_result(&out, &rid, &cmd, &target, false, Some(format!("unknown_cmd:{other}"))),
    }
}

/// Shared error when a backup cmd hits a node the desktop hasn't provisioned.
const NO_TARGET: &str = "backup target not configured on this node";

/// start/stop/restart shared tail: ack + broadcast the new status.
fn lifecycle(
    res: Result<ServerStatus, BackendError>,
    out: &Outbound,
    rid: &Option<String>,
    cmd: &str,
    target: &str,
) {
    match res {
        Ok(status) => {
            cmd_result(out, rid, cmd, target, true, None);
            let _ = out.send(
                json!({ "type": "event", "kind": "server.state_changed", "target": target, "status": status })
                    .to_string(),
            );
        }
        Err(e) => cmd_result(out, rid, cmd, target, false, Some(e.to_string())),
    }
}

fn cmd_result(
    out: &Outbound,
    rid: &Option<String>,
    cmd: &str,
    target: &str,
    success: bool,
    error: Option<String>,
) {
    let _ = out.send(
        json!({
            "type": "event", "kind": "cmd_result",
            "request_id": rid, "cmd": cmd, "target": target,
            "success": success, "error": error,
        })
        .to_string(),
    );
}

/// webpki roots + ring, handed explicitly to tokio-tungstenite — the exact
/// connector the mobile relay uses (its default TLS path hung on connect).
fn relay_tls_connector() -> tokio_tungstenite::Connector {
    // Install a process-default provider if none yet (the HTTPS server may
    // have installed one already; idempotent).
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    tokio_tungstenite::Connector::Rustls(std::sync::Arc::new(config))
}
