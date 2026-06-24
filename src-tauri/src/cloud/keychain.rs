//! OS-native JWT storage. Backed by:
//!   - Windows: Credential Manager
//!   - macOS:   Keychain
//!   - Linux:   Secret Service (gnome-keyring / KWallet)
//!
//! Both `service` and `account` strings show up in the OS credential
//! UI, so they need to read sensibly there.

const SERVICE: &str = "LocalForge Cloud";
const ACCOUNT: &str = "session-jwt";

/// Install the OS-native credential store as keyring's process-wide
/// default. MUST run once at startup, before anything creates an
/// `Entry` (cloud login, sync-key storage and the relay all read the
/// keychain).
///
/// keyring 4 no longer selects the backend at compile time via Cargo
/// features — the application installs the store at runtime. On Linux we
/// deliberately pick the pure-Rust **zbus** Secret Service backend (no
/// system libdbus needed). If installation fails we log loudly instead
/// of letting `Entry::new` silently fall back to a no-op store, which is
/// what produced the v0.1.14 "logs in, then immediately asks to log in
/// again" bug.
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

/// Install the per-platform native store as keyring-core's process-wide
/// default. This is the install snippet keyring's docs tell apps to copy from
/// the crate's `cli` module rather than linking the umbrella crate's `cli`
/// feature. The `cfg`s here mirror the per-target deps in `Cargo.toml`, so each
/// build only references the one store crate it actually links.
fn install_native_store() -> keyring_core::Result<()> {
    // Empty config = the store's defaults.
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
        // Treating "nothing to delete" as success — logout is idempotent.
        Err(keyring_core::Error::NoEntry) => Ok(()),
        Err(err) => Err(err.to_string()),
    }
}
