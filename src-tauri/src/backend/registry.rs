//! Multi-node registry: keeps an [`Arc<dyn NodeBackend>`] per known node
//! and persists their connection configs to `~/LocalForge/nodes.toml`.
//!
//! The local node always exists with id `"local"`; remote nodes are
//! added/removed at runtime via the "Add Node" UI.

use crate::backend::DynBackend;
use crate::paths;
use localforge_backend_local::LocalDockerBackend;
use localforge_backend_remote::{RemoteAgentBackend, RemoteAgentConfig};
use localforge_core::types::Server;
use localforge_core::NodeId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Sync-export shape: every remote node WITH the secret token. Used
/// once-per-sync to build the encrypted blob the cloud stores. Never
/// surfaced to the frontend (it'd defeat the whole "tokens stay
/// local" guarantee).
#[derive(Debug, Clone)]
pub struct RemoteNodeForSync {
    pub id: String,
    pub label: String,
    pub url: String,
    pub token: String,
    pub fingerprint: Option<String>,
}

/// Stable identity for THIS physical machine, persisted to
/// `~/LocalForge/this_machine.toml`.
///
/// `id` is a UUID minted once and never changed. It is the machine's
/// GLOBAL identity: when the user later signs in + upgrades, the cloud
/// ADOPTS this exact id (the device owns the id, the cloud merely borrows
/// it), so servers + history survive the unlogged→paid transition without
/// re-identification. `name` is a free-form, NON-unique label the user can
/// rename (two machines may share a name; they're told apart by `id`).
///
/// Note the deliberate split: INTERNALLY the local node keeps the relative
/// id "local" (see [`NodeId::LOCAL`]) so every existing call site is
/// untouched. This record is only used at the CLOUD boundary (sync tag,
/// enrollment claim, relay addressing) — wired up in later phases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThisMachine {
    pub id: String,
    pub name: String,
}

/// Best-effort OS hostname for the default machine name. The user can
/// rename it afterwards, so a generic fallback is fine.
fn default_machine_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "My machine".to_string())
}

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
    /// Identity of the local machine (loaded/minted on `install_local`).
    /// `None` until Docker is reachable and the local node is installed.
    this_machine: Option<ThisMachine>,
}

impl NodeRegistry {
    pub fn nodes_file() -> PathBuf {
        paths::home_root().join("nodes.toml")
    }

    pub fn this_machine_file() -> PathBuf {
        paths::home_root().join("this_machine.toml")
    }

    /// Load the persisted machine identity, or mint a fresh one (UUID +
    /// hostname-derived default name) and write it. The id is stable
    /// forever after; only the name is user-editable.
    fn load_or_create_this_machine() -> ThisMachine {
        let path = Self::this_machine_file();
        if let Ok(body) = std::fs::read_to_string(&path) {
            if let Ok(m) = toml::from_str::<ThisMachine>(&body) {
                return m;
            }
        }
        let machine = ThisMachine {
            id: uuid::Uuid::new_v4().to_string(),
            name: default_machine_name(),
        };
        if let Ok(body) = toml::to_string(&machine) {
            let _ = std::fs::write(&path, body);
        }
        machine
    }

    /// Replace the local backend (called once Docker is reachable). Also
    /// loads/mints this machine's stable identity and uses its name as the
    /// local node's label.
    pub async fn install_local(&self, backend: DynBackend) {
        let machine = Self::load_or_create_this_machine();
        let mut state = self.inner.write().await;
        state.backends.insert(NodeId::local(), backend);
        state.records.insert(
            NodeId::local(),
            NodeRecord {
                id: NodeId::local(),
                label: machine.name.clone(),
                kind: NodeKindRecord::Local,
            },
        );
        state.this_machine = Some(machine);
    }

    /// This machine's stable identity (id + name), or `None` before the
    /// local node is installed.
    pub async fn this_machine(&self) -> Option<ThisMachine> {
        self.inner.read().await.this_machine.clone()
    }

    /// Rename this machine — updates the persisted record AND the live
    /// local node label. The id is never touched.
    pub async fn set_machine_name(&self, name: String) -> anyhow::Result<ThisMachine> {
        let name = name.trim().to_string();
        if name.is_empty() {
            anyhow::bail!("machine name cannot be empty");
        }
        let mut state = self.inner.write().await;
        let mut machine = state
            .this_machine
            .clone()
            .unwrap_or_else(Self::load_or_create_this_machine);
        machine.name = name.clone();
        if let Ok(body) = toml::to_string(&machine) {
            std::fs::write(Self::this_machine_file(), body)
                .map_err(|e| anyhow::anyhow!("failed to persist machine name: {e}"))?;
        }
        if let Some(rec) = state.records.get_mut(&NodeId::local()) {
            rec.label = name;
        }
        state.this_machine = Some(machine.clone());
        Ok(machine)
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

    /// Plaintext snapshot of every remote node, INCLUDING the secret
    /// token. Used by cloud sync to encrypt-and-push to the cloud — the
    /// resulting blob lets the user restore the same nodes on a second
    /// device with full credentials. The token never leaves the
    /// keychain-protected JSON file in plaintext anywhere else.
    pub fn list_remote_for_sync(&self) -> anyhow::Result<Vec<RemoteNodeForSync>> {
        let file = self.read_nodes_file()?;
        Ok(file
            .nodes
            .into_iter()
            .map(|n| RemoteNodeForSync {
                id: n.id,
                label: n.label,
                url: n.url,
                token: n.token,
                fingerprint: n.fingerprint,
            })
            .collect())
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

    /// Every server across ALL connected nodes (this desktop + reachable
    /// remote agents), each paired with the node id to TAG it with for cloud
    /// sync + relay routing.
    ///
    /// Crucially the LOCAL node's servers are tagged with this machine's
    /// GLOBAL device id (`this_machine.id`), not the relative "local" — that's
    /// the id the cloud adopts and the relay routes to, so a sub-user (or the
    /// owner's other desktop) addresses commands to THIS specific machine.
    /// Remote agents use their own id. Offline/unreachable nodes contribute
    /// nothing — sync is best-effort and re-runs on the next change.
    pub async fn list_servers_for_sync(&self) -> Vec<(Server, String)> {
        // Snapshot under the lock, then do the (awaiting) backend calls
        // without holding it.
        let (records, backends, this_id) = {
            let state = self.inner.read().await;
            (
                state.records.values().cloned().collect::<Vec<_>>(),
                state.backends.clone(),
                state.this_machine.as_ref().map(|m| m.id.clone()),
            )
        };
        let mut out: Vec<(Server, String)> = Vec::new();
        for rec in records {
            let Some(backend) = backends.get(&rec.id) else {
                continue;
            };
            let tag = if rec.id.is_local() {
                // No global identity yet (local node minted but not loaded) →
                // skip; nothing to address it by.
                match &this_id {
                    Some(id) => id.clone(),
                    None => continue,
                }
            } else {
                rec.id.to_string()
            };
            if let Ok(servers) = backend.list_servers().await {
                for s in servers {
                    out.push((s, tag.clone()));
                }
            }
        }
        out
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
