//! Desktop audit-log glue.
//!
//! The POST logic lives in `localforge-cloud-client::audit`. What
//! stays here is the desktop's fire-and-forget pattern — every UI
//! action spawns a Tokio task so the caller doesn't pay any cost on
//! the request path. Mobile will do something similar with its own
//! spawn primitive.

use super::{api, auth};
use serde::{Deserialize, Serialize};

// ── Activity feed (read) ───────────────────────────────────────────────────

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
    /// Rowid tiebreaker for the composite cursor — batch-inserted audit rows
    /// share a created_at ms, and paging on the timestamp alone dropped the
    /// boundary-ms siblings (audit finding).
    #[serde(default)]
    pub next_before_id: Option<i64>,
}

/// Read the active org's activity feed (Team plan, admin+; cloud enforces).
/// `(before, before_id)` is the composite cursor for "load older" — pass both
/// values from the previous page's `nextBefore`/`nextBeforeId`; omit for the
/// latest page.
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

/// Best-effort audit emit. Spawns the HTTP call so callers don't pay
/// any cost on the request path.
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

/// IPC-facing audit emit. The frontend passes the action as a String,
/// we map it to one of the recognised static strings; anything else
/// is logged at debug and dropped (matches the cloud's drop-unknown
/// policy so the wire contract stays in sync).
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
