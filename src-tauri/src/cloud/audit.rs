//! Desktop audit-log glue.
//!
//! The POST logic lives in `localforge-cloud-client::audit`. What
//! stays here is the desktop's fire-and-forget pattern — every UI
//! action spawns a Tokio task so the caller doesn't pay any cost on
//! the request path. Mobile will do something similar with its own
//! spawn primitive.

use super::{api, auth};

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
