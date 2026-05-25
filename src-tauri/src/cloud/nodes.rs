//! Desktop cloud-node (relay agent) enrollment commands.
//!
//! Thin `#[tauri::command]` adapters over `localforge-cloud-client::nodes`:
//! read the bearer token from the OS keychain and delegate. The desktop
//! passes its OWN NodeId on enroll so the cloud row id equals the nodeId the
//! mobile stamps on commands — which is what makes a command route to this
//! agent instead of falling back through the desktop.

use super::{api, auth};
use crate::backend::NodeRegistry;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::State;

#[allow(unused_imports)]
pub use localforge_cloud_client::nodes::{Machine, NodeCreated, NodeRef, NodeSummary};

/// Whether THIS machine has already been claimed into the cloud during this
/// session. Guards `cloud_claim_desktop` so we hit the Worker at most once per
/// login (the call is idempotent server-side; this just spares requests — see
/// the sync/relay request-budget rules). Reset on logout / account switch.
static DESKTOP_CLAIMED: AtomicBool = AtomicBool::new(false);

/// Clear the once-per-session claim guard so the next sign-in re-claims (e.g.
/// after logout or switching accounts, where the target org may differ).
pub fn reset_desktop_claim() {
    DESKTOP_CLAIMED.store(false, Ordering::Relaxed);
}

fn unauth() -> api::ApiError {
    api::ApiError::Server {
        status: 401,
        code: "unauthenticated".into(),
        message: None,
    }
}

/// Enroll (or re-link) a node for direct relay control. `node_id` is the
/// desktop's NodeId for the agent. Returns a one-time enrollment blob to
/// paste into `localforge-agent link <blob>` on the VPS.
#[tauri::command]
pub async fn cloud_node_create(node_id: String, name: String) -> Result<NodeCreated, api::ApiError> {
    let token = auth::current_token().ok_or_else(unauth)?;
    localforge_cloud_client::nodes::create(&name, &node_id, &token).await
}

/// List the org's enrolled agents with live online status.
#[tauri::command]
pub async fn cloud_node_list() -> Result<Vec<NodeSummary>, api::ApiError> {
    let token = auth::current_token().ok_or_else(unauth)?;
    localforge_cloud_client::nodes::list(&token).await
}

/// List EVERY machine in the org — desktops + agents — for the cross-machine
/// switcher. Sub-users use this to see and target all of the owner's machines.
#[tauri::command]
pub async fn cloud_list_machines() -> Result<Vec<Machine>, api::ApiError> {
    let token = auth::current_token().ok_or_else(unauth)?;
    localforge_cloud_client::nodes::machines(&token).await
}

/// Revoke a node — the cloud refuses its token and drops any live socket.
#[tauri::command]
pub async fn cloud_node_revoke(node_id: String) -> Result<(), api::ApiError> {
    let token = auth::current_token().ok_or_else(unauth)?;
    localforge_cloud_client::nodes::revoke(&node_id, &token).await
}

/// Claim THIS machine as a desktop node in the signed-in user's org so the
/// cloud adopts its stable id — enabling relay routing to this specific
/// machine and sub-user visibility. Idempotent server-side; guarded to once
/// per session here to spare Worker requests. Silently no-ops when signed out
/// or before the local node exists (Docker not yet reachable), and returns
/// `false` in those cases. Safe to call on every login / startup.
#[tauri::command]
pub async fn cloud_claim_desktop(state: State<'_, NodeRegistry>) -> Result<bool, api::ApiError> {
    if DESKTOP_CLAIMED.load(Ordering::Relaxed) {
        return Ok(false);
    }
    let Some(token) = auth::current_token() else {
        return Ok(false);
    };
    let Some(machine) = state.this_machine().await else {
        return Ok(false);
    };
    localforge_cloud_client::nodes::claim_desktop(&machine.id, &machine.name, &token).await?;
    DESKTOP_CLAIMED.store(true, Ordering::Relaxed);
    Ok(true)
}
