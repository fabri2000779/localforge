//! JWT storage in the OS credential store (Credential Manager / Keychain / Secret Service).

const SERVICE: &str = "LocalForge Cloud";
const ACCOUNT: &str = "session-jwt";

/// Install the OS credential store as keyring's process default. Must run before any
/// `Entry` is created; a failure is logged loudly instead of silently using a no-op store.
pub fn init() {
    match install_native_store() {
        Ok(_) => tracing::info!("[keychain] OS credential store installed"),
        Err(e) => tracing::error!(
            "[keychain] FAILED to install the OS credential store: {e}. \
             Cloud login and sync-key storage will NOT persist on this \
             machine until this is resolved."
        ),
    }
}

/// Per-platform store install; the `cfg`s mirror the per-target deps in Cargo.toml.
fn install_native_store() -> keyring_core::Result<()> {
    let config: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();

    #[cfg(target_os = "windows")]
    keyring_core::set_default_store(windows_native_keyring_store::Store::new_with_configuration(
        &config,
    )?);
    #[cfg(target_os = "macos")]
    keyring_core::set_default_store(
        apple_native_keyring_store::keychain::Store::new_with_configuration(&config)?,
    );
    #[cfg(all(unix, not(any(target_os = "macos", target_os = "ios", target_os = "android"))))]
    keyring_core::set_default_store(
        zbus_secret_service_keyring_store::Store::new_with_configuration(&config)?,
    );
    #[cfg(not(any(
        target_os = "windows",
        target_os = "macos",
        all(unix, not(any(target_os = "macos", target_os = "ios", target_os = "android")))
    )))]
    let _ = config;

    Ok(())
}

fn entry() -> Result<keyring_core::Entry, keyring_core::Error> {
    keyring_core::Entry::new(SERVICE, ACCOUNT)
}

pub fn save_token(token: &str) -> Result<(), String> {
    entry()
        .map_err(|e| e.to_string())?
        .set_password(token)
        .map_err(|e| e.to_string())
}

pub fn load_token() -> Option<String> {
    let e = entry().ok()?;
    match e.get_password() {
        Ok(t) => Some(t),
        Err(keyring_core::Error::NoEntry) => None,
        Err(err) => {
            tracing::warn!("keychain read failed: {}", err);
            None
        }
    }
}

pub fn clear_token() -> Result<(), String> {
    let e = entry().map_err(|e| e.to_string())?;
    match e.delete_credential() {
        Ok(()) => Ok(()),
        // Logout is idempotent.
        Err(keyring_core::Error::NoEntry) => Ok(()),
        Err(err) => Err(err.to_string()),
    }
}
