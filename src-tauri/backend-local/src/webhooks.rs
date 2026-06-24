//! Outbound event alerts: host-side webhooks (Discord / Slack / generic).
//!
//! Config lives host-side in `<data_root>/webhooks.json`, and the host POSTs to
//! the endpoints DIRECTLY when an event fires (currently container crashes,
//! from the crash-watcher). The URL — which for Discord/Slack embeds a secret
//! token — therefore never leaves the machine, let alone transits the cloud.
//! Every send is best-effort: a failure is logged, never fatal.

use std::path::{Path, PathBuf};
use std::time::Duration;

use localforge_core::types::{CrashEvent, CrashEventKind, WebhookConfig, WebhookKind};
use serde_json::json;

fn store_path(data_root: &Path) -> PathBuf {
    data_root.join("webhooks.json")
}

pub fn load(data_root: &Path) -> Vec<WebhookConfig> {
    match std::fs::read_to_string(store_path(data_root)) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn save(data_root: &Path, list: &[WebhookConfig]) -> std::io::Result<()> {
    let p = store_path(data_root);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(p, serde_json::to_string_pretty(list)?)
}

pub fn upsert(data_root: &Path, hook: WebhookConfig) -> std::io::Result<()> {
    let mut list = load(data_root);
    if let Some(slot) = list.iter_mut().find(|h| h.id == hook.id) {
        *slot = hook;
    } else {
        list.push(hook);
    }
    save(data_root, &list)
}

pub fn remove(data_root: &Path, id: &str) -> std::io::Result<()> {
    let mut list = load(data_root);
    list.retain(|h| h.id != id);
    save(data_root, &list)
}

pub fn set_enabled(data_root: &Path, id: &str, enabled: bool) -> std::io::Result<()> {
    let mut list = load(data_root);
    if let Some(h) = list.iter_mut().find(|h| h.id == id) {
        h.enabled = enabled;
    }
    save(data_root, &list)
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("LocalForge")
        // Hand reqwest an explicit rustls config (it's built with
        // `rustls-no-provider`) so it doesn't fall back to the platform
        // verifier — same approach as the cloud-client/backend-remote crates.
        .use_preconfigured_tls(webpki_tls_config())
        .build()
        .expect("reqwest client build")
}

/// rustls config trusting the bundled Mozilla webpki roots. `rustls-no-provider`
/// means no process-default CryptoProvider is auto-installed, so we install ring
/// here (idempotent — harmless if the host app already did).
fn webpki_tls_config() -> rustls::ClientConfig {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth()
}

/// The human-facing line for a crash event.
fn message_for(ev: &CrashEvent) -> String {
    match ev.kind {
        CrashEventKind::Crashed => format!("🔴 **{}** crashed unexpectedly.", ev.server_name),
        CrashEventKind::Restarted => {
            format!("🟢 **{}** was auto-restarted after a crash.", ev.server_name)
        }
        CrashEventKind::Backoff => format!(
            "⛔ **{}** has crashed repeatedly — auto-restart paused. Check the server.",
            ev.server_name
        ),
    }
}

/// Per-kind JSON body for a crash event.
fn payload_for(kind: WebhookKind, msg: &str, ev: &CrashEvent) -> serde_json::Value {
    match kind {
        WebhookKind::Discord => {
            let mut content = msg.to_string();
            if !ev.log_tail.is_empty() {
                let tail: String = ev.log_tail.join("\n").chars().take(1500).collect();
                content.push_str(&format!("\n```\n{tail}\n```"));
            }
            json!({ "username": "LocalForge", "content": content })
        }
        WebhookKind::Slack => json!({ "text": msg }),
        WebhookKind::Generic => json!({ "message": msg, "event": ev }),
    }
}

/// POST `ev` to every enabled webhook (best-effort). Called from the watcher.
pub async fn dispatch_event(data_root: &Path, ev: &CrashEvent) {
    let hooks: Vec<WebhookConfig> = load(data_root).into_iter().filter(|h| h.enabled).collect();
    if hooks.is_empty() {
        return;
    }
    let msg = message_for(ev);
    let client = client();
    for h in hooks {
        let body = payload_for(h.kind, &msg, ev);
        let res = client
            .post(&h.url)
            .json(&body)
            .send()
            .await
            .and_then(|r| r.error_for_status());
        if let Err(e) = res {
            tracing::warn!("[webhooks] POST to '{}' failed: {e}", h.name);
        }
    }
}

/// Send a test message to verify a webhook (the UI "Test" button).
pub async fn test(url: &str, kind: WebhookKind) -> Result<(), String> {
    let msg = "✅ LocalForge test alert — webhooks are working.";
    let body = match kind {
        WebhookKind::Discord => json!({ "username": "LocalForge", "content": msg }),
        WebhookKind::Slack => json!({ "text": msg }),
        WebhookKind::Generic => json!({ "message": msg, "test": true }),
    };
    client()
        .post(url)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    Ok(())
}
