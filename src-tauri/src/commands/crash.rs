//! Crash-history Tauri command — reads the host's local crash journal.
//!
//! The crash-watcher (in `backend-local`) records unexpected container exits to
//! a local JSONL journal. This surfaces them for the desktop's own servers.
//! Servers on a remote agent keep their journal on that agent; querying those
//! would need a relay/HTTP route, which isn't wired yet.

use crate::paths;
use localforge_core::types::CrashEvent;

#[tauri::command(rename_all = "camelCase")]
pub async fn query_crash_events(
    server_id: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<CrashEvent>, String> {
    Ok(localforge_backend_local::query_crash_events(&paths::home_root(), server_id, limit.unwrap_or(100)).await)
}
