// Docker-related commands

use crate::docker::DockerManager;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct DockerStatus {
    pub available: bool,
    pub running: bool,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DockerInfo {
    pub version: String,
    pub api_version: String,
    pub os: String,
    pub arch: String,
    pub containers_running: u64,
    pub containers_total: u64,
    pub images: u64,
}

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
