//! S3 backup-target storage.
//!
//! The user's bring-your-own bucket credentials (endpoint/region/bucket/key/
//! **secret**) live in the OS keychain alongside the JWT — never a plaintext
//! file or localStorage. One reusable destination per install; it's read here
//! and passed to whichever node's backend runs the backup (local Docker, or an
//! agent over HTTPS), so the secret only travels over an already-encrypted
//! channel and the agent doesn't need to persist it for manual backups.

use localforge_core::types::BackupTarget;

const SERVICE: &str = "LocalForge Cloud";
const ACCOUNT: &str = "backup-target";

fn entry() -> Result<keyring_core::Entry, String> {
    keyring_core::Entry::new(SERVICE, ACCOUNT).map_err(|e| e.to_string())
}

pub fn save_target(target: &BackupTarget) -> Result<(), String> {
    let json = serde_json::to_string(target).map_err(|e| format!("serialize: {e}"))?;
    entry()?.set_password(&json).map_err(|e| e.to_string())
}

pub fn load_target() -> Option<BackupTarget> {
    let e = entry().ok()?;
    match e.get_password() {
        Ok(json) => serde_json::from_str(&json).ok(),
        Err(keyring_core::Error::NoEntry) => None,
        Err(err) => {
            tracing::warn!("[backups] keychain read failed: {err}");
            None
        }
    }
}

pub fn clear_target() -> Result<(), String> {
    let e = entry()?;
    match e.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring_core::Error::NoEntry) => Ok(()),
        Err(err) => Err(err.to_string()),
    }
}
