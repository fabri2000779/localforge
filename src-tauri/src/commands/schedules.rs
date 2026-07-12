//! Scheduled-action Tauri commands.
//!
//! CRUD over the host's persisted schedule defs (the scheduler loop in
//! `backend-local` fires them automatically). Routed to the active node's
//! backend, so they manage schedules on a local server OR an agent-hosted one.

use crate::backend::NodeRegistry;
use crate::commands::require_backend;
use localforge_core::types::Schedule;
use tauri::State;

#[tauri::command(rename_all = "camelCase")]
pub async fn list_schedules(
    server_id: String,
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<Vec<Schedule>, String> {
    let backend = require_backend(&state, node_id.as_deref()).await?;
    backend
        .list_schedules(&server_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn upsert_schedule(
    schedule: Schedule,
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<(), String> {
    let backend = require_backend(&state, node_id.as_deref()).await?;
    backend
        .upsert_schedule(schedule)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_schedule(
    id: String,
    // The server the caller claims the schedule belongs to. Set on the relay
    // path (= the scope-checked `target`). When present we verify the schedule
    // actually belongs to it before deleting — otherwise a member scoped to
    // server-A could delete server-B's schedules by passing an in-scope target
    // with an out-of-scope schedule id (audit finding; the relay scope-checks
    // the target but the delete keyed off the id alone). Omitted for the owner's
    // own local UI, which isn't scope-restricted.
    server_id: Option<String>,
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<(), String> {
    let backend = require_backend(&state, node_id.as_deref()).await?;
    if let Some(server_id) = server_id.as_deref() {
        let schedules = backend
            .list_schedules(server_id)
            .await
            .map_err(|e| e.to_string())?;
        if !schedules.iter().any(|s| s.id == id) {
            return Err("schedule does not belong to the target server".to_string());
        }
    }
    backend.delete_schedule(&id).await.map_err(|e| e.to_string())
}
