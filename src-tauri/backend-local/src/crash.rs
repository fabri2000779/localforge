//! Host-side crash watcher + smart restart.
//!
//! Lives in `backend-local`, so it watches Docker containers on whichever host
//! a server actually runs on — the desktop while open, or the headless agent
//! 24/7. Each tick it compares the *intended* state (the persisted
//! `ServerStatus::Running`) with the container's *actual* state. A server we
//! believe is running whose container has exited is an UNEXPECTED exit — a
//! crash — as opposed to a user-initiated Stop (which persists `Stopped`, and
//! whose graceful-shutdown window is covered by `stop_server` persisting
//! `Stopping` first).
//!
//! On a crash it: marks the server `Crashed` (so the UI stops showing a stale
//! "Running"), appends a metadata-only entry to the crash journal (the event
//! source the cloud alerting + mobile push build on), and — per the server's
//! [`RestartPolicy`] — auto-restarts it, with crash-loop backoff so a server
//! that keeps dying can't spin forever.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use localforge_core::backend::NodeBackend;
use localforge_core::types::{CrashEvent, CrashEventKind, RestartPolicy, ServerStatus};

const WATCH_SECS: u64 = 20;
const STARTUP_DELAY_SECS: u64 = 20;
const LOG_TAIL: usize = 20;
/// Crash-loop backoff: more than this many crashes within the window and we
/// stop auto-restarting until the window clears.
const BACKOFF_MAX: usize = 5;
const BACKOFF_WINDOW_MS: i64 = 10 * 60 * 1000; // 10 minutes
const EVENT_RETENTION_MS: i64 = 60 * 24 * 60 * 60 * 1000; // 60 days

fn journal_file(data_root: &Path) -> PathBuf {
    data_root.join("crash-events.jsonl")
}

/// Append one crash-journal entry (best-effort, single-line append).
fn append_event(data_root: &Path, ev: &CrashEvent) {
    let path = journal_file(data_root);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(line) = serde_json::to_string(ev) {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// Read crash events (newest first), optionally filtered to one server, capped
/// at `limit`. Torn/unparseable lines are skipped.
pub async fn query_crash_events(
    data_root: &Path,
    server_id: Option<String>,
    limit: usize,
) -> Vec<CrashEvent> {
    let path = journal_file(data_root);
    tokio::task::spawn_blocking(move || {
        let file = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };
        let mut out: Vec<CrashEvent> = Vec::new();
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(ev) = serde_json::from_str::<CrashEvent>(line) {
                if server_id.as_deref().is_none_or(|id| ev.server_id == id) {
                    out.push(ev);
                }
            }
        }
        out.reverse(); // newest first
        out.truncate(limit);
        out
    })
    .await
    .unwrap_or_default()
}

static WATCHER_STARTED: AtomicBool = AtomicBool::new(false);

/// Spawn the per-process crash watcher (idempotent — extra calls are no-ops).
pub fn spawn_crash_watcher(backend: Arc<dyn NodeBackend>, data_root: PathBuf) {
    if WATCHER_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(STARTUP_DELAY_SECS)).await;
        // server_id -> recent crash timestamps (for crash-loop backoff).
        let mut recent: HashMap<String, Vec<i64>> = HashMap::new();
        // The first tick recovers servers that went down while this process
        // wasn't watching (host reboot, app restart) WITHOUT logging them as
        // crashes — otherwise, now that Docker no longer auto-restarts (see
        // manager.rs), every persisted-Running server would fire a false crash
        // alert on every reboot (audit finding follow-up).
        let mut first_tick = true;
        // 0 ⇒ the first tick prunes immediately. Seeding "now" meant a prune
        // only ever ran after 24h of CONTINUOUS process uptime — which a
        // desktop app never reaches, so the journal grew forever (audit
        // finding). Long-lived agents keep the 24h cadence after the first.
        let mut last_prune = 0i64;
        loop {
            watch_once(&*backend, &data_root, &mut recent, first_tick).await;
            first_tick = false;
            let now = Utc::now().timestamp_millis();
            if now - last_prune > 24 * 60 * 60 * 1000 {
                prune_journal(&data_root, now - EVENT_RETENTION_MS);
                last_prune = now;
            }
            tokio::time::sleep(Duration::from_secs(WATCH_SECS)).await;
        }
    });
}

async fn watch_once(
    backend: &dyn NodeBackend,
    data_root: &Path,
    recent: &mut HashMap<String, Vec<i64>>,
    first_tick: bool,
) {
    let servers = match backend.list_servers().await {
        Ok(s) => s,
        Err(_) => return,
    };
    let now = Utc::now().timestamp_millis();
    for s in servers {
        // Only watch servers we BELIEVE are up. Any other persisted state
        // (Stopped/Stopping/Starting/Installing/Error/Crashed) is either
        // intended or already handled.
        if s.status != ServerStatus::Running || s.container_id.is_none() {
            continue;
        }
        // What's the container actually doing? Skip on a transient query error
        // so a Docker hiccup can never trigger a false restart.
        let actual = match backend.server_status(&s.id).await {
            Ok(st) => st,
            Err(_) => continue,
        };
        if actual == ServerStatus::Running || actual == ServerStatus::Starting {
            continue; // still up (or coming up) — all good
        }

        // Re-check the PERSISTED intent right before acting: a user stop can
        // land while we're querying Docker (stop_server persists Stopping
        // before its graceful console-stop window) — that exit is intended,
        // not a crash (audit finding: the watcher auto-restarted servers the
        // user was stopping).
        match crate::persistence::load_server(data_root, &s.id) {
            Ok(cur) if cur.status == ServerStatus::Running => {}
            _ => continue,
        }

        // First tick after (re)start: the exit happened while we weren't
        // watching (reboot / app restart), not an observed crash. Recover
        // quietly per policy — no journal entry, webhook, push, or backoff.
        if first_tick {
            if s.restart_policy == RestartPolicy::Off {
                mark_status(data_root, &s.id, ServerStatus::Stopped);
            } else {
                match backend.start_server(&s.id).await {
                    Ok(_) => tracing::info!("[crash-watcher] recovered {} after downtime", s.name),
                    Err(e) => {
                        tracing::warn!("[crash-watcher] failed to recover {}: {e}", s.name);
                        mark_status(data_root, &s.id, ServerStatus::Stopped);
                    }
                }
            }
            continue;
        }

        // Unexpected exit: the container is gone but we thought it was running.
        tracing::warn!("[crash-watcher] {} ({}) exited unexpectedly", s.name, s.id);
        let log_tail = backend.get_logs(&s.id, LOG_TAIL).await.unwrap_or_default();

        // Reflect reality so the UI stops showing "Running" and we don't
        // re-handle it next tick (list_servers will then return Crashed).
        mark_status(data_root, &s.id, ServerStatus::Crashed);
        let crashed = CrashEvent {
            ts: now,
            server_id: s.id.clone(),
            server_name: s.name.clone(),
            kind: CrashEventKind::Crashed,
            log_tail,
        };
        append_event(data_root, &crashed);
        dispatch(data_root, &crashed); // fire webhooks (best-effort, off-thread)

        if s.restart_policy == RestartPolicy::Off {
            continue;
        }

        // Crash-loop backoff: count crashes within the rolling window.
        let hits = recent.entry(s.id.clone()).or_default();
        hits.retain(|&t| now - t < BACKOFF_WINDOW_MS);
        hits.push(now);
        if hits.len() > BACKOFF_MAX {
            tracing::warn!(
                "[crash-watcher] {} crashed {} times within the window — backing off",
                s.name,
                hits.len()
            );
            let backoff = CrashEvent {
                ts: now,
                server_id: s.id.clone(),
                server_name: s.name.clone(),
                kind: CrashEventKind::Backoff,
                log_tail: Vec::new(),
            };
            append_event(data_root, &backoff);
            dispatch(data_root, &backoff);
            continue; // leave it Crashed until the user intervenes
        }

        // Auto-restart per policy. `start_server` persists Running on success.
        match backend.start_server(&s.id).await {
            Ok(_) => {
                tracing::info!("[crash-watcher] auto-restarted {}", s.name);
                let restarted = CrashEvent {
                    ts: Utc::now().timestamp_millis(),
                    server_id: s.id.clone(),
                    server_name: s.name.clone(),
                    kind: CrashEventKind::Restarted,
                    log_tail: Vec::new(),
                };
                append_event(data_root, &restarted);
                // Recovery notice → webhooks too (it was journaled but never
                // dispatched, leaving webhooks::message_for's Restarted arm
                // dead; audit finding).
                dispatch(data_root, &restarted);
            }
            Err(e) => tracing::warn!("[crash-watcher] failed to restart {}: {e}", s.name),
        }
    }
}

/// Fire configured webhooks for an event without blocking the watch loop —
/// the POST runs on its own task (best-effort; failures are logged inside).
fn dispatch(data_root: &Path, ev: &CrashEvent) {
    let data_root = data_root.to_path_buf();
    let ev = ev.clone();
    tokio::spawn(async move {
        crate::webhooks::dispatch_event(&data_root, &ev).await;
    });
}

/// Flip a server's persisted status (load+save so other fields are untouched).
/// Host-only: the watcher only ever runs against the local backend.
fn mark_status(data_root: &Path, server_id: &str, status: ServerStatus) {
    if let Ok(mut server) = crate::persistence::load_server(data_root, server_id) {
        server.status = status;
        let _ = crate::persistence::save_server(data_root, &server);
    }
}

/// Rewrite the journal keeping only entries newer than `cutoff` (temp + rename
/// so a crash mid-prune can't lose the file).
fn prune_journal(data_root: &Path, cutoff: i64) {
    let path = journal_file(data_root);
    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(_) => return,
    };
    let mut kept = String::new();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if let Ok(ev) = serde_json::from_str::<CrashEvent>(line.trim()) {
            if ev.ts >= cutoff {
                kept.push_str(&line);
                kept.push('\n');
            }
        }
    }
    let mut tmp = path.clone().into_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    if let Ok(mut f) = std::fs::File::create(&tmp) {
        if f.write_all(kept.as_bytes()).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        } else {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}
