//! Cron schedules persisted in `<data_root>/schedules.json` and fired by one per-process
//! loop through the `NodeBackend` trait (desktop while open, agent 24/7).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Local, TimeZone};
use croner::parser::{CronParser, Seconds, Year};
use localforge_core::backend::NodeBackend;
use localforge_core::types::{BackupTarget, Schedule, ScheduleAction};

/// Resolves a backup `target_id` (None = default) to credentials; injected per host because
/// the desktop keeps targets in the keychain and the agent in a file.
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

/// Create or replace a schedule; a replace keeps `last_run` so an edit doesn't re-fire.
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

/// Remove every schedule of a deleted server.
pub fn delete_for_server(data_root: &Path, server_id: &str) -> std::io::Result<()> {
    let mut list = load(data_root);
    let before = list.len();
    list.retain(|s| s.server_id != server_id);
    if list.len() == before {
        return Ok(());
    }
    save(data_root, &list)
}

fn ms_to_local(ms: i64) -> Option<DateTime<Local>> {
    Local.timestamp_millis_opt(ms).single()
}

/// Strict 5-field cron parser (croner 3+ would otherwise accept 6/7-field patterns with a shifted meaning).
fn cron_parser() -> CronParser {
    CronParser::builder()
        .seconds(Seconds::Disallowed)
        .year(Year::Disallowed)
        .build()
}

/// Next fire time strictly after `from`, per the cron expression (local time).
fn next_fire(expr: &str, from: &DateTime<Local>) -> Option<DateTime<Local>> {
    cron_parser()
        .parse(expr)
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

/// Prune after an upload: the `keep_last` newest are never deleted; `max_age_days` prunes
/// older objects beyond them. Best-effort.
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
    entries.sort_by_key(|e| std::cmp::Reverse(e.created_at));
    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut deleted = 0u32;
    for (i, e) in entries.iter().enumerate() {
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

/// Spawn the scheduler loop once per process: every 30 s, fire enabled schedules whose next
/// occurrence after their last run (or the loop start) has passed.
pub fn spawn_scheduler(
    backend: Arc<dyn NodeBackend>,
    data_root: PathBuf,
    resolve_target: BackupTargetResolver,
) {
    if SCHEDULER_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    // The metrics sampler shares the host's background-loop lifecycle.
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
                    // Re-load and patch only this schedule: run_action can take minutes, and saving the
                    // pre-tick snapshot would resurrect deleted schedules and clobber edits.
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Timelike};

    #[test]
    fn five_field_patterns_parse_and_advance() {
        let from = Local.with_ymd_and_hms(2026, 3, 10, 12, 3, 7).single().unwrap();
        let next = next_fire("*/5 * * * *", &from).unwrap();
        assert_eq!((next.hour(), next.minute(), next.second()), (12, 5, 0));
        assert_eq!(next.day(), 10);
        // "0 4 * * *" from 12:03 → tomorrow 04:00.
        let next = next_fire("0 4 * * *", &from).unwrap();
        assert_eq!((next.day(), next.hour(), next.minute()), (11, 4, 0));
    }

    #[test]
    fn next_fire_is_strictly_after_from() {
        // Exactly on a boundary → the NEXT boundary, never the same instant.
        let from = Local.with_ymd_and_hms(2026, 3, 10, 12, 5, 0).single().unwrap();
        let next = next_fire("*/5 * * * *", &from).unwrap();
        assert_eq!(next.minute(), 10);
    }

    #[test]
    fn six_and_seven_field_patterns_are_rejected() {
        let from = Local.with_ymd_and_hms(2026, 3, 10, 12, 3, 7).single().unwrap();
        assert!(next_fire("0 */5 * * * *", &from).is_none());
        assert!(next_fire("0 0 4 * * * 2026", &from).is_none());
        assert!(next_fire("not a cron", &from).is_none());
        assert!(next_fire("", &from).is_none());
    }

    #[test]
    fn dom_dow_keep_or_semantics() {
        // Standard cron: DOM OR DOW. 2026-03-10 is a Tuesday; the next Monday is 03-16.
        let from = Local.with_ymd_and_hms(2026, 3, 10, 12, 0, 0).single().unwrap();
        let next = next_fire("0 0 1 * 1", &from).unwrap();
        assert_eq!((next.month(), next.day()), (3, 16));
    }
}
