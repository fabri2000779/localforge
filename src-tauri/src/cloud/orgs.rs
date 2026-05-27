//! Desktop org / member / invitation commands.
//!
//! All wire types + HTTP calls live in
//! `localforge-cloud-client::orgs` so desktop and mobile see the
//! identical shapes. The desktop layer here is just `#[tauri::command]`
//! adapters that read the bearer token from the OS keychain and
//! delegate. Re-exports keep `cloud::orgs::{OrgInfo, Member, …}` at
//! the same path so the React layer's TypeScript bindings don't
//! shift.

use base64::Engine;

use super::{api, auth};

#[allow(unused_imports)]
pub use localforge_cloud_client::orgs::{Invitation, Member, OrgInfo, OrgSummary};

/// Result of creating an invite: the invitation id + the base64 invite
/// secret the caller embeds in the link #fragment (so the invitee can unwrap
/// the handoff DEK on accept). The secret is NEVER sent to the cloud.
#[derive(serde::Serialize)]
pub struct InviteCreated {
    pub id: String,
    pub secret: String,
}

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

#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_orgs_invite(
    org_id: String,
    email: String,
    role: String,
) -> Result<InviteCreated, api::ApiError> {
    let token = auth::current_token().ok_or_else(unauth)?;
    // Handoff: wrap the ACTIVE org's DEK with a fresh per-invite secret so the
    // invitee decrypts the instant they accept — no waiting for us to seal.
    // active_dek() is our own DEK when inviting to our own org, or the org DEK
    // we hold (admin-member) when inviting to an org we belong to.
    let dek = super::vault::active_dek().map_err(|e| api::ApiError::Decode(format!("vault: {e}")))?;
    let secret = super::vault::generate_key();
    let wrapped = super::vault::wrap_dek(&secret, &dek).map_err(api::ApiError::Decode)?;
    let secret_b64 = base64::engine::general_purpose::STANDARD.encode(secret);
    let resp = localforge_cloud_client::orgs::invite_with_dek(
        &org_id,
        &email,
        &role,
        Some(&wrapped),
        &token,
    )
    .await?;
    let id = resp
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    Ok(InviteCreated { id, secret: secret_b64 })
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
/// deep link OR pasted by the user). Returns the org id they joined so the UI
/// can switch to it. `secret` is the base64 invite secret from the link
/// #fragment — when present (handoff invite), we unwrap the org DEK and adopt
/// it so the new member decrypts immediately + durably.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_orgs_accept_invite(
    token: String,
    secret: Option<String>,
) -> Result<String, api::ApiError> {
    let bearer = auth::current_token().ok_or_else(unauth)?;
    let res = localforge_cloud_client::orgs::accept_invite_full(&token, &bearer).await?;
    if let (Some(secret_b64), Some(wrapped)) = (secret, res.wrapped_dek.as_ref()) {
        if let Ok(s) = base64::engine::general_purpose::STANDARD.decode(secret_b64.trim()) {
            if s.len() == 32 {
                let mut sk = [0u8; 32];
                sk.copy_from_slice(&s);
                if let Ok(dek) = super::vault::unwrap_dek(&sk, wrapped) {
                    super::vault::adopt_org_dek(&res.org_id, &dek);
                }
            }
        }
    }
    Ok(res.org_id)
}
