//! Node management commands for the "Nodes" UI.

use crate::backend::{NodeRecord, NodeRegistry, ThisMachine};
use crate::commands::require_backend;
use localforge_backend_remote::RemoteAgentConfig;
use localforge_core::{DockerInfo, NodeId, NodeStats};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
pub struct AddRemoteNodeRequest {
    pub label: String,
    /// Base URL of the agent, e.g. `https://1.2.3.4:7878`.
    pub url: String,
    /// Bearer token printed by the install script.
    pub token: String,
    /// SHA-256 cert fingerprint; empty/None for a CA-signed cert.
    #[serde(default)]
    pub fingerprint: Option<String>,
}

/// List all known nodes (local + remote, online or not).
#[tauri::command]
pub async fn list_nodes(state: State<'_, NodeRegistry>) -> Result<Vec<NodeRecord>, String> {
    Ok(state.list_records().await)
}

/// This machine's identity; `None` before the local node is installed.
#[tauri::command]
pub async fn get_this_machine(
    state: State<'_, NodeRegistry>,
) -> Result<Option<ThisMachine>, String> {
    Ok(state.this_machine().await)
}

/// Rename this machine (the id never changes).
#[tauri::command(rename_all = "camelCase")]
pub async fn set_machine_name(
    name: String,
    state: State<'_, NodeRegistry>,
) -> Result<ThisMachine, String> {
    state
        .set_machine_name(name)
        .await
        .map_err(|e| e.to_string())
}

/// Mark the first-run "name this machine" prompt as handled (persisted in `this_machine.toml`).
#[tauri::command]
pub async fn set_machine_name_prompt_dismissed(
    state: State<'_, NodeRegistry>,
) -> Result<ThisMachine, String> {
    state
        .set_name_prompt_dismissed()
        .await
        .map_err(|e| e.to_string())
}

/// Probe a candidate agent ("Test connection"); returns its Docker info.
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

/// Persist and connect a new remote agent; returns the saved record.
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

/// Copy-pasteable install one-liners (Linux + Windows); `version` is the release tag (default "latest").
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInstallCommands {
    pub linux: String,
    pub windows: String,
}

#[tauri::command(rename_all = "camelCase")]
pub fn agent_install_command(
    version: Option<String>,
    domain: Option<String>,
    label: Option<String>,
) -> AgentInstallCommands {
    let version = version.as_deref().unwrap_or("latest");
    let domain = domain.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let label = label.as_deref().map(str::trim).filter(|s| !s.is_empty());

    // GitHub serves "latest" at a different path than a pinned tag.
    let base = if version == "latest" {
        "https://github.com/fabri2000779/localforge/releases/latest/download".to_string()
    } else {
        format!("https://github.com/fabri2000779/localforge/releases/download/{version}")
    };

    let mut linux_env = String::new();
    if let Some(d) = domain {
        linux_env.push_str(&format!("LOCALFORGE_AGENT_DOMAIN={} ", shell_escape(d)));
    }
    if let Some(l) = label {
        linux_env.push_str(&format!("LOCALFORGE_AGENT_LABEL={} ", shell_escape(l)));
    }
    // Env vars go after `sudo` (env_reset strips them otherwise); `-f` makes curl fail loudly.
    let linux = format!(
        "curl -fsSL {base}/install-agent.sh | sudo {env}bash",
        base = base,
        env = linux_env,
    );

    // PowerShell (elevated); `$env:` assignments pass through to the script.
    let mut ps_env = String::new();
    if let Some(d) = domain {
        ps_env.push_str(&format!("$env:LOCALFORGE_AGENT_DOMAIN = {}; ", ps_quote(d)));
    }
    if let Some(l) = label {
        ps_env.push_str(&format!("$env:LOCALFORGE_AGENT_LABEL = {}; ", ps_quote(l)));
    }
    let windows = format!(
        "{env}iex \"& {{ $(irm {base}/install-agent.ps1) }}\"",
        env = ps_env,
        base = base,
    );

    AgentInstallCommands { linux, windows }
}

/// Compact 8-char hex id for a new node.
fn short_id() -> String {
    Uuid::new_v4().simple().to_string()[..8].to_string()
}

/// Shell-quote a value unless it's plain alphanumeric.
fn shell_escape(s: &str) -> String {
    if s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '/')) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

/// PowerShell single-quote (escape `'` as `''`).
fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

#[derive(Serialize)]
pub struct ClusterSummary {
    pub total_nodes: usize,
    pub online_nodes: usize,
    pub containers_running: u64,
    pub containers_total: u64,
    pub images: u64,
}

/// Host-level metrics for a node (Nodes page gauges).
#[tauri::command(rename_all = "camelCase")]
pub async fn get_node_stats(
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<NodeStats, String> {
    require_backend(&state, node_id.as_deref())
        .await?
        .node_stats()
        .await
        .map_err(|e| e.to_string())
}

/// Aggregate docker_info across every node; offline nodes count toward total_nodes only.
#[tauri::command]
pub async fn cluster_summary(state: State<'_, NodeRegistry>) -> Result<ClusterSummary, String> {
    let records = state.list_records().await;
    let total_nodes = records.len();
    let mut online_nodes = 0usize;
    let mut containers_running = 0u64;
    let mut containers_total = 0u64;
    let mut images = 0u64;

    for rec in records {
        if let Some(backend) = state.backend(&rec.id).await {
            if let Ok(info) = backend.docker_info().await {
                online_nodes += 1;
                containers_running += info.containers_running;
                containers_total += info.containers_total;
                images += info.images;
            }
        }
    }

    Ok(ClusterSummary {
        total_nodes,
        online_nodes,
        containers_running,
        containers_total,
        images,
    })
}
