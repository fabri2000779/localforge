//! `NodeBackend` implementations used by the desktop app.
//!
//! Today the only backend is [`local::LocalDockerBackend`]; later phases
//! will add a `RemoteAgentBackend` that talks to a remote agent over HTTPS.

pub mod local;

pub use local::LocalDockerBackend;

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
