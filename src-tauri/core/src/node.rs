//! Node identity: the local Docker host or a remote agent.

use serde::{Deserialize, Serialize};

/// Stable identifier for a node. The local node always has id `"local"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub String);

impl NodeId {
    pub const LOCAL: &'static str = "local";

    pub fn local() -> Self {
        Self(Self::LOCAL.to_string())
    }

    pub fn new<S: Into<String>>(s: S) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_local(&self) -> bool {
        self.0 == Self::LOCAL
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Connection-specific config for a node, persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NodeKind {
    /// The user's own machine via the local Docker socket.
    Local,

    /// A remote LocalForge agent reachable over HTTPS.
    Remote(RemoteConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteConfig {
    /// Display label shown in the UI.
    pub label: String,

    /// Base URL of the agent, e.g. `https://1.2.3.4:7878`.
    pub url: String,

    /// Bearer token issued when the agent was installed.
    pub token: String,

    /// Pinned SHA-256 cert fingerprint; `None` trusts the system CA store.
    #[serde(default)]
    pub cert_fingerprint: Option<String>,
}

/// Persisted record of a node the desktop knows about.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRecord {
    pub id: NodeId,
    pub kind: NodeKind,
}

impl NodeRecord {
    pub fn local() -> Self {
        Self {
            id: NodeId::local(),
            kind: NodeKind::Local,
        }
    }
}
