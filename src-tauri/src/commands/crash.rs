//! Crash-journal reader for the desktop's own servers (remote agents keep their own journal).

use crate::paths;
use localforge_core::types::CrashEvent;

#[tauri::command(rename_all = "camelCase")]
pub async fn query_crash_events(
    server_id: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<CrashEvent>, String> {
    Ok(localforge_backend_local::query_crash_events(&paths::home_root(), server_id, limit.unwrap_or(100)).await)
}
