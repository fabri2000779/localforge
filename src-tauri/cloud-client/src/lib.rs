//! Platform-agnostic LocalForge Cloud client.
//!
//! This crate exists so the desktop and mobile apps share one
//! implementation of "what does it mean to talk to the LocalForge
//! cloud" — HTTP requests, envelope encryption, audit producer, sync
//! push/pull, the relay WebSocket. Platform-specific things (where
//! the JWT is stored, how OS keychain works, how OAuth deep-links are
//! received) are NOT in here; they're abstracted behind traits that
//! the consuming app implements.
//!
//! Migration status: currently extracting from the desktop's
//! `src/cloud/*` modules in stages. See the README / commit messages
//! for the running plan. Stage 1 (this commit) extracts:
//!
//!   - `mod` helpers (`api_origin`, `user_agent`)
//!   - `api` (HTTP client, ApiError)
//!   - trait definitions for `TokenStore` and `DekStore`
//!
//! Subsequent stages migrate auth, vault, oauth, sync, relay, audit,
//! orgs, billing. The desktop re-exports everything from this crate
//! so call sites in `src/cloud/mod.rs` stay stable during the
//! migration.

#![warn(unused_must_use)]

pub mod api;

// ---------------------------------------------------------------------------
// Global config / base helpers
// ---------------------------------------------------------------------------

/// The Cloud API base URL. Override at runtime with the
/// `LOCALFORGE_CLOUD_API` env var when iterating locally.
///
/// Kept as a free function — the few callers that need a configurable
/// origin are happy reading env at request time, and we avoid having
/// to thread a config struct through every API call.
pub fn api_origin() -> String {
    std::env::var("LOCALFORGE_CLOUD_API")
        .unwrap_or_else(|_| "https://api.localforge.gg".to_string())
}

/// User-Agent we send on every cloud call. Each consuming app
/// initialises this once at startup with its own product name +
/// version — that's why we don't bake `env!("CARGO_PKG_VERSION")` in
/// here (that would resolve to this crate's version, which is not
/// what cloud-side logs want to see).
pub fn user_agent() -> String {
    USER_AGENT
        .get()
        .cloned()
        .unwrap_or_else(|| "LocalForge/unknown".to_string())
}

static USER_AGENT: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Initialise the User-Agent string. Call once at process startup.
/// Subsequent calls are no-ops (deliberate — protects against weird
/// init races in test code). Recommended format:
/// `"LocalForge/<version> (<os> <arch>)"`.
pub fn init_user_agent(ua: impl Into<String>) {
    let _ = USER_AGENT.set(ua.into());
}

// ---------------------------------------------------------------------------
// Storage traits — the platform-specific bits
// ---------------------------------------------------------------------------

/// Where the JWT lives between sessions.
///
/// On desktop this is backed by the OS keychain (Win Credential
/// Manager, macOS Keychain, Linux Secret Service) via the `keyring`
/// crate. On mobile we'll back it with Tauri's app data dir for v0.0.1
/// and migrate to iOS Keychain / Android Keystore later. On
/// hypothetical headless / test consumers, an in-memory impl is fine.
pub trait TokenStore: Send + Sync {
    fn load(&self) -> Option<String>;
    fn save(&self, token: &str) -> Result<(), String>;
    fn clear(&self) -> Result<(), String>;
}

/// Where the unwrapped DEK lives once the user has authenticated.
///
/// Same shape as `TokenStore` but separated because (a) the DEK is
/// fixed-length 32 bytes, not an arbitrary string, and (b) the two
/// can have different storage backends — the JWT might survive
/// reboots while the DEK might be re-derived on every cold start, for
/// example.
pub trait DekStore: Send + Sync {
    fn load(&self) -> Option<[u8; 32]>;
    fn save(&self, dek: &[u8; 32]) -> Result<(), String>;
    fn clear(&self) -> Result<(), String>;
}

// ---------------------------------------------------------------------------
// Re-exports for convenience
// ---------------------------------------------------------------------------

pub use api::{ApiError, get, post};
