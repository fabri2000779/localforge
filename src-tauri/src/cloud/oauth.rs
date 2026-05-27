//! Desktop OAuth glue.
//!
//! URL building + token parsing live in `localforge-cloud-client::oauth`
//! (so the mobile companion reuses the exact same shape). What stays
//! here is the desktop-specific wiring: opening the system browser via
//! `tauri_plugin_opener`, receiving deep-links via the single-instance
//! / deep-link plugins, persisting the JWT to the OS keychain, and
//! emitting events the React layer listens for.
//!
//! Flow:
//!   1. UI calls `cloud_oauth_start("discord" | "google" | "github")`.
//!   2. We open `${API_ORIGIN}/v1/auth/<provider>/start?redirect_to=localforge://auth/callback`
//!      in the user's default browser via tauri-plugin-opener.
//!   3. They sign in / authorise.
//!   4. The cloud API 302s the browser to `localforge://auth/callback?token=<jwt>`.
//!   5. The OS hands that URL to whichever process registered the
//!      `localforge` scheme — that's us. The deep-link plugin fires
//!      a tauri event we subscribe to in `main.rs::setup`.
//!   6. handle_auth_callback stashes the JWT, fetches /me, and emits
//!      `cloud://signed-in` for the React layer.
//!
//! Errors land on a `cloud://auth-error` event with `{code, message}`.

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

use super::{api_origin, auth, keychain};

use localforge_cloud_client::oauth as shared;

const REDIRECT_URI: &str = "localforge://auth/callback";

#[tauri::command]
pub async fn cloud_oauth_start(app: AppHandle, provider: String) -> Result<(), String> {
    let url = shared::start_url(&api_origin(), &provider, REDIRECT_URI)
        .map_err(|e| e.to_string())?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| format!("failed to open browser: {e}"))
}

/// Process an incoming deep-link URL. Called from `main.rs::setup` via
/// the `tauri-plugin-deep-link` `on_open_url` callback. Tolerates noise
/// (URLs for unrelated schemes/paths) by ignoring them silently.
pub async fn handle_deep_link(app: AppHandle, url: String) {
    // OAuth callback path — auth flow.
    if url.starts_with("localforge://auth/callback") {
        handle_auth_callback(app, url).await;
        return;
    }
    // Invite acceptance path — user clicked an invite link.
    if url.starts_with("localforge://invite") {
        handle_invite(app, url).await;
    }
}

async fn handle_auth_callback(app: AppHandle, url: String) {
    let Some(token) = shared::parse_callback_token(&url) else {
        emit_error(&app, "no_token", "callback URL had no token");
        return;
    };

    if let Err(e) = keychain::save_token(&token) {
        emit_error(&app, "keychain", &e);
        return;
    }

    // Bring the LocalForge window to the front so the user sees the
    // result. On macOS in particular, the browser might still be focused.
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_focus();
        let _ = w.unminimize();
    }

    match auth::fetch_me(&token).await {
        Ok(me) => {
            let _ = app.emit("cloud://signed-in", &me);
        }
        Err(e) => {
            // Token was accepted but /me failed — keep the token, the UI
            // can retry. Surface as a soft warning.
            tracing::warn!("oauth callback /me failed: {:?}", e);
            let _ = app.emit("cloud://signed-in-partial", &serde_json::Value::Null);
        }
    }
}

fn emit_error(app: &AppHandle, code: &str, message: &str) {
    let _ = app.emit(
        "cloud://auth-error",
        serde_json::json!({ "code": code, "message": message }),
    );
}

/// localforge://invite?token=<id> — fired when the user clicks an
/// invitation email link. We surface the token to the frontend as an
/// `cloud://invite-received` event; the React layer pops a dialog
/// asking the user whether to accept (they need to be logged in
/// already; if not, the dialog prompts them to sign in first).
async fn handle_invite(app: AppHandle, url: String) {
    let Some(token) = shared::parse_query_param(&url, "token") else {
        emit_error(&app, "no_invite_token", "the invite URL had no token");
        return;
    };
    // Optional handoff secret. It rides in the link's #fragment (`#k=…`, which
    // never reaches a server); a web bridge that turns the HTTPS link into the
    // localforge:// deep link may instead pass it as a `&k=` query. Accept
    // either. A plain invite has none → the member falls back to the owner's
    // background grant.
    let secret = shared::parse_query_param(&url, "k").or_else(|| parse_fragment_param(&url, "k"));
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_focus();
        let _ = w.unminimize();
    }
    let _ = app.emit(
        "cloud://invite-received",
        serde_json::json!({ "token": token, "secret": secret }),
    );
}

/// Pull `key` out of a URL `#fragment` (`…#k=value&other=…`). Fragments are
/// client-side only and never transmitted, which is exactly why the invite
/// handoff secret travels there.
fn parse_fragment_param(url: &str, key: &str) -> Option<String> {
    let frag = url.split_once('#')?.1;
    frag.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| v.to_string())
    })
}
