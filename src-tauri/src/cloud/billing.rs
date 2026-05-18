//! Stripe Checkout + Customer Portal. The actual payment UI lives on
//! `checkout.stripe.com` / `billing.stripe.com` — we POST our authed
//! request to the cloud API, get back a URL, and open it in the user's
//! default browser via tauri-plugin-shell.

use serde::Deserialize;
use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

use super::{api, auth};

#[derive(Debug, Deserialize)]
struct UrlResponse {
    url: String,
}

#[tauri::command]
pub async fn cloud_open_checkout(app: AppHandle, plan: String) -> Result<(), api::ApiError> {
    if !matches!(plan.as_str(), "hobby" | "team") {
        return Err(api::ApiError::Server {
            status: 400,
            code: "unknown_plan".into(),
            message: None,
        });
    }
    let Some(t) = auth::current_token() else {
        return Err(api::ApiError::Server {
            status: 401,
            code: "unauthenticated".into(),
            message: None,
        });
    };
    let r: UrlResponse = api::post(
        &format!("/v1/stripe/checkout/{plan}"),
        &serde_json::json!({}),
        Some(&t),
    )
    .await?;
    app.shell()
        .open(r.url, None)
        .map_err(|e| api::ApiError::Decode(format!("open: {e}")))
}

#[tauri::command]
pub async fn cloud_open_portal(app: AppHandle) -> Result<(), api::ApiError> {
    let Some(t) = auth::current_token() else {
        return Err(api::ApiError::Server {
            status: 401,
            code: "unauthenticated".into(),
            message: None,
        });
    };
    let r: UrlResponse = api::post("/v1/stripe/portal", &serde_json::json!({}), Some(&t)).await?;
    app.shell()
        .open(r.url, None)
        .map_err(|e| api::ApiError::Decode(format!("open: {e}")))
}
