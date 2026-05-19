//! Desktop billing glue.
//!
//! Stripe Checkout + Customer Portal sessions are built by the cloud
//! API; we POST and get back a URL. The pure HTTP + plan validation
//! lives in `localforge-cloud-client::billing`; what stays here is
//! the desktop bit — opening the returned URL in the user's default
//! browser via `tauri-plugin-opener`.

use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use super::{api, auth};

#[tauri::command]
pub async fn cloud_open_checkout(app: AppHandle, plan: String) -> Result<(), api::ApiError> {
    let token = auth::current_token().ok_or_else(|| api::ApiError::Server {
        status: 401,
        code: "unauthenticated".into(),
        message: None,
    })?;
    let url = localforge_cloud_client::billing::start_checkout(&plan, &token).await?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| api::ApiError::Decode(format!("open: {e}")))
}

#[tauri::command]
pub async fn cloud_open_portal(app: AppHandle) -> Result<(), api::ApiError> {
    let token = auth::current_token().ok_or_else(|| api::ApiError::Server {
        status: 401,
        code: "unauthenticated".into(),
        message: None,
    })?;
    let url = localforge_cloud_client::billing::portal_url(&token).await?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| api::ApiError::Decode(format!("open: {e}")))
}
