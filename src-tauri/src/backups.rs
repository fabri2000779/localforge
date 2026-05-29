//! Local S3 backup-target storage (named list per org).
//!
//! The list of `OrgBackupTarget` values (id, name + S3 credentials) lives in
//! the OS keychain. The secret key is never returned to the frontend — only the
//! redacted view is surfaced. On first load after the v0.1.47→v0.1.48 upgrade
//! the old single-target format is auto-migrated to a one-element list.

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
            // Try Vec<OrgBackupTarget> first (new format).
            if let Ok(v) = serde_json::from_str::<Vec<OrgBackupTarget>>(&json) {
                return v;
            }
            // Old single-target format (BackupTarget without id/name):
            // promote to a one-element list so nothing breaks.
            if let Ok(t) = serde_json::from_str::<BackupTarget>(&json) {
                return vec![OrgBackupTarget {
                    id: "local-default".into(),
                    name: "Default".into(),
                    credentials: t,
                }];
            }
            Vec::new()
        }
        Err(keyring_core::Error::NoEntry) => Vec::new(),
        Err(err) => {
            tracing::warn!("[backups] keychain read failed: {err}");
            Vec::new()
        }
    }
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

/// Clear ALL targets.
pub fn clear_targets() -> Result<(), String> {
    let e = entry()?;
    match e.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring_core::Error::NoEntry) => Ok(()),
        Err(err) => Err(err.to_string()),
    }
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

// ── Legacy aliases kept for call sites that haven't been updated ───────────

/// Save a single target (replaces the old single-target API; wraps as
/// id="local-default", name="Default"). Existing multi-target list is
/// preserved; the default slot is updated.
pub fn save_target(target: &BackupTarget) -> Result<(), String> {
    upsert_target(OrgBackupTarget {
        id: "local-default".into(),
        name: "Default".into(),
        credentials: target.clone(),
    })
}

/// Return the first target's credentials (old single-target callers).
pub fn load_target() -> Option<BackupTarget> {
    find_target(None).map(|(_, t)| t)
}

/// Drop all targets (old single-target callers).
pub fn clear_target() -> Result<(), String> {
    clear_targets()
}
