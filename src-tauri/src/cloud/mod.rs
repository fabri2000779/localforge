//! Optional cloud integration (auth, billing, sync, relay, orgs); everything degrades
//! gracefully without a JWT. Shared HTTP and crypto live in `localforge-cloud-client`.

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

pub use localforge_cloud_client::{api, api_origin};
