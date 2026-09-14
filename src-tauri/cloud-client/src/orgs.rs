//! Org, member and invitation HTTP surface (`/v1/orgs/*`) with the shared wire types.

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
    /// Org DEK wrapped with the invite secret (the secret travels only in the link fragment).
    #[serde(rename = "wrappedDek", skip_serializing_if = "Option::is_none")]
    wrapped_dek: Option<&'a str>,
}

/// Accept result: the org joined, plus the handoff `wrapped_dek` when the invite carried one.
#[derive(Debug, Clone, Deserialize)]
pub struct AcceptResult {
    #[serde(rename = "organizationId")]
    pub org_id: String,
    /// Caller's user id, used to self-seal a durable grant right after accepting.
    #[serde(rename = "userId", default)]
    pub user_id: Option<String>,
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

/// The active org's detail view: name, role and member list.
pub async fn me(token: &str) -> Result<OrgInfo, ApiError> {
    api::get("/v1/orgs/me", Some(token)).await
}

/// Send an invitation; the cloud emails a deep link and 409s if already a member.
pub async fn invite(
    org_id: &str,
    email: &str,
    role: &str,
    token: &str,
) -> Result<serde_json::Value, ApiError> {
    invite_with_dek(org_id, email, role, None, token).await
}

/// Invite with an invite-secret-wrapped org DEK so the invitee can decrypt on accept.
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

/// Cancel a pending invitation. `_api_origin` is kept for call-site compatibility.
pub async fn revoke_invitation(
    org_id: &str,
    invitation_id: &str,
    token: &str,
    _api_origin: &str,
) -> Result<(), ApiError> {
    let _: serde_json::Value = api::delete(
        &format!("/v1/orgs/{org_id}/invitations/{invitation_id}"),
        Some(token),
    )
    .await?;
    Ok(())
}

/// Remove a member from the org.
pub async fn remove_member(
    org_id: &str,
    user_id: &str,
    token: &str,
    _api_origin: &str,
) -> Result<(), ApiError> {
    let _: serde_json::Value =
        api::delete(&format!("/v1/orgs/{org_id}/members/{user_id}"), Some(token)).await?;
    Ok(())
}

/// Accept an invite; returns the org id and the handoff DEK when present.
pub async fn accept_invite_full(invite_token: &str, bearer: &str) -> Result<AcceptResult, ApiError> {
    api::post(
        &format!("/v1/orgs/invitations/{invite_token}/accept"),
        &serde_json::json!({}),
        Some(bearer),
    )
    .await
}
