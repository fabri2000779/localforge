//! bollard-backed Docker operations for game-server containers.

use bollard::container::{AttachContainerResults, LogOutput};
use bollard::models::{ContainerCreateBody, ContainerStateStatusEnum, HostConfig, PortBinding};
use bollard::query_parameters::{
    AttachContainerOptions, CreateContainerOptions, CreateImageOptions, LogsOptions,
    RemoveContainerOptions, StartContainerOptions, StatsOptions, StopContainerOptions,
    WaitContainerOptions,
};
use bollard::Docker;
use futures_util::stream::StreamExt;
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;
use uuid::Uuid;

pub use localforge_core::{ContainerStats, DockerInfo, PortConfig, PortProtocol, ServerStatus};

#[derive(Error, Debug)]
pub enum DockerError {
    #[error("Docker connection error: {0}")]
    ConnectionError(#[from] bollard::errors::Error),

    #[error("Image pull failed: {0}")]
    ImagePullFailed(String),

    #[error("Attach failed: {0}")]
    AttachFailed(String),
}

#[derive(Clone)]
pub struct DockerManager {
    docker: Docker,
}

/// Inputs for [`DockerManager::create_container`], grouped to avoid a 9-argument call.
pub struct CreateContainerSpec<'a> {
    pub name: &'a str,
    pub image: &'a str,
    /// Host port the players connect to.
    pub port: u16,
    /// Port the game listens on inside the container (`port` is published onto it, TCP + UDP).
    pub container_port: u16,
    pub data_path: &'a Path,
    pub env: &'a HashMap<String, String>,
    pub extra_ports: &'a [PortConfig],
    pub volume_path: Option<&'a str>,
    pub memory_mb: Option<u32>,
    pub startup_command: Option<&'a str>,
}

impl DockerManager {
    pub async fn new() -> Result<Self, DockerError> {
        let docker = Docker::connect_with_local_defaults()?;
        Ok(Self { docker })
    }

    pub fn client(&self) -> &Docker {
        &self.docker
    }

    pub async fn ping(&self) -> Result<(), DockerError> {
        self.docker.ping().await?;
        Ok(())
    }

    pub async fn get_info(&self) -> Result<DockerInfo, DockerError> {
        let info = self.docker.info().await?;
        let version = self.docker.version().await?;

        Ok(DockerInfo {
            version: version.version.unwrap_or_default(),
            api_version: version.api_version.unwrap_or_default(),
            os: info.operating_system.unwrap_or_default(),
            arch: info.architecture.unwrap_or_default(),
            containers_running: info.containers_running.unwrap_or(0) as u64,
            containers_total: info.containers.unwrap_or(0) as u64,
            images: info.images.unwrap_or(0) as u64,
        })
    }

    pub async fn pull_image(&self, image: &str) -> Result<(), DockerError> {
        tracing::info!("Pulling image: {}", image);
        let options = Some(CreateImageOptions {
            from_image: Some(image.to_string()),
            ..Default::default()
        });

        let mut stream = self.docker.create_image(options, None, None);

        while let Some(result) = stream.next().await {
            match result {
                Ok(info) => {
                    if let Some(status) = info.status {
                        tracing::debug!("Pulling {}: {}", image, status);
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to pull image {}: {}", image, e);
                    return Err(DockerError::ImagePullFailed(e.to_string()));
                }
            }
        }

        tracing::info!("Successfully pulled image: {}", image);
        Ok(())
    }

    pub async fn create_container(
        &self,
        spec: CreateContainerSpec<'_>,
    ) -> Result<String, DockerError> {
        let CreateContainerSpec {
            name,
            image,
            port,
            container_port,
            data_path,
            env,
            extra_ports,
            volume_path,
            memory_mb,
            startup_command,
        } = spec;
        self.pull_image(image).await?;

        let env_vars: Vec<String> = env.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
        // Redact secret-ish values in the debug log; the real env still goes to the container.
        if tracing::enabled!(tracing::Level::DEBUG) {
            let redacted: Vec<String> = env
                .iter()
                .map(|(k, v)| {
                    let ku = k.to_ascii_uppercase();
                    if ["PASSWORD", "PASS", "TOKEN", "SECRET", "KEY"]
                        .iter()
                        .any(|needle| ku.contains(needle))
                    {
                        format!("{}=<redacted>", k)
                    } else {
                        format!("{}={}", k, v)
                    }
                })
                .collect();
            tracing::debug!("Environment variables: {:?}", redacted);
        }

        let mut port_bindings = HashMap::new();
        let mut exposed_ports: Vec<String> = Vec::new();

        // Main port (both TCP and UDP): host `port` → the game's in-container port.
        let container_port_tcp = format!("{}/tcp", container_port);
        let container_port_udp = format!("{}/udp", container_port);

        port_bindings.insert(
            container_port_tcp.clone(),
            Some(vec![PortBinding {
                host_ip: Some("0.0.0.0".to_string()),
                host_port: Some(port.to_string()),
            }]),
        );

        port_bindings.insert(
            container_port_udp.clone(),
            Some(vec![PortBinding {
                host_ip: Some("0.0.0.0".to_string()),
                host_port: Some(port.to_string()),
            }]),
        );

        exposed_ports.push(container_port_tcp);
        exposed_ports.push(container_port_udp);

        for extra in extra_ports {
            let protocols = match extra.protocol {
                PortProtocol::Tcp => vec!["tcp"],
                PortProtocol::Udp => vec!["udp"],
                PortProtocol::Both => vec!["tcp", "udp"],
            };

            for proto in protocols {
                let port_key = format!("{}/{}", extra.container_port, proto);
                port_bindings.insert(
                    port_key.clone(),
                    Some(vec![PortBinding {
                        host_ip: Some("0.0.0.0".to_string()),
                        host_port: Some(extra.container_port.to_string()),
                    }]),
                );
                exposed_ports.push(port_key);
            }

            let desc = extra.description.as_deref().unwrap_or("extra port");
            tracing::info!("Added extra port: {} ({:?}) - {}", extra.container_port, extra.protocol, desc);
        }

        // Docker on Windows wants forward slashes in bind paths.
        let data_path_str = data_path.to_string_lossy().replace('\\', "/");
        let container_volume_path = volume_path.unwrap_or("/data");
        let data_mount = format!("{}:{}", data_path_str, container_volume_path);
        tracing::info!("Volume mount: {}", data_mount);
        
        // Create a persistent machine-id file for hardware identification (needed by Hytale)
        let machine_id_path = data_path.join(".machine-id");
        if !machine_id_path.exists() {
            let machine_id = format!("{}\n", Uuid::new_v4().to_string().replace("-", ""));
            if let Err(e) = std::fs::write(&machine_id_path, &machine_id) {
                tracing::warn!("Failed to create machine-id file: {}", e);
            } else {
                tracing::info!("Created machine-id file: {}", machine_id.trim());
            }
        }
        let machine_id_mount = format!("{}/.machine-id:/etc/machine-id:ro", data_path_str);

        let memory_limit = memory_mb.map(|mb| (mb as i64) * 1024 * 1024);
        if let Some(mb) = memory_mb {
            tracing::info!("Container memory limit: {} MB", mb);
        }

        let host_config = HostConfig {
            port_bindings: Some(port_bindings),
            binds: Some(vec![data_mount, machine_id_mount]),
            memory: memory_limit,
            memory_swap: memory_limit, // Same as memory to disable swap
            // No Docker restart policy: the crash-watcher owns restarts (backoff, alerts, per-server
            // RestartPolicy), and Docker auto-restarting in ~100 ms hid every crash from it.
            restart_policy: Some(bollard::models::RestartPolicy {
                name: Some(bollard::models::RestartPolicyNameEnum::NO),
                ..Default::default()
            }),
            ..Default::default()
        };

        let cmd: Option<Vec<String>> = startup_command.and_then(|startup| {
            if startup.is_empty() {
                None
            } else {
                let full_cmd = format!("cd {} && exec {}", container_volume_path, startup);
                // debug, not info: a custom startup command can carry sensitive args.
                tracing::debug!("Container command: {}", full_cmd);
                Some(vec!["/bin/bash".to_string(), "-c".to_string(), full_cmd])
            }
        });

        let config = ContainerCreateBody {
            image: Some(image.to_string()),
            env: Some(env_vars),
            exposed_ports: Some(exposed_ports),
            host_config: Some(host_config),
            cmd,
            tty: Some(true),
            open_stdin: Some(true),
            attach_stdin: Some(true),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            ..Default::default()
        };

        let container_name = format!("localforge-{}", name);
        let options = Some(CreateContainerOptions {
            name: Some(container_name.clone()),
            platform: String::new(),
        });

        tracing::info!("Creating container: {}", container_name);
        let response = self.docker.create_container(options, config).await?;
        tracing::info!("Container created with ID: {}", response.id);

        Ok(response.id)
    }

    pub async fn start_container(&self, container_id: &str) -> Result<(), DockerError> {
        tracing::info!("Starting container: {}", container_id);
        self.docker
            .start_container(container_id, None::<StartContainerOptions>)
            .await?;
        tracing::info!("Container start command sent: {}", container_id);
        Ok(())
    }

    pub async fn stop_container(&self, container_id: &str) -> Result<(), DockerError> {
        tracing::info!("Stopping container: {}", container_id);
        let options = Some(StopContainerOptions {
            t: Some(30),
            ..Default::default()
        });
        self.docker.stop_container(container_id, options).await?;
        Ok(())
    }

    /// Whether the container still carries the inputs `create_container` froze in: every desired env
    /// var, the same startup command (when one is set) and the main port binding. `false` means the
    /// saved configuration only takes effect after a recreate.
    pub async fn container_matches(
        &self,
        container_id: &str,
        env: &HashMap<String, String>,
        startup_command: Option<&str>,
        volume_path: Option<&str>,
        port: u16,
        container_port: u16,
    ) -> Result<bool, DockerError> {
        let info = self
            .docker
            .inspect_container(
                container_id,
                None::<bollard::query_parameters::InspectContainerOptions>,
            )
            .await?;
        let config = info.config.unwrap_or_default();
        let have: std::collections::HashSet<String> =
            config.env.unwrap_or_default().into_iter().collect();
        if !env.iter().all(|(k, v)| have.contains(&format!("{}={}", k, v))) {
            return Ok(false);
        }
        if let Some(startup) = startup_command.filter(|s| !s.is_empty()) {
            let want = vec![
                "/bin/bash".to_string(),
                "-c".to_string(),
                format!("cd {} && exec {}", volume_path.unwrap_or("/data"), startup),
            ];
            if config.cmd.as_deref() != Some(want.as_slice()) {
                return Ok(false);
            }
        }
        let bindings = info
            .host_config
            .and_then(|h| h.port_bindings)
            .unwrap_or_default();
        let bound = bindings
            .get(&format!("{}/tcp", container_port))
            .and_then(|b| b.as_ref())
            .map(|list| list.iter().any(|pb| pb.host_port.as_deref() == Some(port.to_string().as_str())))
            .unwrap_or(false);
        Ok(bound)
    }

    pub async fn remove_container(&self, container_id: &str) -> Result<(), DockerError> {
        tracing::info!("Removing container: {}", container_id);
        let options = Some(RemoveContainerOptions {
            force: true,
            v: false,
            ..Default::default()
        });
        self.docker.remove_container(container_id, options).await?;
        Ok(())
    }

    pub async fn get_container_status(
        &self,
        container_id: &str,
    ) -> Result<ServerStatus, DockerError> {
        match self.docker.inspect_container(container_id, None).await {
            Ok(info) => {
                if let Some(state) = info.state {
                    let status = state.status;
                    tracing::debug!("Container {} status: {:?}", container_id, status);
                    
                    return Ok(match status {
                        Some(ContainerStateStatusEnum::RUNNING) => ServerStatus::Running,
                        Some(ContainerStateStatusEnum::CREATED) => ServerStatus::Stopped,
                        Some(ContainerStateStatusEnum::RESTARTING) => ServerStatus::Starting,
                        Some(ContainerStateStatusEnum::PAUSED) => ServerStatus::Stopped,
                        Some(ContainerStateStatusEnum::REMOVING) => ServerStatus::Stopping,
                        Some(ContainerStateStatusEnum::STOPPING) => ServerStatus::Stopping,
                        Some(ContainerStateStatusEnum::EXITED) => ServerStatus::Stopped,
                        Some(ContainerStateStatusEnum::DEAD) => ServerStatus::Error,
                        None | Some(ContainerStateStatusEnum::EMPTY) => ServerStatus::Stopped,
                    });
                }
                Ok(ServerStatus::Stopped)
            }
            Err(e) => {
                // Propagate: mapping this to Ok(Stopped) made a daemon hiccup look like a mass exit to the crash-watcher.
                tracing::warn!("Failed to inspect container {}: {}", container_id, e);
                Err(e.into())
            }
        }
    }

    pub async fn get_container_stats(
        &self,
        container_id: &str,
    ) -> Result<ContainerStats, DockerError> {
        let options = Some(StatsOptions {
            stream: false,
            one_shot: true,
        });

        let mut stream = self.docker.stats(container_id, options);

        if let Some(Ok(stats)) = stream.next().await {
            // Every layer of bollard's stats tree is Option; fall back to 0 (1 for online_cpus).
            let cpu_total = stats
                .cpu_stats
                .as_ref()
                .and_then(|c| c.cpu_usage.as_ref())
                .and_then(|u| u.total_usage)
                .unwrap_or(0);
            let precpu_total = stats
                .precpu_stats
                .as_ref()
                .and_then(|c| c.cpu_usage.as_ref())
                .and_then(|u| u.total_usage)
                .unwrap_or(0);
            let cpu_delta = cpu_total as f64 - precpu_total as f64;

            let sys_cpu = stats
                .cpu_stats
                .as_ref()
                .and_then(|c| c.system_cpu_usage)
                .unwrap_or(0);
            let pre_sys_cpu = stats
                .precpu_stats
                .as_ref()
                .and_then(|c| c.system_cpu_usage)
                .unwrap_or(0);
            let system_delta = sys_cpu as f64 - pre_sys_cpu as f64;

            let num_cpus = stats
                .cpu_stats
                .as_ref()
                .and_then(|c| c.online_cpus)
                .unwrap_or(1) as f64;

            let cpu_percent = if system_delta > 0.0 && cpu_delta > 0.0 {
                (cpu_delta / system_delta) * num_cpus * 100.0
            } else {
                0.0
            };

            let memory_usage = stats
                .memory_stats
                .as_ref()
                .and_then(|m| m.usage)
                .unwrap_or(0) as f64
                / 1024.0
                / 1024.0;
            let memory_limit = stats
                .memory_stats
                .as_ref()
                .and_then(|m| m.limit)
                .unwrap_or(1) as f64
                / 1024.0
                / 1024.0;
            let memory_percent = if memory_limit > 0.0 {
                (memory_usage / memory_limit) * 100.0
            } else {
                0.0
            };

            let (net_rx_bytes, net_tx_bytes) = stats
                .networks
                .as_ref()
                .map(|nets| {
                    nets.values().fold((0u64, 0u64), |(rx, tx), n| {
                        (
                            rx + n.rx_bytes.unwrap_or(0),
                            tx + n.tx_bytes.unwrap_or(0),
                        )
                    })
                })
                .unwrap_or((0, 0));

            return Ok(ContainerStats {
                cpu_percent,
                memory_usage_mb: memory_usage,
                memory_limit_mb: memory_limit,
                memory_percent,
                net_rx_bytes,
                net_tx_bytes,
            });
        }

        Ok(ContainerStats {
            cpu_percent: 0.0,
            memory_usage_mb: 0.0,
            memory_limit_mb: 0.0,
            memory_percent: 0.0,
            net_rx_bytes: 0,
            net_tx_bytes: 0,
        })
    }

    pub async fn send_stdin(&self, container_id: &str, input: &str) -> Result<(), DockerError> {
        use tokio::io::AsyncWriteExt;
        
        tracing::info!("Sending stdin to container {}: {}", container_id, input);
        
        let options = AttachContainerOptions {
            stdin: true,
            stdout: false,
            stderr: false,
            stream: true,
            logs: false,
            ..Default::default()
        };

        match self.docker.attach_container(container_id, Some(options)).await {
            Ok(AttachContainerResults { input: mut stdin_writer, .. }) => {
                // Write verbatim: every caller newline-terminates already.
                stdin_writer.write_all(input.as_bytes()).await
                    .map_err(|e| DockerError::AttachFailed(format!("Failed to write to stdin: {}", e)))?;
                stdin_writer.flush().await
                    .map_err(|e| DockerError::AttachFailed(format!("Failed to flush stdin: {}", e)))?;
                tracing::info!("Successfully sent command to container stdin");
                Ok(())
            }
            Err(e) => {
                tracing::error!("Failed to attach to container: {}", e);
                Err(DockerError::AttachFailed(e.to_string()))
            }
        }
    }

    pub async fn get_logs(
        &self,
        container_id: &str,
        lines: u32,
    ) -> Result<Vec<String>, DockerError> {
        let options = Some(LogsOptions {
            stdout: true,
            stderr: true,
            tail: lines.to_string(),
            timestamps: false,
            ..Default::default()
        });

        let mut stream = self.docker.logs(container_id, options);
        let mut logs = Vec::new();

        while let Some(result) = stream.next().await {
            match result {
                Ok(output) => {
                    let line = match output {
                        LogOutput::StdOut { message } => {
                            String::from_utf8_lossy(&message).to_string()
                        }
                        LogOutput::StdErr { message } => {
                            String::from_utf8_lossy(&message).to_string()
                        }
                        LogOutput::Console { message } => {
                            String::from_utf8_lossy(&message).to_string()
                        }
                        LogOutput::StdIn { message } => {
                            String::from_utf8_lossy(&message).to_string()
                        }
                    };
                    for l in line.lines() {
                        if !l.trim().is_empty() {
                            logs.push(l.to_string());
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Error reading log: {}", e);
                }
            }
        }

        Ok(logs)
    }

    /// Run `script` in a one-off container, streaming its output; returns `(exit_code, container_id)`.
    /// `on_container_created` receives the id before start so it can be persisted for log recovery.
    #[allow(clippy::too_many_arguments)]
    pub async fn run_script<F, C>(
        &self,
        image: &str,
        data_path: &std::path::Path,
        volume_path: &str,
        script: &str,
        env: &HashMap<String, String>,
        on_container_created: C,
        mut on_output: F,
    ) -> Result<(i64, String), DockerError>
    where
        F: FnMut(String),
        C: FnOnce(&str),
    {
        use base64::Engine;
        
        tracing::info!("Running install script in temporary container");
        
        self.pull_image(image).await?;
        
        let data_path_str = data_path.to_string_lossy().replace('\\', "/");
        let data_mount = format!("{}:{}", data_path_str, volume_path);
        
        // Create a persistent machine-id file for hardware identification (needed by Hytale)
        let machine_id_path = data_path.join(".machine-id");
        if !machine_id_path.exists() {
            let machine_id = format!("{}\n", Uuid::new_v4().to_string().replace("-", ""));
            if let Err(e) = std::fs::write(&machine_id_path, &machine_id) {
                tracing::warn!("Failed to create machine-id file: {}", e);
            } else {
                tracing::info!("Created machine-id file: {}", machine_id.trim());
            }
        }
        let machine_id_mount = format!("{}/.machine-id:/etc/machine-id:ro", data_path_str);
        
        // Encode script to base64 to avoid shell escaping issues
        let encoded_script = base64::engine::general_purpose::STANDARD.encode(script);
        
        let cmd = format!(
            "echo '{}' | base64 -d > /tmp/install.sh && chmod +x /tmp/install.sh && exec /tmp/install.sh",
            encoded_script
        );
        
        let host_config = HostConfig {
            binds: Some(vec![
                data_mount,
                machine_id_mount,
            ]),
            ..Default::default()
        };
        
        let container_name = format!("localforge-install-{}", &Uuid::new_v4().to_string()[..8]);
        
        let config = ContainerCreateBody {
            image: Some(image.to_string()),
            cmd: Some(vec!["/bin/sh".to_string(), "-c".to_string(), cmd]),
            env: Some(env.iter().map(|(k, v)| format!("{}={}", k, v)).collect()),
            host_config: Some(host_config),
            working_dir: Some(volume_path.to_string()),
            tty: Some(false),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            ..Default::default()
        };

        let options = Some(CreateContainerOptions {
            name: Some(container_name.clone()),
            platform: String::new(),
        });

        tracing::info!("Creating temporary install container: {}", container_name);
        let container = self.docker.create_container(options, config).await?;
        let container_id = container.id.clone();

        on_container_created(&container_id);

        tracing::info!("Starting install container: {}", container_id);

        self.docker
            .start_container(&container_id, None::<StartContainerOptions>)
            .await?;

        let log_options = LogsOptions {
            follow: true,
            stdout: true,
            stderr: true,
            timestamps: false,
            ..Default::default()
        };
        let mut log_stream = self.docker.logs(&container_id, Some(log_options));

        // wait_container is the authoritative completion signal: on Windows the follow-log stream
        // over the named pipe doesn't reliably end, which left installs stuck in "installing".
        let mut wait_stream = self
            .docker
            .wait_container(&container_id, None::<WaitContainerOptions>);

        let mut exit_code: Option<i64> = None;
        while exit_code.is_none() {
            tokio::select! {
                log = log_stream.next() => {
                    match log {
                        Some(Ok(output)) => emit_log_lines(output, &mut on_output),
                        Some(Err(e)) => tracing::warn!("Install log stream error: {}", e),
                        // Logs ended before the wait fired: container is gone.
                        None => break,
                    }
                }
                wait = wait_stream.next() => {
                    exit_code = Some(match wait {
                        Some(Ok(resp)) => resp.status_code,
                        // A non-zero exit surfaces as an error carrying the code.
                        Some(Err(bollard::errors::Error::DockerContainerWaitError { code, .. })) => code,
                        Some(Err(e)) => {
                            tracing::warn!("wait_container error: {}", e);
                            -1
                        }
                        None => -1,
                    });
                }
            }
        }

        // Drain buffered logs so the final lines aren't truncated; bounded so a stuck stream can't wedge us.
        while let Ok(Some(Ok(output))) = tokio::time::timeout(
            tokio::time::Duration::from_millis(500),
            log_stream.next(),
        )
        .await
        {
            emit_log_lines(output, &mut on_output);
        }

        // Log stream ended before wait produced a code: inspect for the exit status.
        let exit_code = match exit_code {
            Some(code) => code,
            None => match self.docker.inspect_container(&container_id, None).await {
                Ok(info) => info.state.and_then(|s| s.exit_code).unwrap_or(-1),
                Err(e) => {
                    tracing::error!("Failed to inspect container for exit code: {}", e);
                    -1
                }
            },
        };

        tracing::info!("Install container finished with exit code: {}", exit_code);

        // Kept for log retrieval; removed by run_install_inner or on delete.
        Ok((exit_code, container_id))
    }

    pub async fn remove_install_container(&self, container_id: &str) -> Result<(), DockerError> {
        let _ = self
            .docker
            .remove_container(
                container_id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await;
        Ok(())
    }
}

/// Decode a log frame and forward each non-empty line.
fn emit_log_lines<F: FnMut(String)>(output: LogOutput, on_output: &mut F) {
    let text = match output {
        LogOutput::StdOut { message } => String::from_utf8_lossy(&message).to_string(),
        LogOutput::StdErr { message } => String::from_utf8_lossy(&message).to_string(),
        LogOutput::Console { message } => String::from_utf8_lossy(&message).to_string(),
        _ => String::new(),
    };
    for line in text.lines() {
        if !line.is_empty() {
            on_output(line.to_string());
        }
    }
}
