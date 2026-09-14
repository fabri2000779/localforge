//! Desktop OAuth glue: open the provider URL in the browser, receive the `localforge://`
//! deep link, persist the JWT and notify the React layer via `cloud://` events.

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

use super::{api_origin, auth, keychain};

use localforge_cloud_client::oauth as shared;

const REDIRECT_URI: &str = "localforge://auth/callback";

/// How long a started OAuth flow stays redeemable.
const OAUTH_STATE_TTL: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// The one pending OAuth flow (state nonce + issue time). The callback is an OS deep link any
/// web page can fire, so it must match a flow this app started (login-CSRF protection).
static PENDING_OAUTH: std::sync::Mutex<Option<(String, std::time::Instant)>> =
    std::sync::Mutex::new(None);

fn new_state_nonce() -> String {
    use base64::Engine;
    let bytes = localforge_cloud_client::vault::generate_key();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Consume the pending nonce if `state` matches and hasn't expired (single use).
fn consume_pending_state(state: Option<&str>) -> bool {
    let mut guard = PENDING_OAUTH.lock().unwrap_or_else(|e| e.into_inner());
    let Some((nonce, issued)) = guard.take() else {
        return false;
    };
    if issued.elapsed() > OAUTH_STATE_TTL {
        return false;
    }
    // Constant-time compare.
    match state {
        Some(s) if s.len() == nonce.len() => {
            s.bytes().zip(nonce.bytes()).fold(0u8, |d, (a, b)| d | (a ^ b)) == 0
        }
        _ => false,
    }
}

#[tauri::command]
pub async fn cloud_oauth_start(app: AppHandle, provider: String) -> Result<(), String> {
    let nonce = new_state_nonce();
    let redirect = format!("{REDIRECT_URI}?state={nonce}");
    let url = shared::start_url(&api_origin(), &provider, &redirect).map_err(|e| e.to_string())?;
    {
        let mut guard = PENDING_OAUTH.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some((nonce, std::time::Instant::now()));
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| format!("failed to open browser: {e}"))
}

/// Route an incoming deep link (OAuth callback or invite); unrelated URLs are ignored.
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
    // Drop callbacks that don't match a flow started here (see PENDING_OAUTH).
    let state = shared::parse_query_param(&url, "state");
    if !consume_pending_state(state.as_deref()) {
        tracing::warn!("[oauth] ignoring callback that doesn't match a sign-in started here");
        emit_error(
            &app,
            "unexpected_callback",
            "sign-in wasn't started from this app (or it expired) — please try again",
        );
        return;
    }

    let Some(token) = shared::parse_callback_token(&url) else {
        emit_error(&app, "no_token", "callback URL had no token");
        return;
    };

    if let Err(e) = keychain::save_token(&token) {
        emit_error(&app, "keychain", &e);
        return;
    }

    // Bring the window to the front (the browser may still be focused).
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_focus();
        let _ = w.unminimize();
    }

    match auth::fetch_me(&token).await {
        Ok(me) => {
            let _ = app.emit("cloud://signed-in", &me);
        }
        Err(e) => {
            // Token accepted but /me failed: keep the token, the UI can retry.
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

/// `localforge://invite?token=<id>`: surface the invite to the React layer, which asks the user.
async fn handle_invite(app: AppHandle, url: String) {
    let Some(token) = shared::parse_query_param(&url, "token") else {
        emit_error(&app, "no_invite_token", "the invite URL had no token");
        return;
    };
    // Optional handoff secret from the link #fragment (or `&k=` from a web bridge).
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

/// Value of `key` in a URL `#fragment` (fragments never reach a server, hence the handoff secret).
fn parse_fragment_param(url: &str, key: &str) -> Option<String> {
    let frag = url.split_once('#')?.1;
    frag.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| v.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arm(nonce: &str) {
        *PENDING_OAUTH.lock().unwrap() = Some((nonce.to_string(), std::time::Instant::now()));
    }

    #[test]
    fn callback_requires_a_pending_matching_state() {
        // Nothing pending → refused (the drive-by case).
        *PENDING_OAUTH.lock().unwrap() = None;
        assert!(!consume_pending_state(Some("abc")));

        // Pending but mismatched → refused, and the slot is consumed.
        arm("expected");
        assert!(!consume_pending_state(Some("other")));
        assert!(!consume_pending_state(Some("expected")));

        // Pending + matching → accepted exactly once.
        arm("expected");
        assert!(consume_pending_state(Some("expected")));
        assert!(!consume_pending_state(Some("expected")));

        // Missing state → refused.
        arm("expected");
        assert!(!consume_pending_state(None));
    }

    #[test]
    fn expired_state_is_refused() {
        *PENDING_OAUTH.lock().unwrap() = Some((
            "n".to_string(),
            std::time::Instant::now() - OAUTH_STATE_TTL - std::time::Duration::from_secs(1),
        ));
        assert!(!consume_pending_state(Some("n")));
    }

    #[test]
    fn state_rides_through_the_redirect_uri() {
        // The cloud appends `token` to our redirect_to; the nonce must round-trip.
        let cb = format!("{REDIRECT_URI}?state=abc123&token=jwt.here");
        assert_eq!(shared::parse_query_param(&cb, "state").as_deref(), Some("abc123"));
        assert_eq!(shared::parse_callback_token(&cb).as_deref(), Some("jwt.here"));
    }
}
