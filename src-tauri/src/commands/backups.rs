//! Backup Tauri commands.
//!
//! The S3 destination is read from the OS keychain (see `crate::backups`) and
//! passed to the active node's backend — so the same commands back up a local
//! Docker server OR an agent-hosted one (the target travels to the agent over
//! its existing HTTPS channel). The secret key is never returned to the UI;
//! `get_backup_target` hands back a redacted view.

use crate::backend::NodeRegistry;
use crate::commands::require_backend;
use localforge_core::types::{BackupEntry, BackupTarget, BackupTargetView};
use tauri::State;

fn require_target() -> Result<BackupTarget, String> {
    crate::backups::load_target()
        .ok_or_else(|| "No backup destination configured. Add your S3 credentials first.".into())
}

/// Best-effort: push (or clear) the S3 target on every linked node so
/// relay-triggered backups (e.g. from the mobile app) work even when this
/// desktop is offline. The local backend's impl is a no-op; only remote agents
/// persist it, and the push travels over each agent's direct HTTPS channel —
/// never the relay/cloud, so the secret stays off Cloudflare.
async fn provision_nodes(state: &State<'_, NodeRegistry>, target: Option<&BackupTarget>) {
    for rec in state.list_records().await {
        let Some(backend) = state.backend(&rec.id).await else {
            continue;
        };
        let res = match target {
            Some(t) => backend.set_backup_target(t).await,
            None => backend.clear_backup_target().await,
        };
        if let Err(e) = res {
            tracing::warn!("provision backup target to node {}: {}", rec.id, e);
        }
    }
}

/// Save (or replace) the S3 backup destination, then provision it to agents.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_set_backup_target(
    target: BackupTarget,
    state: State<'_, NodeRegistry>,
) -> Result<(), String> {
    crate::backups::save_target(&target)?;
    provision_nodes(&state, Some(&target)).await;
    // Best-effort: sync to the cloud (E2E with the org DEK) so the owner's other
    // devices and the org's members share one backup storage. A free org gets
    // 402 from the sync gate — fine, it just stays local; don't fail on a hiccup.
    if let Err(e) = crate::cloud::sync::push_backup_target(&target).await {
        tracing::info!("backup target not synced to cloud: {}", e);
    }
    Ok(())
}

/// The configured destination, redacted (no secret key). `None` = not set up.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_get_backup_target() -> Result<Option<BackupTargetView>, String> {
    Ok(crate::backups::load_target().map(|t| t.view()))
}

/// Forget the configured destination, and clear it from agents too.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_clear_backup_target(state: State<'_, NodeRegistry>) -> Result<(), String> {
    crate::backups::clear_target()?;
    provision_nodes(&state, None).await;
    if let Err(e) = crate::cloud::sync::clear_backup_target_remote().await {
        tracing::info!("backup target not cleared from cloud: {}", e);
    }
    Ok(())
}

/// Pull the org's shared S3 target from the cloud (if any), cache it locally,
/// and provision agents — so a fresh device, or a member's host, acquires the
/// org backup storage without re-entering credentials. Best-effort; returns the
/// redacted view (the synced one if found, else whatever is already local).
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_pull_backup_target(
    state: State<'_, NodeRegistry>,
) -> Result<Option<BackupTargetView>, String> {
    match crate::cloud::sync::pull_backup_target().await {
        Ok(Some(target)) => {
            crate::backups::save_target(&target)?;
            provision_nodes(&state, Some(&target)).await;
            Ok(Some(target.view()))
        }
        Ok(None) => Ok(crate::backups::load_target().map(|t| t.view())),
        Err(e) => {
            tracing::info!("backup target pull skipped: {}", e);
            Ok(crate::backups::load_target().map(|t| t.view()))
        }
    }
}

/// Archive the server's data dir and upload it. Returns the object key.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_backup_now(
    server_id: String,
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<String, String> {
    let backend = require_backend(&state, node_id.as_deref()).await?;
    let target = require_target()?;
    backend
        .create_backup(&server_id, &target)
        .await
        .map_err(|e| e.to_string())
}

/// List the server's backups in the bucket, newest first.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_list_backups(
    server_id: String,
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<Vec<BackupEntry>, String> {
    let backend = require_backend(&state, node_id.as_deref()).await?;
    let target = require_target()?;
    backend
        .list_backups(&server_id, &target)
        .await
        .map_err(|e| e.to_string())
}

/// Restore a backup over the server's data dir (stops the server first).
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_restore_backup(
    server_id: String,
    key: String,
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<(), String> {
    let backend = require_backend(&state, node_id.as_deref()).await?;
    let target = require_target()?;
    backend
        .restore_backup(&server_id, &target, &key)
        .await
        .map_err(|e| e.to_string())
}

/// Delete a backup object from the bucket.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_delete_backup(
    key: String,
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<(), String> {
    let backend = require_backend(&state, node_id.as_deref()).await?;
    let target = require_target()?;
    backend
        .delete_backup(&target, &key)
        .await
        .map_err(|e| e.to_string())
}
