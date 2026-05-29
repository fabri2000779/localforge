// Commands module - Tauri command handlers

pub mod backups;
pub mod docker;
pub mod files;
pub mod games;
pub mod metrics;
pub mod nodes;
pub mod players;
pub mod schedules;
pub mod server;

use crate::backend::{DynBackend, NodeRegistry};
use localforge_core::NodeId;
use tauri::State;

/// Resolve the requested node's backend, or "local" if the caller didn't
/// specify one. Used by every node-scoped command — keeps the Tauri
/// handler bodies down to a single line of routing.
pub(crate) async fn require_backend(
    state: &State<'_, NodeRegistry>,
    node_id: Option<&str>,
) -> Result<DynBackend, String> {
    let id = node_id
        .map(NodeId::new)
        .unwrap_or_else(NodeId::local);
    state
        .backend(&id)
        .await
        .ok_or_else(|| format!("Node '{}' is not connected", id))
}
