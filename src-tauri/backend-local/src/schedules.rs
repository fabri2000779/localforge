//! Scheduled actions.
//!
//! Schedules are persisted host-side (`<data_root>/schedules.json`) and fired
//! by a single per-process scheduler loop. Because the loop lives in
//! `backend-local`, it runs both on the desktop (while the app is open) and on
//! the headless agent (24/7) — wherever the server actually lives. Actions are
//! dispatched through the public `NodeBackend` trait so they reuse the exact
//! lifecycle logic (no duplicated start/stop/send paths).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Local, TimeZone};
use croner::Cron;
use localforge_core::backend::NodeBackend;
use localforge_core::types::{Schedule, ScheduleAction};

fn schedules_file(data_root: &Path) -> PathBuf {
    data_root.join("schedules.json")
}

pub fn load(data_root: &Path) -> Vec<Schedule> {
    match std::fs::read_to_string(schedules_file(data_root)) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn save(data_root: &Path, list: &[Schedule]) -> std::io::Result<()> {
    let path = schedules_file(data_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(list)?)
}

pub fn list_for(data_root: &Path, server_id: &str) -> Vec<Schedule> {
    load(data_root)
        .into_iter()
        .filter(|s| s.server_id == server_id)
        .collect()
}

/// Create or replace a schedule (matched by id). A replace preserves the
/// existing `last_run` so editing a schedule doesn't make it re-fire.
pub fn upsert(data_root: &Path, mut schedule: Schedule) -> std::io::Result<()> {
    let mut list = load(data_root);
    if let Some(slot) = list.iter_mut().find(|s| s.id == schedule.id) {
        if schedule.last_run.is_none() {
            schedule.last_run = slot.last_run;
        }
        *slot = schedule;
    } else {
        list.push(schedule);
    }
    save(data_root, &list)
}

pub fn delete(data_root: &Path, id: &str) -> std::io::Result<()> {
    let mut list = load(data_root);
    list.retain(|s| s.id != id);
    save(data_root, &list)
}

/// Remove every schedule belonging to a server. Called when the server is
/// deleted so its cron entries don't linger (and never fire against a gone id).
pub fn delete_for_server(data_root: &Path, server_id: &str) -> std::io::Result<()> {
    let mut list = load(data_root);
    let before = list.len();
    list.retain(|s| s.server_id != server_id);
    if list.len() == before {
        return Ok(()); // nothing belonged to this server — don't rewrite
    }
    save(data_root, &list)
}

fn ms_to_local(ms: i64) -> Option<DateTime<Local>> {
    Local.timestamp_millis_opt(ms).single()
}

/// Next fire time strictly after `from`, per the cron expression (local time).
fn next_fire(expr: &str, from: &DateTime<Local>) -> Option<DateTime<Local>> {
    Cron::new(expr)
        .parse()
        .ok()?
        .find_next_occurrence(from, false)
        .ok()
}

async fn run_action(backend: &dyn NodeBackend, s: &Schedule) {
    match &s.action {
        ScheduleAction::Restart => {
            let _ = backend.stop_server(&s.server_id).await;
            let _ = backend.start_server(&s.server_id).await;
        }
        ScheduleAction::Command { command } => {
            let _ = backend.send_command(&s.server_id, command).await;
        }
        ScheduleAction::Broadcast { message } => {
            // `say` is Minecraft's broadcast; other games vary (best-effort).
            let _ = backend
                .send_command(&s.server_id, &format!("say {message}"))
                .await;
        }
    }
}

static SCHEDULER_STARTED: AtomicBool = AtomicBool::new(false);

/// Spawn the host scheduler loop ONCE per process (extra calls are no-ops).
/// Ticks every 30s and fires any enabled schedule whose next cron occurrence
/// — after its last run, or the loop's start for a never-run schedule — has
/// passed. A schedule that came due while the host was off fires once on the
/// next tick after startup.
pub fn spawn_scheduler(backend: Arc<dyn NodeBackend>, data_root: PathBuf) {
    if SCHEDULER_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    // Start the metrics sampler alongside the scheduler — both are the host's
    // background loops and share the same lifecycle/spawn sites.
    crate::metrics::spawn_sampler(backend.clone(), data_root.clone());
    tokio::spawn(async move {
        let start = Local::now();
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            let now = Local::now();
            let mut list = load(&data_root);
            let mut changed = false;
            for s in list.iter_mut() {
                if !s.enabled {
                    continue;
                }
                let baseline = s.last_run.and_then(ms_to_local).unwrap_or(start);
                let Some(next) = next_fire(&s.cron, &baseline) else {
                    // A schedule with an unparseable cron expression will never
                    // fire. Log a warning so the operator can spot-fix it.
                    tracing::warn!(
                        "[scheduler] schedule {} has unparseable cron {:?} — skipping",
                        s.id, s.cron,
                    );
                    continue;
                };
                if next <= now {
                    tracing::info!(
                        "[scheduler] firing {:?} for server {}",
                        s.action,
                        s.server_id
                    );
                    run_action(&*backend, s).await;
                    s.last_run = Some(now.timestamp_millis());
                    changed = true;
                }
            }
            if changed {
                if let Err(e) = save(&data_root, &list) {
                    tracing::warn!("[scheduler] persist failed: {e}");
                }
            }
        }
    });
}
