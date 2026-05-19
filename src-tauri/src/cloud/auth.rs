//! Desktop Tauri command surface for auth.
//!
//! The HTTP, types and pure parsing live in `localforge-cloud-client`.
//! What stays here is the desktop-specific glue: OS keychain access
//! (via `super::keychain`), the auto-setup/unlock of the envelope-
//! encryption DEK after sign-in (via `super::vault`), and the
//! `#[tauri::command]` registrations that connect those to the React
//! layer.
//!
//! Commands:
//!
//!   cloud_signup(email, password, displayName?) -> Me
//!   cloud_login(email, password)                -> Me
//!   cloud_logout()                              -> ()
//!   cloud_me()                                  -> Option<Me>  (None if not signed in)
//!   cloud_request_password_reset(email)         -> ()
//!   cloud_resend_verification()                 -> ()
//!   cloud_export_data()                         -> String      (path written)

use super::{api, keychain};

// Re-export the wire types so the React layer (via #[tauri::command]
// return types) keeps seeing the same shape. The unused-import lint
// fires on `Subscription` / `SyncKeyInfo` here because they only
// appear transitively inside `Me`'s JSON — neither is named directly
// in this file. Suppressing rather than dropping them keeps the
// `cloud::auth::*` namespace stable in case any future desktop code
// needs them.
#[allow(unused_imports)]
pub use localforge_cloud_client::auth::{Me, Subscription, SyncKeyInfo, fetch_me};

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn cloud_signup(
    email: String,
    password: String,
    display_name: Option<String>,
) -> Result<Me, api::ApiError> {
    let token =
        localforge_cloud_client::auth::signup(&email, &password, display_name.as_deref()).await?;
    keychain::save_token(&token).map_err(|e| api::ApiError::Decode(format!("keychain: {e}")))?;
    // Set up the envelope-encryption wrap NOW while we still have the
    // password in hand. Without this the user couldn't sync on a second
    // device without copying their recovery key by hand.
    if let Err(e) = super::vault::cloud_sync_key_setup(password.clone(), Some(false)).await {
        tracing::warn!("[signup] sync-key setup failed: {:?}", e);
    }
    fetch_me(&token).await
}

#[tauri::command]
pub async fn cloud_login(email: String, password: String) -> Result<Me, api::ApiError> {
    let token = localforge_cloud_client::auth::login(&email, &password).await?;
    keychain::save_token(&token).map_err(|e| api::ApiError::Decode(format!("keychain: {e}")))?;
    // Best-effort: try to unlock the DEK with this password so the
    // user can sync immediately. If they never set up the wrap (legacy
    // v0.1.14 user) this 412s silently and we fall back to setup. If
    // they DID set up but typed the wrong password the server already
    // rejected the login above, so any error here means a desktop bug
    // — log it loud.
    if let Err(e) = super::vault::cloud_sync_key_unlock(password.clone()).await {
        match &e {
            api::ApiError::Server { code, .. } if code == "sync_key_not_set" => {
                // Legacy user — set up the wrap so future logins work.
                if let Err(ee) =
                    super::vault::cloud_sync_key_setup(password.clone(), Some(false)).await
                {
                    tracing::warn!("[login] sync-key setup on legacy user failed: {:?}", ee);
                }
            }
            _ => tracing::warn!("[login] sync-key unlock failed: {:?}", e),
        }
    }
    fetch_me(&token).await
}

#[tauri::command]
pub async fn cloud_logout() -> Result<(), api::ApiError> {
    // Tell the API to revoke the session so other devices syncing from
    // it stop working immediately. Fire-and-forget — if it fails
    // (offline, etc.) we still clear the local copy.
    if let Some(t) = keychain::load_token() {
        let _ = localforge_cloud_client::auth::logout(&t).await;
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

#[tauri::command]
pub async fn cloud_request_password_reset(email: String) -> Result<(), api::ApiError> {
    localforge_cloud_client::auth::request_password_reset(&email).await
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
    localforge_cloud_client::auth::resend_verification(&t).await
}

/// Convenience for other desktop modules (sync, billing, etc.) that
/// need the current bearer token. Returns None if the user isn't
/// signed in. This stays desktop-only because it reads from the OS
/// keychain; mobile has its own equivalent backed by app-data storage.
pub fn current_token() -> Option<String> {
    keychain::load_token()
}

/// GET /v1/account/export, save the body to a path the user picks.
/// Returns the absolute path written (or an error). The user sees a
/// native save dialog so they choose where the file lands.
///
/// Stays here (not in the shared crate) because the save dialog is
/// `tauri_plugin_dialog`-specific and `std::fs::write` doesn't make
/// sense on mobile (sandboxed storage, share sheet instead).
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
