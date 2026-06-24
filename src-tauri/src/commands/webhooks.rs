//! Webhook-alert config commands (host-local).
//!
//! These manage the desktop host's own `webhooks.json` — the endpoints its
//! crash-watcher POSTs to when one of its servers crashes. The full URL stays
//! Rust-side; the frontend only ever sees a redacted [`WebhookConfigView`].
//! (Alerting for servers on a remote agent uses that agent's own config —
//! provisioning it from here is a follow-up, mirroring backup targets.)

use crate::paths;
use localforge_backend_local::webhooks;
use localforge_core::types::{WebhookConfig, WebhookConfigView, WebhookKind};
use uuid::Uuid;

#[tauri::command(rename_all = "camelCase")]
pub async fn list_webhooks() -> Result<Vec<WebhookConfigView>, String> {
    Ok(webhooks::load(&paths::home_root()).iter().map(|h| h.view()).collect())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn add_webhook(
    name: String,
    kind: WebhookKind,
    url: String,
) -> Result<WebhookConfigView, String> {
    let hook = WebhookConfig {
        id: Uuid::new_v4().to_string(),
        name,
        kind,
        url,
        enabled: true,
    };
    let view = hook.view();
    webhooks::upsert(&paths::home_root(), hook).map_err(|e| e.to_string())?;
    Ok(view)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn remove_webhook(id: String) -> Result<(), String> {
    webhooks::remove(&paths::home_root(), &id).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_webhook_enabled(id: String, enabled: bool) -> Result<(), String> {
    webhooks::set_enabled(&paths::home_root(), &id, enabled).map_err(|e| e.to_string())
}

/// Send a test alert to a saved webhook so the user can confirm it works.
#[tauri::command(rename_all = "camelCase")]
pub async fn test_webhook(id: String) -> Result<(), String> {
    let hook = webhooks::load(&paths::home_root())
        .into_iter()
        .find(|h| h.id == id)
        .ok_or_else(|| "webhook not found".to_string())?;
    webhooks::test(&hook.url, hook.kind).await
}
