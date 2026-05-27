//! Desktop org / member / invitation commands.
//!
//! All wire types + HTTP calls live in
//! `localforge-cloud-client::orgs` so desktop and mobile see the
//! identical shapes. The desktop layer here is just `#[tauri::command]`
//! adapters that read the bearer token from the OS keychain and
//! delegate. Re-exports keep `cloud::orgs::{OrgInfo, Member, …}` at
//! the same path so the React layer's TypeScript bindings don't
//! shift.

use super::{api, auth};

#[allow(unused_imports)]
pub use localforge_cloud_client::orgs::{Invitation, Member, OrgInfo, OrgSummary};

fn unauth() -> api::ApiError {
    api::ApiError::Server {
        status: 401,
        code: "unauthenticated".into(),
        message: None,
    }
}

/// List every org the user belongs to. Used by the org switcher.
#[tauri::command]
pub async fn cloud_orgs_list() -> Result<Vec<OrgSummary>, api::ApiError> {
    let token = auth::current_token().ok_or_else(unauth)?;
    localforge_cloud_client::orgs::list(&token).await
}

/// Point every subsequent cloud call at a specific org (the active org in
/// the switcher). A sub-user viewing the owner's org sets it here so sync +
/// machine listing resolve to the OWNER's org (sent as `X-LocalForge-Org`,
/// membership-verified server-side). Pass `None`/empty on sign-out to fall
/// back to the primary org.
#[tauri::command(rename_all = "camelCase")]
pub fn cloud_set_active_org(org_id: Option<String>) {
    localforge_cloud_client::api::set_active_org(org_id);
}

#[tauri::command]
pub async fn cloud_orgs_me() -> Result<OrgInfo, api::ApiError> {
    let token = auth::current_token().ok_or_else(unauth)?;
    localforge_cloud_client::orgs::me(&token).await
}

#[tauri::command]
pub async fn cloud_orgs_invite(
    org_id: String,
    email: String,
    role: String,
) -> Result<serde_json::Value, api::ApiError> {
    let token = auth::current_token().ok_or_else(unauth)?;
    localforge_cloud_client::orgs::invite(&org_id, &email, &role, &token).await
}

#[tauri::command]
pub async fn cloud_orgs_list_invitations(org_id: String) -> Result<Vec<Invitation>, api::ApiError> {
    let token = auth::current_token().ok_or_else(unauth)?;
    localforge_cloud_client::orgs::list_invitations(&org_id, &token).await
}

#[tauri::command]
pub async fn cloud_orgs_revoke_invitation(
    org_id: String,
    invitation_id: String,
) -> Result<(), api::ApiError> {
    let token = auth::current_token().ok_or_else(unauth)?;
    localforge_cloud_client::orgs::revoke_invitation(
        &org_id,
        &invitation_id,
        &token,
        &super::api_origin(),
    )
    .await
}

#[tauri::command]
pub async fn cloud_orgs_remove_member(
    org_id: String,
    user_id: String,
) -> Result<(), api::ApiError> {
    let token = auth::current_token().ok_or_else(unauth)?;
    localforge_cloud_client::orgs::remove_member(
        &org_id,
        &user_id,
        &token,
        &super::api_origin(),
    )
    .await
}

/// Accept an invitation token (delivered via the `localforge://invite`
/// deep link OR pasted by the user). Returns the org id they joined
/// so the UI can switch to it.
#[tauri::command]
pub async fn cloud_orgs_accept_invite(token: String) -> Result<String, api::ApiError> {
    let bearer = auth::current_token().ok_or_else(unauth)?;
    localforge_cloud_client::orgs::accept_invite(&token, &bearer).await
}
