//! Docker-related Tauri commands. Thin wrappers around [`NodeBackend`]
//! so the same code path works for local and remote nodes once we add
//! per-node routing in Phase 3.

use crate::backend::{BackendState, LocalDockerBackend};
use std::sync::Arc;
use tauri::State;

pub use localforge_core::{DockerInfo, DockerStatus};

/// Check whether the local node's Docker daemon is reachable.
///
/// If the backend was not connected at startup (Docker was off) this
/// command will retry the connection — that's what the "retry" button
/// on the Docker-required screen invokes.
#[tauri::command]
pub async fn check_docker_status(state: State<'_, BackendState>) -> Result<DockerStatus, String> {
    // Fast path: backend already connected.
    if let Some(backend) = state.local().await {
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

    // Slow path: try to (re)connect.
    match LocalDockerBackend::connect().await {
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
    }
}

/// Get Docker daemon information for the local node.
#[tauri::command]
pub async fn get_docker_info(state: State<'_, BackendState>) -> Result<DockerInfo, String> {
    let backend = state
        .local()
        .await
        .ok_or_else(|| "Local backend not connected".to_string())?;
    backend.docker_info().await.map_err(|e| e.to_string())
}
