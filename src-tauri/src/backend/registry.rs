//! Node registry: one [`Arc<dyn NodeBackend>`] per known node, remote configs persisted in
//! `~/LocalForge/nodes.toml`. The local node always exists with id `"local"`.

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

/// Remote node WITH its secret token, for the encrypted cloud-sync blob; never sent to the frontend.
#[derive(Debug, Clone)]
pub struct RemoteNodeForSync {
    pub id: String,
    pub label: String,
    pub url: String,
    pub token: String,
    pub fingerprint: Option<String>,
}

/// This machine's stable identity (`this_machine.toml`): a UUID the cloud adopts as the device id plus a
/// user-editable name. Internally the local node keeps the id "local"; this record is for the cloud boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThisMachine {
    pub id: String,
    pub name: String,
    /// Unix ms when the first-run "name this machine" prompt was dismissed; stored here rather
    /// than in localStorage so it survives WebView resets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_prompt_dismissed_at: Option<i64>,
}

/// OS hostname as the default machine name.
fn default_machine_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "My machine".to_string())
}

/// User-visible node record (everything except the live backend handle).
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
        // The token is deliberately not surfaced in the listing.
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
    /// Local machine identity; `None` until the local node is installed.
    this_machine: Option<ThisMachine>,
}

impl NodeRegistry {
    pub fn nodes_file() -> PathBuf {
        paths::home_root().join("nodes.toml")
    }

    pub fn this_machine_file() -> PathBuf {
        paths::home_root().join("this_machine.toml")
    }

    /// Load the persisted machine identity or mint one (UUID + hostname name).
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
            name_prompt_dismissed_at: None,
        };
        if let Ok(body) = toml::to_string(&machine) {
            let _ = std::fs::write(&path, body);
        }
        machine
    }

    /// Install the local backend once Docker is reachable; the machine name becomes the node label.
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

    /// This machine's identity, or `None` before the local node is installed.
    pub async fn this_machine(&self) -> Option<ThisMachine> {
        self.inner.read().await.this_machine.clone()
    }

    /// Rename this machine (record and live label); the id never changes.
    pub async fn set_machine_name(&self, name: String) -> anyhow::Result<ThisMachine> {
        let name = name.trim().to_string();
        if name.is_empty() {
            anyhow::bail!("machine name cannot be empty");
        }
        // Drop the lock before the blocking fs write so other registry users aren't stalled.
        let (machine, body) = {
            let mut state = self.inner.write().await;
            let mut machine = state
                .this_machine
                .clone()
                .unwrap_or_else(Self::load_or_create_this_machine);
            machine.name = name.clone();
            if let Some(rec) = state.records.get_mut(&NodeId::local()) {
                rec.label = name;
            }
            state.this_machine = Some(machine.clone());
            let body = toml::to_string(&machine).ok();
            (machine, body)
        };
        if let Some(body) = body {
            std::fs::write(Self::this_machine_file(), body)
                .map_err(|e| anyhow::anyhow!("failed to persist machine name: {e}"))?;
        }
        Ok(machine)
    }

    /// Record the first-run prompt dismissal (idempotent; the first timestamp wins).
    pub async fn set_name_prompt_dismissed(&self) -> anyhow::Result<ThisMachine> {
        let (machine, body) = {
            let mut state = self.inner.write().await;
            let mut machine = state
                .this_machine
                .clone()
                .unwrap_or_else(Self::load_or_create_this_machine);
            if machine.name_prompt_dismissed_at.is_none() {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                machine.name_prompt_dismissed_at = Some(now);
            }
            state.this_machine = Some(machine.clone());
            let body = toml::to_string(&machine).ok();
            (machine, body)
        };
        if let Some(body) = body {
            std::fs::write(Self::this_machine_file(), body)
                .map_err(|e| anyhow::anyhow!("failed to persist machine identity: {e}"))?;
        }
        Ok(machine)
    }

    /// Reload remote configs and connect to each; an offline node still gets a record.
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

            // Record first so the UI can show the node as offline if the connection fails.
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

    /// Connect to a candidate agent without persisting it ("Test connection").
    pub async fn probe(
        cfg: RemoteAgentConfig,
    ) -> Result<localforge_core::DockerInfo, localforge_core::BackendError> {
        let backend = RemoteAgentBackend::connect(cfg).await?;
        use localforge_core::NodeBackend;
        backend.docker_info().await
    }

    /// Persist + activate a new remote node; fails if the id exists or the agent is unreachable.
    pub async fn add_remote(
        &self,
        id: String,
        label: String,
        cfg: RemoteAgentConfig,
    ) -> anyhow::Result<NodeRecord> {
        let node_id = self.ensure_new_remote_id(&id).await?;
        let backend = RemoteAgentBackend::connect(cfg.clone())
            .await
            .map_err(|e| anyhow::anyhow!("agent unreachable: {}", e))?;

        // Persist before inserting into memory so the file is the source of truth on next launch.
        let record = self.persist_remote(&id, &label, &cfg)?;
        let mut state = self.inner.write().await;
        state.backends.insert(node_id.clone(), Arc::new(backend));
        state.records.insert(node_id, record.clone());
        Ok(record)
    }

    /// Restore a node pulled from cloud sync. Unlike `add_remote`, an unreachable agent still gets
    /// a record (as on startup), so a fresh install shows the whole fleet before every VPS answers.
    pub async fn import_remote(
        &self,
        id: String,
        label: String,
        cfg: RemoteAgentConfig,
    ) -> anyhow::Result<()> {
        let node_id = self.ensure_new_remote_id(&id).await?;
        let record = self.persist_remote(&id, &label, &cfg)?;
        let backend = RemoteAgentBackend::connect(cfg).await;
        let mut state = self.inner.write().await;
        state.records.insert(node_id.clone(), record);
        match backend {
            Ok(b) => {
                state.backends.insert(node_id, Arc::new(b));
            }
            Err(e) => tracing::warn!("imported node '{}' unreachable: {}", node_id, e),
        }
        Ok(())
    }

    async fn ensure_new_remote_id(&self, id: &str) -> anyhow::Result<NodeId> {
        let node_id = NodeId::new(id);
        if node_id.is_local() {
            anyhow::bail!("'{}' is reserved for the local node", NodeId::LOCAL);
        }
        if self.inner.read().await.records.contains_key(&node_id) {
            anyhow::bail!("a node with id '{}' already exists", id);
        }
        Ok(node_id)
    }

    /// Append a remote node to `nodes.toml` and return its UI record.
    fn persist_remote(
        &self,
        id: &str,
        label: &str,
        cfg: &RemoteAgentConfig,
    ) -> anyhow::Result<NodeRecord> {
        let mut file = self.read_nodes_file()?;
        file.nodes.push(StoredRemoteNode {
            id: id.to_string(),
            label: label.to_string(),
            url: cfg.url.clone(),
            token: cfg.token.clone(),
            fingerprint: cfg.fingerprint.clone(),
        });
        self.write_nodes_file(&file)?;
        Ok(NodeRecord {
            id: NodeId::new(id),
            label: label.to_string(),
            kind: NodeKindRecord::Remote {
                url: cfg.url.clone(),
                fingerprint: cfg.fingerprint.clone(),
            },
        })
    }

    /// Every remote node including its token, for cloud sync's encrypted push.
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
            match (a.id.is_local(), b.id.is_local()) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.label.cmp(&b.label),
            }
        });
        out
    }

    pub async fn backend(&self, id: &NodeId) -> Option<DynBackend> {
        let state = self.inner.read().await;
        if let Some(b) = state.backends.get(id) {
            return Some(b.clone());
        }
        // The relay addresses the local node by this machine's global id; map it back to "local".
        if state
            .this_machine
            .as_ref()
            .is_some_and(|m| m.id.as_str() == id.as_str())
        {
            return state.backends.get(&NodeId::local()).cloned();
        }
        None
    }

    /// Every server across all connected nodes, tagged with the id cloud sync and the relay route
    /// by: this machine's global id for the local node, the agent id for remotes.
    pub async fn list_servers_for_sync(&self) -> Vec<(Server, String)> {
        // Snapshot under the lock; await backend calls without holding it.
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
                // No global identity yet: nothing to address it by.
                match &this_id {
                    Some(id) => id.clone(),
                    None => continue,
                }
            } else {
                rec.id.to_string()
            };
            // Bound each node so a downed remote can't stall the whole sync.
            match tokio::time::timeout(
                std::time::Duration::from_secs(6),
                backend.list_servers(),
            )
            .await
            {
                Ok(Ok(servers)) => {
                    for s in servers {
                        out.push((s, tag.clone()));
                    }
                }
                _ => {
                    tracing::warn!("sync: skipping unresponsive node '{}'", rec.id);
                }
            }
        }
        out
    }

    /// Re-attempt a connection to a node ("reconnect" on offline nodes).
    pub async fn reconnect(&self, id: &NodeId) -> anyhow::Result<()> {
        if id.is_local() {
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
        // Temp + rename: a torn write used to drop every remote node on the next launch.
        let mut tmp = path.clone().into_os_string();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        std::fs::write(&tmp, toml::to_string_pretty(file)?)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }
}
