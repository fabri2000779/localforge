//! Platform-agnostic auth HTTP surface. Token persistence and DEK unlock live in each app.

use serde::{Deserialize, Serialize};

use crate::api::{self, ApiError};

// Wire types for /v1/account/me.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub plan: String, // 'free' | 'hobby' | 'team'
    #[serde(rename = "currentPeriodEnd")]
    pub current_period_end: Option<i64>,
    #[serde(rename = "cancelAtPeriodEnd")]
    pub cancel_at_period_end: bool,
    #[serde(rename = "trialEndsAt")]
    pub trial_ends_at: Option<i64>,
    /// Unix ms when cloud data is purged (set after dropping to free).
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
    /// None until the user sets up their sync key (typical for fresh OAuth accounts).
    #[serde(rename = "syncKey", default)]
    pub sync_key: Option<SyncKeyInfo>,
    /// KEK-wrapped X25519 secret for opening org-DEK grants; None until published.
    #[serde(rename = "wrappedX25519Sk", default)]
    pub wrapped_x25519_sk: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncKeyInfo {
    pub wrapped_dek: String,
    pub kek_salt: String,
    #[serde(default)]
    pub kek_params: Option<serde_json::Value>,
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

#[derive(Debug, Serialize)]
struct EmailOnly<'a> {
    email: &'a str,
}

/// POST /v1/auth/signup; returns the session JWT for the caller to persist.
pub async fn signup(
    email: &str,
    password: &str,
    display_name: Option<&str>,
) -> Result<String, ApiError> {
    let r: AuthResponse = api::post(
        "/v1/auth/signup",
        &SignupBody {
            email,
            password,
            display_name,
        },
        None,
    )
    .await?;
    Ok(r.token)
}

/// POST /v1/auth/login; returns the session JWT. Wrong passwords surface as 401 `invalid_credentials`.
pub async fn login(email: &str, password: &str) -> Result<String, ApiError> {
    let r: AuthResponse = api::post(
        "/v1/auth/login",
        &LoginBody { email, password },
        None,
    )
    .await?;
    Ok(r.token)
}

/// POST /v1/auth/logout (revokes the session server-side).
pub async fn logout(token: &str) -> Result<(), ApiError> {
    let _: serde_json::Value =
        api::post("/v1/auth/logout", &serde_json::json!({}), Some(token)).await?;
    Ok(())
}

/// GET /v1/account/me — current user + subscription + sync-key state.
pub async fn fetch_me(token: &str) -> Result<Me, ApiError> {
    api::get("/v1/account/me", Some(token)).await
}

/// POST /v1/auth/request-password-reset; always succeeds to avoid an enumeration oracle.
pub async fn request_password_reset(email: &str) -> Result<(), ApiError> {
    let _: serde_json::Value = api::post(
        "/v1/auth/request-password-reset",
        &EmailOnly { email },
        None,
    )
    .await?;
    Ok(())
}

/// POST /v1/auth/resend-verification (needs a session token).
pub async fn resend_verification(token: &str) -> Result<(), ApiError> {
    let _: serde_json::Value = api::post(
        "/v1/auth/resend-verification",
        &serde_json::json!({}),
        Some(token),
    )
    .await?;
    Ok(())
}
