use crate::commands::games::GamesState;
use crate::docker::DockerManager;
use crate::games::{build_env_vars, apply_config_variables, GameType};
use bollard::container::{LogOutput, LogsOptions};
use bollard::exec::{CreateExecOptions, StartExecResults};
use futures_util::stream::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Server {
    pub id: String,
    pub name: String,
    pub game_type: GameType,
    pub status: ServerStatus,
    pub container_id: Option<String>,
    pub port: u16,
    pub memory_mb: u32,
    pub data_path: PathBuf,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub config: HashMap<String, String>,
    #[serde(default)]
    pub installed: bool,
    #[serde(default)]
    pub install_container_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ServerStatus {
    Stopped,
    Starting,
    Installing,
    Running,
    Stopping,
    Error,
}

#[derive(Debug, Deserialize)]
pub struct CreateServerRequest {
    pub name: String,
    pub game_type: GameType,
    pub port: Option<u16>,
    pub config: Option<HashMap<String, String>>,
    pub memory_mb: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct ServerResponse {
    pub success: bool,
    pub server: Option<Server>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LogsResponse {
    pub logs: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogEvent {
    pub server_id: String,
    pub line: String,
}

pub struct LogStreamHandle {
    pub cancel_tx: tokio::sync::watch::Sender<bool>,
}

pub struct ServerState {
    pub streams: Arc<Mutex<HashMap<String, LogStreamHandle>>>,
}

impl Default for ServerState {
    fn default() -> Self {
        Self {
            streams: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[tauri::command]
pub async fn create_server(
    request: CreateServerRequest,
    games_state: State<'_, GamesState>,
) -> Result<ServerResponse, String> {
    tracing::info!("Creating server: {:?}", request.name);

    let docker = DockerManager::new().await.map_err(|e| e.to_string())?;
    
    let games_manager = games_state.manager.lock().await;
    let game_config = games_manager.get_game(&request.game_type)
        .ok_or_else(|| format!("Game type '{}' not found", request.game_type))?;
    
    let server_id = Uuid::new_v4().to_string()[..8].to_string();
    
    let port = request.port.unwrap_or_else(|| {
        game_config.ports.first()
            .map(|p| p.container_port)
            .unwrap_or(25565)
    });

    let memory_mb = request.memory_mb.unwrap_or(game_config.recommended_ram_mb);

    let data_path = get_servers_dir()
        .join(request.game_type.to_string())
        .join(&server_id);

    std::fs::create_dir_all(&data_path).map_err(|e| e.to_string())?;

    let user_config = request.config.clone().unwrap_or_default();
    let env = build_env_vars(&game_config, memory_mb, port, &user_config);

    tracing::info!("Server memory limit: {} MB", memory_mb);

    let extra_ports: Vec<_> = game_config.ports.iter()
        .skip(1)
        .cloned()
        .collect();

    // Get startup command if defined
    let startup_command = if game_config.startup.is_empty() {
        None
    } else {
        // Resolve variables in startup command
        let mut startup = game_config.startup.clone();
        for (key, value) in &env {
            startup = startup.replace(&format!("{{{{{}}}}}", key), value);
        }
        Some(startup)
    };

    drop(games_manager);

    let container_id = docker
        .create_container(
            &server_id,
            &game_config.docker_image,
            port,
            &data_path,
            &env,
            &extra_ports,
            Some(&game_config.volume_path),
            Some(memory_mb),
            startup_command.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())?;

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

    save_server_config(&server)?;

    Ok(ServerResponse {
        success: true,
        server: Some(server),
        error: None,
    })
}

#[tauri::command(rename_all = "camelCase")]
pub async fn start_server(
    server_id: String,
    app: AppHandle,
    state: State<'_, ServerState>,
    games_state: State<'_, GamesState>,
) -> Result<ServerResponse, String> {
    tracing::info!("Starting server: {}", server_id);

    let docker = DockerManager::new().await.map_err(|e| e.to_string())?;
    let mut server = load_server_config(&server_id)?;

    // Check if we need to run install first
    if !server.installed {
        let has_install = {
            let games_manager = games_state.manager.lock().await;
            games_manager.get_game(&server.game_type)
                .and_then(|g| g.install_script.clone())
                .map(|s| !s.is_empty())
                .unwrap_or(false)
        };
        
        if has_install {
            tracing::info!("Server needs installation, running install script first");
            server = run_install_script_internal(&server_id, &app, &state, &games_state).await?;
        } else {
            server.installed = true;
            save_server_config(&server)?;
        }
    }

    let container_id = server.container_id.clone().ok_or("No container ID")?;

    docker
        .start_container(&container_id)
        .await
        .map_err(|e| e.to_string())?;

    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    let status = docker
        .get_container_status(&container_id)
        .await
        .map_err(|e| e.to_string())?;

    if status == ServerStatus::Stopped || status == ServerStatus::Error {
        return Err("Container failed to start".to_string());
    }

    server.status = status;
    save_server_config(&server)?;

    start_log_stream(&server_id, &container_id, app, &state).await;

    Ok(ServerResponse {
        success: true,
        server: Some(server),
        error: None,
    })
}

async fn start_log_stream(
    server_id: &str,
    container_id: &str,
    app: AppHandle,
    state: &State<'_, ServerState>,
) {
    {
        let mut streams = state.streams.lock().await;
        if let Some(handle) = streams.remove(server_id) {
            let _ = handle.cancel_tx.send(true);
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

    {
        let mut streams = state.streams.lock().await;
        streams.insert(server_id.to_string(), LogStreamHandle { cancel_tx });
    }

    let server_id = server_id.to_string();
    let container_id = container_id.to_string();

    tokio::spawn(async move {
        stream_logs_loop(server_id, container_id, app, cancel_rx).await;
    });
}

async fn stream_logs_loop(
    server_id: String,
    container_id: String,
    app: AppHandle,
    mut cancel_rx: tokio::sync::watch::Receiver<bool>,
) {
    let mut reconnect_attempts = 0;
    let max_reconnects = 10;

    loop {
        if *cancel_rx.borrow() {
            break;
        }

        let docker = match DockerManager::new().await {
            Ok(d) => d,
            Err(e) => {
                tracing::error!("Docker connect failed: {}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                reconnect_attempts += 1;
                if reconnect_attempts > max_reconnects {
                    break;
                }
                continue;
            }
        };

        match docker.get_container_status(&container_id).await {
            Ok(status) if status != ServerStatus::Running && status != ServerStatus::Installing => {
                break;
            }
            Err(_) => {}
            _ => {}
        }

        let options = LogsOptions::<String> {
            follow: true,
            stdout: true,
            stderr: true,
            timestamps: false,
            tail: "50".to_string(),
            ..Default::default()
        };

        let mut log_stream = docker.client().logs(&container_id, Some(options));

        loop {
            tokio::select! {
                biased;
                
                _ = cancel_rx.changed() => {
                    if *cancel_rx.borrow() {
                        return;
                    }
                }
                
                result = log_stream.next() => {
                    match result {
                        Some(Ok(log)) => {
                            reconnect_attempts = 0;
                            
                            let text = match &log {
                                LogOutput::StdOut { message } => String::from_utf8_lossy(message).to_string(),
                                LogOutput::StdErr { message } => String::from_utf8_lossy(message).to_string(),
                                LogOutput::Console { message } => String::from_utf8_lossy(message).to_string(),
                                LogOutput::StdIn { message } => String::from_utf8_lossy(message).to_string(),
                            };

                            for line in text.lines() {
                                if !line.is_empty() {
                                    let event = LogEvent {
                                        server_id: server_id.clone(),
                                        line: line.to_string(),
                                    };
                                    let _ = app.emit("server-log", event);
                                }
                            }
                        }
                        Some(Err(_)) | None => {
                            break;
                        }
                    }
                }
            }
        }

        reconnect_attempts += 1;
        if reconnect_attempts > max_reconnects {
            break;
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn stop_server(
    server_id: String,
    state: State<'_, ServerState>,
    games_state: State<'_, GamesState>,
) -> Result<ServerResponse, String> {
    tracing::info!("Stopping server: {}", server_id);

    {
        let mut streams = state.streams.lock().await;
        if let Some(handle) = streams.remove(&server_id) {
            let _ = handle.cancel_tx.send(true);
        }
    }

    let docker = DockerManager::new().await.map_err(|e| e.to_string())?;
    let mut server = load_server_config(&server_id)?;

    if let Some(container_id) = &server.container_id {
        let games_manager = games_state.manager.lock().await;
        if let Some(game_config) = games_manager.get_game(&server.game_type) {
            if !game_config.stop_command.is_empty() {
                tracing::info!("Sending stop command: {}", game_config.stop_command);
                let _ = docker.send_stdin(container_id, &game_config.stop_command).await;
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        }
        drop(games_manager);
        
        docker
            .stop_container(container_id)
            .await
            .map_err(|e| e.to_string())?;
        server.status = ServerStatus::Stopped;
        save_server_config(&server)?;
    }

    Ok(ServerResponse {
        success: true,
        server: Some(server),
        error: None,
    })
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_server(
    server_id: String,
    delete_data: Option<bool>,
    state: State<'_, ServerState>,
) -> Result<ServerResponse, String> {
    tracing::info!("Deleting server: {}", server_id);

    {
        let mut streams = state.streams.lock().await;
        if let Some(handle) = streams.remove(&server_id) {
            let _ = handle.cancel_tx.send(true);
        }
    }

    let docker = DockerManager::new().await.map_err(|e| e.to_string())?;
    let server = load_server_config(&server_id)?;

    if let Some(container_id) = &server.container_id {
        docker.stop_container(container_id).await.ok();
        docker.remove_container(container_id).await.ok();
    }
    
    // Also remove install container if it exists
    if let Some(install_container_id) = &server.install_container_id {
        docker.remove_install_container(install_container_id).await.ok();
    }

    let config_path = get_server_config_path(&server_id);
    std::fs::remove_file(config_path).ok();

    if delete_data.unwrap_or(true) {
        if server.data_path.exists() {
            std::fs::remove_dir_all(&server.data_path).ok();
        }
    }

    Ok(ServerResponse {
        success: true,
        server: None,
        error: None,
    })
}

#[tauri::command]
pub async fn list_servers() -> Result<Vec<Server>, String> {
    let docker = DockerManager::new().await.map_err(|e| e.to_string())?;
    let config_dir = get_servers_config_dir();

    if !config_dir.exists() {
        return Ok(Vec::new());
    }

    let mut servers = Vec::new();

    for entry in std::fs::read_dir(config_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();

        if path.extension().map(|e| e == "json").unwrap_or(false) {
            let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
            let mut server: Server = serde_json::from_str(&content).map_err(|e| e.to_string())?;

            // Don't overwrite Installing status - it's managed by the install process
            if server.status != ServerStatus::Installing {
                if let Some(container_id) = &server.container_id {
                    server.status = docker
                        .get_container_status(container_id)
                        .await
                        .unwrap_or(ServerStatus::Error);
                }
            }

            servers.push(server);
        }
    }

    servers.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(servers)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_server_status(server_id: String) -> Result<ServerStatus, String> {
    let server = load_server_config(&server_id)?;
    
    // Don't overwrite Installing status
    if server.status == ServerStatus::Installing {
        return Ok(ServerStatus::Installing);
    }
    
    let docker = DockerManager::new().await.map_err(|e| e.to_string())?;

    if let Some(container_id) = &server.container_id {
        docker
            .get_container_status(container_id)
            .await
            .map_err(|e| e.to_string())
    } else {
        Ok(ServerStatus::Stopped)
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn send_command(
    server_id: String,
    command: String,
) -> Result<String, String> {
    tracing::info!("Sending command to {}: {}", server_id, command);

    let docker = DockerManager::new().await.map_err(|e| e.to_string())?;
    let server = load_server_config(&server_id)?;
    let container_id = server.container_id.ok_or("No container ID")?;

    if docker.send_stdin(&container_id, &command).await.is_ok() {
        return Ok("Command sent".to_string());
    }

    send_via_mc_console(&docker, &container_id, &command).await
}

async fn send_via_mc_console(
    docker: &DockerManager,
    container_id: &str,
    command: &str,
) -> Result<String, String> {
    let exec_cmd = vec!["mc-send-to-console".to_string(), command.to_string()];
    
    let exec_options = CreateExecOptions {
        cmd: Some(exec_cmd),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        ..Default::default()
    };

    let exec = docker
        .client()
        .create_exec(container_id, exec_options)
        .await
        .map_err(|e| e.to_string())?;

    let mut output = String::new();
    match docker.client().start_exec(&exec.id, None).await {
        Ok(StartExecResults::Attached { output: mut out, .. }) => {
            while let Some(Ok(msg)) = out.next().await {
                let text = match msg {
                    LogOutput::StdOut { message } => String::from_utf8_lossy(&message).to_string(),
                    LogOutput::StdErr { message } => String::from_utf8_lossy(&message).to_string(),
                    _ => String::new(),
                };
                output.push_str(&text);
            }
        }
        Ok(StartExecResults::Detached) => {}
        Err(e) => return Err(e.to_string()),
    }
    
    Ok(output)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_server_stats(server_id: String) -> Result<crate::docker::ContainerStats, String> {
    let docker = DockerManager::new().await.map_err(|e| e.to_string())?;
    let server = load_server_config(&server_id)?;

    if let Some(container_id) = &server.container_id {
        docker
            .get_container_stats(container_id)
            .await
            .map_err(|e| e.to_string())
    } else {
        Ok(crate::docker::ContainerStats {
            cpu_percent: 0.0,
            memory_usage_mb: 0.0,
            memory_limit_mb: 0.0,
            memory_percent: 0.0,
        })
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_server_logs(server_id: String, lines: Option<u32>) -> Result<LogsResponse, String> {
    let server = load_server_config(&server_id)?;
    let docker = DockerManager::new().await.map_err(|e| e.to_string())?;
    
    tracing::info!("get_server_logs: server status = {:?}, install_container_id = {:?}", 
        server.status, server.install_container_id);
    
    // If server is installing, try to get logs from install container
    if server.status == ServerStatus::Installing {
        if let Some(install_container_id) = &server.install_container_id {
            tracing::info!("Fetching logs from install container: {}", install_container_id);
            let logs = docker
                .get_logs(install_container_id, lines.unwrap_or(500))
                .await
                .unwrap_or_else(|e| {
                    tracing::error!("Failed to get install logs: {}", e);
                    vec!["[Serverwave] Installation in progress...".to_string()]
                });
            tracing::info!("Got {} log lines from install container", logs.len());
            return Ok(LogsResponse { logs, error: None });
        }
        tracing::info!("No install_container_id found, showing placeholder");
        return Ok(LogsResponse {
            logs: vec!["[Serverwave] Installation in progress...".to_string()],
            error: None,
        });
    }

    if let Some(container_id) = &server.container_id {
        let logs = docker
            .get_logs(container_id, lines.unwrap_or(500))
            .await
            .map_err(|e| e.to_string())?;

        Ok(LogsResponse { logs, error: None })
    } else {
        Ok(LogsResponse {
            logs: Vec::new(),
            error: Some("No container".to_string()),
        })
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn attach_server(
    server_id: String,
    app: AppHandle,
    state: State<'_, ServerState>,
) -> Result<(), String> {
    let docker = DockerManager::new().await.map_err(|e| e.to_string())?;
    let server = load_server_config(&server_id)?;

    // Don't attach if server is installing - the install logs are emitted separately
    if server.status == ServerStatus::Installing {
        tracing::info!("Server {} is installing, skipping main container attach", server_id);
        return Ok(());
    }

    let container_id = match &server.container_id {
        Some(id) => id.clone(),
        None => return Ok(()),
    };

    let status = docker.get_container_status(&container_id).await.map_err(|e| e.to_string())?;
    
    if status != ServerStatus::Running {
        return Ok(());
    }

    start_log_stream(&server_id, &container_id, app, &state).await;
    Ok(())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn detach_server(server_id: String, state: State<'_, ServerState>) -> Result<(), String> {
    let mut streams = state.streams.lock().await;
    if let Some(handle) = streams.remove(&server_id) {
        let _ = handle.cancel_tx.send(true);
    }
    Ok(())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn update_server_config(
    server_id: String,
    config: HashMap<String, String>,
) -> Result<ServerResponse, String> {
    let mut server = load_server_config(&server_id)?;
    server.config = config;
    save_server_config(&server)?;
    
    Ok(ServerResponse {
        success: true,
        server: Some(server),
        error: None,
    })
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_server_disk_usage(server_id: String) -> Result<u64, String> {
    let server = load_server_config(&server_id)?;
    
    if !server.data_path.exists() {
        return Ok(0);
    }
    
    Ok(calculate_dir_size(&server.data_path).unwrap_or(0))
}

// Internal function for running install script
async fn run_install_script_internal(
    server_id: &str,
    app: &AppHandle,
    _state: &State<'_, ServerState>,
    games_state: &State<'_, GamesState>,
) -> Result<Server, String> {
    tracing::info!("Running install script for server: {}", server_id);
    
    let docker = DockerManager::new().await.map_err(|e| e.to_string())?;
    let mut server = load_server_config(server_id)?;
    
    let games_manager = games_state.manager.lock().await;
    let game_config = games_manager.get_game(&server.game_type)
        .ok_or_else(|| format!("Game type '{}' not found", server.game_type))?;
    
    let install_script = match &game_config.install_script {
        Some(script) if !script.is_empty() => script.clone(),
        _ => {
            tracing::info!("No install script for game type: {}", server.game_type);
            server.installed = true;
            save_server_config(&server)?;
            return Ok(server);
        }
    };
    let volume_path = game_config.volume_path.clone();
    let install_image = game_config.install_image.clone()
        .unwrap_or_else(|| game_config.docker_image.clone());
    drop(games_manager);
    
    // Set installing status
    server.status = ServerStatus::Installing;
    save_server_config(&server)?;
    
    let _ = app.emit("server-log", LogEvent {
        server_id: server_id.to_string(),
        line: "[Serverwave] Starting installation...".to_string(),
    });
    
    // Run install script using docker run (temporary container)
    // This avoids issues with the main container's startup command failing
    let app_clone = app.clone();
    let server_id_clone = server_id.to_string();
    let opened_urls: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>> = 
        std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
    let opened_urls_clone = opened_urls.clone();
    
    // Callback to save install container ID for log recovery
    let server_id_for_callback = server_id.to_string();
    let on_container_created = move |container_id: &str| {
        if let Ok(mut srv) = load_server_config(&server_id_for_callback) {
            srv.install_container_id = Some(container_id.to_string());
            let _ = save_server_config(&srv);
            tracing::info!("Saved install container ID: {}", container_id);
        }
    };
    
    let (exit_code, install_container_id) = docker.run_script(
        &install_image,
        &server.data_path,
        &volume_path,
        &install_script,
        on_container_created,
        move |line| {
            tracing::info!("[Install] {}", line);
            
            // Check for OAuth URLs and open them in browser (only once per URL)
            if line.contains("https://") {
                tracing::info!("[Install] Found line with URL: {}", line);
                for word in line.split_whitespace() {
                    if word.starts_with("https://") {
                        tracing::info!("[Install] Checking URL: {}", word);
                        if word.contains("oauth") || word.contains("auth") || 
                           word.contains("login") || word.contains("verify") ||
                           word.contains("device") {
                            let url = word.trim_matches(|c| c == '"' || c == '\'' || c == '<' || c == '>').to_string();
                            
                            // Only open if we haven't opened this URL before
                            let mut opened = opened_urls_clone.lock().unwrap();
                            if !opened.contains(&url) {
                                tracing::info!("[Install] Opening OAuth URL in browser: {}", url);
                                // Use platform-specific command to open URL
                                #[cfg(target_os = "windows")]
                                let result = std::process::Command::new("cmd")
                                    .args(["/c", "start", "", &url])
                                    .spawn();
                                #[cfg(target_os = "macos")]
                                let result = std::process::Command::new("open")
                                    .arg(&url)
                                    .spawn();
                                #[cfg(target_os = "linux")]
                                let result = std::process::Command::new("xdg-open")
                                    .arg(&url)
                                    .spawn();
                                
                                match result {
                                    Ok(_) => tracing::info!("[Install] Browser opened successfully"),
                                    Err(e) => tracing::error!("[Install] Failed to open browser: {}", e),
                                }
                                opened.insert(url);
                            } else {
                                tracing::info!("[Install] URL already opened, skipping: {}", url);
                            }
                        }
                    }
                }
            }
            
            let _ = app_clone.emit("server-log", LogEvent {
                server_id: server_id_clone.clone(),
                line,
            });
        },
    ).await.map_err(|e| e.to_string())?;
    
    // Clean up install container
    docker.remove_install_container(&install_container_id).await.ok();
    
    // Reload and update server status
    let mut server = load_server_config(server_id)?;
    
    if exit_code == 0 {
        server.installed = true;
        server.status = ServerStatus::Stopped;
        server.install_container_id = None;
        save_server_config(&server)?;
        
        let _ = app.emit("server-log", LogEvent {
            server_id: server_id.to_string(),
            line: "[Serverwave] Installation completed successfully!".to_string(),
        });
        
        Ok(server)
    } else {
        server.status = ServerStatus::Error;
        server.install_container_id = None;
        save_server_config(&server)?;
        
        let _ = app.emit("server-log", LogEvent {
            server_id: server_id.to_string(),
            line: format!("[Serverwave] Installation failed with exit code: {}", exit_code),
        });
        
        Err(format!("Install script failed with exit code: {}", exit_code))
    }
}

/// Run install script (can be called manually)
#[tauri::command(rename_all = "camelCase")]
pub async fn run_install_script(
    server_id: String,
    app: AppHandle,
    state: State<'_, ServerState>,
    games_state: State<'_, GamesState>,
) -> Result<ServerResponse, String> {
    let server = run_install_script_internal(&server_id, &app, &state, &games_state).await?;
    Ok(ServerResponse {
        success: true,
        server: Some(server),
        error: None,
    })
}

/// Reinstall server - delete all data and run install again
#[tauri::command(rename_all = "camelCase")]
pub async fn reinstall_server(
    server_id: String,
    app: AppHandle,
    state: State<'_, ServerState>,
    games_state: State<'_, GamesState>,
) -> Result<ServerResponse, String> {
    tracing::info!("Reinstalling server: {}", server_id);
    
    // Stop log streaming
    {
        let mut streams = state.streams.lock().await;
        if let Some(handle) = streams.remove(&server_id) {
            let _ = handle.cancel_tx.send(true);
        }
    }
    
    let docker = DockerManager::new().await.map_err(|e| e.to_string())?;
    let mut server = load_server_config(&server_id)?;
    
    // Stop container
    if let Some(container_id) = &server.container_id {
        docker.stop_container(container_id).await.ok();
    }
    
    // Delete all data in server folder
    if server.data_path.exists() {
        let _ = app.emit("server-log", LogEvent {
            server_id: server_id.clone(),
            line: "[Serverwave] Deleting server data...".to_string(),
        });
        
        for entry in std::fs::read_dir(&server.data_path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.is_dir() {
                std::fs::remove_dir_all(&path).map_err(|e| e.to_string())?;
            } else {
                std::fs::remove_file(&path).map_err(|e| e.to_string())?;
            }
        }
    }
    
    // Reset installed flag
    server.installed = false;
    server.status = ServerStatus::Stopped;
    save_server_config(&server)?;
    
    let _ = app.emit("server-log", LogEvent {
        server_id: server_id.clone(),
        line: "[Serverwave] Server data cleared. Starting reinstallation...".to_string(),
    });
    
    // Run install script
    let server = run_install_script_internal(&server_id, &app, &state, &games_state).await?;
    Ok(ServerResponse {
        success: true,
        server: Some(server),
        error: None,
    })
}

/// Update server - run install script without deleting data (overwrites)
#[tauri::command(rename_all = "camelCase")]
pub async fn update_server_game(
    server_id: String,
    app: AppHandle,
    state: State<'_, ServerState>,
    games_state: State<'_, GamesState>,
) -> Result<ServerResponse, String> {
    tracing::info!("Updating server: {}", server_id);
    
    // Stop log streaming
    {
        let mut streams = state.streams.lock().await;
        if let Some(handle) = streams.remove(&server_id) {
            let _ = handle.cancel_tx.send(true);
        }
    }
    
    let docker = DockerManager::new().await.map_err(|e| e.to_string())?;
    let server = load_server_config(&server_id)?;
    
    // Stop container
    if let Some(container_id) = &server.container_id {
        docker.stop_container(container_id).await.ok();
    }
    
    let _ = app.emit("server-log", LogEvent {
        server_id: server_id.clone(),
        line: "[Serverwave] Starting update (running install script)...".to_string(),
    });
    
    // Run install script (will overwrite existing files)
    let server = run_install_script_internal(&server_id, &app, &state, &games_state).await?;
    Ok(ServerResponse {
        success: true,
        server: Some(server),
        error: None,
    })
}

/// Check if server needs installation
#[tauri::command(rename_all = "camelCase")]
pub async fn check_needs_install(
    server_id: String,
    games_state: State<'_, GamesState>,
) -> Result<bool, String> {
    let server = load_server_config(&server_id)?;
    
    if server.installed {
        return Ok(false);
    }
    
    let games_manager = games_state.manager.lock().await;
    if let Some(game_config) = games_manager.get_game(&server.game_type) {
        if let Some(script) = &game_config.install_script {
            return Ok(!script.is_empty());
        }
    }
    
    Ok(false)
}

fn calculate_dir_size(path: &PathBuf) -> Result<u64, std::io::Error> {
    let mut total = 0;
    
    if path.is_file() {
        return Ok(std::fs::metadata(path)?.len());
    }
    
    for entry in std::fs::read_dir(path)? {
        let path = entry?.path();
        if path.is_file() {
            total += std::fs::metadata(&path)?.len();
        } else if path.is_dir() {
            total += calculate_dir_size(&path).unwrap_or(0);
        }
    }
    
    Ok(total)
}

fn get_servers_dir() -> PathBuf {
    directories::UserDirs::new()
        .map(|d| d.home_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ServerWaveAnywhere")
        .join("servers")
}

fn get_servers_config_dir() -> PathBuf {
    directories::UserDirs::new()
        .map(|d| d.home_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ServerWaveAnywhere")
        .join("config")
}

fn get_server_config_path(server_id: &str) -> PathBuf {
    get_servers_config_dir().join(format!("{}.json", server_id))
}

fn save_server_config(server: &Server) -> Result<(), String> {
    let config_dir = get_servers_config_dir();
    std::fs::create_dir_all(&config_dir).map_err(|e| e.to_string())?;
    let config_path = get_server_config_path(&server.id);
    let content = serde_json::to_string_pretty(server).map_err(|e| e.to_string())?;
    std::fs::write(config_path, content).map_err(|e| e.to_string())
}

fn load_server_config(server_id: &str) -> Result<Server, String> {
    let config_path = get_server_config_path(server_id);
    let content = std::fs::read_to_string(config_path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}
