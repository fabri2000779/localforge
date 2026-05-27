//! Org / members / invitations — pure HTTP slice of the Team-plan
//! features. Mirrors `/v1/orgs/*` in the cloud API.
//!
//! Wire types live here so the desktop and mobile React layers see
//! identical JSON shapes. Tauri command wrappers stay on each consumer.

use serde::{Deserialize, Serialize};

use crate::api::{self, ApiError};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Member {
    pub id: String,
    pub email: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub role: String, // owner | admin | operator | viewer
    pub joined_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgInfo {
    pub id: String,
    pub name: String,
    pub role: String,
    pub is_owner: bool,
    pub created_at: i64,
    pub members: Vec<Member>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Invitation {
    pub id: String,
    pub email: String,
    pub role: String,
    pub expires_at: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgSummary {
    pub id: String,
    pub name: String,
    pub role: String,
    pub is_owner: bool,
    pub created_at: i64,
    pub joined_at: i64,
}

#[derive(Serialize)]
struct InviteBody<'a> {
    email: &'a str,
    role: &'a str,
    /// Org DEK wrapped with the invite secret (which travels in the link
    /// #fragment, never sent here). Omitted on a plain invite.
    #[serde(rename = "wrappedDek", skip_serializing_if = "Option::is_none")]
    wrapped_dek: Option<&'a str>,
}

/// Result of accepting an invite — the org joined + (when the invite carried
/// the handoff) the DEK wrapped with the invite secret, for the client to
/// unwrap with the fragment secret.
#[derive(Debug, Clone, Deserialize)]
pub struct AcceptResult {
    #[serde(rename = "organizationId")]
    pub org_id: String,
    #[serde(rename = "wrappedDek", default)]
    pub wrapped_dek: Option<String>,
}

/// List every org the user belongs to. Powers the org switcher.
pub async fn list(token: &str) -> Result<Vec<OrgSummary>, ApiError> {
    #[derive(Deserialize)]
    struct Resp {
        orgs: Vec<OrgSummary>,
    }
    let r: Resp = api::get("/v1/orgs", Some(token)).await?;
    Ok(r.orgs)
}

/// Detail view of the user's primary org — name, role, current
/// membership list.
pub async fn me(token: &str) -> Result<OrgInfo, ApiError> {
    api::get("/v1/orgs/me", Some(token)).await
}

/// Send an invitation. The cloud emails the invitee a `localforge://`
/// deep link; if they're already a member it rejects with a 409.
pub async fn invite(
    org_id: &str,
    email: &str,
    role: &str,
    token: &str,
) -> Result<serde_json::Value, ApiError> {
    invite_with_dek(org_id, email, role, None, token).await
}

/// Send an invitation carrying an invite-secret-wrapped org DEK so the
/// invitee can decrypt the moment they accept (handoff). `wrapped_dek` is the
/// org DEK encrypted under the invite secret; the secret itself goes in the
/// link #fragment, never here.
pub async fn invite_with_dek(
    org_id: &str,
    email: &str,
    role: &str,
    wrapped_dek: Option<&str>,
    token: &str,
) -> Result<serde_json::Value, ApiError> {
    api::post(
        &format!("/v1/orgs/{org_id}/invitations"),
        &InviteBody {
            email,
            role,
            wrapped_dek,
        },
        Some(token),
    )
    .await
}

/// List pending invitations on an org (for the admin's invite list UI).
pub async fn list_invitations(org_id: &str, token: &str) -> Result<Vec<Invitation>, ApiError> {
    #[derive(Deserialize)]
    struct Resp {
        invitations: Vec<Invitation>,
    }
    let r: Resp = api::get(&format!("/v1/orgs/{org_id}/invitations"), Some(token)).await?;
    Ok(r.invitations)
}

/// Cancel an outstanding invitation by id. DELETE — uses the raw
/// reqwest client because our `api::post` / `api::get` helpers only
/// cover the verbs we use most. Stage-5 cleanup adds an `api::delete`
/// so this can go through the standard path.
pub async fn revoke_invitation(
    org_id: &str,
    invitation_id: &str,
    token: &str,
    api_origin: &str,
) -> Result<(), ApiError> {
    let url = format!("{api_origin}/v1/orgs/{org_id}/invitations/{invitation_id}");
    let res = api::client()
        .delete(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(ApiError::Network)?;
    if res.status().is_success() {
        Ok(())
    } else {
        Err(ApiError::Server {
            status: res.status().as_u16(),
            code: "delete_failed".into(),
            message: None,
        })
    }
}

/// Kick a member out of the org. Same DELETE-via-raw-client story as
/// `revoke_invitation`.
pub async fn remove_member(
    org_id: &str,
    user_id: &str,
    token: &str,
    api_origin: &str,
) -> Result<(), ApiError> {
    let url = format!("{api_origin}/v1/orgs/{org_id}/members/{user_id}");
    let res = api::client()
        .delete(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(ApiError::Network)?;
    if res.status().is_success() {
        Ok(())
    } else {
        Err(ApiError::Server {
            status: res.status().as_u16(),
            code: "delete_failed".into(),
            message: None,
        })
    }
}

/// Accept an invitation token (delivered via the `localforge://invite`
/// deep link OR pasted by the user). Returns the org id they joined
/// so the UI can switch to it.
pub async fn accept_invite(invite_token: &str, bearer: &str) -> Result<String, ApiError> {
    Ok(accept_invite_full(invite_token, bearer).await?.org_id)
}

/// Accept an invite and also return the handoff `wrapped_dek` (if the invite
/// carried one), so the client can unwrap the org DEK with the fragment
/// secret and decrypt immediately.
pub async fn accept_invite_full(invite_token: &str, bearer: &str) -> Result<AcceptResult, ApiError> {
    api::post(
        &format!("/v1/orgs/invitations/{invite_token}/accept"),
        &serde_json::json!({}),
        Some(bearer),
    )
    .await
}
