//! Cloud integration — auth, billing, sync (later).
//!
//! Login is OPTIONAL. The desktop runs fully without an account; the
//! cloud bits unlock cross-device sync, sub-user invites, alerts, etc.
//! Everything in this module degrades gracefully when there's no JWT.
//!
//! The auth path that opens the system browser (OAuth) lives in
//! `oauth.rs`. Email/password and account queries live in `auth.rs`.
//! JWT storage in the OS keychain lives in `keychain.rs`. Stripe
//! checkout / portal commands live in `billing.rs`.
pub mod api;
pub mod auth;
pub mod billing;
pub mod keychain;
pub mod oauth;
pub mod sync;
pub mod vault;

/// The Cloud API base URL. Override at runtime with the
/// `LOCALFORGE_CLOUD_API` env var when iterating locally.
pub fn api_origin() -> String {
    std::env::var("LOCALFORGE_CLOUD_API")
        .unwrap_or_else(|_| "https://api.localforge.gg".to_string())
}

/// Default UA we send on every cloud call — helps cloud-side logs.
pub fn user_agent() -> String {
    format!(
        "LocalForge/{} ({} {})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}
