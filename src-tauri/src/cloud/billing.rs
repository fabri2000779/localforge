//! Stripe Checkout / Portal: fetch the URL via cloud-client and open it in the browser.

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
