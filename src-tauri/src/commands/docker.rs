//! Docker-related Tauri commands. Thin wrappers around [`NodeBackend`]
//! so the same code path works for local and remote nodes — the caller
//! just supplies a `nodeId` (defaulting to "local").

use crate::backend::{LocalDockerBackend, NodeRegistry};
use crate::commands::require_backend;
use crate::paths;
use localforge_core::NodeId;
use std::sync::Arc;
use tauri::State;

pub use localforge_core::{DockerInfo, DockerStatus};

/// Probe the given node's Docker daemon. For the local node we also try
/// to (re)connect the backend if it wasn't reachable at startup — that's
/// what the "Retry" button on the Docker-required screen invokes.
#[tauri::command(rename_all = "camelCase")]
pub async fn check_docker_status(
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<DockerStatus, String> {
    let id = node_id
        .as_deref()
        .map(NodeId::new)
        .unwrap_or_else(NodeId::local);

    // Fast path: backend already connected.
    if let Some(backend) = state.backend(&id).await {
        return match backend.ping().await {
            Ok(_) => Ok(DockerStatus {
                available: true,
                running: true,
                error: None,
            }),
            Err(e) => Ok(DockerStatus {
                available: true,
                running: false,
                error: Some(format!("Docker not responding: {}", e)),
            }),
        };
    }

    // Slow path for the local node: try to connect to the local socket.
    if id.is_local() {
        return match LocalDockerBackend::connect(paths::home_root()).await {
            Ok(backend) => {
                let arc: Arc<dyn localforge_core::NodeBackend> = Arc::new(backend);
                match arc.ping().await {
                    Ok(_) => {
                        state.install_local(arc).await;
                        Ok(DockerStatus {
                            available: true,
                            running: true,
                            error: None,
                        })
                    }
                    Err(e) => Ok(DockerStatus {
                        available: true,
                        running: false,
                        error: Some(format!("Docker not responding: {}", e)),
                    }),
                }
            }
            Err(e) => Ok(DockerStatus {
                available: false,
                running: false,
                error: Some(format!("Docker not available: {}", e)),
            }),
        };
    }

    // Remote node: offline.
    Ok(DockerStatus {
        available: false,
        running: false,
        error: Some(format!("Remote node '{}' not connected", id)),
    })
}

/// Get Docker daemon information for the given node.
#[tauri::command(rename_all = "camelCase")]
pub async fn get_docker_info(
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<DockerInfo, String> {
    require_backend(&state, node_id.as_deref())
        .await?
        .docker_info()
        .await
        .map_err(|e| e.to_string())
}
