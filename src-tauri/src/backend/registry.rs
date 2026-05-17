//! Multi-node registry: keeps an [`Arc<dyn NodeBackend>`] per known node
//! and persists their connection configs to `~/LocalForge/nodes.toml`.
//!
//! The local node always exists with id `"local"`; remote nodes are
//! added/removed at runtime via the "Add Node" UI.

use crate::backend::DynBackend;
use crate::paths;
use localforge_backend_local::LocalDockerBackend;
use localforge_backend_remote::{RemoteAgentBackend, RemoteAgentConfig};
use localforge_core::NodeId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// User-visible record of a node (everything except its live backend
/// handle). This is what the UI lists in the "Nodes" page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRecord {
    pub id: NodeId,
    pub label: String,
    pub kind: NodeKindRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NodeKindRecord {
    Local,
    Remote {
        url: String,
        /// `None` when the agent has a real CA-signed cert.
        fingerprint: Option<String>,
        // Token is intentionally not surfaced in the listing — UI uses a
        // separate command to "reveal" it if the user wants to re-copy it.
    },
}

/// On-disk shape: just the remote nodes (local is implicit).
#[derive(Debug, Default, Serialize, Deserialize)]
struct NodesFile {
    #[serde(default)]
    nodes: Vec<StoredRemoteNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredRemoteNode {
    id: String,
    label: String,
    url: String,
    token: String,
    #[serde(default)]
    fingerprint: Option<String>,
}

#[derive(Default)]
pub struct NodeRegistry {
    inner: RwLock<RegistryInner>,
}

#[derive(Default)]
struct RegistryInner {
    backends: HashMap<NodeId, DynBackend>,
    records: HashMap<NodeId, NodeRecord>,
}

impl NodeRegistry {
    pub fn nodes_file() -> PathBuf {
        paths::home_root().join("nodes.toml")
    }

    /// Replace the local backend (called once Docker is reachable).
    pub async fn install_local(&self, backend: DynBackend) {
        let mut state = self.inner.write().await;
        state.backends.insert(NodeId::local(), backend);
        state.records.insert(
            NodeId::local(),
            NodeRecord {
                id: NodeId::local(),
                label: "This machine".to_string(),
                kind: NodeKindRecord::Local,
            },
        );
    }

    /// Reload remote node configs from disk and try to connect to each.
    /// Errors per-node are logged but don't fail the whole load — a node
    /// that's offline still appears in the list, just without a live
    /// backend.
    pub async fn load_remotes(&self) -> anyhow::Result<()> {
        let path = Self::nodes_file();
        if !path.exists() {
            return Ok(());
        }
        let body = std::fs::read_to_string(&path)?;
        let file: NodesFile = toml::from_str(&body)?;

        for node in file.nodes {
            let node_id = NodeId::new(&node.id);
            let label = node.label.clone();
            let url = node.url.clone();
            let fingerprint = node.fingerprint.clone();

            // Record always present so the UI can show it as offline if
            // the connection fails.
            {
                let mut state = self.inner.write().await;
                state.records.insert(
                    node_id.clone(),
                    NodeRecord {
                        id: node_id.clone(),
                        label,
                        kind: NodeKindRecord::Remote {
                            url: url.clone(),
                            fingerprint: fingerprint.clone(),
                        },
                    },
                );
            }

            match RemoteAgentBackend::connect(RemoteAgentConfig {
                url,
                token: node.token,
                fingerprint,
            })
            .await
            {
                Ok(backend) => {
                    let mut state = self.inner.write().await;
                    state.backends.insert(node_id, Arc::new(backend));
                }
                Err(e) => {
                    tracing::warn!("remote node '{}' unreachable: {}", node_id, e);
                }
            }
        }
        Ok(())
    }

    /// Try to connect a candidate remote agent (without persisting it) —
    /// used by the UI's "Test connection" button. Returns the agent's
    /// reported Docker info so the user can confirm they reached the
    /// right machine.
    pub async fn probe(
        cfg: RemoteAgentConfig,
    ) -> Result<localforge_core::DockerInfo, localforge_core::BackendError> {
        let backend = RemoteAgentBackend::connect(cfg).await?;
        use localforge_core::NodeBackend;
        backend.docker_info().await
    }

    /// Persist + activate a new remote node. Fails if `id` already
    /// exists or the agent isn't reachable.
    pub async fn add_remote(
        &self,
        id: String,
        label: String,
        cfg: RemoteAgentConfig,
    ) -> anyhow::Result<NodeRecord> {
        let node_id = NodeId::new(&id);
        if node_id.is_local() {
            anyhow::bail!("'{}' is reserved for the local node", NodeId::LOCAL);
        }
        {
            let state = self.inner.read().await;
            if state.records.contains_key(&node_id) {
                anyhow::bail!("a node with id '{}' already exists", id);
            }
        }

        let backend = RemoteAgentBackend::connect(cfg.clone())
            .await
            .map_err(|e| anyhow::anyhow!("agent unreachable: {}", e))?;

        // Persist BEFORE inserting into memory so the file is the source
        // of truth on next launch.
        let mut file = self.read_nodes_file()?;
        file.nodes.push(StoredRemoteNode {
            id: id.clone(),
            label: label.clone(),
            url: cfg.url.clone(),
            token: cfg.token.clone(),
            fingerprint: cfg.fingerprint.clone(),
        });
        self.write_nodes_file(&file)?;

        let record = NodeRecord {
            id: node_id.clone(),
            label,
            kind: NodeKindRecord::Remote {
                url: cfg.url,
                fingerprint: cfg.fingerprint,
            },
        };
        let mut state = self.inner.write().await;
        state.backends.insert(node_id.clone(), Arc::new(backend));
        state.records.insert(node_id, record.clone());
        Ok(record)
    }

    pub async fn remove(&self, id: &NodeId) -> anyhow::Result<()> {
        if id.is_local() {
            anyhow::bail!("cannot remove the local node");
        }
        let mut file = self.read_nodes_file()?;
        file.nodes.retain(|n| n.id != id.as_str());
        self.write_nodes_file(&file)?;

        let mut state = self.inner.write().await;
        state.backends.remove(id);
        state.records.remove(id);
        Ok(())
    }

    pub async fn list_records(&self) -> Vec<NodeRecord> {
        let state = self.inner.read().await;
        let mut out: Vec<_> = state.records.values().cloned().collect();
        out.sort_by(|a, b| {
            // Local first, then by label.
            match (a.id.is_local(), b.id.is_local()) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.label.cmp(&b.label),
            }
        });
        out
    }

    pub async fn backend(&self, id: &NodeId) -> Option<DynBackend> {
        self.inner.read().await.backends.get(id).cloned()
    }

    /// Convenience for the very common "use the local backend" code path.
    pub async fn local(&self) -> Option<DynBackend> {
        self.backend(&NodeId::local()).await
    }

    /// Re-attempt connection to a remote node (used by the "reconnect"
    /// button on offline nodes).
    pub async fn reconnect(&self, id: &NodeId) -> anyhow::Result<()> {
        if id.is_local() {
            // Local reconnect is handled separately via Docker probe.
            let backend = LocalDockerBackend::connect(paths::home_root()).await?;
            let mut state = self.inner.write().await;
            state.backends.insert(id.clone(), Arc::new(backend));
            return Ok(());
        }

        let file = self.read_nodes_file()?;
        let stored = file
            .nodes
            .into_iter()
            .find(|n| n.id == id.as_str())
            .ok_or_else(|| anyhow::anyhow!("no stored config for node '{}'", id))?;

        let backend = RemoteAgentBackend::connect(RemoteAgentConfig {
            url: stored.url,
            token: stored.token,
            fingerprint: stored.fingerprint,
        })
        .await?;

        let mut state = self.inner.write().await;
        state.backends.insert(id.clone(), Arc::new(backend));
        Ok(())
    }

    fn read_nodes_file(&self) -> anyhow::Result<NodesFile> {
        let path = Self::nodes_file();
        if !path.exists() {
            return Ok(NodesFile::default());
        }
        let body = std::fs::read_to_string(&path)?;
        Ok(toml::from_str(&body)?)
    }

    fn write_nodes_file(&self, file: &NodesFile) -> anyhow::Result<()> {
        let path = Self::nodes_file();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, toml::to_string_pretty(file)?)?;
        Ok(())
    }
}
