//! Relay WebSocket client: connects to the org's relay DO with jittered backoff, forwards
//! frames to the React layer as `cloud://relay-*` events, and detects epoch/seq gaps.

use futures_util::{SinkExt, StreamExt};
use rand::RngExt;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;

use super::{api, auth};

/// Every server message; we route on `kind`.
#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(rename = "type")]
    ty: String,
    #[serde(default)]
    epoch: Option<String>,
    #[serde(default)]
    seq: Option<u64>,
    #[serde(default)]
    kind: Option<String>,
}

/// Outbound queue drained into the WS while connected.
type CmdQueue = Mutex<Option<tokio::sync::mpsc::UnboundedSender<String>>>;

#[derive(Default)]
pub struct RelayState {
    /// Cancellation handle for the current loop (replaced on each start).
    pub cancel: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    pub outbound: CmdQueue,
}

#[tauri::command]
pub async fn cloud_relay_stop(
    state: tauri::State<'_, Arc<RelayState>>,
) -> Result<(), String> {
    let mut guard = state.cancel.lock().await;
    if let Some(tx) = guard.take() {
        let _ = tx.send(());
    }
    let mut out_guard = state.outbound.lock().await;
    out_guard.take();
    Ok(())
}

/// Send a `cmd` frame (full message body from the React layer) to the relay.
#[tauri::command]
pub async fn cloud_relay_send_cmd(
    state: tauri::State<'_, Arc<RelayState>>,
    payload: serde_json::Value,
) -> Result<(), String> {
    let guard = state.outbound.lock().await;
    let Some(tx) = guard.as_ref() else {
        return Err("relay not connected".into());
    };
    let text = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
    tx.send(text).map_err(|_| "relay channel closed".to_string())
}

/// Owner side: broadcast an `event` frame to connected members.
#[tauri::command]
pub async fn cloud_relay_send_event(
    state: tauri::State<'_, Arc<RelayState>>,
    payload: serde_json::Value,
) -> Result<(), String> {
    let guard = state.outbound.lock().await;
    let Some(tx) = guard.as_ref() else {
        return Err("relay not connected".into());
    };
    let mut p = payload;
    if let serde_json::Value::Object(ref mut m) = p {
        m.insert("type".into(), serde_json::Value::String("event".into()));
    }
    let text = serde_json::to_string(&p).map_err(|e| e.to_string())?;
    tx.send(text).map_err(|_| "relay channel closed".to_string())
}

/// Resolve the user's primary org id by hitting /v1/orgs/me.
async fn fetch_org_id(token: &str) -> Result<String, api::ApiError> {
    #[derive(Deserialize)]
    struct OrgMe { id: String }
    let r: OrgMe = api::get("/v1/orgs/me", Some(token)).await?;
    Ok(r.id)
}

/// Start (or restart) the relay loop for `org_id` (the ACTIVE org, so a sub-user joins the
/// owner's DO); defaults to the caller's primary org.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_relay_start(
    app: AppHandle,
    state: tauri::State<'_, Arc<RelayState>>,
    org_id: Option<String>,
) -> Result<(), String> {
    {
        let mut guard = state.cancel.lock().await;
        if let Some(tx) = guard.take() {
            let _ = tx.send(());
        }
    }

    let token = match auth::current_token() {
        Some(t) => t,
        None => return Err("unauthenticated".into()),
    };
    let org_id = match org_id {
        Some(id) if !id.is_empty() => id,
        _ => fetch_org_id(&token)
            .await
            .map_err(|e| format!("fetch org: {e}"))?,
    };

    let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel::<()>();
    let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    {
        let mut guard = state.cancel.lock().await;
        *guard = Some(cancel_tx);
        let mut out_guard = state.outbound.lock().await;
        *out_guard = Some(out_tx);
    }

    let app_for_loop = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut backoff = Backoff::new();
        let mut last_epoch: Option<String> = None;
        let mut last_seq: Option<u64> = None;

        loop {
            if let Ok(()) = cancel_rx.try_recv() {
                return;
            }

            let token = match auth::current_token() {
                Some(t) => t,
                None => {
                    tracing::info!("[relay] no token; bailing out");
                    return;
                }
            };
            // Assert this machine's device id so targeted commands reach this exact desktop; re-read
            // per connect so a late local node is picked up.
            let device_id = app_for_loop
                .state::<crate::backend::NodeRegistry>()
                .this_machine()
                .await
                .map(|m| m.id);
            let mut url = format!(
                "wss://{}/v1/relay/{}?token={}",
                localforge_cloud_client::relay::ws_host(&super::api_origin()),
                org_id,
                urlencoded(&token),
            );
            if let Some(did) = &device_id {
                url.push_str("&device_id=");
                url.push_str(&urlencoded(did));
            }

            // Never log `url`: it carries the JWT.
            tracing::debug!(
                "[relay] connecting to wss://{}/v1/relay/{}",
                localforge_cloud_client::relay::ws_host(&super::api_origin()),
                org_id,
            );
            match tokio_tungstenite::connect_async(&url).await {
                Ok((mut ws, _)) => {
                    backoff.reset();
                    let _ = app_for_loop.emit("cloud://relay-connected", ());
                    // App-level keepalive: Cloudflare drops idle sockets and the DO expects a client ping.
                    let mut ping = tokio::time::interval(Duration::from_secs(30));
                    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    ping.tick().await; // consume the immediate first tick
                    loop {
                        tokio::select! {
                            biased;
                            _ = &mut cancel_rx => {
                                let _ = ws.send(Message::Close(None)).await;
                                return;
                            }
                            outbound = out_rx.recv() => match outbound {
                                Some(text) => {
                                    if ws.send(Message::Text(text.into())).await.is_err() {
                                        tracing::warn!("[relay] send failed; reconnecting");
                                        break;
                                    }
                                }
                                None => return,    // channel closed
                            },
                            frame = ws.next() => match frame {
                                Some(Ok(Message::Text(txt))) => {
                                    handle_text(
                                        &app_for_loop,
                                        &txt,
                                        &mut last_epoch,
                                        &mut last_seq,
                                    ).await;
                                }
                                Some(Ok(Message::Ping(p))) => {
                                    let _ = ws.send(Message::Pong(p)).await;
                                }
                                Some(Ok(_)) => {/* ignore binary / pong */}
                                Some(Err(e)) => {
                                    tracing::warn!("[relay] frame err: {}", e);
                                    break;
                                }
                                None => break,
                            },
                            _ = ping.tick() => {
                                if ws.send(Message::Text("{\"type\":\"ping\"}".to_string().into())).await.is_err() {
                                    tracing::warn!("[relay] keepalive send failed; reconnecting");
                                    break;
                                }
                            }
                        }
                    }
                    let _ = app_for_loop.emit("cloud://relay-disconnected", ());
                    // Drop commands queued during the dead connection so stale cmds don't replay on reconnect.
                    while out_rx.try_recv().is_ok() {}
                }
                Err(e) => {
                    tracing::warn!("[relay] connect failed: {}", e);
                }
            }

            // Cancellable backoff wait so stop/restart is honoured promptly.
            let delay = backoff.next();
            tokio::select! {
                biased;
                _ = &mut cancel_rx => return,
                _ = tokio::time::sleep(delay) => {}
            }
        }
    });

    Ok(())
}

async fn handle_text(
    app: &AppHandle,
    txt: &str,
    last_epoch: &mut Option<String>,
    last_seq: &mut Option<u64>,
) {
    let env_msg: Envelope = match serde_json::from_str(txt) {
        Ok(e) => e,
        Err(_) => return,
    };

    // Detect DO restarts (or first connection) — fetch state on epoch change.
    if let Some(ep) = &env_msg.epoch {
        let changed = last_epoch.as_ref().is_some_and(|prev| prev != ep);
        let first = last_epoch.is_none();
        *last_epoch = Some(ep.clone());
        if first || changed {
            // Reset the seq tracker too — a new epoch starts at 1.
            *last_seq = None;
            // Recover missed events: the JS store owns the re-pull.
            let _ = app.emit("cloud://sync-changed", ());
        }
    }

    // Gap detection within an epoch: anything beyond +1 means a lost frame.
    if let Some(seq) = env_msg.seq {
        if let Some(prev) = last_seq {
            if seq > *prev + 1 {
                tracing::warn!("[relay] seq gap: {} → {} (missed {})", prev, seq, seq - *prev - 1);
                let _ = app.emit("cloud://sync-changed", ());
            }
        }
        *last_seq = Some(seq);
    }

    match env_msg.ty.as_str() {
        "hello" => {
            // Forward raw so the UI sees session/epoch/peers.
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(txt) {
                let _ = app.emit("cloud://relay-hello", &value);
            }
        }
        "event" => {
            if env_msg.kind.as_deref() == Some("sync_changed")
                || env_msg.kind.as_deref() == Some("sync_deleted")
            {
                // The JS store owns the re-pull.
                let _ = app.emit("cloud://sync-changed", ());
            }
            // Forward raw so the React layer can match on `kind`.
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(txt) {
                let _ = app.emit("cloud://relay-event", &value);
            }
        }
        "cmd" => {
            // Owner side: the React executor maps the cmd to a local invoke and replies with an event.
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(txt) {
                let _ = app.emit("cloud://relay-cmd", &value);
            }
        }
        "error" => {
            // Server-side rejections (forbidden, unknown_cmd, …).
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(txt) {
                let _ = app.emit("cloud://relay-error", &value);
            }
        }
        "presence" => {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(txt) {
                let _ = app.emit("cloud://relay-presence", &value);
            }
        }
        _ => {}
    }
}

fn urlencoded(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

// Backoff

struct Backoff {
    attempt: u32,
}

impl Backoff {
    fn new() -> Self {
        Self { attempt: 0 }
    }
    fn reset(&mut self) {
        self.attempt = 0;
    }
    fn next(&mut self) -> Duration {
        // 250ms → 500ms → 1s → 2s → 4s → 8s → 16s → cap 30s, plus ±20% jitter.
        let base_ms: u64 = match self.attempt {
            0 => 250,
            1 => 500,
            2 => 1_000,
            3 => 2_000,
            4 => 4_000,
            5 => 8_000,
            6 => 16_000,
            _ => 30_000,
        };
        self.attempt = (self.attempt + 1).min(7);
        let jitter: f64 = rand::rng().random_range(-0.2..0.2);
        let ms = (base_ms as f64 * (1.0 + jitter)).max(100.0);
        Duration::from_millis(ms as u64)
    }
}
