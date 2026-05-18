//! OS-native JWT storage. Backed by:
//!   - Windows: Credential Manager
//!   - macOS:   Keychain
//!   - Linux:   Secret Service (gnome-keyring / KWallet)
//!
//! Both `service` and `account` strings show up in the OS credential
//! UI, so they need to read sensibly there.

const SERVICE: &str = "LocalForge Cloud";
const ACCOUNT: &str = "session-jwt";

fn entry() -> Result<keyring::Entry, keyring::Error> {
    keyring::Entry::new(SERVICE, ACCOUNT)
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
        Err(keyring::Error::NoEntry) => None,
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
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(err.to_string()),
    }
}
