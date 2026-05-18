//! Audit log producer for the desktop.
//!
//! Hits POST /v1/audit (the cloud has the queue + consumer wired). One
//! call per user-initiated action — start, stop, restart, send console
//! command, file write, etc. Free users still call this — the cloud
//! drops their entries server-side.
//!
//! ALL calls are fire-and-forget. Audit is observational, not
//! authoritative — if the network is dead it's fine to lose a record.

use serde::Serialize;

use super::{api, auth};
use crate::backend::NodeRegistry;
use localforge_core::NodeId;

#[derive(Debug, Serialize)]
struct AuditPayload<'a> {
    action: &'a str,
    target: Option<&'a str>,
    metadata: Option<serde_json::Value>,
}

/// Best-effort audit emit. Spawns the HTTP call so callers don't pay
/// any cost on the request path.
pub fn emit(
    app: &tauri::AppHandle,
    action: &'static str,
    target: Option<String>,
    metadata: Option<serde_json::Value>,
) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = emit_inner(&app, action, target.as_deref(), metadata).await {
            tracing::debug!("[audit] emit({}) failed: {:?}", action, e);
        }
    });
}

async fn emit_inner(
    _app: &tauri::AppHandle,
    action: &'static str,
    target: Option<&str>,
    metadata: Option<serde_json::Value>,
) -> Result<(), api::ApiError> {
    let Some(token) = auth::current_token() else { return Ok(()) };
    // The cloud will need an org id eventually — for now the API
    // route picks the caller's primary org. When we surface a switcher
    // in the UI we'll pass it explicitly.
    let _: serde_json::Value = api::post(
        "/v1/audit",
        &AuditPayload {
            action,
            target,
            metadata,
        },
        Some(&token),
    )
    .await?;
    Ok(())
}

/// Helper a few future callers will want: pull the server's name for
/// the `target` field so audit entries read like "server.start: My
/// Minecraft". Kept here but unused today — wire it in when the audit
/// log gets a viewer page.
#[allow(dead_code)]
pub async fn server_target(
    state: &tauri::State<'_, NodeRegistry>,
    server_id: &str,
) -> Option<String> {
    let backend = state.backend(&NodeId::local()).await?;
    backend
        .list_servers()
        .await
        .ok()?
        .into_iter()
        .find(|s| s.id == server_id)
        .map(|s| format!("{} ({})", s.name, server_id))
}

// Re-export for callers that have `&AppHandle`.
#[tauri::command]
pub async fn cloud_audit_emit(
    app: tauri::AppHandle,
    action: String,
    target: Option<String>,
    metadata: Option<serde_json::Value>,
) -> Result<(), String> {
    // We keep `action` typed as String for the IPC contract, but the
    // queue is fine with arbitrary strings — the cloud picks a stable
    // set, anything else is ignored.
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
    let _ = app;
    emit(&app.clone(), action_static, target, metadata);
    Ok(())
}
