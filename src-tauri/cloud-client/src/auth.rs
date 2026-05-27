//! Authentication HTTP surface: email/password signup + login, /me,
//! logout, password reset request, verification resend.
//!
//! This module is the PLATFORM-AGNOSTIC slice. Each consumer (desktop,
//! mobile) wraps these in its own `#[tauri::command]` handler that
//! also (a) saves/loads the JWT through its platform-specific token
//! store and (b) wires up auto-setup/unlock of the envelope-encryption
//! DEK after sign-in. We deliberately don't combine those concerns
//! here — keychain access and the desktop's vault module are not
//! portable.

use serde::{Deserialize, Serialize};

use crate::api::{self, ApiError};

// ---------------------------------------------------------------------------
// Wire types — these match the JSON shape /v1/account/me returns.
// ---------------------------------------------------------------------------

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
    /// Envelope-encryption material. None when the user hasn't set up
    /// their sync key yet (typical for fresh OAuth accounts — the
    /// desktop / mobile prompts on first launch). Email-password
    /// users get this populated automatically at signup.
    #[serde(rename = "syncKey", default)]
    pub sync_key: Option<SyncKeyInfo>,
    /// The user's X25519 secret, KEK-wrapped, for opening org-DEK grants
    /// sealed to them. None until a keypair is published (the client mints +
    /// publishes one on first unlock). Recovered on any device by unwrapping
    /// with the passphrase-derived KEK, exactly like the DEK.
    #[serde(rename = "wrappedX25519Sk", default)]
    pub wrapped_x25519_sk: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncKeyInfo {
    pub wrapped_dek: String,
    pub kek_salt: String,
    // kek_params kept opaque — consumers hard-code the defaults and
    // we'd consult these only when rotating to harder params.
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

// ---------------------------------------------------------------------------
// Pure HTTP — no keychain, no events, no Tauri.
// ---------------------------------------------------------------------------

/// POST /v1/auth/signup. Returns the new session JWT — caller is
/// responsible for persisting it (OS keychain on desktop, app-data dir
/// on mobile, whatever else for tests / web).
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

/// POST /v1/auth/login. Same contract as signup — token only, caller
/// persists it. The cloud rejects wrong passwords with a `401
/// invalid_credentials`; we propagate as-is so the UI can show the
/// right error.
pub async fn login(email: &str, password: &str) -> Result<String, ApiError> {
    let r: AuthResponse = api::post(
        "/v1/auth/login",
        &LoginBody { email, password },
        None,
    )
    .await?;
    Ok(r.token)
}

/// POST /v1/auth/logout. Tells the cloud to revoke the session.
/// Fire-and-forget — if the network is dead the caller still clears
/// the local token; the server-side revoke catches up next sync.
pub async fn logout(token: &str) -> Result<(), ApiError> {
    let _: serde_json::Value =
        api::post("/v1/auth/logout", &serde_json::json!({}), Some(token)).await?;
    Ok(())
}

/// GET /v1/account/me — current user + subscription + sync-key state.
pub async fn fetch_me(token: &str) -> Result<Me, ApiError> {
    api::get("/v1/account/me", Some(token)).await
}

/// POST /v1/auth/request-password-reset. Always succeeds even when
/// the email doesn't exist (to avoid an enumeration oracle).
pub async fn request_password_reset(email: &str) -> Result<(), ApiError> {
    let _: serde_json::Value = api::post(
        "/v1/auth/request-password-reset",
        &EmailOnly { email },
        None,
    )
    .await?;
    Ok(())
}

/// POST /v1/auth/resend-verification. Requires a session token (the
/// API uses it to identify which account's verification to resend).
pub async fn resend_verification(token: &str) -> Result<(), ApiError> {
    let _: serde_json::Value = api::post(
        "/v1/auth/resend-verification",
        &serde_json::json!({}),
        Some(token),
    )
    .await?;
    Ok(())
}
