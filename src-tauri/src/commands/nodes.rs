//! Tauri commands that the "Nodes" UI uses to list, add, remove, test
//! and re-connect remote agents.

use crate::backend::{NodeKindRecord, NodeRecord, NodeRegistry, ThisMachine};
use crate::commands::require_backend;
use localforge_backend_remote::RemoteAgentConfig;
use localforge_core::{DockerInfo, NodeId, NodeStats};
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

/// This machine's stable identity (id + name). `None` before the local
/// node is installed (i.e. Docker not yet reachable).
#[tauri::command]
pub async fn get_this_machine(
    state: State<'_, NodeRegistry>,
) -> Result<Option<ThisMachine>, String> {
    Ok(state.this_machine().await)
}

/// Rename this machine. The stable id is never changed — only the label
/// the user sees in the switcher.
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

/// Both copy-pasteable one-liners (Linux + Windows) so the wizard can
/// toggle between them without round-tripping to the backend on every
/// click. `version` is the GitHub release tag (defaults to "latest").
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

    // GitHub serves "latest" at a DIFFERENT path than a pinned tag:
    //   latest : /releases/latest/download/<asset>
    //   tag    : /releases/download/<tag>/<asset>
    // Building `/releases/download/latest/...` 404s, and `curl -sSL`
    // (no -f) would pipe the "Not Found" page into the shell, producing
    // the cryptic `Not: command not found`.
    let base = if version == "latest" {
        "https://github.com/fabri2000779/localforge/releases/latest/download".to_string()
    } else {
        format!("https://github.com/fabri2000779/localforge/releases/download/{version}")
    };

    // Linux: env-var prefix before sudo bash. POSIX shell quoting.
    let mut linux_env = String::new();
    if let Some(d) = domain {
        linux_env.push_str(&format!("LOCALFORGE_AGENT_DOMAIN={} ", shell_escape(d)));
    }
    if let Some(l) = label {
        linux_env.push_str(&format!("LOCALFORGE_AGENT_LABEL={} ", shell_escape(l)));
    }
    // The env vars go AFTER `sudo` (`sudo VAR=x bash`), not before. With
    // them before (`VAR=x sudo bash`) the shell sets them on the sudo
    // process, whose default env_reset strips them before exec'ing bash —
    // so the install script never sees DOMAIN/LABEL. `-f` makes curl fail
    // loudly on a bad URL instead of piping an HTML error page into bash.
    let linux = format!(
        "curl -fsSL {base}/install-agent.sh | sudo {env}bash",
        base = base,
        env = linux_env,
    );

    // Windows: PowerShell — must be run elevated. Assigning $env:VAR
    // before the iex invocation passes the value through to the script.
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

/// Quote a value for PowerShell. Always single-quote so `$`, backticks
/// and `"` inside the label don't get expanded; PowerShell's escape for
/// a literal single-quote inside a single-quoted string is `''`.
fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Used by the Nodes UI to distinguish local from remote at a glance —
/// helps the front-end render the right icon without re-reading the
/// `kind` enum repeatedly.
#[allow(dead_code)]
pub fn is_local(rec: &NodeRecord) -> bool {
    matches!(rec.kind, NodeKindRecord::Local)
}

#[derive(Serialize)]
pub struct ClusterSummary {
    pub total_nodes: usize,
    pub online_nodes: usize,
    pub containers_running: u64,
    pub containers_total: u64,
    pub images: u64,
}

/// Host-level metrics for a specific node — CPU%, mem, disk, uptime.
/// Used by the Nodes page to render per-node health gauges.
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

/// Aggregate docker_info across every registered node (local + every
/// reachable remote). Offline nodes are simply skipped — they count
/// towards total_nodes but not online_nodes.
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
