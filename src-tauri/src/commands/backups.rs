//! Backup commands: org S3 targets (keychain, synced E2E) and backup operations on any node.

use crate::backend::NodeRegistry;
use crate::commands::require_backend;
use localforge_core::types::{BackupEntry, BackupTarget, OrgBackupTarget, OrgBackupTargetView};
use tauri::State;

/// Push the full target list to every linked agent over its direct HTTPS channel (never the relay).
async fn provision_all_nodes(state: &State<'_, NodeRegistry>) {
    let targets = crate::backups::load_targets();
    for rec in state.list_records().await {
        // The local backend resolves credentials from the keychain itself.
        if rec.id.is_local() {
            continue;
        }
        let Some(backend) = state.backend(&rec.id).await else {
            continue;
        };
        if let Err(e) = backend.set_backup_targets(&targets).await {
            tracing::warn!("provision backup targets to node {}: {}", rec.id, e);
        }
    }
}

fn require_target(id: Option<&str>) -> Result<BackupTarget, String> {
    crate::backups::find_target(id)
        .map(|(_, t)| t)
        .ok_or_else(|| "No backup storage configured. Add one in the Backups tab first.".into())
}

/// All configured backup targets (no secret keys in the response).
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_list_backup_targets() -> Result<Vec<OrgBackupTargetView>, String> {
    Ok(crate::backups::load_targets().iter().map(|t| t.view()).collect())
}

/// Add (or update) a named backup target locally + cloud + provision agents.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_add_backup_target(
    target: OrgBackupTarget,
    state: State<'_, NodeRegistry>,
) -> Result<OrgBackupTargetView, String> {
    let view = target.view();
    crate::backups::upsert_target(target.clone())?;
    provision_all_nodes(&state).await;
    if let Err(e) = crate::cloud::sync::push_backup_target(&target).await {
        tracing::info!("backup target {} not synced to cloud: {}", target.id, e);
    }
    Ok(view)
}

/// Remove a backup target by id locally + cloud.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_remove_backup_target(
    id: String,
    state: State<'_, NodeRegistry>,
) -> Result<(), String> {
    crate::backups::remove_target(&id)?;
    provision_all_nodes(&state).await;
    if let Err(e) = crate::cloud::sync::delete_backup_target_remote(&id).await {
        tracing::info!("backup target {} not removed from cloud: {}", id, e);
    }
    Ok(())
}

/// Pull the org's targets from the cloud, cache them and provision agents; falls back to the local list.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_pull_backup_targets(
    state: State<'_, NodeRegistry>,
) -> Result<Vec<OrgBackupTargetView>, String> {
    match crate::cloud::sync::pull_backup_targets().await {
        Ok(targets) if !targets.is_empty() => {
            for t in &targets {
                crate::backups::upsert_target(t.clone())?;
            }
            provision_all_nodes(&state).await;
            Ok(targets.iter().map(|t| t.view()).collect())
        }
        Ok(_) => Ok(crate::backups::load_targets().iter().map(|t| t.view()).collect()),
        Err(e) => {
            tracing::info!("backup targets pull skipped: {}", e);
            Ok(crate::backups::load_targets().iter().map(|t| t.view()).collect())
        }
    }
}

/// Archive and upload the server's data dir; `target_id` defaults to the first target.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_backup_now(
    server_id: String,
    node_id: Option<String>,
    target_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<String, String> {
    let backend = require_backend(&state, node_id.as_deref()).await?;
    let target = require_target(target_id.as_deref())?;
    backend.create_backup(&server_id, &target).await.map_err(|e| e.to_string())
}

/// List the server's backups in the chosen target, newest first.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_list_backups(
    server_id: String,
    node_id: Option<String>,
    target_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<Vec<BackupEntry>, String> {
    let backend = require_backend(&state, node_id.as_deref()).await?;
    let target = require_target(target_id.as_deref())?;
    backend.list_backups(&server_id, &target).await.map_err(|e| e.to_string())
}

/// Restore a backup over the server's data dir.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_restore_backup(
    server_id: String,
    key: String,
    node_id: Option<String>,
    target_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<(), String> {
    let backend = require_backend(&state, node_id.as_deref()).await?;
    let target = require_target(target_id.as_deref())?;
    backend.restore_backup(&server_id, &target, &key).await.map_err(|e| e.to_string())
}

/// Delete a backup object from the bucket.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_delete_backup(
    server_id: String,
    key: String,
    node_id: Option<String>,
    target_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<(), String> {
    let backend = require_backend(&state, node_id.as_deref()).await?;
    let target = require_target(target_id.as_deref())?;
    backend.delete_backup(&server_id, &target, &key).await.map_err(|e| e.to_string())
}
