//! Desktop audit-log glue: fire-and-forget emits plus the activity-feed reader.

use super::{api, auth};
use serde::{Deserialize, Serialize};

// Activity feed (read)

/// One row of the org's activity feed, as the cloud returns it (camelCase).
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    #[serde(default)]
    pub actor_user_id: Option<String>,
    pub actor_name: String,
    pub action: String,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    pub created_at: i64,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditList {
    pub entries: Vec<AuditEntry>,
    #[serde(default)]
    pub next_before: Option<i64>,
    /// Rowid tiebreaker for the composite cursor (batch inserts share a created_at ms).
    #[serde(default)]
    pub next_before_id: Option<i64>,
}

/// The active org's activity feed (Team, admin+). `(before, before_id)` is the "load older" cursor.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_audit_list(
    before: Option<i64>,
    before_id: Option<i64>,
    limit: Option<u32>,
) -> Result<AuditList, String> {
    let token = auth::current_token().ok_or("not signed in")?;
    let mut path = format!("/v1/audit?limit={}", limit.unwrap_or(50));
    if let Some(b) = before {
        path.push_str(&format!("&before={b}"));
        if let Some(bid) = before_id {
            path.push_str(&format!("&beforeId={bid}"));
        }
    }
    api::get::<AuditList>(&path, Some(&token))
        .await
        .map_err(|e| e.to_string())
}

/// Best-effort emit on a spawned task.
pub fn emit(
    app: &tauri::AppHandle,
    action: &'static str,
    target: Option<String>,
    metadata: Option<serde_json::Value>,
) {
    let _ = app; // present so the desktop's command signature stays stable
    tauri::async_runtime::spawn(async move {
        if let Err(e) = emit_inner(action, target.as_deref(), metadata).await {
            tracing::debug!("[audit] emit({}) failed: {:?}", action, e);
        }
    });
}

async fn emit_inner(
    action: &'static str,
    target: Option<&str>,
    metadata: Option<serde_json::Value>,
) -> Result<(), api::ApiError> {
    let Some(token) = auth::current_token() else { return Ok(()) };
    localforge_cloud_client::audit::emit(action, target, metadata, &token).await
}

/// IPC emit; unknown actions are dropped, matching the cloud's policy.
#[tauri::command]
pub async fn cloud_audit_emit(
    app: tauri::AppHandle,
    action: String,
    target: Option<String>,
    metadata: Option<serde_json::Value>,
) -> Result<(), String> {
    let action_static: &'static str = match action.as_str() {
        "server.start" => "server.start",
        "server.stop" => "server.stop",
        "server.restart" => "server.restart",
        "server.delete" => "server.delete",
        "server.send_command" => "server.send_command",
        "server.update_config" => "server.update_config",
        other => {
            tracing::debug!("[audit] unrecognised action {}", other);
            return Ok(());
        }
    };
    emit(&app, action_static, target, metadata);
    Ok(())
}
