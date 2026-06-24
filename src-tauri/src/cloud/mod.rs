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
//!
//! Refactor in progress (stage 1): the platform-agnostic core of this
//! module is moving into the `localforge-cloud-client` crate so the
//! desktop and the mobile companion share one implementation. The
//! desktop modules below stay during the migration so existing call
//! sites (and the Tauri command registry in `main.rs`) don't break;
//! the API helpers (`api_origin`, `api`) are re-exported from the
//! shared crate below.

pub mod audit;
pub mod auth;
pub mod billing;
pub mod keychain;
pub mod nodes;
pub mod oauth;
pub mod orgs;
pub mod push;
pub mod relay;
pub mod sync;
pub mod templates;
pub mod vault;

// HTTP wrapper + base helpers come from the shared crate now. Existing
// `super::api::...` and `super::api_origin()` call sites keep working
// because they resolve through these re-exports.
pub use localforge_cloud_client::{api, api_origin};
