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
use localforge_core::types::{BackupTarget, Schedule, ScheduleAction};

/// Resolves a backup `target_id` (None = the first/default target) to its S3
/// credentials. Injected per host at [`spawn_scheduler`] time because the
/// desktop keeps targets in the OS keychain while the headless agent keeps them
/// in a file under its data root — `backend-local` can reach neither layer
/// directly, so each host hands in its own lookup closure.
pub type BackupTargetResolver =
    Arc<dyn Fn(Option<&str>) -> Option<BackupTarget> + Send + Sync>;

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

async fn run_action(backend: &dyn NodeBackend, s: &Schedule, resolve_target: &BackupTargetResolver) {
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
        ScheduleAction::Backup {
            target_id,
            keep_last,
            max_age_days,
        } => match resolve_target(target_id.as_deref()) {
            Some(target) => match backend.create_backup(&s.server_id, &target).await {
                Ok(key) => {
                    tracing::info!("[scheduler] backup of {} uploaded: {key}", s.server_id);
                    if keep_last.is_some() || max_age_days.is_some() {
                        if let Err(e) =
                            enforce_retention(backend, &target, &s.server_id, *keep_last, *max_age_days)
                                .await
                        {
                            tracing::warn!(
                                "[scheduler] retention prune for {} failed: {e}",
                                s.server_id
                            );
                        }
                    }
                }
                Err(e) => tracing::warn!("[scheduler] backup of {} failed: {e}", s.server_id),
            },
            None => tracing::warn!(
                "[scheduler] backup schedule {} references target {:?} which is not \
                 configured on this host — skipping",
                s.id,
                target_id
            ),
        },
    }
}

/// Prune a server's backup objects after a scheduled upload. `keep_last` is a
/// floor (the N most recent are never deleted); `max_age_days` deletes anything
/// older than that. With both set, the N newest are kept and older-than-max
/// beyond them are pruned. Best-effort: a failed delete is logged, not fatal.
async fn enforce_retention(
    backend: &dyn NodeBackend,
    target: &BackupTarget,
    server_id: &str,
    keep_last: Option<u32>,
    max_age_days: Option<u32>,
) -> Result<(), String> {
    let mut entries = backend
        .list_backups(server_id, target)
        .await
        .map_err(|e| e.to_string())?;
    // Sort newest-first ourselves rather than trusting the listing order.
    entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut deleted = 0u32;
    for (i, e) in entries.iter().enumerate() {
        // keep_last is a floor: never delete the N most recent.
        if let Some(n) = keep_last {
            if i < n as usize {
                continue;
            }
        }
        let too_old = max_age_days
            .map(|d| now_ms - e.created_at > (d as i64) * 86_400_000)
            .unwrap_or(false);
        let delete = match (keep_last, max_age_days) {
            (Some(_), Some(_)) => too_old, // beyond the floor AND older than max age
            (Some(_), None) => true,       // beyond the floor, no age limit → prune
            (None, Some(_)) => too_old,    // age limit only
            (None, None) => false,         // no policy → keep everything
        };
        if delete {
            match backend.delete_backup(server_id, target, &e.key).await {
                Ok(()) => deleted += 1,
                Err(err) => tracing::warn!("[scheduler] failed to prune backup {}: {err}", e.key),
            }
        }
    }
    if deleted > 0 {
        tracing::info!("[scheduler] retention pruned {deleted} old backup(s) for {server_id}");
    }
    Ok(())
}

static SCHEDULER_STARTED: AtomicBool = AtomicBool::new(false);

/// Spawn the host scheduler loop ONCE per process (extra calls are no-ops).
/// Ticks every 30s and fires any enabled schedule whose next cron occurrence
/// — after its last run, or the loop's start for a never-run schedule — has
/// passed. A schedule that came due while the host was off fires once on the
/// next tick after startup.
pub fn spawn_scheduler(
    backend: Arc<dyn NodeBackend>,
    data_root: PathBuf,
    resolve_target: BackupTargetResolver,
) {
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
            let list = load(&data_root);
            for s in list.iter() {
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
                    run_action(&*backend, s, &resolve_target).await;
                    // Re-load and patch ONLY this schedule's last_run.
                    // run_action can await for minutes (backups) — saving the
                    // whole pre-tick snapshot afterwards resurrected schedules
                    // the user deleted meanwhile and clobbered concurrent
                    // edits (audit finding). A schedule deleted mid-run simply
                    // isn't patched.
                    let mut fresh = load(&data_root);
                    if let Some(cur) = fresh.iter_mut().find(|x| x.id == s.id) {
                        cur.last_run = Some(now.timestamp_millis());
                        if let Err(e) = save(&data_root, &fresh) {
                            tracing::warn!("[scheduler] persist failed: {e}");
                        }
                    }
                }
            }
        }
    });
}
