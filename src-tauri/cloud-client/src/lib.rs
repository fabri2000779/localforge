//! Platform-agnostic LocalForge Cloud client shared by the desktop and mobile
//! apps: HTTP, envelope encryption, sync, relay, audit. Platform-specific pieces
//! (token storage, keychain, OAuth deep links) live in the consuming app.

#![warn(unused_must_use)]

pub mod api;
pub mod audit;
pub mod auth;
pub mod billing;
pub mod keys;
pub mod nodes;
pub mod oauth;
pub mod orgs;
pub mod relay;
pub mod sync;
pub mod vault;

/// Cloud API base URL; override with `LOCALFORGE_CLOUD_API` when testing locally.
pub fn api_origin() -> String {
    std::env::var("LOCALFORGE_CLOUD_API")
        .unwrap_or_else(|_| "https://api.localforge.gg".to_string())
}

/// User-Agent for every cloud call; the host app sets it once via `init_user_agent`.
pub fn user_agent() -> String {
    USER_AGENT
        .get()
        .cloned()
        .unwrap_or_else(|| "LocalForge/unknown".to_string())
}

static USER_AGENT: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Set the User-Agent once at startup; later calls are no-ops.
pub fn init_user_agent(ua: impl Into<String>) {
    let _ = USER_AGENT.set(ua.into());
}

pub use api::{ApiError, get, post};
