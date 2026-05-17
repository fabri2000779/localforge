//! Local Docker backend — implements [`NodeBackend`] using the user's own
//! Docker daemon via bollard plus the on-disk server registry.
//!
//! Tauri commands receive an `Arc<dyn NodeBackend>` from app state and never
//! create [`DockerManager`] themselves; this is what makes "local" and
//! "remote" nodes interchangeable from the UI's point of view.

use crate::docker::DockerManager;
use crate::persistence;
use async_trait::async_trait;
use localforge_core::backend::{BackendError, NodeBackend, Result};
use localforge_core::types::{
    ContainerStats, DirectoryContents, DockerInfo, FileEntry, Server, ServerStatus,
};
use std::path::{Path, PathBuf};

pub struct LocalDockerBackend {
    docker: DockerManager,
}

impl LocalDockerBackend {
    /// Try to connect to the local Docker daemon. Returns an error if the
    /// socket is unreachable; the caller can surface that to the UI as the
    /// Docker-required screen.
    pub async fn connect() -> Result<Self> {
        let docker = DockerManager::new()
            .await
            .map_err(|e| BackendError::NotConnected(e.to_string()))?;
        Ok(Self { docker })
    }

    /// Resolve a server id to the full record, or return [`BackendError::NotFound`].
    fn require_server(id: &str) -> Result<Server> {
        persistence::load_server(id).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => BackendError::not_found(format!("server '{}'", id)),
            _ => BackendError::io(e),
        })
    }
}

#[async_trait]
impl NodeBackend for LocalDockerBackend {
    // ---- health -----------------------------------------------------------

    async fn ping(&self) -> Result<()> {
        self.docker.ping().await.map_err(BackendError::docker)
    }

    async fn docker_info(&self) -> Result<DockerInfo> {
        self.docker.get_info().await.map_err(BackendError::docker)
    }

    // ---- server read-side ------------------------------------------------

    async fn list_servers(&self) -> Result<Vec<Server>> {
        persistence::list_servers().map_err(BackendError::io)
    }

    async fn get_server(&self, id: &str) -> Result<Option<Server>> {
        match persistence::load_server(id) {
            Ok(s) => Ok(Some(s)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(BackendError::io(e)),
        }
    }

    async fn server_status(&self, id: &str) -> Result<ServerStatus> {
        let server = Self::require_server(id)?;
        let container_id = server
            .container_id
            .ok_or_else(|| BackendError::invalid("server has no container_id"))?;
        self.docker
            .get_container_status(&container_id)
            .await
            .map_err(BackendError::docker)
    }

    async fn get_stats(&self, id: &str) -> Result<ContainerStats> {
        let server = Self::require_server(id)?;
        let container_id = server
            .container_id
            .ok_or_else(|| BackendError::invalid("server has no container_id"))?;
        self.docker
            .get_container_stats(&container_id)
            .await
            .map_err(BackendError::docker)
    }

    async fn get_disk_usage(&self, id: &str) -> Result<u64> {
        let server = Self::require_server(id)?;
        let path = persistence::server_data_path(&server);
        persistence::directory_size(&path).map_err(BackendError::io)
    }

    async fn get_logs(&self, id: &str, lines: usize) -> Result<Vec<String>> {
        let server = Self::require_server(id)?;
        // Prefer install-container logs while installing, otherwise the
        // running container's logs.
        let container_id = server
            .install_container_id
            .clone()
            .or(server.container_id.clone())
            .ok_or_else(|| BackendError::invalid("server has no container"))?;
        let lines_u32 = u32::try_from(lines).unwrap_or(u32::MAX);
        self.docker
            .get_logs(&container_id, lines_u32)
            .await
            .map_err(BackendError::docker)
    }

    // ---- file operations -------------------------------------------------

    async fn list_files(&self, path: &str) -> Result<DirectoryContents> {
        let dir = PathBuf::from(path);
        if !dir.exists() {
            return Err(BackendError::not_found(format!(
                "directory does not exist: {}",
                path
            )));
        }
        if !dir.is_dir() {
            return Err(BackendError::invalid(format!(
                "path is not a directory: {}",
                path
            )));
        }

        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&dir).map_err(BackendError::io)? {
            let entry = entry.map_err(BackendError::io)?;
            let metadata = entry.metadata().map_err(BackendError::io)?;
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name.starts_with('.') {
                continue;
            }
            let modified = metadata.modified().ok().and_then(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .map(|d| d.as_secs())
            });
            let extension = if metadata.is_file() {
                Path::new(&file_name)
                    .extension()
                    .map(|e| e.to_string_lossy().to_string())
            } else {
                None
            };
            entries.push(FileEntry {
                name: file_name,
                path: entry.path().to_string_lossy().to_string(),
                is_dir: metadata.is_dir(),
                size: metadata.len(),
                modified,
                extension,
            });
        }

        entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        });

        Ok(DirectoryContents {
            path: dir.to_string_lossy().to_string(),
            parent: dir
                .parent()
                .map(|p| p.to_string_lossy().to_string()),
            entries,
        })
    }

    async fn read_file_text(&self, path: &str) -> Result<String> {
        std::fs::read_to_string(path).map_err(BackendError::io)
    }

    async fn write_file_text(&self, path: &str, content: &str) -> Result<()> {
        std::fs::write(path, content).map_err(BackendError::io)
    }

    async fn create_file(&self, path: &str) -> Result<()> {
        if Path::new(path).exists() {
            return Err(BackendError::invalid(format!(
                "file already exists: {}",
                path
            )));
        }
        std::fs::write(path, "").map_err(BackendError::io)
    }

    async fn create_directory(&self, path: &str) -> Result<()> {
        std::fs::create_dir_all(path).map_err(BackendError::io)
    }

    async fn delete_path(&self, path: &str) -> Result<()> {
        let p = Path::new(path);
        if !p.exists() {
            return Err(BackendError::not_found(format!("not found: {}", path)));
        }
        if p.is_dir() {
            std::fs::remove_dir_all(p).map_err(BackendError::io)
        } else {
            std::fs::remove_file(p).map_err(BackendError::io)
        }
    }

    async fn rename_path(&self, from: &str, to: &str) -> Result<()> {
        std::fs::rename(from, to).map_err(BackendError::io)
    }

    async fn move_path(&self, from: &str, to: &str) -> Result<()> {
        // For cross-volume moves, fall back to copy + delete.
        if std::fs::rename(from, to).is_ok() {
            return Ok(());
        }
        let src = Path::new(from);
        if src.is_dir() {
            copy_dir_recursive(src, Path::new(to)).map_err(BackendError::io)?;
            std::fs::remove_dir_all(src).map_err(BackendError::io)
        } else {
            std::fs::copy(from, to).map_err(BackendError::io)?;
            std::fs::remove_file(from).map_err(BackendError::io)
        }
    }

    async fn copy_path(&self, from: &str, to: &str) -> Result<()> {
        let src = Path::new(from);
        if src.is_dir() {
            copy_dir_recursive(src, Path::new(to)).map_err(BackendError::io)
        } else {
            std::fs::copy(from, to).map_err(BackendError::io)?;
            Ok(())
        }
    }

    async fn file_info(&self, path: &str) -> Result<FileEntry> {
        let p = PathBuf::from(path);
        let metadata = std::fs::metadata(&p).map_err(BackendError::io)?;
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let modified = metadata.modified().ok().and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs())
        });
        let extension = if metadata.is_file() {
            p.extension().map(|e| e.to_string_lossy().to_string())
        } else {
            None
        };
        Ok(FileEntry {
            name,
            path: p.to_string_lossy().to_string(),
            is_dir: metadata.is_dir(),
            size: metadata.len(),
            modified,
            extension,
        })
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let entry_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry_path, &dst_path)?;
        } else {
            std::fs::copy(&entry_path, &dst_path)?;
        }
    }
    Ok(())
}
