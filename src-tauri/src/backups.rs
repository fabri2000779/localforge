//! Org backup-target list stored in the OS keychain; only the redacted view reaches the frontend.

use localforge_core::types::{BackupTarget, OrgBackupTarget};

const SERVICE: &str = "LocalForge Cloud";
const ACCOUNT: &str = "backup-targets"; // was "backup-target" pre-0.1.48

fn entry() -> Result<keyring_core::Entry, String> {
    keyring_core::Entry::new(SERVICE, ACCOUNT).map_err(|e| e.to_string())
}

pub fn load_targets() -> Vec<OrgBackupTarget> {
    let e = match entry() {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    match e.get_password() {
        Ok(json) => {
            if let Ok(v) = serde_json::from_str::<Vec<OrgBackupTarget>>(&json) {
                return v;
            }
            Vec::new()
        }
        Err(keyring_core::Error::NoEntry) => {
            // One-time migration from the pre-0.1.48 single-target entry.
            migrate_from_legacy()
        }
        Err(err) => {
            tracing::warn!("[backups] keychain read failed: {err}");
            Vec::new()
        }
    }
}

const LEGACY_ACCOUNT: &str = "backup-target"; // pre-v0.1.48 single-target

fn migrate_from_legacy() -> Vec<OrgBackupTarget> {
    let legacy = match keyring_core::Entry::new(SERVICE, LEGACY_ACCOUNT) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let json = match legacy.get_password() {
        Ok(j) => j,
        Err(_) => return Vec::new(),
    };
    let target = match serde_json::from_str::<BackupTarget>(&json) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let promoted = vec![OrgBackupTarget {
        id: "local-default".into(),
        name: "Default".into(),
        credentials: target,
    }];
    if save_targets(&promoted).is_ok() {
        let _ = legacy.delete_credential();
        tracing::info!("[backups] migrated legacy single-target to multi-target list");
    }
    promoted
}

fn save_targets(targets: &[OrgBackupTarget]) -> Result<(), String> {
    let json = serde_json::to_string(targets).map_err(|e| format!("serialize: {e}"))?;
    entry()?.set_password(&json).map_err(|e| e.to_string())
}

/// Add or replace a target (matched by id).
pub fn upsert_target(target: OrgBackupTarget) -> Result<(), String> {
    let mut list = load_targets();
    if let Some(slot) = list.iter_mut().find(|t| t.id == target.id) {
        *slot = target;
    } else {
        list.push(target);
    }
    save_targets(&list)
}

/// Remove a target by id (idempotent).
pub fn remove_target(id: &str) -> Result<(), String> {
    let mut list = load_targets();
    list.retain(|t| t.id != id);
    save_targets(&list)
}

/// Find by id (or the first entry when id is None).
pub fn find_target(id: Option<&str>) -> Option<(String, BackupTarget)> {
    let list = load_targets();
    match id {
        Some(id) => list
            .into_iter()
            .find(|t| t.id == id)
            .map(|t| (t.id, t.credentials)),
        None => list.into_iter().next().map(|t| (t.id, t.credentials)),
    }
}
