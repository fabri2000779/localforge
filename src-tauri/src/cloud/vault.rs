//! End-to-end encryption keys for cloud sync.
//!
//! Two layers:
//!
//!   DEK (Data Encryption Key) — 256-bit AES-GCM key. Encrypts every
//!     server / node blob before it leaves the device. Cached in the
//!     OS keychain so day-to-day operation is instant.
//!
//!   KEK (Key Encryption Key) — 256-bit AES-GCM key. NEVER stored
//!     anywhere; re-derived on demand from the user's password (for
//!     email/pwd accounts) or a sync passphrase (OAuth-only accounts)
//!     via scrypt. Used to wrap/unwrap the DEK.
//!
//! The cloud stores `wrapped_dek = AES-GCM(DEK, KEK)` plus the scrypt
//! salt + params. On a new device the user authenticates as normal,
//! the desktop fetches the wrap from /me, re-derives the KEK from
//! their password / passphrase, unwraps the DEK, caches it locally,
//! and decryption proceeds. The cloud can never produce the DEK on
//! its own.
//!
//! For users who don't want to trust their password against a cloud
//! breach, the "Show recovery key" UI exposes the raw DEK so they can
//! restore it manually on another device — same as v0.1.14.

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

// ---------------------------------------------------------------------------
// KEK derivation (scrypt) + DEK wrap/unwrap (AES-GCM)
// ---------------------------------------------------------------------------

/// Scrypt parameters. log2(N)=15, r=8, p=1, key_len=32. ~150ms on a
/// modern desktop CPU, ~30M ops/year of brute-force on a single GPU
/// — adequate for protecting against a leaked DB row, far beyond
/// "instant" for an attacker.
pub const KEK_LOG_N: u8 = 15;
pub const KEK_R: u32 = 8;
pub const KEK_P: u32 = 1;
pub const KEK_LEN: usize = 32;

/// scrypt(password_or_passphrase, salt) → 32-byte KEK.
pub fn derive_kek(password: &str, salt: &[u8]) -> Result<[u8; KEK_LEN], String> {
    let params = scrypt::Params::new(KEK_LOG_N, KEK_R, KEK_P, KEK_LEN)
        .map_err(|e| format!("scrypt params: {e}"))?;
    let mut out = [0u8; KEK_LEN];
    scrypt::scrypt(password.as_bytes(), salt, &params, &mut out)
        .map_err(|e| format!("scrypt: {e}"))?;
    Ok(out)
}

/// Wrap the DEK with the KEK so it's safe to store server-side.
/// Same envelope format as the rest of the vault — reuses encrypt().
pub fn wrap_dek(kek: &[u8; KEY_LEN], dek: &[u8; KEY_LEN]) -> Result<String, String> {
    encrypt(kek, dek)
}

/// Unwrap a server-stored wrapped_dek with a freshly-derived KEK.
pub fn unwrap_dek(kek: &[u8; KEY_LEN], wrapped: &str) -> Result<[u8; KEY_LEN], String> {
    let plain = decrypt(kek, wrapped)?;
    if plain.len() != KEY_LEN {
        return Err(format!("unwrapped DEK wrong length: {}", plain.len()));
    }
    let mut out = [0u8; KEY_LEN];
    out.copy_from_slice(&plain);
    Ok(out)
}

/// Generate a fresh 16-byte salt for scrypt. Stored alongside the
/// wrapped_dek on the cloud. Public so it doesn't have to be secret.
pub fn generate_salt() -> [u8; 16] {
    let mut s = [0u8; 16];
    rand::rngs::OsRng.try_fill_bytes(&mut s).expect("OS RNG");
    s
}

// ---------------------------------------------------------------------------
// Tauri commands for the React layer
// ---------------------------------------------------------------------------

#[derive(serde::Serialize, serde::Deserialize)]
pub struct KekParams {
    pub algo: &'static str,
    pub n: u32,
    pub r: u32,
    pub p: u32,
    pub len: u32,
}

impl KekParams {
    pub fn defaults() -> Self {
        Self {
            algo: "scrypt",
            n: 1u32 << KEK_LOG_N,
            r: KEK_R,
            p: KEK_P,
            len: KEK_LEN as u32,
        }
    }
}

/// Set up envelope encryption for the current user. Used at signup
/// time (when the password is in hand) and on first-time sync setup
/// for OAuth users (when they pick a passphrase).
///
/// Generates a fresh DEK + salt, derives the KEK from the password
/// or passphrase, wraps the DEK, and POSTs everything to the cloud.
/// The DEK is cached locally so subsequent encrypt/decrypt calls are
/// instant.
///
/// Idempotency: the cloud rejects with 409 if a wrap already exists
/// for this user. Pass `force = true` to rotate (this orphans every
/// existing blob — use only for password change with re-wrap).
#[tauri::command]
pub async fn cloud_sync_key_setup(
    secret: String,
    force: Option<bool>,
) -> Result<(), super::api::ApiError> {
    use super::api;
    let token = super::auth::current_token().ok_or_else(|| api::ApiError::Server {
        status: 401,
        code: "unauthenticated".into(),
        message: None,
    })?;

    // Use the local DEK if we already generated one (existing
    // v0.1.14 users), otherwise generate a fresh one.
    let dek = match load_key().map_err(|e| api::ApiError::Decode(format!("vault: {e}")))? {
        Some(k) => k,
        None => {
            let k = generate_key();
            save_key(&k).map_err(|e| api::ApiError::Decode(format!("vault: {e}")))?;
            k
        }
    };

    let salt = generate_salt();
    let kek = derive_kek(&secret, &salt).map_err(api::ApiError::Decode)?;
    let wrapped = wrap_dek(&kek, &dek).map_err(api::ApiError::Decode)?;

    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Body {
        wrapped_dek: String,
        kek_salt: String,
        kek_params: KekParams,
        force: bool,
    }
    let body = Body {
        wrapped_dek: wrapped,
        kek_salt: base64::engine::general_purpose::STANDARD.encode(salt),
        kek_params: KekParams::defaults(),
        force: force.unwrap_or(false),
    };
    let _: serde_json::Value = api::post("/v1/account/sync-key", &body, Some(&token)).await?;
    Ok(())
}

/// Unlock the DEK on a fresh device. Fetches the wrapped_dek from
/// /me, re-derives the KEK from the user's secret, unwraps, and
/// caches the DEK locally.
///
/// `secret` is the password (email/pwd users) or sync passphrase
/// (OAuth users) — whichever the user gave at setup time. Wrong
/// secret → AES-GCM authentication fails → returns
/// `wrong_secret` so the UI can prompt again without locking
/// anything out.
#[tauri::command]
pub async fn cloud_sync_key_unlock(secret: String) -> Result<(), super::api::ApiError> {
    use super::api;
    let token = super::auth::current_token().ok_or_else(|| api::ApiError::Server {
        status: 401,
        code: "unauthenticated".into(),
        message: None,
    })?;

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct SyncKey {
        wrapped_dek: String,
        kek_salt: String,
        // kek_params currently always scrypt; we'd use it for
        // rotation later.
    }
    #[derive(serde::Deserialize)]
    struct Me { #[serde(rename = "syncKey")] sync_key: Option<SyncKey> }
    let me: Me = api::get("/v1/account/me", Some(&token)).await?;
    let Some(sk) = me.sync_key else {
        return Err(api::ApiError::Server {
            status: 412,
            code: "sync_key_not_set".into(),
            message: Some("call cloud_sync_key_setup first".into()),
        });
    };

    let salt = base64::engine::general_purpose::STANDARD
        .decode(&sk.kek_salt)
        .map_err(|e| api::ApiError::Decode(format!("bad salt: {e}")))?;
    let kek = derive_kek(&secret, &salt).map_err(api::ApiError::Decode)?;
    let dek = unwrap_dek(&kek, &sk.wrapped_dek).map_err(|e| api::ApiError::Server {
        status: 400,
        code: "wrong_secret".into(),
        message: Some(e),
    })?;
    save_key(&dek).map_err(|e| api::ApiError::Decode(format!("vault: {e}")))?;
    Ok(())
}

/// Three-state status the UI uses to decide which dialog to show.
///   `not_set_up`  — no wrapped_dek on the server yet (first time)
///   `locked`      — wrap exists, but the DEK isn't cached locally
///   `unlocked`    — DEK cached, ready to sync
#[tauri::command]
pub async fn cloud_sync_key_status() -> Result<&'static str, super::api::ApiError> {
    use super::api;
    let Some(token) = super::auth::current_token() else {
        return Ok("not_set_up"); // not signed in at all
    };
    let local = load_key().ok().flatten();

    #[derive(serde::Deserialize)]
    struct SyncKey { /* fields ignored — only existence matters */ }
    #[derive(serde::Deserialize)]
    struct Me { #[serde(rename = "syncKey")] sync_key: Option<SyncKey> }
    let me: Me = api::get("/v1/account/me", Some(&token)).await?;
    match (me.sync_key.is_some(), local.is_some()) {
        (true, true) => Ok("unlocked"),
        (true, false) => Ok("locked"),
        (false, _) => Ok("not_set_up"),
    }
}
