//! Docker commands routed through [`NodeBackend`] (`nodeId` defaults to "local").

use crate::backend::{LocalDockerBackend, NodeRegistry};
use crate::commands::require_backend;
use crate::paths;
use localforge_core::NodeId;
use std::sync::Arc;
use tauri::State;

pub use localforge_core::{DockerInfo, DockerStatus};

/// Probe a node's Docker; for the local node also (re)connect the backend ("Retry" button).
#[tauri::command(rename_all = "camelCase")]
pub async fn check_docker_status(
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<DockerStatus, String> {
    let id = node_id
        .as_deref()
        .map(NodeId::new)
        .unwrap_or_else(NodeId::local);

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

    if id.is_local() {
        return match LocalDockerBackend::connect(paths::home_root()).await {
            Ok(backend) => {
                let arc: Arc<dyn localforge_core::NodeBackend> = Arc::new(backend);
                match arc.ping().await {
                    Ok(_) => {
                        state.install_local(arc.clone()).await;
                        let resolver: localforge_backend_local::BackupTargetResolver =
                            Arc::new(|id| crate::backups::find_target(id).map(|(_, t)| t));
                        localforge_backend_local::spawn_scheduler(arc.clone(), paths::home_root(), resolver);
                        localforge_backend_local::spawn_crash_watcher(arc, paths::home_root());
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

    Ok(DockerStatus {
        available: false,
        running: false,
        error: Some(format!("Remote node '{}' not connected", id)),
    })
}

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
