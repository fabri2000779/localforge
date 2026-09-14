//! Node enrollment commands; the desktop passes its own NodeId so the cloud row id matches
//! the id commands are routed by.

use super::{api, auth};
use crate::backend::NodeRegistry;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::State;

pub use localforge_cloud_client::nodes::{Machine, NodeCreated, NodeSummary};

/// Once-per-session guard for `cloud_claim_desktop` (idempotent server-side; spares requests).
static DESKTOP_CLAIMED: AtomicBool = AtomicBool::new(false);

/// Reset the claim guard on logout / account switch.
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

/// Enroll (or re-link) a node; returns the one-time blob for `localforge-agent link`.
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

/// Every machine in the org (desktops + agents) for the cross-machine switcher.
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

/// Claim this machine as a desktop node in the signed-in user's org (once per session).
/// Returns `false` when signed out or before the local node exists.
#[tauri::command]
pub async fn cloud_claim_desktop(state: State<'_, NodeRegistry>) -> Result<bool, api::ApiError> {
    if DESKTOP_CLAIMED.load(Ordering::Relaxed) {
        return Ok(false);
    }
    let Some(token) = auth::current_token() else {
        return Ok(false);
    };
    // The local node is installed asynchronously once Docker is up; wait briefly for it.
    let mut machine = state.this_machine().await;
    let mut waited = 0;
    while machine.is_none() && waited < 20 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        machine = state.this_machine().await;
        waited += 1;
    }
    let Some(machine) = machine else {
        return Ok(false);
    };
    match localforge_cloud_client::nodes::claim_desktop(&machine.id, &machine.name, &token).await {
        Ok(_) => {
            DESKTOP_CLAIMED.store(true, Ordering::Relaxed);
            Ok(true)
        }
        Err(e) => {
            // Log it (the frontend call is fire-and-forget); the guard stays unset so a later sign-in retries.
            tracing::warn!("[cloud] desktop claim failed: {}", e);
            Err(e)
        }
    }
}
