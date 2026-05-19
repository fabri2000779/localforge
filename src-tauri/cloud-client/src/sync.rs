//! Cross-device server sync — pure HTTP slice.
//!
//! The desktop's sync.rs is much larger because it owns the local
//! NodeRegistry + Server type and orchestrates encrypt-on-push /
//! decrypt-on-pull. That stays desktop-resident for now (entangled
//! with platform-specific state).
//!
//! What lives here is what the mobile companion actually needs: list
//! the cloud-stored servers so the user can see their fleet. The
//! mobile is observation-only in v0.0.x — it doesn't write servers,
//! and it doesn't decrypt the blob (no DEK), so the list view shows
//! only plaintext columns (id, name, updated_at). The encrypted blob
//! comes back in the response too — the mobile just ignores it.

use serde::Deserialize;

use crate::api::{self, ApiError};

/// A server as the cloud returns it on `GET /v1/sync/servers`.
///
/// `encrypted_blob` is the AES-GCM-wrapped ciphertext containing all
/// the sensitive config (env vars, install scripts, port maps). The
/// mobile companion never decrypts it; the field is exposed here so
/// the desktop's pull path keeps round-tripping cleanly.
#[derive(Debug, Clone, Deserialize)]
pub struct SyncedServer {
    pub id: String,
    pub name: String,
    /// Base64+envelope-wrapped config blob. Opaque to mobile, decoded
    /// by the desktop with the local DEK.
    pub encrypted_blob: String,
    pub updated_at: i64,
}

/// GET /v1/sync/servers — every server the user can see in their
/// primary org, newest first.
///
/// The cloud requires a paid plan (Hobby+ or Team) for this endpoint;
/// free users get a 402. Callers should check `Me::subscription.plan`
/// before invoking, or be ready to handle the error.
pub async fn list_servers(token: &str) -> Result<Vec<SyncedServer>, ApiError> {
    #[derive(Deserialize)]
    struct Resp {
        servers: Vec<SyncedServer>,
    }
    let r: Resp = api::get("/v1/sync/servers", Some(token)).await?;
    Ok(r.servers)
}
