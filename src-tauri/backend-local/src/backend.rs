//! Local Docker [`NodeBackend`]: bollard plus the on-disk server registry.

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
    BackupEntry, BackupTarget, ContainerStats, CreateServerRequest, DirectoryContents, DockerInfo,
    FileEntry, GameConfig, InstallEvent, MetricPoint, NodeStats, Player, PlayerAction, Schedule,
    Server, ServerStatus,
};
use localforge_core::{
    apply_config_files, build_env_vars, detect_oauth_url, PortConfig as CorePortConfig,
    SystemMapping,
};
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
    /// Sysinfo snapshot refreshed in the background (CPU deltas need two refreshes).
    system: Arc<RwLock<System>>,
    /// Server ids with an install in flight, so two installs can't share one bind-mount.
    installing: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
}

impl LocalDockerBackend {
    /// Connect to the local Docker daemon; `data_root` holds servers/, config/ and other state.
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
            installing: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        })
    }

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

    /// Docker container for `server` as the game currently defines it (image, env, ports, startup).
    async fn create_container_for(&self, server: &Server, game: &GameConfig) -> Result<String> {
        let env = build_env_vars(game, server.memory_mb, server.port, &server.config);
        let extra_ports: Vec<CorePortConfig> = game.ports.iter().skip(1).cloned().collect();
        let startup_command = render_startup(game, &env);
        self.docker
            .create_container(CreateContainerSpec {
                name: &server.id,
                image: &game.docker_image,
                port: server.port,
                container_port: container_port_for(game, server.port),
                data_path: &server.data_path,
                env: &env,
                extra_ports: &extra_ports,
                volume_path: Some(&game.volume_path),
                memory_mb: Some(server.memory_mb),
                startup_command: startup_command.as_deref(),
            })
            .await
            .map_err(BackendError::docker)
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

        // Disk of the mount point that is the longest prefix of the data root.
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
        // Install-container logs while installing, else the running container's.
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
        let dir = confine_path(&self.data_root, path)?;
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
        std::fs::read_to_string(confine_path(&self.data_root, path)?).map_err(BackendError::io)
    }

    async fn write_file_text(&self, path: &str, content: &str) -> Result<()> {
        std::fs::write(confine_path(&self.data_root, path)?, content).map_err(BackendError::io)
    }

    async fn create_file(&self, path: &str) -> Result<()> {
        let path = confine_path(&self.data_root, path)?;
        if path.exists() {
            return Err(BackendError::invalid("file already exists"));
        }
        std::fs::write(path, "").map_err(BackendError::io)
    }

    async fn create_directory(&self, path: &str) -> Result<()> {
        std::fs::create_dir_all(confine_path(&self.data_root, path)?).map_err(BackendError::io)
    }

    async fn delete_path(&self, path: &str) -> Result<()> {
        let p = confine_path(&self.data_root, path)?;
        if !p.exists() {
            return Err(BackendError::not_found(format!("not found: {}", path)));
        }
        if p.is_dir() {
            std::fs::remove_dir_all(&p).map_err(BackendError::io)
        } else {
            std::fs::remove_file(&p).map_err(BackendError::io)
        }
    }

    async fn rename_path(&self, from: &str, to: &str) -> Result<()> {
        let from = confine_path(&self.data_root, from)?;
        let to = confine_path(&self.data_root, to)?;
        std::fs::rename(from, to).map_err(BackendError::io)
    }

    async fn move_path(&self, from: &str, to: &str) -> Result<()> {
        let from = confine_path(&self.data_root, from)?;
        let to = confine_path(&self.data_root, to)?;
        ensure_not_nested(&from, &to)?;
        // For cross-volume moves, fall back to copy + delete.
        if std::fs::rename(&from, &to).is_ok() {
            return Ok(());
        }
        if from.is_dir() {
            copy_dir_recursive(&from, &to).map_err(BackendError::io)?;
            std::fs::remove_dir_all(&from).map_err(BackendError::io)
        } else {
            std::fs::copy(&from, &to).map_err(BackendError::io)?;
            std::fs::remove_file(&from).map_err(BackendError::io)
        }
    }

    async fn copy_path(&self, from: &str, to: &str) -> Result<()> {
        let from = confine_path(&self.data_root, from)?;
        let to = confine_path(&self.data_root, to)?;
        ensure_not_nested(&from, &to)?;
        if from.is_dir() {
            copy_dir_recursive(&from, &to).map_err(BackendError::io)
        } else {
            std::fs::copy(&from, &to).map_err(BackendError::io)?;
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

        // game_type is a path component under the data root; reject anything that could escape it.
        validate_path_component(&game.game_type.to_string())
            .map_err(|e| BackendError::invalid(format!("invalid game_type: {e}")))?;

        let data_path = persistence::servers_data_root(&self.data_root)
            .join(game.game_type.to_string())
            .join(&server_id);

        create_server_data_dir(&data_path)?;

        let mut server = Server {
            id: server_id,
            name: request.name,
            game_type: request.game_type,
            status: ServerStatus::Stopped,
            container_id: None,
            port,
            memory_mb,
            data_path,
            created_at: chrono::Utc::now(),
            config: request.config.clone().unwrap_or_default(),
            installed: false,
            install_container_id: None,
            restart_policy: Default::default(),
        };
        server.container_id = Some(self.create_container_for(&server, &game).await?);

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

    async fn apply_server_config(&self, id: &str, game: GameConfig) -> Result<Server> {
        let mut server = self.require_server(id)?;
        let env = build_env_vars(&game, server.memory_mb, server.port, &server.config);
        // Config files live in the bind mount, so the game picks them up on its next start.
        for path in apply_config_files(&server.data_path, &game, &env).map_err(BackendError::io)? {
            tracing::info!("server {}: applied {}", id, path);
        }
        // Env, command and the port binding are frozen into the container: recreate it when they no
        // longer match — only while stopped; a live container is left alone.
        let Some(container_id) = server.container_id.clone() else {
            return Ok(server);
        };
        let status = self
            .docker
            .get_container_status(&container_id)
            .await
            .map_err(BackendError::docker)?;
        if status != ServerStatus::Stopped && status != ServerStatus::Error {
            return Ok(server);
        }
        let startup_command = render_startup(&game, &env);
        let matches = self
            .docker
            .container_matches(
                &container_id,
                &env,
                startup_command.as_deref(),
                Some(&game.volume_path),
                server.port,
                container_port_for(&game, server.port),
            )
            .await
            .map_err(BackendError::docker)?;
        if matches {
            return Ok(server);
        }
        self.docker
            .remove_container(&container_id)
            .await
            .map_err(BackendError::docker)?;
        server.container_id = Some(self.create_container_for(&server, &game).await?);
        persistence::save_server(&self.data_root, &server).map_err(BackendError::io)?;
        tracing::info!("server {}: container recreated with the current configuration", id);
        Ok(server)
    }

    async fn delete_server(&self, id: &str) -> Result<()> {
        let server = self.require_server(id)?;

        // Best-effort: the container may already be gone.
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

        if server.data_path.exists() {
            std::fs::remove_dir_all(&server.data_path).map_err(BackendError::io)?;
        }
        persistence::delete_server_record(&self.data_root, id).map_err(BackendError::io)?;

        // S3 backups are intentionally kept: they're the user's off-box safety net.
        crate::metrics::remove_server(&self.data_root, id);
        let _ = crate::schedules::delete_for_server(&self.data_root, id);
        Ok(())
    }

    async fn delete_server_keep_data(&self, id: &str) -> Result<()> {
        let server = self.require_server(id)?;

        // Same teardown as `delete_server`; only the world directory is left behind.
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

        persistence::delete_server_record(&self.data_root, id).map_err(BackendError::io)?;
        // Schedules and metrics belong to the record, so drop them like the full delete does.
        crate::metrics::remove_server(&self.data_root, id);
        let _ = crate::schedules::delete_for_server(&self.data_root, id);
        Ok(())
    }

    async fn start_server(&self, id: &str) -> Result<ServerStatus> {
        let mut server = self.require_server(id)?;
        let container_id = server
            .container_id
            .clone()
            .ok_or_else(|| BackendError::invalid("server has no container"))?;

        // Self-heal volume permissions: the container's uid 1000 must be able to write a dir that
        // may have been created by a differently-owned process. Unix only.
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

        // Persist Running first: the container IS started, and a failed probe must not leave the
        // record at Stopped/Crashed (outside crash-watcher coverage) while it runs.
        server.status = ServerStatus::Running;
        let _ = persistence::save_server(&self.data_root, &server);

        // Give the container a moment to settle before we read the status.
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        let status = match self.docker.get_container_status(&container_id).await {
            Ok(s) => s,
            // Probe hiccup — keep the optimistic Running we just persisted.
            Err(_) => return Ok(ServerStatus::Running),
        };

        if status == ServerStatus::Stopped || status == ServerStatus::Error {
            // It really failed to start; roll the optimistic Running back.
            server.status = status;
            let _ = persistence::save_server(&self.data_root, &server);
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

        // Persist Stopping first so the crash-watcher (which only acts on Running) doesn't
        // mistake the graceful shutdown for a crash.
        server.status = ServerStatus::Stopping;
        let _ = persistence::save_server(&self.data_root, &server);

        if let Err(e) = self.docker.stop_container(&container_id).await {
            // Roll back to reality; a record stuck at Stopping is outside crash-watcher coverage.
            if let Ok(actual) = self.docker.get_container_status(&container_id).await {
                server.status = actual;
                let _ = persistence::save_server(&self.data_root, &server);
            }
            return Err(BackendError::docker(e));
        }

        // Stop succeeded; a probe hiccup must not strand the record at Stopping.
        let status = self
            .docker
            .get_container_status(&container_id)
            .await
            .unwrap_or(ServerStatus::Stopped);

        server.status = status.clone();
        persistence::save_server(&self.data_root, &server).map_err(BackendError::io)?;
        Ok(status)
    }

    async fn send_command(&self, id: &str, command: &str) -> Result<()> {
        let server = self.require_server(id)?;
        let container_id = server
            .container_id
            .ok_or_else(|| BackendError::invalid("server has no container"))?;
        // Newline-terminate so the console treats it as a complete command.
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
        // Persist Stopping first (as in stop_server): the wipe below can outlast a watcher tick.
        if server.status != ServerStatus::Stopping && server.status != ServerStatus::Stopped {
            server.status = ServerStatus::Stopping;
            persistence::save_server(&self.data_root, &server).map_err(BackendError::io)?;
        }
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

        // A second concurrent install would run over the same bind-mount and corrupt it.
        let install_guard = InstallGuard::acquire(&self.installing, id).ok_or_else(|| {
            BackendError::invalid("an install is already in progress for this server")
        })?;

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
        // The installer reads the same variables as the runtime (version, build, jar name…).
        let env = build_env_vars(&game, server.memory_mb, server.port, &server.config);

        // Mark Installing so status queries reflect it mid-install.
        let mut server = server;
        server.status = ServerStatus::Installing;
        persistence::save_server(&self.data_root, &server).map_err(BackendError::io)?;

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Result<InstallEvent>>();
        let docker = self.docker.clone();
        let data_root = self.data_root.clone();
        let server_id = server.id.clone();
        let server_data_path = server.data_path.clone();

        tokio::spawn(async move {
            // Held for the install's lifetime; dropping frees the slot.
            let _guard = install_guard;
            let install_result = run_install_inner(
                docker,
                data_root,
                server_id,
                server_data_path,
                install_image,
                volume_path,
                install_script,
                env,
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
        let path = confine_path(&self.data_root, path)?;
        let file = tokio::fs::File::open(path).await.map_err(BackendError::io)?;
        let stream = tokio_util::io::ReaderStream::new(file)
            .map(|r| r.map_err(BackendError::io));
        Ok(Box::pin(stream))
    }

    async fn upload_file(&self, path: &str, mut body: ByteStream) -> Result<()> {
        let path = confine_path(&self.data_root, path)?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(BackendError::io)?;
        }
        // Temp file + rename so a stream that fails mid-transfer can't truncate the existing file.
        let tmp_path = {
            let mut name = path
                .file_name()
                .map(|n| n.to_os_string())
                .unwrap_or_else(|| std::ffi::OsString::from("upload"));
            name.push(format!(".tmp-{}", Uuid::new_v4()));
            path.with_file_name(name)
        };

        let write_result = async {
            let mut file = tokio::fs::File::create(&tmp_path)
                .await
                .map_err(BackendError::io)?;
            while let Some(chunk) = body.next().await {
                let bytes = chunk?;
                file.write_all(&bytes).await.map_err(BackendError::io)?;
            }
            file.flush().await.map_err(BackendError::io)?;
            Ok::<(), BackendError>(())
        }
        .await;

        match write_result {
            Ok(()) => {
                tokio::fs::rename(&tmp_path, &path)
                    .await
                    .map_err(BackendError::io)?;
                Ok(())
            }
            Err(e) => {
                let _ = tokio::fs::remove_file(&tmp_path).await;
                Err(e)
            }
        }
    }

    async fn file_info(&self, path: &str) -> Result<FileEntry> {
        let p = confine_path(&self.data_root, path)?;
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

    // ----- backups (bring-your-own S3) -----------------------------------

    async fn create_backup(&self, id: &str, target: &BackupTarget) -> Result<String> {
        let server = self.require_server(id)?;
        let data_path = persistence::server_data_path(&self.data_root, &server);
        if !data_path.exists() {
            return Err(BackendError::not_found("server data directory"));
        }
        // Minecraft Java: flush the world and pause autosave while archiving a running server
        // (best-effort); autosave is re-enabled afterwards. Other games are archived live.
        let flush =
            server.game_type.0 == "minecraft-java" && server.status == ServerStatus::Running;
        let cid = server.container_id.clone();
        if flush {
            if let Some(cid) = &cid {
                let _ = self.docker.send_stdin(cid, "save-off\n").await;
                let _ = self.docker.send_stdin(cid, "save-all flush\n").await;
                tokio::time::sleep(Duration::from_millis(1500)).await;
            }
        }
        let res = crate::backups::upload(target, id, &data_path).await;
        if flush {
            if let Some(cid) = &cid {
                let _ = self.docker.send_stdin(cid, "save-on\n").await;
            }
        }
        res
    }

    async fn list_backups(&self, id: &str, target: &BackupTarget) -> Result<Vec<BackupEntry>> {
        self.require_server(id)?;
        crate::backups::list(target, id).await
    }

    async fn restore_backup(&self, id: &str, target: &BackupTarget, key: &str) -> Result<()> {
        let mut server = self.require_server(id)?;
        let data_path = persistence::server_data_path(&self.data_root, &server);

        // Download first: a bad key or a network drop must not leave the server with an empty data dir.
        let archive = crate::backups::download_archive(target, id, key).await?;

        // Restoring over a live world corrupts it — stop the container first.
        if let Some(cid) = server.container_id.clone() {
            let _ = self.docker.stop_container(&cid).await;
            if let Ok(status) = self.docker.get_container_status(&cid).await {
                server.status = status;
                let _ = persistence::save_server(&self.data_root, &server);
            }
        }
        // Move the current data aside (never destroy it) so a failed restore stays recoverable.
        let aside = if data_path.exists() {
            let aside =
                data_path.with_extension(format!("bak-{}", chrono::Utc::now().timestamp()));
            if let Err(e) = std::fs::rename(&data_path, &aside) {
                let _ = std::fs::remove_file(&archive);
                return Err(BackendError::io(e));
            }
            Some(aside)
        } else {
            None
        };
        let extracted = match create_server_data_dir(&data_path) {
            Ok(()) => crate::backups::extract_archive(&archive, &data_path).await,
            Err(e) => Err(e),
        };
        let _ = tokio::fs::remove_file(&archive).await;
        if let Err(e) = extracted {
            // Put the previous world back; the aside copy is kept if this fails.
            let _ = std::fs::remove_dir_all(&data_path);
            if let Some(aside) = aside {
                let _ = std::fs::rename(&aside, &data_path);
            }
            return Err(e);
        }
        Ok(())
    }

    async fn delete_backup(&self, id: &str, target: &BackupTarget, key: &str) -> Result<()> {
        crate::backups::delete(target, id, key).await
    }

    // ----- scheduled actions ---------------------------------------------

    async fn list_schedules(&self, server_id: &str) -> Result<Vec<Schedule>> {
        Ok(crate::schedules::list_for(&self.data_root, server_id))
    }

    async fn upsert_schedule(&self, schedule: Schedule) -> Result<()> {
        crate::schedules::upsert(&self.data_root, schedule).map_err(BackendError::io)
    }

    async fn delete_schedule(&self, id: &str) -> Result<()> {
        crate::schedules::delete(&self.data_root, id).map_err(BackendError::io)
    }

    // ----- metrics history -----------------------------------------------

    async fn query_metrics(&self, server_id: &str, since_ms: i64) -> Result<Vec<MetricPoint>> {
        crate::metrics::query_range(&self.data_root, server_id, since_ms)
            .await
            .map_err(BackendError::other)
    }

    // ----- player administration -----------------------------------------

    async fn list_players(&self, server_id: &str) -> Result<Vec<Player>> {
        let server = self.require_server(server_id)?;
        if server.status != ServerStatus::Running || !crate::players::supports(&server.game_type.0) {
            return Ok(Vec::new());
        }
        let cid = server
            .container_id
            .ok_or_else(|| BackendError::invalid("server has no container"))?;
        crate::players::list_players_mc(&self.docker, &cid)
            .await
            .map_err(BackendError::other)
    }

    async fn player_action(&self, server_id: &str, action: PlayerAction) -> Result<()> {
        let server = self.require_server(server_id)?;
        if !crate::players::supports(&server.game_type.0) {
            return Err(BackendError::Other(
                "player administration is not supported for this game".into(),
            ));
        }
        let cid = server
            .container_id
            .ok_or_else(|| BackendError::invalid("server has no container"))?;
        crate::players::player_action_mc(&self.docker, &cid, &action)
            .await
            .map_err(BackendError::other)
    }
}

/// Resolve a file-manager path and confine it under `<data_root>/servers` (canonicalising `..`
/// and symlinks; a not-yet-existing final component canonicalises its parent).
fn confine_path(data_root: &Path, path: &str) -> Result<PathBuf> {
    let root = std::fs::canonicalize(persistence::servers_data_root(data_root))
        .map_err(BackendError::io)?;
    let p = Path::new(path);
    let canon = match std::fs::canonicalize(p) {
        Ok(c) => c,
        Err(_) => {
            let parent = p
                .parent()
                .ok_or_else(|| BackendError::invalid("invalid path"))?;
            let name = p
                .file_name()
                .ok_or_else(|| BackendError::invalid("invalid path"))?;
            std::fs::canonicalize(parent)
                .map_err(BackendError::io)?
                .join(name)
        }
    };
    if !canon.starts_with(&root) {
        return Err(BackendError::invalid(
            "path is outside the server data directory",
        ));
    }
    Ok(canon)
}

/// RAII guard for the per-server install set; `acquire` returns `None` while an install runs.
struct InstallGuard {
    set: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    id: String,
}

impl InstallGuard {
    fn acquire(
        set: &Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
        id: &str,
    ) -> Option<Self> {
        let mut guard = set.lock().unwrap_or_else(|p| p.into_inner());
        if !guard.insert(id.to_string()) {
            return None; // already installing
        }
        Some(Self {
            set: set.clone(),
            id: id.to_string(),
        })
    }
}

impl Drop for InstallGuard {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.set.lock() {
            guard.remove(&self.id);
        }
    }
}

/// A single relative, separator-free path component; guards `create_server` against a
/// game_type that escapes the data root.
fn validate_path_component(s: &str) -> std::result::Result<(), String> {
    if s.is_empty() {
        return Err("must not be empty".into());
    }
    if s == "." || s == ".." {
        return Err("must not be '.' or '..'".into());
    }
    if s.contains('/') || s.contains('\\') {
        return Err("must not contain path separators".into());
    }
    // Also reject Windows drive/stream qualifiers.
    if s.contains(':') {
        return Err("must not contain ':'".into());
    }
    let mut comps = Path::new(s).components();
    match (comps.next(), comps.next()) {
        (Some(std::path::Component::Normal(_)), None) => Ok(()),
        _ => Err("must be a single relative path component".into()),
    }
}

/// Create the data dir and widen it to 0o777 on Unix so the container's uid 1000 can write it
/// even when the host process (agent as `localforge`) has a different uid.
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

/// Reject a destination equal to or inside the source: copying a folder into itself recurses into the
/// copy it is creating, and moving would delete the source out from under it.
fn ensure_not_nested(from: &Path, to: &Path) -> Result<()> {
    if to == from || to.starts_with(from) {
        return Err(BackendError::invalid(
            "the destination is inside the source",
        ));
    }
    Ok(())
}

/// The port the game listens on INSIDE the container: the chosen port when the game reads it from a
/// `SystemMapping::Port` variable, otherwise the image's fixed default (Minecraft's 25565, …).
fn container_port_for(game: &GameConfig, port: u16) -> u16 {
    let configurable = game
        .variables
        .iter()
        .any(|v| matches!(v.system_mapping, Some(SystemMapping::Port)));
    if configurable {
        port
    } else {
        game.ports.first().map(|p| p.container_port).unwrap_or(port)
    }
}

/// The game's startup command with `{{VAR}}` placeholders filled in; `None` when the image's CMD runs.
fn render_startup(game: &GameConfig, env: &HashMap<String, String>) -> Option<String> {
    if game.startup.is_empty() {
        return None;
    }
    let mut startup = game.startup.clone();
    for (key, value) in env {
        startup = startup.replace(&format!("{{{{{}}}}}", key), value);
    }
    Some(startup)
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

/// Install pipeline body; a free function so it can be spawned without borrowing `&self`.
#[allow(clippy::too_many_arguments)]
async fn run_install_inner(
    docker: DockerManager,
    data_root: PathBuf,
    server_id: String,
    server_data_path: PathBuf,
    install_image: String,
    volume_path: String,
    install_script: String,
    env: HashMap<String, String>,
    tx: tokio::sync::mpsc::UnboundedSender<Result<InstallEvent>>,
) -> Result<()> {
    // Persist the install container id immediately so log recovery works if interrupted.
    let id_for_callback = server_id.clone();
    let data_root_for_callback = data_root.clone();
    let on_container_created = move |container_id: &str| {
        if let Ok(mut srv) = persistence::load_server(&data_root_for_callback, &id_for_callback) {
            srv.install_container_id = Some(container_id.to_string());
            let _ = persistence::save_server(&data_root_for_callback, &srv);
        }
    };

    // The channel is UNBOUNDED on purpose: the callback runs on a runtime thread, and a
    // blocking send on a full bounded channel would panic ("Cannot block the current thread").
    let tx_lines = tx.clone();
    let on_output = move |line: String| {
        if let Some(url) = detect_oauth_url(&line) {
            let _ = tx_lines.send(Ok(InstallEvent::OauthUrl { url }));
        }
        let _ = tx_lines.send(Ok(InstallEvent::Log { line }));
    };

    let run_result = docker
        .run_script(
            &install_image,
            &server_data_path,
            &volume_path,
            &install_script,
            &env,
            on_container_created,
            on_output,
        )
        .await;

    let (exit_code, install_container_id) = match run_result {
        Ok(v) => v,
        Err(e) => {
            // Failed before an exit code: remove any container created, clear the id, mark Error.
            if let Ok(mut srv) = persistence::load_server(&data_root, &server_id) {
                if let Some(cid) = srv.install_container_id.take() {
                    let _ = docker.remove_install_container(&cid).await;
                }
                srv.status = ServerStatus::Error;
                let _ = persistence::save_server(&data_root, &srv);
            }
            return Err(BackendError::docker(e));
        }
    };

    // Clean up the install container (best-effort).
    let _ = docker.remove_install_container(&install_container_id).await;

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
