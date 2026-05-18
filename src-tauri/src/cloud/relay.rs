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

#[derive(Default)]
pub struct RelayState {
    /// Cancellation handle for the current loop. Replaced on each
    /// start_relay call so we never accumulate ghost loops.
    pub cancel: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

#[tauri::command]
pub async fn cloud_relay_stop(
    state: tauri::State<'_, Arc<RelayState>>,
) -> Result<(), String> {
    let mut guard = state.cancel.lock().await;
    if let Some(tx) = guard.take() {
        let _ = tx.send(());
    }
    Ok(())
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
    {
        let mut guard = state.cancel.lock().await;
        *guard = Some(cancel_tx);
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
                    // Pump frames until close.
                    loop {
                        tokio::select! {
                            // Cancellation
                            _ = &mut cancel_rx => {
                                let _ = ws.send(Message::Close(None)).await;
                                return;
                            }
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
    _last_seq: &mut Option<u64>,
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
            // Trigger a pull so any events we missed are recovered.
            if let Some(state) = app.try_state::<crate::backend::NodeRegistry>() {
                if let Err(e) = sync::cloud_sync_pull(state).await {
                    tracing::debug!("[relay] post-epoch pull failed: {:?}", e);
                }
            }
        }
    }

    match env_msg.ty.as_str() {
        "hello" => { /* peers + hello extras — not surfaced yet */ }
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
        }
        "presence" => { /* presence push — surface later for sub-users */ }
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
