//! File-manager commands routed through the active [`NodeBackend`].

use crate::backend::NodeRegistry;
use crate::commands::require_backend;
use bytes::Bytes;
use futures_util::stream::StreamExt;
use localforge_core::{BackendError, DirectoryContents};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::io::AsyncWriteExt;

/// Progress event for chunked transfers; `id` correlates with the UI's transfer dialog.
#[derive(Debug, Clone, Serialize)]
struct TransferProgress {
    id: String,
    direction: &'static str, // "upload" | "download"
    path: String,
    bytes: u64,
}

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

/// OS save dialog; the destination is chosen in Rust so a hostile frontend can't write anywhere.
/// `None` on cancel.
async fn prompt_save_path(app: &AppHandle, suggested_name: &str) -> Option<std::path::PathBuf> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel::<Option<std::path::PathBuf>>();
    app.dialog()
        .file()
        .set_file_name(suggested_name)
        .save_file(move |path| {
            let _ = tx.send(path.and_then(|p| p.into_path().ok()));
        });
    rx.await.ok().flatten()
}

/// OS open dialog; the upload source is chosen in Rust so a hostile frontend can't exfiltrate files.
async fn prompt_open_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel::<Option<std::path::PathBuf>>();
    app.dialog().file().pick_file(move |path| {
        let _ = tx.send(path.and_then(|p| p.into_path().ok()));
    });
    rx.await.ok().flatten()
}

/// Stream a node file to a desktop path chosen via the save dialog; emits progress every ~256 KB.
/// Returns 0 on cancel.
#[tauri::command(rename_all = "camelCase")]
pub async fn download_file_to_local(
    transfer_id: String,
    src_path: String,
    suggested_name: String,
    node_id: Option<String>,
    app: AppHandle,
    state: State<'_, NodeRegistry>,
) -> Result<u64, String> {
    let backend = require_backend(&state, node_id.as_deref()).await?;

    let Some(dest_pathbuf) = prompt_save_path(&app, &suggested_name).await else {
        return Ok(0);
    };
    let dest_path = dest_pathbuf.to_string_lossy().to_string();

    let mut stream = backend
        .download_file(&src_path)
        .await
        .map_err(|e| e.to_string())?;

    if let Some(parent) = dest_pathbuf.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }
    let mut file = tokio::fs::File::create(&dest_path)
        .await
        .map_err(|e| e.to_string())?;

    let mut bytes_total: u64 = 0;
    let mut since_last_emit: u64 = 0;
    const EMIT_EVERY: u64 = 256 * 1024;

    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| e.to_string())?;
        file.write_all(&bytes).await.map_err(|e| e.to_string())?;
        bytes_total += bytes.len() as u64;
        since_last_emit += bytes.len() as u64;
        if since_last_emit >= EMIT_EVERY {
            let _ = app.emit(
                "file-transfer-progress",
                TransferProgress {
                    id: transfer_id.clone(),
                    direction: "download",
                    path: dest_path.clone(),
                    bytes: bytes_total,
                },
            );
            since_last_emit = 0;
        }
    }
    file.flush().await.map_err(|e| e.to_string())?;

    let _ = app.emit(
        "file-transfer-progress",
        TransferProgress {
            id: transfer_id,
            direction: "download",
            path: dest_path,
            bytes: bytes_total,
        },
    );
    Ok(bytes_total)
}

/// Stream a desktop file (picked via the open dialog) into `dest_dir` on the node, asking before
/// overwriting an existing file. Returns 0 on cancel.
#[tauri::command(rename_all = "camelCase")]
pub async fn upload_file_from_local(
    transfer_id: String,
    dest_dir: String,
    node_id: Option<String>,
    app: AppHandle,
    state: State<'_, NodeRegistry>,
) -> Result<u64, String> {
    let backend = require_backend(&state, node_id.as_deref()).await?;

    let Some(src_pathbuf) = prompt_open_path(&app).await else {
        return Ok(0);
    };
    let src_path = src_pathbuf.to_string_lossy().to_string();
    let file_name = src_pathbuf
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "upload.bin".to_string());
    let dest_path = format!("{}/{}", dest_dir.trim_end_matches(['/', '\\']), file_name);

    // Ask before clobbering an existing file.
    if backend.file_info(&dest_path).await.is_ok() {
        use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
        let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
        app.dialog()
            .message(format!("\"{file_name}\" already exists here. Overwrite it?"))
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Overwrite".into(),
                "Cancel".into(),
            ))
            .show(move |ok| {
                let _ = tx.send(ok);
            });
        if !rx.await.unwrap_or(false) {
            return Ok(0);
        }
    }

    let file = tokio::fs::File::open(&src_path)
        .await
        .map_err(|e| e.to_string())?;
    let total_size = file
        .metadata()
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    // Tap each chunk to emit progress as it flows to the backend.
    let app_for_progress = app.clone();
    let transfer_id_for_progress = transfer_id.clone();
    let dest_for_progress = dest_path.clone();
    let bytes_seen = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let since_last = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let bytes_seen_clone = bytes_seen.clone();
    let since_last_clone = since_last.clone();
    const EMIT_EVERY: u64 = 256 * 1024;

    let raw_stream = tokio_util::io::ReaderStream::new(file);
    let metered = raw_stream.map(move |item| match item {
        Ok(chunk) => {
            let bytes: Bytes = chunk;
            let n = bytes.len() as u64;
            let total = bytes_seen_clone.fetch_add(n, std::sync::atomic::Ordering::Relaxed) + n;
            let last =
                since_last_clone.fetch_add(n, std::sync::atomic::Ordering::Relaxed) + n;
            if last >= EMIT_EVERY {
                since_last_clone.store(0, std::sync::atomic::Ordering::Relaxed);
                let _ = app_for_progress.emit(
                    "file-transfer-progress",
                    TransferProgress {
                        id: transfer_id_for_progress.clone(),
                        direction: "upload",
                        path: dest_for_progress.clone(),
                        bytes: total,
                    },
                );
            }
            Ok(bytes)
        }
        Err(e) => Err(BackendError::io(e)),
    });

    backend
        .upload_file(&dest_path, Box::pin(metered))
        .await
        .map_err(|e| e.to_string())?;

    let final_total = bytes_seen.load(std::sync::atomic::Ordering::Relaxed);
    let _ = app.emit(
        "file-transfer-progress",
        TransferProgress {
            id: transfer_id,
            direction: "upload",
            path: dest_path,
            bytes: final_total.max(total_size),
        },
    );
    Ok(final_total)
}
