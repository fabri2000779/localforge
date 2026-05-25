//! Cloud node (relay agent) enrollment client — desktop-driven.
//!
//! Enroll a VPS agent for direct relay control (returns a one-time blob the
//! operator pastes into `localforge-agent link <blob>`), list the org's
//! enrolled agents with live online status, and revoke. The agent itself
//! never calls these — only the owner's desktop does. See the cloud's
//! docs/adr/0001-agent-direct-relay.md.

use serde::{Deserialize, Serialize};

use crate::api::{self, ApiError};

/// Result of enrolling/re-linking a node. `enrollment_blob` is shown ONCE —
/// the raw token isn't recoverable later.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeCreated {
    pub node: NodeRef,
    #[serde(rename = "enrollmentBlob")]
    pub enrollment_blob: String,
    #[serde(rename = "nodeToken")]
    pub node_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRef {
    pub id: String,
    pub name: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

/// A row from `GET /v1/nodes`. `online` is live (the relay DO's socket set),
/// `last_seen_at` is the offline "last seen" label.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSummary {
    pub id: String,
    pub name: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "lastSeenAt")]
    pub last_seen_at: Option<i64>,
    pub revoked: bool,
    pub online: bool,
}

#[derive(Serialize)]
struct CreateBody<'a> {
    name: &'a str,
    /// The desktop's existing NodeId, so the cloud row id == the nodeId the
    /// mobile stamps on commands (required for routing to reach this agent).
    node_id: &'a str,
}

/// Enroll (or re-link) `node_id` under `name`. Returns the one-time blob.
pub async fn create(name: &str, node_id: &str, bearer: &str) -> Result<NodeCreated, ApiError> {
    api::post("/v1/nodes", &CreateBody { name, node_id }, Some(bearer)).await
}

#[derive(Serialize)]
struct ClaimDesktopBody<'a> {
    node_id: &'a str,
    name: &'a str,
}

#[derive(Deserialize)]
struct DesktopClaimResp {
    node: NodeRef,
}

/// Claim THIS machine as a desktop node in the caller's org. The cloud ADOPTS
/// the locally-minted `node_id` (so it matches what the device already uses
/// offline), registers it `kind='desktop'` (uncapped, JWT-authed — no token),
/// and returns the row. Idempotent: re-claiming just refreshes the name.
pub async fn claim_desktop(node_id: &str, name: &str, bearer: &str) -> Result<NodeRef, ApiError> {
    let r: DesktopClaimResp = api::put(
        "/v1/nodes/desktop",
        &ClaimDesktopBody { node_id, name },
        Some(bearer),
    )
    .await?;
    Ok(r.node)
}

#[derive(Deserialize)]
struct NodesList {
    nodes: Vec<NodeSummary>,
}

pub async fn list(bearer: &str) -> Result<Vec<NodeSummary>, ApiError> {
    let r: NodesList = api::get("/v1/nodes", Some(bearer)).await?;
    Ok(r.nodes)
}

/// A machine in the org — a desktop OR an agent. Powers the cross-machine
/// switcher: owner + sub-users enumerate everything they can address.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Machine {
    pub id: String,
    pub name: String,
    /// "desktop" | "agent".
    pub kind: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "lastSeenAt")]
    pub last_seen_at: Option<i64>,
    pub online: bool,
}

#[derive(Deserialize)]
struct MachinesList {
    machines: Vec<Machine>,
}

/// Every machine in the caller's org (desktops + agents), with live online
/// status from the relay.
pub async fn machines(bearer: &str) -> Result<Vec<Machine>, ApiError> {
    let r: MachinesList = api::get("/v1/nodes/machines", Some(bearer)).await?;
    Ok(r.machines)
}

#[derive(Deserialize)]
struct RevokeResp {
    #[allow(dead_code)]
    revoked: bool,
}

pub async fn revoke(id: &str, bearer: &str) -> Result<(), ApiError> {
    let _: RevokeResp = api::delete(&format!("/v1/nodes/{}", id), Some(bearer)).await?;
    Ok(())
}
