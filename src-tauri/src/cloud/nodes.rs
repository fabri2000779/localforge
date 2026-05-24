//! Desktop cloud-node (relay agent) enrollment commands.
//!
//! Thin `#[tauri::command]` adapters over `localforge-cloud-client::nodes`:
//! read the bearer token from the OS keychain and delegate. The desktop
//! passes its OWN NodeId on enroll so the cloud row id equals the nodeId the
//! mobile stamps on commands — which is what makes a command route to this
//! agent instead of falling back through the desktop.

use super::{api, auth};

#[allow(unused_imports)]
pub use localforge_cloud_client::nodes::{NodeCreated, NodeRef, NodeSummary};

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

/// Revoke a node — the cloud refuses its token and drops any live socket.
#[tauri::command]
pub async fn cloud_node_revoke(node_id: String) -> Result<(), api::ApiError> {
    let token = auth::current_token().ok_or_else(unauth)?;
    localforge_cloud_client::nodes::revoke(&node_id, &token).await
}
