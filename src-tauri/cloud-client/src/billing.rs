//! Stripe Checkout / Customer Portal URLs. Opening the URL is the host app's job.

use serde::Deserialize;

use crate::api::{self, ApiError};

#[derive(Debug, Deserialize)]
struct UrlResponse {
    url: String,
}

/// POST /v1/stripe/checkout/<plan>; returns the Checkout URL. The plan is validated client-side.
pub async fn start_checkout(plan: &str, token: &str) -> Result<String, ApiError> {
    if !matches!(plan, "hobby" | "team") {
        return Err(ApiError::Server {
            status: 400,
            code: "unknown_plan".into(),
            message: Some(format!("plan must be hobby or team, got {plan}")),
        });
    }
    let r: UrlResponse = api::post(
        &format!("/v1/stripe/checkout/{plan}"),
        &serde_json::json!({}),
        Some(token),
    )
    .await?;
    Ok(r.url)
}

/// POST /v1/stripe/portal; returns the Customer Portal URL.
pub async fn portal_url(token: &str) -> Result<String, ApiError> {
    let r: UrlResponse = api::post("/v1/stripe/portal", &serde_json::json!({}), Some(token)).await?;
    Ok(r.url)
}
