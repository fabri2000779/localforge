//! Server lifecycle commands over the active [`NodeBackend`]; installs stream through
//! `run_install` and the desktop opens any `OauthUrl` in the local browser.

use crate::backend::NodeRegistry;
use crate::commands::games::GamesState;
use crate::commands::require_backend;
use crate::paths;
use bollard::exec::{CreateExecOptions, StartExecResults};
use futures_util::stream::StreamExt;
use localforge_backend_local::DockerManager;
use localforge_core::types::InstallEvent;
use localforge_core::NodeId;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

pub use localforge_core::{
    CreateServerRequest, LogEvent, LogsResponse, Server, ServerResponse, ServerStatus,
};

/// Per-server log-pump tasks (`server-log` events); aborting the handle ends the stream.
#[derive(Default)]
pub struct ServerState {
    pub streams: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
}

/// Whether `node_id` addresses this machine: "local"/empty or this machine's global device id.
async fn node_is_local(state: &NodeRegistry, node_id: Option<&str>) -> bool {
    match node_id {
        None => true,
        Some(s) if s == NodeId::LOCAL || s.is_empty() => true,
        Some(s) => state
            .this_machine()
            .await
            .is_some_and(|m| m.id.as_str() == s),
    }
}

// CRUD + lifecycle

#[tauri::command(rename_all = "camelCase")]
pub async fn create_server(
    request: CreateServerRequest,
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
    games_state: State<'_, GamesState>,
) -> Result<ServerResponse, String> {
    let backend = require_backend(&state, node_id.as_deref()).await?;

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
    node_id: Option<String>,
    app: AppHandle,
    state: State<'_, NodeRegistry>,
    server_state: State<'_, ServerState>,
    games_state: State<'_, GamesState>,
) -> Result<ServerResponse, String> {
    let backend = require_backend(&state, node_id.as_deref()).await?;

    let server = backend
        .get_server(&server_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("server '{}' not found", server_id))?;

    // Auto-run the install first (same stream shape for local and remote nodes).
    if !server.installed {
        let needs_install = {
            let games_manager = games_state.manager.lock().await;
            games_manager
                .get_game(&server.game_type)
                .and_then(|g| g.install_script.clone())
                .map(|s| !s.is_empty())
                .unwrap_or(false)
        };
        if needs_install {
            run_install_pipeline(&server_id, node_id.as_deref(), &app, &state, &games_state)
                .await?;
        }
    }

    // Settings saved while the server was running (or written by a fresh install) apply now.
    if let Ok(Some(current)) = backend.get_server(&server_id).await {
        apply_server_config(&backend, &current, &games_state).await;
    }

    let status = backend
        .start_server(&server_id)
        .await
        .map_err(|e| e.to_string())?;

    if status == ServerStatus::Running {
        start_log_stream(&server_id, backend.clone(), app, &server_state).await;
    }

    let server = backend
        .get_server(&server_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("server '{}' not found", server_id))?;
    Ok(ServerResponse {
        success: true,
        server: Some(server),
        error: None,
    })
}

#[tauri::command(rename_all = "camelCase")]
pub async fn stop_server(
    server_id: String,
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
    server_state: State<'_, ServerState>,
    games_state: State<'_, GamesState>,
) -> Result<ServerResponse, String> {
    abort_stream(&server_id, &server_state).await;

    let backend = require_backend(&state, node_id.as_deref()).await?;

    // Persist Stopping before the graceful stop window so the crash-watcher doesn't treat the
    // exit as a crash. Local only; a remote agent does this itself.
    if node_is_local(&state, node_id.as_deref()).await {
        let root = paths::home_root();
        if let Ok(mut server) =
            localforge_backend_local::persistence::load_server(&root, &server_id)
        {
            server.status = ServerStatus::Stopping;
            let _ = localforge_backend_local::persistence::save_server(&root, &server);
        }
    }

    // Graceful stop: send the game's stop command and give it a few seconds.
    {
        let games_manager = games_state.manager.lock().await;
        if let Ok(Some(server)) = backend.get_server(&server_id).await {
            if let Some(game) = games_manager.get_game(&server.game_type) {
                if !game.stop_command.is_empty() {
                    let _ = backend.send_command(&server_id, &game.stop_command).await;
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            }
        }
    }

    backend
        .stop_server(&server_id)
        .await
        .map_err(|e| e.to_string())?;

    let server = backend
        .get_server(&server_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("server '{}' not found", server_id))?;
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
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
    server_state: State<'_, ServerState>,
) -> Result<ServerResponse, String> {
    abort_stream(&server_id, &server_state).await;

    let backend = require_backend(&state, node_id.as_deref()).await?;

    // Both paths go through the node's backend; `delete_server_keep_data` refuses on backends
    // that can't honour it instead of degrading to a wipe.
    if delete_data.unwrap_or(true) {
        backend
            .delete_server(&server_id)
            .await
            .map_err(|e| e.to_string())?;
    } else {
        backend
            .delete_server_keep_data(&server_id)
            .await
            .map_err(|e| e.to_string())?;
    }

    Ok(ServerResponse {
        success: true,
        server: None,
        error: None,
    })
}

#[tauri::command(rename_all = "camelCase")]
pub async fn list_servers(
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<Vec<Server>, String> {
    let backend = require_backend(&state, node_id.as_deref()).await?;
    let mut servers = backend.list_servers().await.map_err(|e| e.to_string())?;
    for server in servers.iter_mut() {
        if server.status == ServerStatus::Installing {
            continue;
        }
        if let Ok(status) = backend.server_status(&server.id).await {
            // Keep a persisted Crashed verdict: the live probe maps an exited container to Stopped.
            if server.status == ServerStatus::Crashed
                && matches!(status, ServerStatus::Stopped | ServerStatus::Error)
            {
                continue;
            }
            server.status = status;
        }
    }
    servers.sort_by_key(|s| std::cmp::Reverse(s.created_at));
    Ok(servers)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn send_command(
    server_id: String,
    command: String,
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<String, String> {
    let is_local = node_is_local(&state, node_id.as_deref()).await;
    let backend = require_backend(&state, node_id.as_deref()).await?;

    if backend.send_command(&server_id, &command).await.is_ok() {
        return Ok("Command sent".to_string());
    }

    if !is_local {
        return Err("Remote node refused the command".to_string());
    }
    let server = paths::load_server(&server_id).map_err(|e| e.to_string())?;
    let container_id = server.container_id.ok_or("No container ID")?;
    let docker = DockerManager::new().await.map_err(|e| e.to_string())?;
    send_via_mc_console(&docker, &container_id, &command).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_server_logs(
    server_id: String,
    lines: Option<u32>,
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<LogsResponse, String> {
    let backend = require_backend(&state, node_id.as_deref()).await?;
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
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<localforge_core::ContainerStats, String> {
    require_backend(&state, node_id.as_deref())
        .await?
        .get_stats(&server_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_server_disk_usage(
    server_id: String,
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<u64, String> {
    require_backend(&state, node_id.as_deref())
        .await?
        .get_disk_usage(&server_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn update_server_config(
    server_id: String,
    config: HashMap<String, String>,
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
    games_state: State<'_, GamesState>,
) -> Result<ServerResponse, String> {
    let backend = require_backend(&state, node_id.as_deref()).await?;
    let mut server = backend
        .update_server_config(&server_id, config)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(applied) = apply_server_config(&backend, &server, &games_state).await {
        server = applied;
    }
    Ok(ServerResponse {
        success: true,
        server: Some(server),
        error: None,
    })
}

/// Push the saved config into the game's files and container (best-effort: an agent built before
/// this route existed answers 404, so the server keeps running with what it had and we log the miss).
async fn apply_server_config(
    backend: &crate::backend::DynBackend,
    server: &localforge_core::Server,
    games_state: &GamesState,
) -> Option<localforge_core::Server> {
    let game = {
        let games_manager = games_state.manager.lock().await;
        games_manager.get_game(&server.game_type)
    }?;
    match backend.apply_server_config(&server.id, game).await {
        Ok(applied) => Some(applied),
        Err(e) => {
            tracing::warn!("apply_server_config({}): {}", server.id, e);
            None
        }
    }
}

// Log streaming

#[tauri::command(rename_all = "camelCase")]
pub async fn attach_server(
    server_id: String,
    node_id: Option<String>,
    app: AppHandle,
    state: State<'_, NodeRegistry>,
    server_state: State<'_, ServerState>,
) -> Result<(), String> {
    let backend = require_backend(&state, node_id.as_deref()).await?;
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
    abort_stream(server_id, state).await;

    let sid_owned = server_id.to_string();
    let stream_handle: JoinHandle<()> = tokio::spawn(async move {
        match backend.stream_logs(&sid_owned).await {
            Ok(mut stream) => {
                while let Some(item) = stream.next().await {
                    match item {
                        Ok(log) => {
                            let _ = app.emit(
                                "server-log",
                                LogEvent {
                                    server_id: log.server_id,
                                    line: log.line,
                                },
                            );
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

// Install pipeline (node-agnostic)

#[tauri::command(rename_all = "camelCase")]
pub async fn reinstall_server(
    server_id: String,
    node_id: Option<String>,
    app: AppHandle,
    state: State<'_, NodeRegistry>,
    server_state: State<'_, ServerState>,
    games_state: State<'_, GamesState>,
) -> Result<ServerResponse, String> {
    abort_stream(&server_id, &server_state).await;

    let backend = require_backend(&state, node_id.as_deref()).await?;

    let _ = app.emit(
        "server-log",
        LogEvent {
            server_id: server_id.clone(),
            line: "[LocalForge] Resetting server data...".to_string(),
        },
    );

    backend
        .reset_server_data(&server_id)
        .await
        .map_err(|e| e.to_string())?;

    let _ = app.emit(
        "server-log",
        LogEvent {
            server_id: server_id.clone(),
            line: "[LocalForge] Server data cleared. Starting reinstallation...".to_string(),
        },
    );

    let server =
        run_install_pipeline(&server_id, node_id.as_deref(), &app, &state, &games_state).await?;
    Ok(ServerResponse {
        success: true,
        server: Some(server),
        error: None,
    })
}

#[tauri::command(rename_all = "camelCase")]
pub async fn update_server_game(
    server_id: String,
    node_id: Option<String>,
    app: AppHandle,
    state: State<'_, NodeRegistry>,
    server_state: State<'_, ServerState>,
    games_state: State<'_, GamesState>,
) -> Result<ServerResponse, String> {
    abort_stream(&server_id, &server_state).await;
    let backend = require_backend(&state, node_id.as_deref()).await?;
    let _ = backend.stop_server(&server_id).await;

    let _ = app.emit(
        "server-log",
        LogEvent {
            server_id: server_id.clone(),
            line: "[LocalForge] Starting update (running install script)...".to_string(),
        },
    );

    let server =
        run_install_pipeline(&server_id, node_id.as_deref(), &app, &state, &games_state).await?;
    Ok(ServerResponse {
        success: true,
        server: Some(server),
        error: None,
    })
}

#[tauri::command(rename_all = "camelCase")]
pub async fn check_needs_install(
    server_id: String,
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
    games_state: State<'_, GamesState>,
) -> Result<bool, String> {
    let backend = require_backend(&state, node_id.as_deref()).await?;
    let server = backend
        .get_server(&server_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("server '{}' not found", server_id))?;
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

/// Drive the backend's `run_install` stream into UI events; OAuth URLs open in the local browser.
async fn run_install_pipeline(
    server_id: &str,
    node_id: Option<&str>,
    app: &AppHandle,
    state: &State<'_, NodeRegistry>,
    games_state: &State<'_, GamesState>,
) -> Result<Server, String> {
    let backend = require_backend(state, node_id).await?;

    let server = backend
        .get_server(server_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("server '{}' not found", server_id))?;

    let game = {
        let games_manager = games_state.manager.lock().await;
        games_manager
            .get_game(&server.game_type)
            .ok_or_else(|| format!("Game '{}' not found in catalogue", server.game_type))?
    };

    let _ = app.emit(
        "server-log",
        LogEvent {
            server_id: server_id.to_string(),
            line: "[LocalForge] Starting installation...".to_string(),
        },
    );

    let mut stream = backend
        .run_install(server_id, game)
        .await
        .map_err(|e| e.to_string())?;

    let mut opened_urls: HashSet<String> = HashSet::new();
    let mut final_exit_code: Option<i64> = None;

    while let Some(item) = stream.next().await {
        match item {
            Ok(InstallEvent::Log { line }) => {
                let _ = app.emit(
                    "server-log",
                    LogEvent {
                        server_id: server_id.to_string(),
                        line,
                    },
                );
            }
            Ok(InstallEvent::OauthUrl { url }) => {
                if opened_urls.insert(url.clone()) {
                    open_url_in_browser(&url);
                    let _ = app.emit(
                        "server-log",
                        LogEvent {
                            server_id: server_id.to_string(),
                            line: format!("[LocalForge] Opened auth URL in your browser: {}", url),
                        },
                    );
                    let _ = app.emit(
                        "install-oauth-opened",
                        serde_json::json!({ "url": url, "server_id": server_id }),
                    );
                }
            }
            Ok(InstallEvent::Done { exit_code }) => {
                final_exit_code = Some(exit_code);
                break;
            }
            Err(e) => return Err(e.to_string()),
        }
    }

    match final_exit_code {
        Some(0) => {
            let _ = app.emit(
                "server-log",
                LogEvent {
                    server_id: server_id.to_string(),
                    line: "[LocalForge] Installation completed successfully!".to_string(),
                },
            );
        }
        Some(code) => {
            let _ = app.emit(
                "server-log",
                LogEvent {
                    server_id: server_id.to_string(),
                    line: format!("[LocalForge] Installation failed with exit code: {}", code),
                },
            );
            return Err(format!("Install script failed with exit code: {}", code));
        }
        None => return Err("Install stream ended without a Done frame".to_string()),
    }

    backend
        .get_server(server_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("server '{}' not found after install", server_id))
}

/// Log-derived URLs are untrusted container output: require https and reject shell-hostile chars.
fn is_safe_external_url(url: &str) -> bool {
    url.starts_with("https://")
        && url.len() <= 2048
        && !url
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '"' || c == '\'' || c == '`')
}

fn open_url_in_browser(url: &str) {
    if !is_safe_external_url(url) {
        tracing::warn!("refusing to open non-https/unsafe URL from logs");
        return;
    }
    // Windows: rundll32's FileProtocolHandler avoids a cmd.exe parse step (no metacharacter injection).
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
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
