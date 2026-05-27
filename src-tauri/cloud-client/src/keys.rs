//! Team key-sharing HTTP surface — sealed org-DEK grants.
//!
//! Pairs with the `vault` sealed-box primitives + cloud migration 0009. The
//! flow: every user publishes an X25519 public key (`publish_pubkey`); an org
//! owner finds members who still need access (`pending_grants`), seals their
//! DEK to each member's pubkey (`vault::seal_to`) and uploads it
//! (`put_grant`); a member fetches their sealed grant (`my_grant`) and opens
//! it (`vault::open_sealed`). Platform-agnostic — the consuming app supplies
//! the bearer and does the crypto with the locally-held secret.

use serde::{Deserialize, Serialize};

use crate::api::{self, ApiError};

#[derive(Debug, Serialize)]
struct PubkeyBody<'a> {
    pubkey: &'a str,
    #[serde(rename = "wrappedX25519Sk")]
    wrapped_x25519_sk: &'a str,
}

/// Publish (or refresh) the caller's X25519 keypair: the public key + the
/// KEK-wrapped secret (so the user's other devices recover it).
pub async fn publish_pubkey(pubkey: &str, wrapped_sk: &str, bearer: &str) -> Result<(), ApiError> {
    let _: serde_json::Value = api::post(
        "/v1/account/pubkey",
        &PubkeyBody {
            pubkey,
            wrapped_x25519_sk: wrapped_sk,
        },
        Some(bearer),
    )
    .await?;
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
pub struct PendingGrant {
    #[serde(rename = "userId")]
    pub user_id: String,
    /// base64 X25519 public key to seal the DEK to.
    pub pubkey: String,
}

/// Members of `org_id` who have a published pubkey but no grant yet.
/// Owner-only server-side; the owner's client seals to each and `put_grant`s.
pub async fn pending_grants(org_id: &str, bearer: &str) -> Result<Vec<PendingGrant>, ApiError> {
    #[derive(Deserialize)]
    struct Resp {
        pending: Vec<PendingGrant>,
    }
    let r: Resp = api::get(&format!("/v1/orgs/{org_id}/grants/pending"), Some(bearer)).await?;
    Ok(r.pending)
}

#[derive(Debug, Serialize)]
struct GrantBody<'a> {
    #[serde(rename = "userId")]
    user_id: &'a str,
    #[serde(rename = "sealedDek")]
    sealed_dek: &'a str,
    #[serde(rename = "sealedEpk")]
    sealed_epk: &'a str,
}

/// Store the owner's DEK sealed to `user_id` in `org_id`. Owner-only.
pub async fn put_grant(
    org_id: &str,
    user_id: &str,
    sealed_dek: &str,
    sealed_epk: &str,
    bearer: &str,
) -> Result<(), ApiError> {
    let _: serde_json::Value = api::post(
        &format!("/v1/orgs/{org_id}/grants"),
        &GrantBody {
            user_id,
            sealed_dek,
            sealed_epk,
        },
        Some(bearer),
    )
    .await?;
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
pub struct Grant {
    #[serde(rename = "sealedDek")]
    pub sealed_dek: String,
    #[serde(rename = "sealedEpk")]
    pub sealed_epk: String,
}

/// The caller's own sealed grant for `org_id`, or `None` until the owner has
/// sealed it (the cloud returns 404).
pub async fn my_grant(org_id: &str, bearer: &str) -> Result<Option<Grant>, ApiError> {
    match api::get::<Grant>(&format!("/v1/orgs/{org_id}/grant"), Some(bearer)).await {
        Ok(g) => Ok(Some(g)),
        Err(ApiError::Server { status: 404, .. }) => Ok(None),
        Err(e) => Err(e),
    }
}
