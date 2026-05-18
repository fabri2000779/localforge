//! Auth commands invoked from the React layer:
//!
//!   cloud_signup(email, password, displayName?) -> Me
//!   cloud_login(email, password)                -> Me
//!   cloud_logout()                              -> ()
//!   cloud_me()                                  -> Option<Me>      (None if not signed in)
//!   cloud_request_reset(email)                  -> ()
//!   cloud_resend_verification()                 -> ()
//!
//! All three "produce a Me" commands also stash the JWT in the OS
//! keychain. `cloud_me()` is the one the frontend calls at startup to
//! re-hydrate state from the stored token (no token → returns None,
//! and the UI shows the "Sign in" affordance).
use serde::{Deserialize, Serialize};

use super::{api, keychain};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub plan: String, // 'free' | 'hobby' | 'team'
    #[serde(rename = "currentPeriodEnd")]
    pub current_period_end: Option<i64>,
    #[serde(rename = "cancelAtPeriodEnd")]
    pub cancel_at_period_end: bool,
    #[serde(rename = "trialEndsAt")]
    pub trial_ends_at: Option<i64>,
    /// Unix ms when cloud-side data will be hard-deleted (set when the
    /// user dropped to free; null while on a paid plan). Optional in
    /// the API surface — older clients haven't seen it yet.
    #[serde(rename = "purgeAt", default)]
    pub purge_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Me {
    pub id: String,
    pub email: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(rename = "emailVerifiedAt")]
    pub email_verified_at: Option<i64>,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    pub subscription: Subscription,
}

#[derive(Debug, Deserialize)]
struct AuthResponse {
    token: String,
}

#[derive(Debug, Serialize)]
struct SignupBody<'a> {
    email: &'a str,
    password: &'a str,
    #[serde(rename = "displayName", skip_serializing_if = "Option::is_none")]
    display_name: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct LoginBody<'a> {
    email: &'a str,
    password: &'a str,
}

// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn cloud_signup(
    email: String,
    password: String,
    display_name: Option<String>,
) -> Result<Me, api::ApiError> {
    let r: AuthResponse = api::post(
        "/v1/auth/signup",
        &SignupBody {
            email: &email,
            password: &password,
            display_name: display_name.as_deref(),
        },
        None,
    )
    .await?;
    keychain::save_token(&r.token).map_err(|e| api::ApiError::Decode(format!("keychain: {e}")))?;
    fetch_me(&r.token).await
}

#[tauri::command]
pub async fn cloud_login(email: String, password: String) -> Result<Me, api::ApiError> {
    let r: AuthResponse = api::post(
        "/v1/auth/login",
        &LoginBody {
            email: &email,
            password: &password,
        },
        None,
    )
    .await?;
    keychain::save_token(&r.token).map_err(|e| api::ApiError::Decode(format!("keychain: {e}")))?;
    fetch_me(&r.token).await
}

#[tauri::command]
pub async fn cloud_logout() -> Result<(), api::ApiError> {
    // Tell the API to revoke the session so other devices syncing from
    // it stop working immediately. Fire-and-forget — if it fails (offline,
    // etc.) we still clear the local copy.
    if let Some(t) = keychain::load_token() {
        let _ = api::post::<_, serde_json::Value>("/v1/auth/logout", &serde_json::json!({}), Some(&t)).await;
    }
    keychain::clear_token().map_err(|e| api::ApiError::Decode(format!("keychain: {e}")))?;
    Ok(())
}

#[tauri::command]
pub async fn cloud_me() -> Result<Option<Me>, api::ApiError> {
    let Some(t) = keychain::load_token() else { return Ok(None) };
    match fetch_me(&t).await {
        Ok(me) => Ok(Some(me)),
        // Token was revoked / expired remotely — clear locally + report
        // unauthenticated so the UI shows the login affordance again.
        Err(api::ApiError::Server { status, .. }) if status == 401 || status == 403 => {
            let _ = keychain::clear_token();
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

#[derive(Debug, Serialize)]
struct EmailOnly<'a> {
    email: &'a str,
}

#[tauri::command]
pub async fn cloud_request_password_reset(email: String) -> Result<(), api::ApiError> {
    let _: serde_json::Value = api::post(
        "/v1/auth/request-password-reset",
        &EmailOnly { email: &email },
        None,
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub async fn cloud_resend_verification() -> Result<(), api::ApiError> {
    let Some(t) = keychain::load_token() else {
        return Err(api::ApiError::Server {
            status: 401,
            code: "unauthenticated".into(),
            message: None,
        });
    };
    let _: serde_json::Value =
        api::post("/v1/auth/resend-verification", &serde_json::json!({}), Some(&t)).await?;
    Ok(())
}

/// Internal: pull /me with a known-good token.
pub(super) async fn fetch_me(token: &str) -> Result<Me, api::ApiError> {
    api::get("/v1/account/me", Some(token)).await
}

/// Convenience for other modules (sync, billing) that need the current
/// bearer token. Returns None if the user isn't signed in.
pub fn current_token() -> Option<String> {
    keychain::load_token()
}

/// GET /v1/account/export, save the body to a path the user picks.
/// Returns the absolute path written (or an error). The user sees a
/// native save dialog so they choose where the file lands.
#[tauri::command]
pub async fn cloud_export_data(app: tauri::AppHandle) -> Result<String, api::ApiError> {
    let token = current_token().ok_or_else(|| api::ApiError::Server {
        status: 401,
        code: "unauthenticated".into(),
        message: None,
    })?;
    // Reuse the shared reqwest client so we don't open a fresh connection.
    let url = format!("{}/v1/account/export", super::api_origin());
    let res = api::client()
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await
        .map_err(api::ApiError::Network)?;
    if !res.status().is_success() {
        return Err(api::ApiError::Server {
            status: res.status().as_u16(),
            code: "export_failed".into(),
            message: None,
        });
    }
    let body = res.bytes().await.map_err(api::ApiError::Network)?;
    let default_name = format!(
        "localforge-export-{}.json",
        chrono::Utc::now().format("%Y-%m-%d")
    );
    // Use the dialog plugin to pick a destination path.
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = std::sync::mpsc::channel::<Option<std::path::PathBuf>>();
    app.dialog()
        .file()
        .add_filter("LocalForge export", &["json"])
        .set_file_name(&default_name)
        .save_file(move |path| {
            let _ = tx.send(path.and_then(|p| p.into_path().ok()));
        });
    let chosen = rx
        .recv()
        .ok()
        .flatten()
        .ok_or_else(|| api::ApiError::Decode("cancelled".into()))?;
    std::fs::write(&chosen, &body).map_err(|e| api::ApiError::Decode(format!("write: {e}")))?;
    Ok(chosen.to_string_lossy().to_string())
}

