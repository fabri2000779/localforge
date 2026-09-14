//! Schedule CRUD commands, routed to the active node's backend.

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
    // Set on the relay path: verify the schedule belongs to this (scope-checked) server before deleting.
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
