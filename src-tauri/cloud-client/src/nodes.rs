//! Node (relay agent) enrollment: enroll, list with live status, revoke. Desktop-driven.

use serde::{Deserialize, Serialize};

use crate::api::{self, ApiError};

/// Enrollment result; `enrollment_blob` is shown once and is not recoverable later.
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

/// Row of `GET /v1/nodes`; `online` is live from the relay.
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
    /// The desktop's NodeId, so the cloud row id matches the id commands are routed by.
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

/// Register this machine as a desktop node in the caller's org; idempotent.
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

/// A machine in the org (desktop or agent), for the cross-machine switcher.
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

/// Every machine in the caller's org, with live online status.
pub async fn machines(bearer: &str) -> Result<Vec<Machine>, ApiError> {
    let r: MachinesList = api::get("/v1/nodes/machines", Some(bearer)).await?;
    Ok(r.machines)
}

pub async fn revoke(id: &str, bearer: &str) -> Result<(), ApiError> {
    let _: serde_json::Value = api::delete(&format!("/v1/nodes/{}", id), Some(bearer)).await?;
    Ok(())
}
