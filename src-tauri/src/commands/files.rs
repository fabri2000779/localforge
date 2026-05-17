//! File-manager Tauri commands. Thin wrappers delegating to the active
//! [`NodeBackend`] (picked by `nodeId`) so file ops on local and remote
//! nodes share one path.

use crate::backend::NodeRegistry;
use crate::commands::require_backend;
use localforge_core::{DirectoryContents, FileEntry};
use tauri::State;

#[tauri::command(rename_all = "camelCase")]
pub async fn list_directory(
    path: String,
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<DirectoryContents, String> {
    require_backend(&state, node_id.as_deref())
        .await?
        .list_files(&path)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn read_file_text(
    path: String,
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<String, String> {
    require_backend(&state, node_id.as_deref())
        .await?
        .read_file_text(&path)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn write_file_text(
    path: String,
    content: String,
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<(), String> {
    require_backend(&state, node_id.as_deref())
        .await?
        .write_file_text(&path, &content)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn create_file(
    path: String,
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<(), String> {
    require_backend(&state, node_id.as_deref())
        .await?
        .create_file(&path)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn create_directory(
    path: String,
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<(), String> {
    require_backend(&state, node_id.as_deref())
        .await?
        .create_directory(&path)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_path(
    path: String,
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<(), String> {
    require_backend(&state, node_id.as_deref())
        .await?
        .delete_path(&path)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn rename_path(
    from: String,
    to: String,
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<(), String> {
    require_backend(&state, node_id.as_deref())
        .await?
        .rename_path(&from, &to)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn move_path(
    from: String,
    to: String,
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<(), String> {
    require_backend(&state, node_id.as_deref())
        .await?
        .move_path(&from, &to)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn copy_path(
    from: String,
    to: String,
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<(), String> {
    require_backend(&state, node_id.as_deref())
        .await?
        .copy_path(&from, &to)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_file_info(
    path: String,
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<FileEntry, String> {
    require_backend(&state, node_id.as_deref())
        .await?
        .file_info(&path)
        .await
        .map_err(|e| e.to_string())
}
