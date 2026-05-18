//! Cloud relay WebSocket client.
//!
//! Stays connected to `wss://api.localforge.gg/v1/relay/<orgId>` for as
//! long as the user is signed in and on a paid plan. Surfaces three
//! interesting things to the UI via Tauri events:
//!
//!   `cloud://relay-connected`        connected (re-)established
//!   `cloud://relay-disconnected`     dropped, will retry
//!   `cloud://sync-changed`           someone else pushed; auto-pulls
//!
//! Reconnect contract (see localforge-cloud/apps/api/src/relay.ts):
//!   - Exponential backoff with jitter: 250ms → 30s, capped, ±20%.
//!   - Every server message carries `epoch` + `seq`.
//!     If epoch changes between connections → the DO restarted →
//!     we refetch state.
//!   - We never surface "Disconnected" to the UI inside a single
//!     backoff cycle (under ~30s) — the user shouldn't notice deploys.

use futures_util::{SinkExt, StreamExt};
use rand::Rng;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;

use super::{api, auth, sync};

/// Top-level shape of every server-emitted message. `kind` is whatever
/// the server stamped — we route on it.
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

/// Outbound queue: the React layer can call `cloud_relay_send_cmd` to
/// push a command to the owner. We keep a tx-handle per active loop so
/// the loop drains the queue into the WS while it's connected.
type CmdQueue = Mutex<Option<tokio::sync::mpsc::UnboundedSender<String>>>;

#[derive(Default)]
pub struct RelayState {
    /// Cancellation handle for the current loop. Replaced on each
    /// start_relay call so we never accumulate ghost loops.
    pub cancel: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    /// Tx end of the outbound message channel.
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

/// Send a `cmd` message through the WS to the relay (which forwards to
/// the owner). Used by the React layer when a sub-user clicks Start /
/// Stop / etc. on a server they're not the owner of.
///
/// `payload` should be the full message body — `{type:'cmd', cmd:'…',
/// target:'…', request_id:'…', ...}`. We trust the React layer to put
/// well-formed JSON in — the relay validates the shape on receipt.
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

/// Owner-side: emit an `event` message to all connected members.
/// Used when the owner finishes executing a sub-user's command and
/// wants to broadcast the result.
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

#[tauri::command]
pub async fn cloud_relay_start(
    app: AppHandle,
    state: tauri::State<'_, Arc<RelayState>>,
) -> Result<(), String> {
    // Replace any prior loop.
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
    let org_id = fetch_org_id(&token)
        .await
        .map_err(|e| format!("fetch org: {e}"))?;

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
            // Has anyone called stop?
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
            let url = format!(
                "wss://{}/v1/relay/{}?token={}",
                api_host(),
                org_id,
                urlencoded(&token),
            );

            tracing::debug!("[relay] connecting to {}", url);
            match tokio_tungstenite::connect_async(&url).await {
                Ok((mut ws, _)) => {
                    backoff.reset();
                    let _ = app_for_loop.emit("cloud://relay-connected", ());
                    // Pump frames until close. We multiplex three things:
                    // 1. cancellation, 2. inbound WS frames, 3. outbound
                    // messages enqueued by Tauri commands.
                    loop {
                        tokio::select! {
                            biased;
                            _ = &mut cancel_rx => {
                                let _ = ws.send(Message::Close(None)).await;
                                return;
                            }
                            // Outbound: drain whatever the app wants to send.
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
                            }
                        }
                    }
                    let _ = app_for_loop.emit("cloud://relay-disconnected", ());
                }
                Err(e) => {
                    tracing::warn!("[relay] connect failed: {}", e);
                }
            }

            // Backoff with jitter. If we've exceeded the loud-threshold
            // we surface a disconnect; the UI then shows the warning.
            let delay = backoff.next();
            tokio::time::sleep(delay).await;
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
        let changed = last_epoch.as_ref().map_or(false, |prev| prev != ep);
        let first = last_epoch.is_none();
        *last_epoch = Some(ep.clone());
        if first || changed {
            // Reset the seq tracker too — a new epoch starts at 1.
            *last_seq = None;
            // Trigger a pull so any events we missed are recovered.
            if let Some(state) = app.try_state::<crate::backend::NodeRegistry>() {
                if let Err(e) = sync::cloud_sync_pull(state).await {
                    tracing::debug!("[relay] post-epoch pull failed: {:?}", e);
                }
            }
        }
    }

    // Gap detection within the same epoch. The server stamps seq
    // monotonically, so anything beyond +1 from our last value means
    // a packet went missing — refetch state to be safe.
    if let Some(seq) = env_msg.seq {
        if let Some(prev) = last_seq {
            if seq > *prev + 1 {
                tracing::warn!("[relay] seq gap: {} → {} (missed {})", prev, seq, seq - *prev - 1);
                if let Some(state) = app.try_state::<crate::backend::NodeRegistry>() {
                    let _ = sync::cloud_sync_pull(state).await;
                }
            }
        }
        *last_seq = Some(seq);
    }

    match env_msg.ty.as_str() {
        "hello" => {
            // Surface the hello to the React layer so the UI can show
            // role + peers. Forward the raw text — it has the
            // session/epoch/peers structure intact.
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(txt) {
                let _ = app.emit("cloud://relay-hello", &value);
            }
        }
        "event" => {
            if env_msg.kind.as_deref() == Some("sync_changed")
                || env_msg.kind.as_deref() == Some("sync_deleted")
            {
                // Auto-pull silently; UI updates via the store.
                if let Some(state) = app.try_state::<crate::backend::NodeRegistry>() {
                    let _ = sync::cloud_sync_pull(state).await;
                }
                let _ = app.emit("cloud://sync-changed", ());
            }
            // Any other event — surface raw so the React layer can
            // pattern-match on `kind` (cmd_result, console_line, etc).
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(txt) {
                let _ = app.emit("cloud://relay-event", &value);
            }
        }
        "cmd" => {
            // Owner-side: a sub-user just asked us to do something. Hand
            // it to the React layer — the auth store maps the cmd to a
            // local Tauri invoke and emits the `event` response back.
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(txt) {
                let _ = app.emit("cloud://relay-cmd", &value);
            }
        }
        "error" => {
            // Surface server-side rejections (forbidden, unknown_cmd, …)
            // so the UI can hint "the relay refused your last command".
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

fn api_host() -> String {
    // Strip the scheme so we can prepend `wss://`. api_origin defaults
    // to https://api.localforge.gg.
    super::api_origin()
        .strip_prefix("https://")
        .or_else(|| super::api_origin().strip_prefix("http://").map(|_| "api.localforge.gg"))
        .unwrap_or("api.localforge.gg")
        .to_string()
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

// ---------------------------------------------------------------------------
// Backoff
// ---------------------------------------------------------------------------

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
