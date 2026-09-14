//! Desktop auth commands: keychain-backed sessions plus DEK setup/unlock after sign-in.

use super::{api, keychain};

pub use localforge_cloud_client::auth::{Me, fetch_me};

#[tauri::command]
pub async fn cloud_signup(
    email: String,
    password: String,
    display_name: Option<String>,
) -> Result<Me, api::ApiError> {
    let token =
        localforge_cloud_client::auth::signup(&email, &password, display_name.as_deref()).await?;
    keychain::save_token(&token).map_err(|e| api::ApiError::Decode(format!("keychain: {e}")))?;
    // Set up the envelope-encryption wrap now, while we still have the password.
    if let Err(e) = super::vault::cloud_sync_key_setup(password.clone(), Some(false)).await {
        tracing::warn!("[signup] sync-key setup failed: {:?}", e);
    }
    fetch_me(&token).await
}

#[tauri::command]
pub async fn cloud_login(email: String, password: String) -> Result<Me, api::ApiError> {
    let token = localforge_cloud_client::auth::login(&email, &password).await?;
    keychain::save_token(&token).map_err(|e| api::ApiError::Decode(format!("keychain: {e}")))?;
    // Unlock the DEK with this password; a legacy user without a wrap gets one set up.
    if let Err(e) = super::vault::cloud_sync_key_unlock(password.clone()).await {
        match &e {
            api::ApiError::Server { code, .. } if code == "sync_key_not_set" => {
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
    // Revoke server-side (best-effort), then clear the local copy.
    if let Some(t) = keychain::load_token() {
        let _ = localforge_cloud_client::auth::logout(&t).await;
    }
    keychain::clear_token().map_err(|e| api::ApiError::Decode(format!("keychain: {e}")))?;
    // Wipe key material so the next account on this machine can't inherit this DEK.
    super::vault::clear_local_keys(&[]);
    super::nodes::reset_desktop_claim();
    Ok(())
}

#[tauri::command]
pub async fn cloud_me() -> Result<Option<Me>, api::ApiError> {
    let Some(t) = keychain::load_token() else { return Ok(None) };
    match fetch_me(&t).await {
        Ok(me) => Ok(Some(me)),
        // Revoked/expired remotely: clear the token and key material so the UI shows login again.
        Err(api::ApiError::Server { status, .. }) if status == 401 || status == 403 => {
            let _ = keychain::clear_token();
            super::vault::clear_local_keys(&[]);
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

/// Current bearer token, or `None` when signed out.
pub fn current_token() -> Option<String> {
    keychain::load_token()
}

/// GET /v1/account/export and save it where the user chooses (native save dialog).
#[tauri::command]
pub async fn cloud_export_data(app: tauri::AppHandle) -> Result<String, api::ApiError> {
    let token = current_token().ok_or_else(|| api::ApiError::Server {
        status: 401,
        code: "unauthenticated".into(),
        message: None,
    })?;
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
