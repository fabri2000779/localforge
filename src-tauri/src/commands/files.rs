//! File-manager Tauri commands. Thin wrappers delegating to the active
//! [`NodeBackend`] (picked by `nodeId`) so file ops on local and remote
//! nodes share one path.

use crate::backend::NodeRegistry;
use crate::commands::require_backend;
use bytes::Bytes;
use futures_util::stream::StreamExt;
use localforge_core::{BackendError, DirectoryContents, FileEntry};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::io::AsyncWriteExt;

/// Emitted periodically while a chunked file transfer is running so the
/// UI can show a progress bar. `id` is a caller-chosen handle so the UI
/// can correlate the events with a specific transfer dialog.
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

/// Prompt the user for a host path with the OS SAVE dialog and return their
/// choice. The destination is chosen in Rust, never taken as a free string from
/// the frontend — otherwise an XSS'd WebView could invoke a transfer command
/// with an arbitrary dest and write anywhere on the host (audit finding).
/// Returns `Ok(None)` if the user cancelled.
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

/// OS OPEN dialog counterpart — the source of an upload is picked in Rust so a
/// hostile caller can't exfiltrate an arbitrary host file (audit finding).
async fn prompt_open_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel::<Option<std::path::PathBuf>>();
    app.dialog().file().pick_file(move |path| {
        let _ = tx.send(path.and_then(|p| p.into_path().ok()));
    });
    rx.await.ok().flatten()
}

/// Stream a file from a node (local or remote) onto the user's desktop
/// filesystem. The host destination is chosen via the OS save dialog IN RUST
/// (not a frontend-supplied path — audit finding). Emits
/// `file-transfer-progress` events every ~256 KB so the UI can render a
/// progress bar. Returns 0 if the user cancelled the save dialog.
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
        return Ok(0); // user cancelled
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

/// Stream a file from the user's desktop filesystem into a node directory. The
/// host SOURCE is picked via the OS open dialog IN RUST (not a frontend string
/// — audit finding), and the node destination is `<dest_dir>/<picked file
/// name>`, still confined by the backend. If a file with that name already
/// exists on the node, the user is asked to confirm the overwrite (audit
/// finding). Returns 0 if the user cancelled the picker or the overwrite prompt.
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
        return Ok(0); // user cancelled
    };
    let src_path = src_pathbuf.to_string_lossy().to_string();
    let file_name = src_pathbuf
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "upload.bin".to_string());
    let dest_path = format!("{}/{}", dest_dir.trim_end_matches(['/', '\\']), file_name);

    // Overwrite guard: unlike delete/rename this used to clobber a same-named
    // file silently with no undo (audit finding). Ask first if it exists.
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
            return Ok(0); // user declined the overwrite
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

    // Wrap the file reader as a Bytes stream and tap each chunk to emit
    // progress events as data flows through to the backend.
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
