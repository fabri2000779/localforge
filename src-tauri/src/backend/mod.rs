//! Tauri-side glue for the [`NodeBackend`] implementations.
//!
//! - [`LocalDockerBackend`] (re-exported from `localforge-backend-local`)
//!   talks to the user's own Docker daemon.
//! - [`RemoteAgentBackend`] (re-exported from `localforge-backend-remote`)
//!   talks to a remote `localforge-agent` over HTTPS+WS.
//! - [`NodeRegistry`] (this module) owns one of each per known node and
//!   persists their configs to `~/LocalForge/nodes.toml`.

pub mod registry;

pub use localforge_backend_local::LocalDockerBackend;
pub use localforge_backend_remote::{RemoteAgentBackend, RemoteAgentConfig};
pub use registry::{NodeKindRecord, NodeRecord, NodeRegistry};

use localforge_core::NodeBackend;
use std::sync::Arc;

/// Erased backend handle stored in Tauri state.
pub type DynBackend = Arc<dyn NodeBackend>;
