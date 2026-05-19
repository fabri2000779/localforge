//! Stripe Checkout + Customer Portal — pure HTTP slice.
//!
//! The cloud API takes care of the real Stripe integration; from a
//! client's perspective these are POST-and-get-a-URL calls. The
//! consumer (desktop / mobile) is responsible for actually opening
//! the URL in the user's browser — that's where the platform-
//! specific bit lives (`tauri-plugin-opener` on desktop, mobile
//! browser intent or shell `open` on mobile).
//!
//! We keep `start_checkout` symmetric with how the rest of the cloud
//! module is structured: input the bearer token + plan, get a URL.
//! No state, no callbacks, easy to test, easy to reuse.

use serde::Deserialize;

use crate::api::{self, ApiError};

#[derive(Debug, Deserialize)]
struct UrlResponse {
    url: String,
}

/// POST /v1/stripe/checkout/<plan>. Returns the Stripe Checkout
/// session URL. The cloud rejects unknown plans with a 400; we
/// pre-validate so the cost stays on the client when the user
/// somehow ends up with garbage in `plan`.
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

/// POST /v1/stripe/portal. Returns the Stripe Customer Portal session
/// URL (where the user can change card, cancel, view invoices).
pub async fn portal_url(token: &str) -> Result<String, ApiError> {
    let r: UrlResponse = api::post("/v1/stripe/portal", &serde_json::json!({}), Some(token)).await?;
    Ok(r.url)
}
