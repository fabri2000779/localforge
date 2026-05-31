//! HTTP router and handlers.
//!
//! Every endpoint just delegates to the `NodeBackend` trait, which means
//! the agent is essentially a thin protocol shim — bug fixes in the
//! local Docker logic land in `localforge-backend-local` once and serve
//! both the desktop and the agent.

use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::{middleware, Json, Router};
use futures_util::{SinkExt, StreamExt, TryStreamExt};
use localforge_core::types::{
    BackupEntry, BackupTarget, ContainerStats, CreateServerRequest, DirectoryContents, DockerInfo,
    FileEntry, GameConfig, InstallEvent, MetricPoint, NodeStats, OrgBackupTarget, Player,
    PlayerAction, Schedule, Server, ServerStatus,
};
use localforge_core::NodeBackend;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub backend: Arc<dyn NodeBackend>,
    pub token: String,
    /// Path to agent.toml — `POST /link` persists the cloud link here.
    pub config_path: std::path::PathBuf,
    /// Data root — where the provisioned S3 backup target is stored so the
    /// agent can run relay-triggered backups without the secret on the wire.
    pub data_root: std::path::PathBuf,
    /// Set once the relay client is running, so `POST /link` doesn't spawn a
    /// second connection loop.
    pub relay_started: Arc<std::sync::atomic::AtomicBool>,
}

pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        // health + info
        .route("/info", get(get_info))
        .route("/node/stats", get(get_node_stats))
        // cloud-relay enrollment (desktop auto-provision)
        .route("/link", post(link_node))
        // server CRUD + lifecycle
        .route("/servers", get(list_servers).post(create_server))
        .route("/servers/{id}", get(get_server).delete(delete_server))
        .route("/servers/{id}/config", patch(update_server_config))
        .route("/servers/{id}/start", post(start_server))
        .route("/servers/{id}/stop", post(stop_server))
        .route("/servers/{id}/status", get(server_status))
        .route("/servers/{id}/stats", get(server_stats))
        .route("/servers/{id}/disk", get(server_disk))
        .route("/servers/{id}/logs", get(get_logs))
        .route("/servers/{id}/command", post(send_command))
        .route("/servers/{id}/stream", get(stream_logs))
        .route("/servers/{id}/install/stream", get(install_stream))
        .route("/servers/{id}/reset-data", post(reset_server_data))
        // backups (BYO S3) — the target (incl. secret) travels in the body over
        // this already-TLS'd channel. POST for list/delete too, since they
        // carry the target body.
        .route("/servers/{id}/backup", post(backup_now))
        .route("/servers/{id}/backups/list", post(backups_list))
        .route("/servers/{id}/restore", post(restore_backup))
        .route("/servers/{id}/backups/delete", post(delete_backup))
        // S3 target provisioning: the desktop pushes the full named list here
        // over direct HTTPS so the agent can run relay-triggered backups itself.
        // The PUT replaces the stored list atomically. 64 KiB is generous for
        // an org's entire target list; explicit limit avoids relying on axum's
        // implicit 2 MiB default.
        .route("/backup-targets", put(set_backup_targets))
        .layer(axum::extract::DefaultBodyLimit::max(64 * 1024))
        // scheduled actions
        .route("/servers/{id}/schedules", get(list_schedules).post(upsert_schedule))
        .route("/schedules/{sid}", delete(delete_schedule))
        // metrics history (local on this host)
        .route("/servers/{id}/metrics", get(server_metrics))
        // player administration (live roster + moderation)
        .route("/servers/{id}/players", get(server_players))
        .route("/servers/{id}/players/action", post(player_action))
        // file ops on the agent host
        .route("/fs", get(fs_list).delete(fs_delete))
        .route("/fs/read", post(fs_read))
        .route("/fs/write", post(fs_write))
        .route("/fs/create-file", post(fs_create_file))
        .route("/fs/create-dir", post(fs_create_dir))
        .route("/fs/rename", post(fs_rename))
        .route("/fs/move", post(fs_move))
        .route("/fs/copy", post(fs_copy))
        .route("/fs/info", get(fs_info))
        .route("/fs/download", get(fs_download))
        .route("/fs/upload", put(fs_upload))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::auth::require_bearer,
        ));

    Router::new()
        // /v1/health is public — used by the desktop to test reachability
        // before sending the token.
        .route("/v1/health", get(health))
        .nest("/v1", protected)
        .with_state(state)
}

// ===========================================================================
// Health
// ===========================================================================

#[derive(Serialize)]
struct Health {
    name: &'static str,
    version: &'static str,
}

async fn health() -> Json<Health> {
    Json(Health {
        name: "localforge-agent",
        version: env!("CARGO_PKG_VERSION"),
    })
}

// ===========================================================================
// Helpers
// ===========================================================================

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        #[derive(Serialize)]
        struct Body {
            error: String,
        }
        (self.status, Json(Body { error: self.message })).into_response()
    }
}

fn map_err(e: localforge_core::BackendError) -> ApiError {
    use localforge_core::BackendError;
    let status = match e {
        BackendError::NotFound(_) => StatusCode::NOT_FOUND,
        BackendError::InvalidInput(_) => StatusCode::BAD_REQUEST,
        BackendError::Unauthorized => StatusCode::UNAUTHORIZED,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    ApiError {
        status,
        message: e.to_string(),
    }
}

// ===========================================================================
// Info + server endpoints
// ===========================================================================

async fn get_info(State(s): State<AppState>) -> Result<Json<DockerInfo>, ApiError> {
    s.backend.docker_info().await.map(Json).map_err(map_err)
}

async fn get_node_stats(State(s): State<AppState>) -> Result<Json<NodeStats>, ApiError> {
    s.backend.node_stats().await.map(Json).map_err(map_err)
}

#[derive(Deserialize)]
struct LinkBody {
    /// One-time enrollment blob (base64url) issued by the cloud's
    /// `POST /v1/nodes`. The desktop pushes it here over the existing HTTPS
    /// session so the operator doesn't have to paste anything on the VPS.
    blob: String,
}

async fn link_node(
    State(s): State<AppState>,
    Json(b): Json<LinkBody>,
) -> Result<StatusCode, ApiError> {
    use base64::Engine;
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(b.blob.trim())
        .map_err(|e| ApiError { status: StatusCode::BAD_REQUEST, message: format!("invalid blob: {e}") })?;
    let link: crate::config::CloudLink = serde_json::from_slice(&raw)
        .map_err(|e| ApiError { status: StatusCode::BAD_REQUEST, message: format!("malformed blob: {e}") })?;
    crate::config::save_cloud_link(&s.config_path, link.clone())
        .map_err(|e| ApiError { status: StatusCode::INTERNAL_SERVER_ERROR, message: e.to_string() })?;
    // Connect now without a restart. swap() returns the prior value: only the
    // first link spawns a loop. (A re-link with a new token applies on next
    // restart — fine, re-linking is rare.)
    if !s.relay_started.swap(true, std::sync::atomic::Ordering::SeqCst) {
        crate::relay::spawn(s.backend.clone(), link, s.data_root.clone());
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn list_servers(State(s): State<AppState>) -> Result<Json<Vec<Server>>, ApiError> {
    s.backend.list_servers().await.map(Json).map_err(map_err)
}

async fn get_server(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Option<Server>>, ApiError> {
    s.backend.get_server(&id).await.map(Json).map_err(map_err)
}

#[derive(Deserialize)]
struct CreateBody {
    request: CreateServerRequest,
    game: GameConfig,
}

async fn create_server(
    State(s): State<AppState>,
    Json(body): Json<CreateBody>,
) -> Result<Json<Server>, ApiError> {
    s.backend
        .create_server(body.request, body.game)
        .await
        .map(Json)
        .map_err(map_err)
}

#[derive(Deserialize)]
struct UpdateConfigBody {
    config: HashMap<String, String>,
}

async fn update_server_config(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateConfigBody>,
) -> Result<Json<Server>, ApiError> {
    s.backend
        .update_server_config(&id, body.config)
        .await
        .map(Json)
        .map_err(map_err)
}

async fn delete_server(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    s.backend.delete_server(&id).await.map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn reset_server_data(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    s.backend.reset_server_data(&id).await.map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn start_server(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ServerStatus>, ApiError> {
    s.backend.start_server(&id).await.map(Json).map_err(map_err)
}

async fn stop_server(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ServerStatus>, ApiError> {
    s.backend.stop_server(&id).await.map(Json).map_err(map_err)
}

async fn server_status(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ServerStatus>, ApiError> {
    s.backend
        .server_status(&id)
        .await
        .map(Json)
        .map_err(map_err)
}

async fn server_stats(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ContainerStats>, ApiError> {
    s.backend.get_stats(&id).await.map(Json).map_err(map_err)
}

#[derive(Serialize)]
struct DiskResponse {
    bytes: u64,
}

async fn server_disk(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DiskResponse>, ApiError> {
    let bytes = s.backend.get_disk_usage(&id).await.map_err(map_err)?;
    Ok(Json(DiskResponse { bytes }))
}

#[derive(Deserialize)]
struct LogsQuery {
    lines: Option<usize>,
}

#[derive(Serialize)]
struct LogsResponse {
    logs: Vec<String>,
}

async fn get_logs(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<LogsQuery>,
) -> Result<Json<LogsResponse>, ApiError> {
    let logs = s
        .backend
        .get_logs(&id, q.lines.unwrap_or(500))
        .await
        .map_err(map_err)?;
    Ok(Json(LogsResponse { logs }))
}

#[derive(Deserialize)]
struct CommandBody {
    command: String,
}

async fn send_command(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CommandBody>,
) -> Result<StatusCode, ApiError> {
    s.backend
        .send_command(&id, &body.command)
        .await
        .map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

// ----- backups -------------------------------------------------------------

#[derive(Serialize)]
struct KeyResponse {
    key: String,
}

async fn backup_now(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(target): Json<BackupTarget>,
) -> Result<Json<KeyResponse>, ApiError> {
    let key = s.backend.create_backup(&id, &target).await.map_err(map_err)?;
    Ok(Json(KeyResponse { key }))
}

async fn backups_list(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(target): Json<BackupTarget>,
) -> Result<Json<Vec<BackupEntry>>, ApiError> {
    s.backend
        .list_backups(&id, &target)
        .await
        .map(Json)
        .map_err(map_err)
}

#[derive(Deserialize)]
struct RestoreBody {
    target: BackupTarget,
    key: String,
}

async fn restore_backup(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<RestoreBody>,
) -> Result<StatusCode, ApiError> {
    s.backend
        .restore_backup(&id, &body.target, &body.key)
        .await
        .map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct DeleteBackupBody {
    target: BackupTarget,
    key: String,
}

async fn delete_backup(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<DeleteBackupBody>,
) -> Result<StatusCode, ApiError> {
    s.backend
        .delete_backup(&id, &body.target, &body.key)
        .await
        .map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

// ----- scheduled actions ---------------------------------------------------

async fn list_schedules(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<Schedule>>, ApiError> {
    s.backend
        .list_schedules(&id)
        .await
        .map(Json)
        .map_err(map_err)
}

async fn upsert_schedule(
    State(s): State<AppState>,
    Path(_id): Path<String>,
    Json(schedule): Json<Schedule>,
) -> Result<StatusCode, ApiError> {
    s.backend.upsert_schedule(schedule).await.map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_schedule(
    State(s): State<AppState>,
    Path(sid): Path<String>,
) -> Result<StatusCode, ApiError> {
    s.backend.delete_schedule(&sid).await.map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

// ----- metrics history -----------------------------------------------------

#[derive(Deserialize)]
struct MetricsQuery {
    since: Option<i64>,
}

async fn server_metrics(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<MetricsQuery>,
) -> Result<Json<Vec<MetricPoint>>, ApiError> {
    s.backend
        .query_metrics(&id, q.since.unwrap_or(0))
        .await
        .map(Json)
        .map_err(map_err)
}

// ----- player administration -----------------------------------------------

async fn server_players(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<Player>>, ApiError> {
    s.backend.list_players(&id).await.map(Json).map_err(map_err)
}

async fn player_action(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(action): Json<PlayerAction>,
) -> Result<StatusCode, ApiError> {
    s.backend
        .player_action(&id, action)
        .await
        .map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

// ----- backup target provisioning (desktop → agent, direct HTTPS) ----------

fn io_error(e: std::io::Error) -> ApiError {
    ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: e.to_string(),
    }
}

async fn set_backup_targets(
    State(s): State<AppState>,
    Json(targets): Json<Vec<OrgBackupTarget>>,
) -> Result<StatusCode, ApiError> {
    crate::backup_target::save(&s.data_root, &targets).map_err(io_error)?;
    Ok(StatusCode::NO_CONTENT)
}

// ===========================================================================
// Log streaming via WebSocket
// ===========================================================================

async fn stream_logs(
    State(s): State<AppState>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| handle_log_socket(socket, s, id))
}

async fn handle_log_socket(mut socket: WebSocket, state: AppState, server_id: String) {
    let mut stream = match state.backend.stream_logs(&server_id).await {
        Ok(s) => s,
        Err(e) => {
            let _ = socket
                .send(Message::Text(
                    serde_json::json!({ "error": e.to_string() }).to_string().into(),
                ))
                .await;
            return;
        }
    };

    while let Some(item) = stream.next().await {
        let msg = match item {
            Ok(line) => serde_json::json!({
                "server_id": line.server_id,
                "line": line.line,
            }),
            Err(e) => serde_json::json!({ "error": e.to_string() }),
        };
        if socket
            .send(Message::Text(msg.to_string().into()))
            .await
            .is_err()
        {
            break;
        }
    }
}

// ===========================================================================
// Install streaming via WebSocket
// ===========================================================================

async fn install_stream(
    State(s): State<AppState>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| handle_install_socket(socket, s, id))
}

#[derive(Deserialize)]
struct InstallInit {
    game: GameConfig,
}

async fn handle_install_socket(mut socket: WebSocket, state: AppState, server_id: String) {
    // Wait for the first text frame carrying the game config.
    let init: InstallInit = loop {
        match socket.recv().await {
            Some(Ok(Message::Text(text))) => match serde_json::from_str::<InstallInit>(&text) {
                Ok(init) => break init,
                Err(e) => {
                    let _ = socket
                        .send(Message::Text(
                            serde_json::json!({ "kind": "error", "message": format!("bad init frame: {}", e) })
                                .to_string()
                                .into(),
                        ))
                        .await;
                    return;
                }
            },
            Some(Ok(Message::Close(_))) | None => return,
            _ => continue,
        }
    };

    let mut stream = match state.backend.run_install(&server_id, init.game).await {
        Ok(s) => s,
        Err(e) => {
            let _ = socket
                .send(Message::Text(
                    serde_json::to_string(&serde_json::json!({
                        "kind": "error",
                        "message": e.to_string()
                    }))
                    .unwrap_or_default()
                    .into(),
                ))
                .await;
            return;
        }
    };

    while let Some(item) = stream.next().await {
        let (payload, is_done) = match item {
            Ok(ev) => {
                let done = matches!(ev, InstallEvent::Done { .. });
                let payload = serde_json::to_string(&ev).unwrap_or_else(|e| {
                    serde_json::json!({ "kind": "error", "message": e.to_string() }).to_string()
                });
                (payload, done)
            }
            Err(e) => (
                serde_json::json!({
                    "kind": "error",
                    "message": e.to_string()
                })
                .to_string(),
                true,
            ),
        };
        if socket.send(Message::Text(payload.into())).await.is_err() {
            break;
        }
        if is_done {
            break;
        }
    }
    let _ = socket.close().await;
}

// ===========================================================================
// File system endpoints
// ===========================================================================

#[derive(Deserialize)]
struct PathQuery {
    path: String,
}

async fn fs_list(
    State(s): State<AppState>,
    Query(q): Query<PathQuery>,
) -> Result<Json<DirectoryContents>, ApiError> {
    s.backend.list_files(&q.path).await.map(Json).map_err(map_err)
}

async fn fs_info(
    State(s): State<AppState>,
    Query(q): Query<PathQuery>,
) -> Result<Json<FileEntry>, ApiError> {
    s.backend.file_info(&q.path).await.map(Json).map_err(map_err)
}

async fn fs_download(
    State(s): State<AppState>,
    Query(q): Query<PathQuery>,
) -> Result<Response, ApiError> {
    let stream = s
        .backend
        .download_file(&q.path)
        .await
        .map_err(map_err)?
        .map_err(|e| std::io::Error::other(e.to_string()));
    let filename = std::path::Path::new(&q.path)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "download.bin".to_string());
    let body = Body::from_stream(stream);
    Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename),
        )
        .body(body)
        .map_err(|e| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: e.to_string(),
        })
}

async fn fs_upload(
    State(s): State<AppState>,
    Query(q): Query<PathQuery>,
    body: Body,
) -> Result<StatusCode, ApiError> {
    let stream = body
        .into_data_stream()
        .map_err(|e| localforge_core::BackendError::Transport(e.to_string()))
        .boxed();
    s.backend.upload_file(&q.path, stream).await.map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn fs_delete(
    State(s): State<AppState>,
    Query(q): Query<PathQuery>,
) -> Result<StatusCode, ApiError> {
    s.backend.delete_path(&q.path).await.map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct ReadBody {
    path: String,
}

#[derive(Serialize)]
struct ReadResponse {
    content: String,
}

async fn fs_read(
    State(s): State<AppState>,
    Json(b): Json<ReadBody>,
) -> Result<Json<ReadResponse>, ApiError> {
    let content = s.backend.read_file_text(&b.path).await.map_err(map_err)?;
    Ok(Json(ReadResponse { content }))
}

#[derive(Deserialize)]
struct WriteBody {
    path: String,
    content: String,
}

async fn fs_write(
    State(s): State<AppState>,
    Json(b): Json<WriteBody>,
) -> Result<StatusCode, ApiError> {
    s.backend
        .write_file_text(&b.path, &b.content)
        .await
        .map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct PathOnlyBody {
    path: String,
}

async fn fs_create_file(
    State(s): State<AppState>,
    Json(b): Json<PathOnlyBody>,
) -> Result<StatusCode, ApiError> {
    s.backend.create_file(&b.path).await.map_err(map_err)?;
    Ok(StatusCode::CREATED)
}

async fn fs_create_dir(
    State(s): State<AppState>,
    Json(b): Json<PathOnlyBody>,
) -> Result<StatusCode, ApiError> {
    s.backend.create_directory(&b.path).await.map_err(map_err)?;
    Ok(StatusCode::CREATED)
}

#[derive(Deserialize)]
struct FromToBody {
    from: String,
    to: String,
}

async fn fs_rename(
    State(s): State<AppState>,
    Json(b): Json<FromToBody>,
) -> Result<StatusCode, ApiError> {
    s.backend.rename_path(&b.from, &b.to).await.map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn fs_move(
    State(s): State<AppState>,
    Json(b): Json<FromToBody>,
) -> Result<StatusCode, ApiError> {
    s.backend.move_path(&b.from, &b.to).await.map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn fs_copy(
    State(s): State<AppState>,
    Json(b): Json<FromToBody>,
) -> Result<StatusCode, ApiError> {
    s.backend.copy_path(&b.from, &b.to).await.map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}
