//! Persistent S3 backup-target list for relay-triggered backups.
//!
//! The desktop pushes the org's full named list over **direct HTTPS**
//! (`PUT /backup-targets`) so the agent can execute relay-triggered backups
//! (e.g. from the mobile app) even when the owner's desktop is offline.
//! Stored as a JSON array in `<data_root>/backup-targets.json` with 0600 perms.

use localforge_core::types::{BackupTarget, OrgBackupTarget};
use std::path::{Path, PathBuf};

fn path(data_root: &Path) -> PathBuf {
    data_root.join("backup-targets.json")
}

/// All provisioned targets, or empty when none have been pushed yet.
pub fn load(data_root: &Path) -> Vec<OrgBackupTarget> {
    match std::fs::read_to_string(path(data_root)) {
        Ok(s) => {
            // Try Vec (new format); fall back to single BackupTarget (old format migration).
            if let Ok(v) = serde_json::from_str::<Vec<OrgBackupTarget>>(&s) {
                return v;
            }
            if let Ok(t) = serde_json::from_str::<BackupTarget>(&s) {
                return vec![OrgBackupTarget {
                    id: "local-default".into(),
                    name: "Default".into(),
                    credentials: t,
                }];
            }
            Vec::new()
        }
        Err(_) => Vec::new(),
    }
}

/// Replace the stored list atomically (temp-file + rename so a crash can't
/// corrupt the file).
pub fn save(data_root: &Path, targets: &[OrgBackupTarget]) -> std::io::Result<()> {
    let p = path(data_root);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(targets)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let mut tmp = p.clone().into_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, &json)?;
    restrict_perms(&tmp);
    std::fs::rename(&tmp, &p)
}

/// Find by id; when `id` is None returns the first entry's credentials.
pub fn find(data_root: &Path, id: Option<&str>) -> Option<BackupTarget> {
    let list = load(data_root);
    match id {
        Some(id) => list.into_iter().find(|t| t.id == id).map(|t| t.credentials),
        None => list.into_iter().next().map(|t| t.credentials),
    }
}

#[cfg(unix)]
fn restrict_perms(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}
#[cfg(not(unix))]
fn restrict_perms(_path: &Path) {}
