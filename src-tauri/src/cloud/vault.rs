//! End-to-end encryption key for cloud sync.
//!
//! Design contract: the cloud server MUST NEVER be able to decrypt a
//! synced server config. The key lives only on the user's hardware,
//! in the OS keychain. If they lose it, they lose access to their
//! cloud-stored data — by design. There is no server-side recovery.
//!
//! The recovery path is for the user to copy the key (showed in
//! Settings) to a password manager or write it down. Setting up a
//! second device = paste the recovery key into the new install.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine;
use rand::TryRngCore;

const SERVICE: &str = "LocalForge Cloud";
const ACCOUNT: &str = "vault-key";
const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

fn entry() -> Result<keyring::Entry, keyring::Error> {
    keyring::Entry::new(SERVICE, ACCOUNT)
}

/// Generate a fresh 256-bit key. Used the very first time the user
/// enables sync, or when they explicitly rotate it.
pub fn generate_key() -> [u8; KEY_LEN] {
    let mut k = [0u8; KEY_LEN];
    rand::rngs::OsRng
        .try_fill_bytes(&mut k)
        .expect("OS RNG must be available");
    k
}

/// Get-or-generate the local vault key. The first call after a fresh
/// install creates one and stashes it in the keychain.
pub fn ensure_key() -> Result<[u8; KEY_LEN], String> {
    if let Some(k) = load_key()? {
        return Ok(k);
    }
    let k = generate_key();
    save_key(&k)?;
    Ok(k)
}

pub fn load_key() -> Result<Option<[u8; KEY_LEN]>, String> {
    let e = entry().map_err(|x| x.to_string())?;
    match e.get_password() {
        Ok(b64) => {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|x| format!("vault key decode: {x}"))?;
            if bytes.len() != KEY_LEN {
                return Err(format!("vault key wrong length: {}", bytes.len()));
            }
            let mut out = [0u8; KEY_LEN];
            out.copy_from_slice(&bytes);
            Ok(Some(out))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(err.to_string()),
    }
}

pub fn save_key(key: &[u8; KEY_LEN]) -> Result<(), String> {
    let b64 = base64::engine::general_purpose::STANDARD.encode(key);
    entry()
        .map_err(|e| e.to_string())?
        .set_password(&b64)
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Encryption envelope
// ---------------------------------------------------------------------------
// Format: `v1.<base64(nonce)>.<base64(ciphertext)>`
// Bumping the prefix is how we'd migrate the format later.

pub fn encrypt(key: &[u8; KEY_LEN], plaintext: &[u8]) -> Result<String, String> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::rngs::OsRng
        .try_fill_bytes(&mut nonce_bytes)
        .map_err(|e| format!("rng: {e}"))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| format!("encrypt: {e}"))?;
    Ok(format!(
        "v1.{}.{}",
        base64::engine::general_purpose::STANDARD.encode(nonce_bytes),
        base64::engine::general_purpose::STANDARD.encode(ct)
    ))
}

pub fn decrypt(key: &[u8; KEY_LEN], envelope: &str) -> Result<Vec<u8>, String> {
    let parts: Vec<&str> = envelope.split('.').collect();
    if parts.len() != 3 || parts[0] != "v1" {
        return Err(format!("bad envelope shape: {}", parts.first().unwrap_or(&"?")));
    }
    let nonce_bytes = base64::engine::general_purpose::STANDARD
        .decode(parts[1])
        .map_err(|e| format!("nonce decode: {e}"))?;
    let ciphertext = base64::engine::general_purpose::STANDARD
        .decode(parts[2])
        .map_err(|e| format!("ct decode: {e}"))?;
    if nonce_bytes.len() != NONCE_LEN {
        return Err("nonce wrong length".into());
    }
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let nonce = Nonce::from_slice(&nonce_bytes);
    cipher
        .decrypt(nonce, ciphertext.as_slice())
        .map_err(|e| format!("decrypt (key mismatch?): {e}"))
}

// ---------------------------------------------------------------------------
// Commands exposed to the frontend
// ---------------------------------------------------------------------------

/// Returns the base64-encoded key for the user to write down / paste
/// into a second device. Generates one if absent.
#[tauri::command]
pub async fn cloud_vault_export_key() -> Result<String, String> {
    let key = ensure_key()?;
    Ok(base64::engine::general_purpose::STANDARD.encode(key))
}

/// Replace the local vault key with the one pasted by the user
/// (typically the recovery key from another device). Validates length
/// before persisting; corrupt input is rejected without touching the
/// existing key.
#[tauri::command]
pub async fn cloud_vault_import_key(key_b64: String) -> Result<(), String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(key_b64.trim())
        .map_err(|e| format!("not valid base64: {e}"))?;
    if bytes.len() != KEY_LEN {
        return Err(format!(
            "wrong length: got {} bytes, expected {}",
            bytes.len(),
            KEY_LEN
        ));
    }
    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(&bytes);
    save_key(&key)
}

/// True if a vault key is stored on this device — used by the UI to
/// gate the "Show recovery key" vs "Set up sync key" prompts.
#[tauri::command]
pub async fn cloud_vault_has_key() -> Result<bool, String> {
    Ok(load_key()?.is_some())
}
