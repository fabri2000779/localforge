//! Org / members / invitations — desktop side of the Team plan.
//!
//! Mirrors the cloud `/v1/orgs/*` API. The audit-log producer is the
//! one helper that everyone else in the codebase eventually calls.

use serde::{Deserialize, Serialize};

use super::{api, auth};

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

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

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

/// List every org the user belongs to. Used by the org switcher.
#[tauri::command]
pub async fn cloud_orgs_list() -> Result<Vec<OrgSummary>, api::ApiError> {
    let token = auth::current_token().ok_or_else(unauth)?;
    #[derive(Deserialize)]
    struct Resp { orgs: Vec<OrgSummary> }
    let r: Resp = api::get("/v1/orgs", Some(&token)).await?;
    Ok(r.orgs)
}

#[tauri::command]
pub async fn cloud_orgs_me() -> Result<OrgInfo, api::ApiError> {
    let token = auth::current_token().ok_or_else(unauth)?;
    api::get("/v1/orgs/me", Some(&token)).await
}

#[derive(Serialize)]
struct InviteBody<'a> {
    email: &'a str,
    role: &'a str,
}

#[tauri::command]
pub async fn cloud_orgs_invite(
    org_id: String,
    email: String,
    role: String,
) -> Result<serde_json::Value, api::ApiError> {
    let token = auth::current_token().ok_or_else(unauth)?;
    api::post(
        &format!("/v1/orgs/{}/invitations", org_id),
        &InviteBody { email: &email, role: &role },
        Some(&token),
    )
    .await
}

#[tauri::command]
pub async fn cloud_orgs_list_invitations(org_id: String) -> Result<Vec<Invitation>, api::ApiError> {
    #[derive(Deserialize)]
    struct Resp { invitations: Vec<Invitation> }
    let token = auth::current_token().ok_or_else(unauth)?;
    let r: Resp = api::get(&format!("/v1/orgs/{}/invitations", org_id), Some(&token)).await?;
    Ok(r.invitations)
}

#[tauri::command]
pub async fn cloud_orgs_revoke_invitation(
    org_id: String,
    invitation_id: String,
) -> Result<(), api::ApiError> {
    let token = auth::current_token().ok_or_else(unauth)?;
    let url = format!(
        "{}/v1/orgs/{}/invitations/{}",
        super::api_origin(),
        org_id,
        invitation_id,
    );
    let res = api::client()
        .delete(&url)
        .bearer_auth(&token)
        .send()
        .await
        .map_err(api::ApiError::Network)?;
    if res.status().is_success() {
        Ok(())
    } else {
        Err(api::ApiError::Server {
            status: res.status().as_u16(),
            code: "delete_failed".into(),
            message: None,
        })
    }
}

#[tauri::command]
pub async fn cloud_orgs_remove_member(
    org_id: String,
    user_id: String,
) -> Result<(), api::ApiError> {
    let token = auth::current_token().ok_or_else(unauth)?;
    let url = format!(
        "{}/v1/orgs/{}/members/{}",
        super::api_origin(),
        org_id,
        user_id,
    );
    let res = api::client()
        .delete(&url)
        .bearer_auth(&token)
        .send()
        .await
        .map_err(api::ApiError::Network)?;
    if res.status().is_success() {
        Ok(())
    } else {
        Err(api::ApiError::Server {
            status: res.status().as_u16(),
            code: "delete_failed".into(),
            message: None,
        })
    }
}

/// Accept an invitation token (delivered via the `localforge://invite`
/// deep link OR pasted by the user). Returns the org id they joined
/// so the UI can switch to it.
#[tauri::command]
pub async fn cloud_orgs_accept_invite(token: String) -> Result<String, api::ApiError> {
    let bearer = auth::current_token().ok_or_else(unauth)?;
    #[derive(Deserialize)]
    struct Resp { #[serde(rename = "organizationId")] org_id: String }
    let r: Resp = api::post(
        &format!("/v1/orgs/invitations/{}/accept", token),
        &serde_json::json!({}),
        Some(&bearer),
    )
    .await?;
    Ok(r.org_id)
}

fn unauth() -> api::ApiError {
    api::ApiError::Server {
        status: 401,
        code: "unauthenticated".into(),
        message: None,
    }
}
