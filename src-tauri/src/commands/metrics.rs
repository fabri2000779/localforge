//! Metrics-history Tauri command.
//!
//! Reads the host's local time-series for a server (CPU/RAM/net) since a given
//! timestamp. Routed to the active node's backend — local Docker, or an agent
//! over HTTPS. The cloud is never involved.

use crate::backend::NodeRegistry;
use crate::commands::require_backend;
use localforge_core::types::MetricPoint;
use tauri::State;

#[tauri::command(rename_all = "camelCase")]
pub async fn query_metrics(
    server_id: String,
    since_ms: i64,
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<Vec<MetricPoint>, String> {
    let backend = require_backend(&state, node_id.as_deref()).await?;
    backend
        .query_metrics(&server_id, since_ms)
        .await
        .map_err(|e| e.to_string())
}
