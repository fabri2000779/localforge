//! OAuth via system browser.
//!
//! Flow:
//!   1. UI calls `cloud_oauth_start("discord" | "google" | "github")`.
//!   2. We open `${API_ORIGIN}/v1/auth/<provider>/start?redirect_to=localforge://auth/callback`
//!      in the user's default browser via tauri-plugin-shell.
//!   3. They sign in / authorise.
//!   4. The cloud API 302s the browser to `localforge://auth/callback?token=<jwt>`.
//!   5. The OS hands that URL to whichever process registered the
//!      `localforge` scheme — that's us. The deep-link plugin fires
//!      a tauri event we already subscribe to in `lib.rs::setup`.
//!   6. The handler parses the URL, stashes the JWT, fetches /me, and
//!      emits a `cloud://signed-in` event the React layer listens to.
//!
//! Errors land on a `cloud://auth-error` event with `{code, message}`.

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_shell::ShellExt;

use super::{api_origin, auth, keychain};

const REDIRECT_URI: &str = "localforge://auth/callback";

#[tauri::command]
pub async fn cloud_oauth_start(app: AppHandle, provider: String) -> Result<(), String> {
    if !matches!(provider.as_str(), "discord" | "google" | "github") {
        return Err(format!("unknown provider {provider}"));
    }
    // urlencode the redirect_to so providers don't fail on the colon
    let encoded = urlencode(REDIRECT_URI);
    let url = format!(
        "{}/v1/auth/{}/start?redirect_to={}",
        api_origin(),
        provider,
        encoded
    );
    app.shell()
        .open(url, None)
        .map_err(|e| format!("failed to open browser: {e}"))
}

/// Process an incoming deep-link URL. Called from `lib.rs::setup` via the
/// `tauri-plugin-deep-link` `on_open_url` callback. Tolerates noise (URLs
/// for unrelated schemes/paths) by ignoring them silently.
pub async fn handle_deep_link(app: AppHandle, url: String) {
    // OAuth callback path — auth flow.
    if url.starts_with("localforge://auth/callback") {
        handle_auth_callback(app, url).await;
        return;
    }
    // Invite acceptance path — user clicked an invite link.
    if url.starts_with("localforge://invite") {
        handle_invite(app, url).await;
        return;
    }
}

async fn handle_auth_callback(app: AppHandle, url: String) {

    // Minimal parse — the URL is fixed-shape so we don't need a real
    // URL crate. Anything we don't recognise → emit a bad_url error.
    let token = url
        .split_once('?')
        .and_then(|(_, q)| {
            q.split('&').find_map(|pair| {
                let (k, v) = pair.split_once('=')?;
                (k == "token").then(|| urldecode(v))
            })
        });

    let Some(token) = token else {
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
    let token = url
        .split_once('?')
        .and_then(|(_, q)| {
            q.split('&').find_map(|pair| {
                let (k, v) = pair.split_once('=')?;
                (k == "token").then(|| urldecode(v))
            })
        });
    if let Some(t) = token {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.set_focus();
            let _ = w.unminimize();
        }
        let _ = app.emit(
            "cloud://invite-received",
            serde_json::json!({ "token": t }),
        );
    } else {
        emit_error(&app, "no_invite_token", "the invite URL had no token");
    }
}

/// Minimal URL-component encoder — we only encode a tiny fixed URL, so
/// pulling `urlencoding` as a dep would be overkill.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{:02X}", b));
            }
        }
    }
    out
}

/// Symmetric of urlencode for the small handful of percent-escapes
/// providers might add (the JWT itself is base64url so it shouldn't have
/// any, but the API uses `URLSearchParams` which percent-encodes `+`/
/// `=` paddings in `?token=…`).
fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            ) {
                out.push(((h << 4) | l) as u8);
                i += 3;
                continue;
            }
        } else if b == b'+' {
            out.push(b' ');
            i += 1;
            continue;
        }
        out.push(b);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
