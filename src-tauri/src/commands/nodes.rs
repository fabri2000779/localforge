//! Tauri commands that the "Nodes" UI uses to list, add, remove, test
//! and re-connect remote agents.

use crate::backend::{NodeKindRecord, NodeRecord, NodeRegistry};
use localforge_backend_remote::RemoteAgentConfig;
use localforge_core::{DockerInfo, NodeId};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
pub struct AddRemoteNodeRequest {
    /// Human-readable name shown in the UI.
    pub label: String,
    /// Base URL of the agent, e.g. `https://1.2.3.4:7878`.
    pub url: String,
    /// Bearer token printed by the install script.
    pub token: String,
    /// SHA-256 cert fingerprint. Leave empty/None when the agent uses a
    /// real CA-signed certificate (e.g. Let's Encrypt).
    #[serde(default)]
    pub fingerprint: Option<String>,
}

/// List all known nodes (local + remote, online or not).
#[tauri::command]
pub async fn list_nodes(state: State<'_, NodeRegistry>) -> Result<Vec<NodeRecord>, String> {
    Ok(state.list_records().await)
}

/// Try to reach a candidate agent — used by the "Test connection" button
/// in the Add Node wizard. Returns the agent's Docker info so the UI can
/// surface "you're connected to a 4-core Debian 12 host" feedback.
#[tauri::command(rename_all = "camelCase")]
pub async fn test_remote_node(req: AddRemoteNodeRequest) -> Result<DockerInfo, String> {
    let normalized_fp = req
        .fingerprint
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    NodeRegistry::probe(RemoteAgentConfig {
        url: req.url,
        token: req.token,
        fingerprint: normalized_fp,
    })
    .await
    .map_err(|e| e.to_string())
}

/// Persist + connect a new remote agent. Returns the saved record so the
/// UI can append it to its list without a reload.
#[tauri::command(rename_all = "camelCase")]
pub async fn add_remote_node(
    req: AddRemoteNodeRequest,
    state: State<'_, NodeRegistry>,
) -> Result<NodeRecord, String> {
    let id = short_id();
    let normalized_fp = req
        .fingerprint
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    state
        .add_remote(
            id,
            req.label.clone(),
            RemoteAgentConfig {
                url: req.url,
                token: req.token,
                fingerprint: normalized_fp,
            },
        )
        .await
        .map_err(|e| e.to_string())
}

/// Remove a remote node (rejects attempts to remove the local one).
#[tauri::command(rename_all = "camelCase")]
pub async fn remove_node(
    node_id: String,
    state: State<'_, NodeRegistry>,
) -> Result<(), String> {
    state
        .remove(&NodeId::new(&node_id))
        .await
        .map_err(|e| e.to_string())
}

/// Re-attempt the agent connection (e.g. after the VPS came back online).
#[tauri::command(rename_all = "camelCase")]
pub async fn reconnect_node(
    node_id: String,
    state: State<'_, NodeRegistry>,
) -> Result<(), String> {
    state
        .reconnect(&NodeId::new(&node_id))
        .await
        .map_err(|e| e.to_string())
}

/// Build the copy-pasteable one-liner the user runs on their VPS. The
/// `version` is the GitHub release tag (defaults to "latest").
#[tauri::command(rename_all = "camelCase")]
pub fn agent_install_command(
    version: Option<String>,
    domain: Option<String>,
    label: Option<String>,
) -> String {
    let version = version.as_deref().unwrap_or("latest");
    let mut env_prefix = String::new();
    if let Some(d) = domain.as_deref().filter(|s| !s.trim().is_empty()) {
        env_prefix.push_str(&format!("LOCALFORGE_AGENT_DOMAIN={} ", shell_escape(d.trim())));
    }
    if let Some(l) = label.as_deref().filter(|s| !s.trim().is_empty()) {
        env_prefix.push_str(&format!("LOCALFORGE_AGENT_LABEL={} ", shell_escape(l.trim())));
    }
    format!(
        "curl -sSL https://github.com/fabri2000779/localforge/releases/download/{ver}/install-agent.sh | {env}sudo bash",
        ver = version,
        env = env_prefix,
    )
}

/// Compact 8-char hex id for a new node — collision risk is negligible
/// for the handful of nodes a user manages.
fn short_id() -> String {
    Uuid::new_v4().simple().to_string()[..8].to_string()
}

/// Escape a value for the shell command shown to the user. We bias
/// towards "if it contains anything weird, quote it" — overzealous but
/// correct.
fn shell_escape(s: &str) -> String {
    if s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '/')) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

/// Used by the Nodes UI to distinguish local from remote at a glance —
/// helps the front-end render the right icon without re-reading the
/// `kind` enum repeatedly.
#[allow(dead_code)]
pub fn is_local(rec: &NodeRecord) -> bool {
    matches!(rec.kind, NodeKindRecord::Local)
}
