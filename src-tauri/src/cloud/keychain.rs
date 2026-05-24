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
    // Empty config = the store's defaults (matches the old v3 behaviour).
    let config: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();

    #[cfg(target_os = "windows")]
    let res = keyring::use_windows_native_store(&config);
    #[cfg(target_os = "macos")]
    let res = keyring::use_apple_keychain_store(&config);
    #[cfg(target_os = "linux")]
    let res = keyring::use_zbus_secret_service_store(&config);
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    let res: keyring_core::Result<()> = {
        let _ = &config;
        Ok(())
    };

    match res {
        Ok(_) => tracing::info!("[keychain] OS credential store installed"),
        Err(e) => tracing::error!(
            "[keychain] FAILED to install the OS credential store: {e}. \
             Cloud login and sync-key storage will NOT persist on this \
             machine until this is resolved."
        ),
    }
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
