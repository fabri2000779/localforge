//! Persistent S3 backup target for relay-triggered backups.
//!
//! The desktop owns the S3 credentials (OS keychain) and PROVISIONS them to
//! each linked agent over **direct HTTPS** (`PUT /backup-target`). That lets the
//! agent run backups by itself when the mobile app triggers them over the relay
//! — the secret never travels through Cloudflare/the relay, only over the
//! desktop→agent TLS channel. Stored as JSON in the agent's `data_root` with
//! 0600 perms (the same trust level as `agent.toml`, which already holds the
//! node token).

use localforge_core::types::BackupTarget;
use std::path::{Path, PathBuf};

fn target_path(data_root: &Path) -> PathBuf {
    data_root.join("backup-target.json")
}

/// The provisioned target, or `None` if the desktop hasn't pushed creds here.
pub fn load(data_root: &Path) -> Option<BackupTarget> {
    let s = std::fs::read_to_string(target_path(data_root)).ok()?;
    serde_json::from_str(&s).ok()
}

/// Store (or replace) the provisioned target.
pub fn save(data_root: &Path, target: &BackupTarget) -> std::io::Result<()> {
    let path = target_path(data_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(target)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&path, json)?;
    restrict_perms(&path);
    Ok(())
}

/// Forget the provisioned target (idempotent — missing file is success).
pub fn clear(data_root: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(target_path(data_root)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(unix)]
fn restrict_perms(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}
#[cfg(not(unix))]
fn restrict_perms(_path: &Path) {}
