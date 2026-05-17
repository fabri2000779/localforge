//! Tauri-side glue for the [`NodeBackend`] implementations.
//!
//! The actual local backend lives in the `localforge-backend-local` crate
//! (shared with the agent binary). This module just holds the Tauri state
//! handle that wraps it.

pub use localforge_backend_local::LocalDockerBackend;

use localforge_core::NodeBackend;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Erased backend handle stored in Tauri state.
pub type DynBackend = Arc<dyn NodeBackend>;

/// Holds the active backend for the "local" node. Wrapped in
/// `RwLock<Option>` because the user may launch the app before Docker is
/// running — in that case the backend is `None` until they retry from
/// the Docker-required screen.
#[derive(Default)]
pub struct BackendState {
    local: RwLock<Option<DynBackend>>,
}

impl BackendState {
    pub async fn local(&self) -> Option<DynBackend> {
        self.local.read().await.clone()
    }

    pub async fn install_local(&self, backend: DynBackend) {
        *self.local.write().await = Some(backend);
    }
}
