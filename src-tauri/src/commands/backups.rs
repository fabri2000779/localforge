//! Backup Tauri commands (multi-target).
//!
//! The org's S3 targets live as a named list (id + label + credentials) in the
//! OS keychain and are synced E2E with the cloud under the org DEK. Agents are
//! provisioned with the full list over direct HTTPS. The secret key is never
//! returned to the frontend; only redacted `OrgBackupTargetView`s are surfaced.

use crate::backend::NodeRegistry;
use crate::commands::require_backend;
use localforge_core::types::{BackupEntry, BackupTarget, OrgBackupTarget, OrgBackupTargetView};
use tauri::State;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Best-effort: push the full target list to every linked node so relay-triggered
/// backups work even when this desktop is offline. The secret travels only over
/// each agent's existing direct-HTTPS channel — never the relay/cloud.
async fn provision_all_nodes(state: &State<'_, NodeRegistry>) {
    let targets = crate::backups::load_targets();
    for rec in state.list_records().await {
        // The local backend resolves credentials from the keychain directly;
        // set_backup_targets is a no-op there, so skip it (saves an await + log).
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

// ---------------------------------------------------------------------------
// Target CRUD commands
// ---------------------------------------------------------------------------

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

/// Pull all org backup targets from the cloud (decrypt with org DEK), cache
/// locally, and provision agents. Returns the redacted list. Best-effort —
/// falls back to whatever is already local on 402 / no DEK / error.
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

// ── Legacy single-target commands kept for older relay paths ───────────────

/// @deprecated — prefer `cloud_add_backup_target` with an explicit id/name.
/// Kept so the desktop BackupsPanel's "Save credentials" still works.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_set_backup_target(
    target: OrgBackupTarget,
    state: State<'_, NodeRegistry>,
) -> Result<(), String> {
    crate::backups::upsert_target(target.clone())?;
    provision_all_nodes(&state).await;
    if let Err(e) = crate::cloud::sync::push_backup_target(&target).await {
        tracing::info!("backup target not synced to cloud: {}", e);
    }
    Ok(())
}

/// @deprecated — prefer `cloud_list_backup_targets`.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_get_backup_target() -> Result<Option<OrgBackupTargetView>, String> {
    Ok(crate::backups::load_targets().first().map(|t| t.view()))
}

/// @deprecated — prefer `cloud_remove_backup_target`.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_clear_backup_target(state: State<'_, NodeRegistry>) -> Result<(), String> {
    crate::backups::clear_targets()?;
    provision_all_nodes(&state).await;
    if let Err(e) = crate::cloud::sync::clear_backup_target_remote().await {
        tracing::info!("backup targets not cleared from cloud: {}", e);
    }
    Ok(())
}

/// @deprecated — kept for pre-multi-target callers.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_pull_backup_target(
    state: State<'_, NodeRegistry>,
) -> Result<Option<OrgBackupTargetView>, String> {
    let v = cloud_pull_backup_targets(state).await?;
    Ok(v.into_iter().next())
}

// ---------------------------------------------------------------------------
// Backup operation commands
// ---------------------------------------------------------------------------

/// Archive the server's data dir and upload it. Optional `target_id` picks
/// which org target to use; defaults to the first one in the list.
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
    key: String,
    node_id: Option<String>,
    target_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<(), String> {
    let backend = require_backend(&state, node_id.as_deref()).await?;
    let target = require_target(target_id.as_deref())?;
    backend.delete_backup(&target, &key).await.map_err(|e| e.to_string())
}
