//! Server lifecycle Tauri commands.
//!
//! Most commands are thin wrappers around the active [`NodeBackend`]
//! pulled out of [`BackendState`]. The legacy install-script logic still
//! lives here because it needs to open OAuth URLs in the user's local
//! browser — that's a desktop-only concern and migrates to the agent
//! protocol in a later phase.

use crate::backend::BackendState;
use crate::commands::games::GamesState;
use crate::docker::DockerManager;
use crate::persistence;
use bollard::exec::{CreateExecOptions, StartExecResults};
use futures_util::stream::StreamExt;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

pub use localforge_core::{
    CreateServerRequest, LogEvent, LogsResponse, Server, ServerResponse, ServerStatus,
};

/// Per-server background tasks that pump log lines from the backend's
/// stream into Tauri `server-log` events. Dropping the handle (via
/// `abort` on detach) ends the stream cleanly.
#[derive(Default)]
pub struct ServerState {
    pub streams: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
}

async fn require_backend(
    state: &State<'_, BackendState>,
) -> Result<Arc<dyn localforge_core::NodeBackend>, String> {
    state
        .local()
        .await
        .ok_or_else(|| "Local backend not connected".to_string())
}

// ===========================================================================
// CRUD + lifecycle
// ===========================================================================

#[tauri::command]
pub async fn create_server(
    request: CreateServerRequest,
    state: State<'_, BackendState>,
    games_state: State<'_, GamesState>,
) -> Result<ServerResponse, String> {
    let backend = require_backend(&state).await?;

    let game = {
        let games_manager = games_state.manager.lock().await;
        games_manager
            .get_game(&request.game_type)
            .ok_or_else(|| format!("Game type '{}' not found", request.game_type))?
    };

    let server = backend
        .create_server(request, game)
        .await
        .map_err(|e| e.to_string())?;

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
    state: State<'_, BackendState>,
    server_state: State<'_, ServerState>,
    games_state: State<'_, GamesState>,
) -> Result<ServerResponse, String> {
    // First-launch install: if the game has an install script and the
    // server hasn't run it yet, run it before starting.
    let needs_install = {
        let mut server = persistence::load_server(&server_id).map_err(|e| e.to_string())?;
        if server.installed {
            false
        } else {
            let games_manager = games_state.manager.lock().await;
            let has_install = games_manager
                .get_game(&server.game_type)
                .and_then(|g| g.install_script.clone())
                .map(|s| !s.is_empty())
                .unwrap_or(false);
            if !has_install {
                server.installed = true;
                persistence::save_server(&server).map_err(|e| e.to_string())?;
            }
            has_install
        }
    };

    if needs_install {
        run_install_script_internal(&server_id, &app, &games_state).await?;
    }

    let backend = require_backend(&state).await?;
    let status = backend
        .start_server(&server_id)
        .await
        .map_err(|e| e.to_string())?;

    // Start streaming logs into Tauri events.
    if status == ServerStatus::Running {
        start_log_stream(&server_id, backend.clone(), app, &server_state).await;
    }

    let server = persistence::load_server(&server_id).map_err(|e| e.to_string())?;
    Ok(ServerResponse {
        success: true,
        server: Some(server),
        error: None,
    })
}

#[tauri::command(rename_all = "camelCase")]
pub async fn stop_server(
    server_id: String,
    state: State<'_, BackendState>,
    server_state: State<'_, ServerState>,
    games_state: State<'_, GamesState>,
) -> Result<ServerResponse, String> {
    // Cancel log streaming first so the disconnect log line doesn't race
    // the stop sequence.
    abort_stream(&server_id, &server_state).await;

    // If the game defines a graceful stop_command, send it and wait a few
    // seconds before forcing the container down.
    {
        let games_manager = games_state.manager.lock().await;
        if let Ok(server) = persistence::load_server(&server_id) {
            if let Some(game) = games_manager.get_game(&server.game_type) {
                if !game.stop_command.is_empty() {
                    if let Some(backend) = state.local().await {
                        let _ = backend.send_command(&server_id, &game.stop_command).await;
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    }
                }
            }
        }
    }

    let backend = require_backend(&state).await?;
    backend
        .stop_server(&server_id)
        .await
        .map_err(|e| e.to_string())?;

    let server = persistence::load_server(&server_id).map_err(|e| e.to_string())?;
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
    state: State<'_, BackendState>,
    server_state: State<'_, ServerState>,
) -> Result<ServerResponse, String> {
    abort_stream(&server_id, &server_state).await;

    let backend = require_backend(&state).await?;

    // The backend's delete_server always wipes the data dir; if the caller
    // wants to keep data, fall back to a stop-only path.
    if delete_data.unwrap_or(true) {
        backend
            .delete_server(&server_id)
            .await
            .map_err(|e| e.to_string())?;
    } else {
        let server = persistence::load_server(&server_id).map_err(|e| e.to_string())?;
        if let Some(container_id) = &server.container_id {
            let _ = backend.stop_server(&server_id).await;
            // Remove just the container, keep the data on disk.
            let docker = DockerManager::new().await.map_err(|e| e.to_string())?;
            let _ = docker.remove_container(container_id).await;
        }
        persistence::delete_server_record(&server_id).map_err(|e| e.to_string())?;
    }

    Ok(ServerResponse {
        success: true,
        server: None,
        error: None,
    })
}

#[tauri::command]
pub async fn list_servers(state: State<'_, BackendState>) -> Result<Vec<Server>, String> {
    let backend = require_backend(&state).await?;
    let mut servers = backend.list_servers().await.map_err(|e| e.to_string())?;
    // Refresh live status from Docker for non-installing servers.
    for server in servers.iter_mut() {
        if server.status == ServerStatus::Installing {
            continue;
        }
        if let Ok(status) = backend.server_status(&server.id).await {
            server.status = status;
        }
    }
    servers.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(servers)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_server_status(
    server_id: String,
    state: State<'_, BackendState>,
) -> Result<ServerStatus, String> {
    let server = persistence::load_server(&server_id).map_err(|e| e.to_string())?;
    if server.status == ServerStatus::Installing {
        return Ok(ServerStatus::Installing);
    }
    if server.container_id.is_none() {
        return Ok(ServerStatus::Stopped);
    }
    let backend = require_backend(&state).await?;
    backend
        .server_status(&server_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn send_command(
    server_id: String,
    command: String,
    state: State<'_, BackendState>,
) -> Result<String, String> {
    let backend = require_backend(&state).await?;
    if backend.send_command(&server_id, &command).await.is_ok() {
        return Ok("Command sent".to_string());
    }

    // Minecraft fallback: containers that disable stdin still let us push a
    // command through `mc-send-to-console` (parkervcp/yolks ships it).
    let server = persistence::load_server(&server_id).map_err(|e| e.to_string())?;
    let container_id = server.container_id.ok_or("No container ID")?;
    let docker = DockerManager::new().await.map_err(|e| e.to_string())?;
    send_via_mc_console(&docker, &container_id, &command).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_server_logs(
    server_id: String,
    lines: Option<u32>,
    state: State<'_, BackendState>,
) -> Result<LogsResponse, String> {
    let backend = require_backend(&state).await?;
    let n = lines.unwrap_or(500) as usize;
    let logs = backend
        .get_logs(&server_id, n)
        .await
        .map_err(|e| e.to_string())?;
    Ok(LogsResponse { logs, error: None })
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_server_stats(
    server_id: String,
    state: State<'_, BackendState>,
) -> Result<localforge_core::ContainerStats, String> {
    let backend = require_backend(&state).await?;
    backend
        .get_stats(&server_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_server_disk_usage(
    server_id: String,
    state: State<'_, BackendState>,
) -> Result<u64, String> {
    let backend = require_backend(&state).await?;
    backend
        .get_disk_usage(&server_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn update_server_config(
    server_id: String,
    config: HashMap<String, String>,
    state: State<'_, BackendState>,
) -> Result<ServerResponse, String> {
    let backend = require_backend(&state).await?;
    let server = backend
        .update_server_config(&server_id, config)
        .await
        .map_err(|e| e.to_string())?;
    Ok(ServerResponse {
        success: true,
        server: Some(server),
        error: None,
    })
}

// ===========================================================================
// Log streaming (Tauri-side fan-out of backend stream into events)
// ===========================================================================

#[tauri::command(rename_all = "camelCase")]
pub async fn attach_server(
    server_id: String,
    app: AppHandle,
    state: State<'_, BackendState>,
    server_state: State<'_, ServerState>,
) -> Result<(), String> {
    let backend = require_backend(&state).await?;
    start_log_stream(&server_id, backend, app, &server_state).await;
    Ok(())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn detach_server(
    server_id: String,
    server_state: State<'_, ServerState>,
) -> Result<(), String> {
    abort_stream(&server_id, &server_state).await;
    Ok(())
}

async fn start_log_stream(
    server_id: &str,
    backend: Arc<dyn localforge_core::NodeBackend>,
    app: AppHandle,
    state: &State<'_, ServerState>,
) {
    // Replace any pre-existing stream for this server.
    abort_stream(server_id, state).await;

    let sid_owned = server_id.to_string();
    let stream_handle: JoinHandle<()> = tokio::spawn(async move {
        match backend.stream_logs(&sid_owned).await {
            Ok(mut stream) => {
                while let Some(item) = stream.next().await {
                    match item {
                        Ok(log) => {
                            let _ = app.emit("server-log", LogEvent {
                                server_id: log.server_id,
                                line: log.line,
                            });
                        }
                        Err(e) => {
                            tracing::warn!("log stream error for {}: {}", sid_owned, e);
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("failed to attach log stream for {}: {}", sid_owned, e);
            }
        }
    });

    let mut streams = state.streams.lock().await;
    streams.insert(server_id.to_string(), stream_handle);
}

async fn abort_stream(server_id: &str, state: &State<'_, ServerState>) {
    let mut streams = state.streams.lock().await;
    if let Some(handle) = streams.remove(server_id) {
        handle.abort();
    }
}

// ===========================================================================
// Install / reinstall / update — still on DockerManager direct because
// the OAuth-URL auto-open behaviour needs to run on the desktop, not on
// the remote node. The agent will surface OAuth events through a
// different channel in a later phase.
// ===========================================================================

#[tauri::command(rename_all = "camelCase")]
pub async fn run_install_script(
    server_id: String,
    app: AppHandle,
    games_state: State<'_, GamesState>,
) -> Result<ServerResponse, String> {
    let server = run_install_script_internal(&server_id, &app, &games_state).await?;
    Ok(ServerResponse {
        success: true,
        server: Some(server),
        error: None,
    })
}

#[tauri::command(rename_all = "camelCase")]
pub async fn reinstall_server(
    server_id: String,
    app: AppHandle,
    state: State<'_, BackendState>,
    server_state: State<'_, ServerState>,
    games_state: State<'_, GamesState>,
) -> Result<ServerResponse, String> {
    abort_stream(&server_id, &server_state).await;

    let mut server = persistence::load_server(&server_id).map_err(|e| e.to_string())?;

    // Stop any running container first.
    if let Some(backend) = state.local().await {
        let _ = backend.stop_server(&server_id).await;
    }

    let _ = app.emit("server-log", LogEvent {
        server_id: server_id.clone(),
        line: "[LocalForge] Deleting server data...".to_string(),
    });

    if server.data_path.exists() {
        std::fs::remove_dir_all(&server.data_path).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&server.data_path).map_err(|e| e.to_string())?;
    }

    server.installed = false;
    server.install_container_id = None;
    persistence::save_server(&server).map_err(|e| e.to_string())?;

    let _ = app.emit("server-log", LogEvent {
        server_id: server_id.clone(),
        line: "[LocalForge] Server data cleared. Starting reinstallation...".to_string(),
    });

    let server = run_install_script_internal(&server_id, &app, &games_state).await?;
    Ok(ServerResponse {
        success: true,
        server: Some(server),
        error: None,
    })
}

#[tauri::command(rename_all = "camelCase")]
pub async fn update_server_game(
    server_id: String,
    app: AppHandle,
    state: State<'_, BackendState>,
    server_state: State<'_, ServerState>,
    games_state: State<'_, GamesState>,
) -> Result<ServerResponse, String> {
    abort_stream(&server_id, &server_state).await;
    if let Some(backend) = state.local().await {
        let _ = backend.stop_server(&server_id).await;
    }

    let _ = app.emit("server-log", LogEvent {
        server_id: server_id.clone(),
        line: "[LocalForge] Starting update (running install script)...".to_string(),
    });

    let server = run_install_script_internal(&server_id, &app, &games_state).await?;
    Ok(ServerResponse {
        success: true,
        server: Some(server),
        error: None,
    })
}

#[tauri::command(rename_all = "camelCase")]
pub async fn check_needs_install(
    server_id: String,
    games_state: State<'_, GamesState>,
) -> Result<bool, String> {
    let server = persistence::load_server(&server_id).map_err(|e| e.to_string())?;
    if server.installed {
        return Ok(false);
    }
    let games_manager = games_state.manager.lock().await;
    Ok(games_manager
        .get_game(&server.game_type)
        .and_then(|g| g.install_script.clone())
        .map(|s| !s.is_empty())
        .unwrap_or(false))
}

async fn run_install_script_internal(
    server_id: &str,
    app: &AppHandle,
    games_state: &State<'_, GamesState>,
) -> Result<Server, String> {
    tracing::info!("Running install script for server: {}", server_id);

    let docker = DockerManager::new().await.map_err(|e| e.to_string())?;
    let mut server = persistence::load_server(server_id).map_err(|e| e.to_string())?;

    let (install_script, install_image, volume_path) = {
        let games_manager = games_state.manager.lock().await;
        let game = games_manager
            .get_game(&server.game_type)
            .ok_or_else(|| format!("Game type '{}' not found", server.game_type))?;
        match game.install_script.clone() {
            Some(s) if !s.is_empty() => (
                s,
                game.install_image
                    .clone()
                    .unwrap_or_else(|| game.docker_image.clone()),
                game.volume_path.clone(),
            ),
            _ => {
                server.installed = true;
                persistence::save_server(&server).map_err(|e| e.to_string())?;
                return Ok(server);
            }
        }
    };

    server.status = ServerStatus::Installing;
    persistence::save_server(&server).map_err(|e| e.to_string())?;

    let _ = app.emit(
        "server-log",
        LogEvent {
            server_id: server_id.to_string(),
            line: "[LocalForge] Starting installation...".to_string(),
        },
    );

    let app_clone = app.clone();
    let server_id_clone = server_id.to_string();
    let opened_urls: Arc<std::sync::Mutex<std::collections::HashSet<String>>> =
        Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
    let opened_urls_clone = opened_urls.clone();

    let server_id_for_callback = server_id.to_string();
    let on_container_created = move |container_id: &str| {
        if let Ok(mut srv) = persistence::load_server(&server_id_for_callback) {
            srv.install_container_id = Some(container_id.to_string());
            let _ = persistence::save_server(&srv);
        }
    };

    let (exit_code, install_container_id) = docker
        .run_script(
            &install_image,
            &server.data_path,
            &volume_path,
            &install_script,
            on_container_created,
            move |line| {
                if line.contains("https://") {
                    for word in line.split_whitespace() {
                        if word.starts_with("https://")
                            && (word.contains("oauth")
                                || word.contains("auth")
                                || word.contains("login")
                                || word.contains("verify")
                                || word.contains("device"))
                        {
                            let url = word
                                .trim_matches(|c| c == '"' || c == '\'' || c == '<' || c == '>')
                                .to_string();
                            let mut opened = opened_urls_clone.lock().unwrap();
                            if !opened.contains(&url) {
                                #[cfg(target_os = "windows")]
                                let _ = std::process::Command::new("cmd")
                                    .args(["/c", "start", "", &url])
                                    .spawn();
                                #[cfg(target_os = "macos")]
                                let _ = std::process::Command::new("open")
                                    .arg(&url)
                                    .spawn();
                                #[cfg(target_os = "linux")]
                                let _ = std::process::Command::new("xdg-open")
                                    .arg(&url)
                                    .spawn();
                                opened.insert(url);
                            }
                        }
                    }
                }
                let _ = app_clone.emit(
                    "server-log",
                    LogEvent {
                        server_id: server_id_clone.clone(),
                        line,
                    },
                );
            },
        )
        .await
        .map_err(|e| e.to_string())?;

    docker
        .remove_install_container(&install_container_id)
        .await
        .ok();

    let mut server = persistence::load_server(server_id).map_err(|e| e.to_string())?;
    if exit_code == 0 {
        server.installed = true;
        server.status = ServerStatus::Stopped;
        server.install_container_id = None;
        persistence::save_server(&server).map_err(|e| e.to_string())?;
        let _ = app.emit(
            "server-log",
            LogEvent {
                server_id: server_id.to_string(),
                line: "[LocalForge] Installation completed successfully!".to_string(),
            },
        );
        Ok(server)
    } else {
        server.status = ServerStatus::Error;
        server.install_container_id = None;
        persistence::save_server(&server).map_err(|e| e.to_string())?;
        let _ = app.emit(
            "server-log",
            LogEvent {
                server_id: server_id.to_string(),
                line: format!("[LocalForge] Installation failed with exit code: {}", exit_code),
            },
        );
        Err(format!("Install script failed with exit code: {}", exit_code))
    }
}

async fn send_via_mc_console(
    docker: &DockerManager,
    container_id: &str,
    command: &str,
) -> Result<String, String> {
    let exec_options = CreateExecOptions {
        cmd: Some(vec!["mc-send-to-console".to_string(), command.to_string()]),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        ..Default::default()
    };

    let exec = docker
        .client()
        .create_exec(container_id, exec_options)
        .await
        .map_err(|e| e.to_string())?;

    match docker
        .client()
        .start_exec(&exec.id, None)
        .await
        .map_err(|e| e.to_string())?
    {
        StartExecResults::Attached { mut output, .. } => {
            let mut out = String::new();
            while let Some(Ok(msg)) = output.next().await {
                out.push_str(&msg.to_string());
            }
            Ok(out)
        }
        StartExecResults::Detached => Ok("Command sent (detached)".to_string()),
    }
}
