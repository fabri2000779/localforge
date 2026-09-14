//! Cloud-stored server list; the encrypted blob is opaque here.

use serde::Deserialize;

use crate::api::{self, ApiError};

/// A row of `GET /v1/sync/servers`; only the desktop decrypts `encrypted_blob`.
#[derive(Debug, Clone, Deserialize)]
pub struct SyncedServer {
    pub id: String,
    pub name: String,
    pub encrypted_blob: String,
    pub updated_at: i64,
}

/// GET /v1/sync/servers (paid plans only; free users get 402).
pub async fn list_servers(token: &str) -> Result<Vec<SyncedServer>, ApiError> {
    #[derive(Deserialize)]
    struct Resp {
        servers: Vec<SyncedServer>,
    }
    let r: Resp = api::get("/v1/sync/servers", Some(token)).await?;
    Ok(r.servers)
}
