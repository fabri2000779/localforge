//! HTTP router and handlers.
//!
//! Every endpoint just delegates to the `NodeBackend` trait, which means
//! the agent is essentially a thin protocol shim — bug fixes in the
//! local Docker logic land in `localforge-backend-local` once and serve
//! both the desktop and the agent.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{middleware, Json, Router};
use futures_util::StreamExt;
use localforge_core::types::{
    ContainerStats, CreateServerRequest, DirectoryContents, DockerInfo, FileEntry, GameConfig,
    Server, ServerStatus,
};
use localforge_core::NodeBackend;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub backend: Arc<dyn NodeBackend>,
    pub token: String,
}

pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        // health + info
        .route("/info", get(get_info))
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
