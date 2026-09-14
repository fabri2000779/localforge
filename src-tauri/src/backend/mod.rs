//! Tauri-side glue: local and remote [`NodeBackend`] implementations plus the [`NodeRegistry`].

pub mod registry;

pub use localforge_backend_local::LocalDockerBackend;
pub use registry::{NodeRecord, NodeRegistry, ThisMachine};

use localforge_core::NodeBackend;
use std::sync::Arc;

/// Erased backend handle stored in Tauri state.
pub type DynBackend = Arc<dyn NodeBackend>;
