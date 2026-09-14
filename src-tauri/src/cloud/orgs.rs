//! Desktop org / member / invitation commands over `localforge-cloud-client::orgs`.

use base64::Engine;

use super::{api, auth};

pub use localforge_cloud_client::orgs::{Invitation, OrgInfo, OrgSummary};

/// New invitation: id plus the base64 invite secret for the link #fragment (never sent to the cloud).
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

/// Whether the active org is ours; `true` when no org is pinned. The sync push consults it.
static ACTIVE_ORG_OWNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn active_org_owned() -> bool {
    ACTIVE_ORG_OWNED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Pin every subsequent cloud call to an org (`X-LocalForge-Org`); `None` on sign-out.
/// `is_owner` says whether we own it (defaults to owned only when nothing is pinned).
#[tauri::command(rename_all = "camelCase")]
pub fn cloud_set_active_org(org_id: Option<String>, is_owner: Option<bool>) {
    let pinned = org_id.as_deref().is_some_and(|s| !s.trim().is_empty());
    ACTIVE_ORG_OWNED.store(
        is_owner.unwrap_or(!pinned),
        std::sync::atomic::Ordering::Relaxed,
    );
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
    // Handoff: wrap the active org's DEK with a fresh per-invite secret so the invitee can decrypt on accept.
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

// Per-server access scopes (Team)

/// A server a member is scoped to; `expires_at` (ms) marks a temporary grant. Empty list = unrestricted.
#[derive(serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MemberScope {
    pub server_id: String,
    #[serde(default)]
    pub expires_at: Option<i64>,
}

#[derive(serde::Deserialize)]
struct ScopesResp {
    scopes: Vec<MemberScope>,
}

#[derive(serde::Serialize)]
struct ScopesBody<'a> {
    scopes: &'a [MemberScope],
}

/// Read a member's per-server access scope (empty = full access).
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_member_scopes_get(
    org_id: String,
    user_id: String,
) -> Result<Vec<MemberScope>, api::ApiError> {
    let token = auth::current_token().ok_or_else(unauth)?;
    let r: ScopesResp = api::get(
        &format!("/v1/orgs/{org_id}/members/{user_id}/scopes"),
        Some(&token),
    )
    .await?;
    Ok(r.scopes)
}

/// Replace a member's per-server access scope. Empty list = unrestricted.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_member_scopes_set(
    org_id: String,
    user_id: String,
    scopes: Vec<MemberScope>,
) -> Result<(), api::ApiError> {
    let token = auth::current_token().ok_or_else(unauth)?;
    let _: serde_json::Value = api::put(
        &format!("/v1/orgs/{org_id}/members/{user_id}/scopes"),
        &ScopesBody { scopes: &scopes },
        Some(&token),
    )
    .await?;
    Ok(())
}

/// Accept an invite; with a handoff `secret` the org DEK is unwrapped and adopted immediately.
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
                    // Seal the DEK to our own pubkey for durable cross-device access (best-effort).
                    if let Some(uid) = res.user_id.as_deref() {
                        let _ = super::vault::self_seal_grant(&res.org_id, uid, &dek, &bearer).await;
                    }
                }
            }
        }
    }
    Ok(res.org_id)
}
