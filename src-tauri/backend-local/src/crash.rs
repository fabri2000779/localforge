//! Host-side crash watcher: compares the persisted `Running` intent with the container's
//! actual state, journals unexpected exits, and auto-restarts per [`RestartPolicy`] with
//! crash-loop backoff.

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
/// More than this many crashes within the window stops auto-restart until it clears.
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

/// Crash events newest first, optionally filtered to one server; bad lines are skipped.
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
        // The first tick recovers servers that went down while nobody was watching (reboot,
        // app restart) without logging them as crashes.
        let mut first_tick = true;
        // 0 so the first tick prunes immediately; a desktop rarely stays up 24 h.
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
        // Only servers we BELIEVE are up; every other persisted state is intended or handled.
        if s.status != ServerStatus::Running || s.container_id.is_none() {
            continue;
        }
        // Skip on a transient query error so a Docker hiccup can't trigger a false restart.
        let actual = match backend.server_status(&s.id).await {
            Ok(st) => st,
            Err(_) => continue,
        };
        if actual == ServerStatus::Running || actual == ServerStatus::Starting {
            continue;
        }

        // Re-check the persisted intent right before acting: a user stop may have landed meanwhile.
        match crate::persistence::load_server(data_root, &s.id) {
            Ok(cur) if cur.status == ServerStatus::Running => {}
            _ => continue,
        }

        // Exit happened while we weren't watching: recover quietly per policy, no journal or alerts.
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

        tracing::warn!("[crash-watcher] {} ({}) exited unexpectedly", s.name, s.id);
        let log_tail = backend.get_logs(&s.id, LOG_TAIL).await.unwrap_or_default();

        // Reflect reality so the UI updates and the next tick doesn't re-handle it.
        mark_status(data_root, &s.id, ServerStatus::Crashed);
        let crashed = CrashEvent {
            ts: now,
            server_id: s.id.clone(),
            server_name: s.name.clone(),
            kind: CrashEventKind::Crashed,
            log_tail,
        };
        append_event(data_root, &crashed);
        dispatch(data_root, &crashed);

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
                dispatch(data_root, &restarted);
            }
            Err(e) => tracing::warn!("[crash-watcher] failed to restart {}: {e}", s.name),
        }
    }
}

/// Fire webhooks on their own task so the watch loop never blocks.
fn dispatch(data_root: &Path, ev: &CrashEvent) {
    let data_root = data_root.to_path_buf();
    let ev = ev.clone();
    tokio::spawn(async move {
        crate::webhooks::dispatch_event(&data_root, &ev).await;
    });
}

/// Flip a server's persisted status, leaving other fields untouched.
fn mark_status(data_root: &Path, server_id: &str, status: ServerStatus) {
    if let Ok(mut server) = crate::persistence::load_server(data_root, server_id) {
        server.status = status;
        let _ = crate::persistence::save_server(data_root, &server);
    }
}

/// Rewrite the journal keeping entries newer than `cutoff` (temp + rename).
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
