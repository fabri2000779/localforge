//! Local Docker backend — implements [`NodeBackend`] using the user's own
//! Docker daemon via bollard plus the on-disk server registry.
//!
//! Tauri commands receive an `Arc<dyn NodeBackend>` from app state and never
//! create [`DockerManager`] themselves; this is what makes "local" and
//! "remote" nodes interchangeable from the UI's point of view.

use crate::docker::{CreateContainerSpec, DockerManager};
use crate::persistence;
use async_trait::async_trait;
use bollard::container::LogOutput;
use bollard::query_parameters::LogsOptions;
use futures_util::stream::{self, StreamExt};
use localforge_core::backend::{
    BackendError, ByteStream, InstallStream, LogLine, LogStream, NodeBackend, Result,
};
use localforge_core::types::{
    ContainerStats, CreateServerRequest, DirectoryContents, DockerInfo, FileEntry, GameConfig,
    InstallEvent, NodeStats, Server, ServerStatus,
};
use localforge_core::{build_env_vars, detect_oauth_url, PortConfig as CorePortConfig};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use sysinfo::{Disks, System};
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;
use tokio_stream::wrappers::UnboundedReceiverStream;
use uuid::Uuid;

pub struct LocalDockerBackend {
    docker: DockerManager,
    data_root: PathBuf,
    /// Long-lived sysinfo snapshot, refreshed by a background task so
    /// CPU readings are accurate (sysinfo needs two refreshes ~200ms
    /// apart to compute per-CPU deltas).
    system: Arc<RwLock<System>>,
}

impl LocalDockerBackend {
    /// Try to connect to the local Docker daemon. Returns an error if the
    /// socket is unreachable; the caller can surface that to the UI as the
    /// Docker-required screen.
    ///
    /// `data_root` is the directory under which servers/, config/ and
    /// other on-disk state live. The desktop passes `~/LocalForge`, the
    /// agent passes whatever was configured at install time.
    pub async fn connect(data_root: PathBuf) -> Result<Self> {
        let docker = DockerManager::new()
            .await
            .map_err(|e| BackendError::NotConnected(e.to_string()))?;
        std::fs::create_dir_all(persistence::servers_data_root(&data_root))
            .map_err(BackendError::io)?;
        std::fs::create_dir_all(persistence::servers_config_dir(&data_root))
            .map_err(BackendError::io)?;

        let system = Arc::new(RwLock::new(System::new_all()));
        {
            // Prime CPU readings — first refresh always reports 0%.
            let mut s = system.write().await;
            s.refresh_cpu_usage();
            s.refresh_memory();
        }
        // Background refresh loop. Lives until the backend is dropped.
        let bg = system.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(2)).await;
                let mut s = bg.write().await;
                s.refresh_cpu_usage();
                s.refresh_memory();
            }
        });

        Ok(Self {
            docker,
            data_root,
            system,
        })
    }

    /// Borrow the configured data root.
    pub fn data_root(&self) -> &Path {
        &self.data_root
    }

    /// Resolve a server id to the full record, or return [`BackendError::NotFound`].
    fn require_server(&self, id: &str) -> Result<Server> {
        persistence::load_server(&self.data_root, id).map_err(|e| match e.kind() {
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

    async fn node_stats(&self) -> Result<NodeStats> {
        let s = self.system.read().await;

        // CPU% averaged across all logical cores.
        let cpus = s.cpus();
        let cpu_count = cpus.len() as u32;
        let cpu_percent = if cpu_count > 0 {
            cpus.iter().map(|c| c.cpu_usage()).sum::<f32>() / cpu_count as f32
        } else {
            0.0
        };

        let memory_total_bytes = s.total_memory();
        let memory_used_bytes = s.used_memory();
        let swap_total_bytes = s.total_swap();
        let swap_used_bytes = s.used_swap();

        // Disk stats: pick the mount point that's the longest prefix of
        // the data root path — that's the filesystem where the server
        // data lives.
        let disks = Disks::new_with_refreshed_list();
        let data_root_str = self.data_root.to_string_lossy().to_string();
        let mut best_match: Option<&sysinfo::Disk> = None;
        let mut best_len = 0usize;
        for disk in disks.list() {
            let mount = disk.mount_point().to_string_lossy().to_string();
            if data_root_str.starts_with(&mount) && mount.len() >= best_len {
                best_len = mount.len();
                best_match = Some(disk);
            }
        }
        let (disk_total_bytes, disk_used_bytes) = if let Some(d) = best_match {
            let total = d.total_space();
            let free = d.available_space();
            (total, total.saturating_sub(free))
        } else {
            (0, 0)
        };

        let uptime_secs = System::uptime();
        let load_avg_1m = {
            let la = System::load_average();
            if la.one.is_finite() && la.one > 0.0 {
                Some(la.one)
            } else {
                None
            }
        };

        Ok(NodeStats {
            cpu_percent,
            cpu_count,
            memory_used_bytes,
            memory_total_bytes,
            swap_used_bytes,
            swap_total_bytes,
            disk_used_bytes,
            disk_total_bytes,
            uptime_secs,
            load_avg_1m,
        })
    }

    // ---- server read-side ------------------------------------------------

    async fn list_servers(&self) -> Result<Vec<Server>> {
        persistence::list_servers(&self.data_root).map_err(BackendError::io)
    }

    async fn get_server(&self, id: &str) -> Result<Option<Server>> {
        match persistence::load_server(&self.data_root, id) {
            Ok(s) => Ok(Some(s)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(BackendError::io(e)),
        }
    }

    async fn server_status(&self, id: &str) -> Result<ServerStatus> {
        let server = self.require_server(id)?;
        let container_id = server
            .container_id
            .ok_or_else(|| BackendError::invalid("server has no container_id"))?;
        self.docker
            .get_container_status(&container_id)
            .await
            .map_err(BackendError::docker)
    }

    async fn get_stats(&self, id: &str) -> Result<ContainerStats> {
        let server = self.require_server(id)?;
        let container_id = server
            .container_id
            .ok_or_else(|| BackendError::invalid("server has no container_id"))?;
        self.docker
            .get_container_stats(&container_id)
            .await
            .map_err(BackendError::docker)
    }

    async fn get_disk_usage(&self, id: &str) -> Result<u64> {
        let server = self.require_server(id)?;
        let path = persistence::server_data_path(&self.data_root, &server);
        persistence::directory_size(&path).map_err(BackendError::io)
    }

    async fn get_logs(&self, id: &str, lines: usize) -> Result<Vec<String>> {
        let server = self.require_server(id)?;
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

    // ---- server lifecycle -----------------------------------------------

    async fn create_server(
        &self,
        request: CreateServerRequest,
        game: GameConfig,
    ) -> Result<Server> {
        let server_id = Uuid::new_v4().to_string()[..8].to_string();

        let port = request.port.unwrap_or_else(|| {
            game.ports
                .first()
                .map(|p| p.container_port)
                .unwrap_or(25565)
        });

        let memory_mb = request.memory_mb.unwrap_or(game.recommended_ram_mb);

        let data_path = persistence::servers_data_root(&self.data_root)
            .join(game.game_type.to_string())
            .join(&server_id);

        create_server_data_dir(&data_path)?;

        let user_config = request.config.clone().unwrap_or_default();
        let env = build_env_vars(&game, memory_mb, port, &user_config);

        let extra_ports: Vec<CorePortConfig> =
            game.ports.iter().skip(1).cloned().collect();

        let startup_command = if game.startup.is_empty() {
            None
        } else {
            let mut startup = game.startup.clone();
            for (key, value) in &env {
                startup = startup.replace(&format!("{{{{{}}}}}", key), value);
            }
            Some(startup)
        };

        let container_id = self
            .docker
            .create_container(CreateContainerSpec {
                name: &server_id,
                image: &game.docker_image,
                port,
                data_path: &data_path,
                env: &env,
                extra_ports: &extra_ports,
                volume_path: Some(&game.volume_path),
                memory_mb: Some(memory_mb),
                startup_command: startup_command.as_deref(),
            })
            .await
            .map_err(BackendError::docker)?;

        let server = Server {
            id: server_id,
            name: request.name,
            game_type: request.game_type,
            status: ServerStatus::Stopped,
            container_id: Some(container_id),
            port,
            memory_mb,
            data_path,
            created_at: chrono::Utc::now(),
            config: user_config,
            installed: false,
            install_container_id: None,
        };

        persistence::save_server(&self.data_root, &server).map_err(BackendError::io)?;
        Ok(server)
    }

    async fn update_server_config(
        &self,
        id: &str,
        config: HashMap<String, String>,
    ) -> Result<Server> {
        let mut server = self.require_server(id)?;
        server.config = config;
        persistence::save_server(&self.data_root, &server).map_err(BackendError::io)?;
        Ok(server)
    }

    async fn delete_server(&self, id: &str) -> Result<()> {
        let server = self.require_server(id)?;

        // Best-effort stop + remove of the runtime container (ignore errors —
        // the container may already be gone).
        if let Some(container_id) = &server.container_id {
            let _ = self.docker.stop_container(container_id).await;
            let _ = self.docker.remove_container(container_id).await;
        }
        if let Some(install_container_id) = &server.install_container_id {
            let _ = self
                .docker
                .remove_install_container(install_container_id)
                .await;
        }

        // Delete on-disk data.
        if server.data_path.exists() {
            std::fs::remove_dir_all(&server.data_path).map_err(BackendError::io)?;
        }
        persistence::delete_server_record(&self.data_root, id).map_err(BackendError::io)?;
        Ok(())
    }

    async fn start_server(&self, id: &str) -> Result<ServerStatus> {
        let mut server = self.require_server(id)?;
        let container_id = server
            .container_id
            .clone()
            .ok_or_else(|| BackendError::invalid("server has no container"))?;

        // Self-heal volume permissions on boot. Servers created before the
        // data dir was made world-writable (or whose dir was created by a
        // differently-owned process) may be unwritable by the container's
        // uid 1000 while the agent runs as the localforge user (uid ~999),
        // which crashes the server on first start (e.g. it can't create the
        // jar's cache dir). Best-effort widen so the existing dir heals
        // without a reset. cfg-gated out on Windows/macOS.
        #[cfg(unix)]
        if server.data_path.exists() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(
                &server.data_path,
                std::fs::Permissions::from_mode(0o777),
            );
        }

        self.docker
            .start_container(&container_id)
            .await
            .map_err(BackendError::docker)?;

        // Give the container a moment to settle before we read the status.
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        let status = self
            .docker
            .get_container_status(&container_id)
            .await
            .map_err(BackendError::docker)?;

        if status == ServerStatus::Stopped || status == ServerStatus::Error {
            return Err(BackendError::Docker("container failed to start".into()));
        }

        server.status = status.clone();
        persistence::save_server(&self.data_root, &server).map_err(BackendError::io)?;
        Ok(status)
    }

    async fn stop_server(&self, id: &str) -> Result<ServerStatus> {
        let mut server = self.require_server(id)?;
        let container_id = server
            .container_id
            .clone()
            .ok_or_else(|| BackendError::invalid("server has no container"))?;

        self.docker
            .stop_container(&container_id)
            .await
            .map_err(BackendError::docker)?;

        let status = self
            .docker
            .get_container_status(&container_id)
            .await
            .map_err(BackendError::docker)?;

        server.status = status.clone();
        persistence::save_server(&self.data_root, &server).map_err(BackendError::io)?;
        Ok(status)
    }

    async fn send_command(&self, id: &str, command: &str) -> Result<()> {
        let server = self.require_server(id)?;
        let container_id = server
            .container_id
            .ok_or_else(|| BackendError::invalid("server has no container"))?;
        // Newline-terminate so the container's shell/console treats it as a
        // complete command.
        let payload = if command.ends_with('\n') {
            command.to_string()
        } else {
            format!("{}\n", command)
        };
        self.docker
            .send_stdin(&container_id, &payload)
            .await
            .map_err(BackendError::docker)
    }

    async fn stream_logs(&self, id: &str) -> Result<LogStream> {
        let server = self.require_server(id)?;
        let container_id = server
            .container_id
            .ok_or_else(|| BackendError::invalid("server has no container"))?;
        let server_id = server.id.clone();

        let options = Some(LogsOptions {
            stdout: true,
            stderr: true,
            follow: true,
            tail: "0".to_string(),
            ..Default::default()
        });

        let docker_stream = self.docker.client().logs(&container_id, options);

        let mapped = docker_stream.flat_map(move |item| {
            let server_id = server_id.clone();
            let items: Vec<std::result::Result<LogLine, BackendError>> = match item {
                Ok(output) => {
                    let bytes = match output {
                        LogOutput::StdOut { message }
                        | LogOutput::StdErr { message }
                        | LogOutput::Console { message }
                        | LogOutput::StdIn { message } => message,
                    };
                    let text = String::from_utf8_lossy(&bytes).to_string();
                    text.lines()
                        .filter(|l| !l.trim().is_empty())
                        .map(|l| {
                            Ok(LogLine {
                                server_id: server_id.clone(),
                                line: l.to_string(),
                            })
                        })
                        .collect()
                }
                Err(e) => vec![Err(BackendError::Docker(e.to_string()))],
            };
            stream::iter(items)
        });

        Ok(Box::pin(mapped))
    }

    async fn reset_server_data(&self, id: &str) -> Result<()> {
        let mut server = self.require_server(id)?;
        if let Some(container_id) = &server.container_id {
            let _ = self.docker.stop_container(container_id).await;
        }
        if server.data_path.exists() {
            std::fs::remove_dir_all(&server.data_path).map_err(BackendError::io)?;
        }
        create_server_data_dir(&server.data_path)?;
        server.installed = false;
        server.install_container_id = None;
        server.status = ServerStatus::Stopped;
        persistence::save_server(&self.data_root, &server).map_err(BackendError::io)?;
        Ok(())
    }

    async fn run_install(&self, id: &str, game: GameConfig) -> Result<InstallStream> {
        let server = self.require_server(id)?;

        // No install script => mark installed and emit a one-shot Done.
        let install_script = match game.install_script.clone() {
            Some(s) if !s.is_empty() => s,
            _ => {
                let mut s = server.clone();
                s.installed = true;
                persistence::save_server(&self.data_root, &s).map_err(BackendError::io)?;
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                let _ = tx.send(Ok(InstallEvent::Done { exit_code: 0 }));
                drop(tx);
                return Ok(Box::pin(UnboundedReceiverStream::new(rx)));
            }
        };

        let install_image = game
            .install_image
            .clone()
            .unwrap_or_else(|| game.docker_image.clone());
        let volume_path = game.volume_path.clone();

        // Mark Installing in the persisted record so the UI's status
        // queries surface it even if a refresh happens mid-install.
        let mut server = server;
        server.status = ServerStatus::Installing;
        persistence::save_server(&self.data_root, &server).map_err(BackendError::io)?;

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Result<InstallEvent>>();
        let docker = self.docker.clone();
        let data_root = self.data_root.clone();
        let server_id = server.id.clone();
        let server_data_path = server.data_path.clone();

        tokio::spawn(async move {
            let install_result = run_install_inner(
                docker,
                data_root,
                server_id,
                server_data_path,
                install_image,
                volume_path,
                install_script,
                tx.clone(),
            )
            .await;
            if let Err(e) = install_result {
                let _ = tx.send(Err(e));
            }
        });

        Ok(Box::pin(UnboundedReceiverStream::new(rx)))
    }

    async fn download_file(&self, path: &str) -> Result<ByteStream> {
        let file = tokio::fs::File::open(path).await.map_err(BackendError::io)?;
        let stream = tokio_util::io::ReaderStream::new(file)
            .map(|r| r.map_err(BackendError::io));
        Ok(Box::pin(stream))
    }

    async fn upload_file(&self, path: &str, mut body: ByteStream) -> Result<()> {
        // Make sure the destination directory exists.
        if let Some(parent) = std::path::Path::new(path).parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(BackendError::io)?;
        }
        let mut file = tokio::fs::File::create(path)
            .await
            .map_err(BackendError::io)?;
        while let Some(chunk) = body.next().await {
            let bytes = chunk?;
            file.write_all(&bytes).await.map_err(BackendError::io)?;
        }
        file.flush().await.map_err(BackendError::io)?;
        Ok(())
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

/// Create a server's on-disk data directory and make it writable by the
/// container's runtime user.
///
/// Game images run their process as uid 1000 (`container`), but the agent's
/// systemd service runs as the unprivileged `localforge` user (uid ~999). A
/// bind-mounted volume is created owned by whoever runs the agent, so without
/// this the container can't create its working files (e.g. the server jar's
/// `/mnt/server/cache`) on first boot and dies with `AccessDeniedException`.
/// We can't `chown` to another uid as a non-root process, but we own the dir
/// we just created, so we widen its mode to 0o777. On the desktop (where the
/// user is usually uid 1000 already) this is a harmless no-op; on Windows and
/// macOS the cfg-gate compiles it out entirely.
fn create_server_data_dir(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path).map_err(BackendError::io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o777))
            .map_err(BackendError::io)?;
    }
    Ok(())
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

/// The actual install pipeline. Lives outside the impl so it can be
/// spawned onto its own task without borrowing `&self`.
#[allow(clippy::too_many_arguments)]
async fn run_install_inner(
    docker: DockerManager,
    data_root: PathBuf,
    server_id: String,
    server_data_path: PathBuf,
    install_image: String,
    volume_path: String,
    install_script: String,
    tx: tokio::sync::mpsc::UnboundedSender<Result<InstallEvent>>,
) -> Result<()> {
    // Persist the install container id as soon as the helper creates
    // it, so log recovery works if the install is interrupted.
    let id_for_callback = server_id.clone();
    let data_root_for_callback = data_root.clone();
    let on_container_created = move |container_id: &str| {
        if let Ok(mut srv) = persistence::load_server(&data_root_for_callback, &id_for_callback) {
            srv.install_container_id = Some(container_id.to_string());
            let _ = persistence::save_server(&data_root_for_callback, &srv);
        }
    };

    // Channel adapter: DockerManager::run_script takes a sync callback,
    // but the events flow out on an mpsc. The channel is UNBOUNDED so
    // `send` is synchronous and never blocks. This is critical: the
    // callback runs on a tokio worker thread (run_script awaits the log
    // stream inline), and `blocking_send` on a *full* bounded channel
    // tries to park the runtime thread → panics ("Cannot block the
    // current thread from within a runtime"), which with panic=abort
    // crashed the whole app on a burst of install output.
    let tx_lines = tx.clone();
    let on_output = move |line: String| {
        if let Some(url) = detect_oauth_url(&line) {
            let _ = tx_lines.send(Ok(InstallEvent::OauthUrl { url }));
        }
        let _ = tx_lines.send(Ok(InstallEvent::Log { line }));
    };

    let (exit_code, install_container_id) = docker
        .run_script(
            &install_image,
            &server_data_path,
            &volume_path,
            &install_script,
            on_container_created,
            on_output,
        )
        .await
        .map_err(BackendError::docker)?;

    // Clean up the install container (best-effort).
    let _ = docker.remove_install_container(&install_container_id).await;

    // Update the persisted server record with the install outcome.
    if let Ok(mut srv) = persistence::load_server(&data_root, &server_id) {
        srv.install_container_id = None;
        if exit_code == 0 {
            srv.installed = true;
            srv.status = ServerStatus::Stopped;
        } else {
            srv.status = ServerStatus::Error;
        }
        let _ = persistence::save_server(&data_root, &srv);
    }

    let _ = tx.send(Ok(InstallEvent::Done { exit_code }));
    Ok(())
}
