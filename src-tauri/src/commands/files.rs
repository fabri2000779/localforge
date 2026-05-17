//! File-manager Tauri commands. Thin wrappers delegating to the active
//! [`NodeBackend`] so file ops on local and remote nodes share one path.

use crate::backend::{NodeRegistry, DynBackend};
use localforge_core::{DirectoryContents, FileEntry};
use tauri::State;

async fn backend(state: &State<'_, NodeRegistry>) -> Result<DynBackend, String> {
    state
        .local()
        .await
        .ok_or_else(|| "Local backend not connected".to_string())
}

#[tauri::command]
pub async fn list_directory(
    path: String,
    state: State<'_, NodeRegistry>,
) -> Result<DirectoryContents, String> {
    backend(&state)
        .await?
        .list_files(&path)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn read_file_text(
    path: String,
    state: State<'_, NodeRegistry>,
) -> Result<String, String> {
    backend(&state)
        .await?
        .read_file_text(&path)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn write_file_text(
    path: String,
    content: String,
    state: State<'_, NodeRegistry>,
) -> Result<(), String> {
    backend(&state)
        .await?
        .write_file_text(&path, &content)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_file(
    path: String,
    state: State<'_, NodeRegistry>,
) -> Result<(), String> {
    backend(&state)
        .await?
        .create_file(&path)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_directory(
    path: String,
    state: State<'_, NodeRegistry>,
) -> Result<(), String> {
    backend(&state)
        .await?
        .create_directory(&path)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_path(
    path: String,
    state: State<'_, NodeRegistry>,
) -> Result<(), String> {
    backend(&state)
        .await?
        .delete_path(&path)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn rename_path(
    from: String,
    to: String,
    state: State<'_, NodeRegistry>,
) -> Result<(), String> {
    backend(&state)
        .await?
        .rename_path(&from, &to)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn move_path(
    from: String,
    to: String,
    state: State<'_, NodeRegistry>,
) -> Result<(), String> {
    backend(&state)
        .await?
        .move_path(&from, &to)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn copy_path(
    from: String,
    to: String,
    state: State<'_, NodeRegistry>,
) -> Result<(), String> {
    backend(&state)
        .await?
        .copy_path(&from, &to)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_file_info(
    path: String,
    state: State<'_, NodeRegistry>,
) -> Result<FileEntry, String> {
    backend(&state)
        .await?
        .file_info(&path)
        .await
        .map_err(|e| e.to_string())
}
