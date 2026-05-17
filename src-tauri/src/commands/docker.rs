// Docker-related commands

use crate::docker::DockerManager;

pub use localforge_core::{DockerInfo, DockerStatus};

/// Check if Docker is available and running
#[tauri::command]
pub async fn check_docker_status() -> Result<DockerStatus, String> {
    match DockerManager::new().await {
        Ok(docker) => match docker.ping().await {
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
        },
        Err(e) => Ok(DockerStatus {
            available: false,
            running: false,
            error: Some(format!("Docker not available: {}", e)),
        }),
    }
}

/// Get Docker system information
#[tauri::command]
pub async fn get_docker_info() -> Result<DockerInfo, String> {
    let docker = DockerManager::new().await.map_err(|e| e.to_string())?;
    docker.get_info().await.map_err(|e| e.to_string())
}
