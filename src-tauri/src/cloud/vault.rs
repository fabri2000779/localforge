//! Desktop vault glue.
//!
//! All the pure crypto (AES-GCM envelope, scrypt KEK derivation, wrap
//! / unwrap, KekParams) lives in `localforge-cloud-client::vault`.
//! What stays here is the desktop-specific bit: persisting the
//! unwrapped DEK in the OS keychain (Win Credential Manager, macOS
//! Keychain, Linux Secret Service) and the `#[tauri::command]`
//! wrappers the React layer calls.
//!
//! Mobile reuses the same shared crate for the crypto and wires its
//! own DEK storage (sandboxed app-data on iOS / Android — Keychain
//! Services + Android Keystore come later).

use base64::Engine;

use localforge_cloud_client::vault as crypto;

// Re-export the pure helpers + constants so existing call sites
// (sync.rs, relay.rs) keep working through `super::vault::*`.
#[allow(unused_imports)]
pub use crypto::{
    KekParams, KEK_LEN, KEK_LOG_N, KEK_P, KEK_R, decrypt, derive_kek, encrypt, generate_key,
    generate_salt, unwrap_dek, wrap_dek,
};

const SERVICE: &str = "LocalForge Cloud";
const ACCOUNT: &str = "vault-key";
const KEY_LEN: usize = crypto::KEY_LEN;

// ---------------------------------------------------------------------------
// OS-keychain-backed DEK storage. Desktop only — mobile uses a
// different backend (sandboxed app-data file, behind the same logical
// shape).
// ---------------------------------------------------------------------------

fn entry() -> Result<keyring_core::Entry, keyring_core::Error> {
    keyring_core::Entry::new(SERVICE, ACCOUNT)
}

/// Get-or-generate the local DEK. First call after a fresh install
/// creates one and stashes it in the OS keychain.
pub fn ensure_key() -> Result<[u8; KEY_LEN], String> {
    if let Some(k) = load_key()? {
        return Ok(k);
    }
    let k = crypto::generate_key();
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
        Err(keyring_core::Error::NoEntry) => Ok(None),
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
// Tauri commands
// ---------------------------------------------------------------------------

/// Returns the base64-encoded DEK for the user to write down / paste
/// into a second device. Generates one if absent.
#[tauri::command]
pub async fn cloud_vault_export_key() -> Result<String, String> {
    let key = ensure_key()?;
    Ok(base64::engine::general_purpose::STANDARD.encode(key))
}

/// Replace the local DEK with the one pasted by the user (typically
/// the recovery key from another device). Validates length before
/// persisting; corrupt input is rejected without touching the
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

/// Set up envelope encryption for the current user. Used at signup
/// time (when the password is in hand) and on first-time sync setup
/// for OAuth users (when they pick a passphrase).
///
/// Generates a fresh DEK + salt, derives the KEK from the password or
/// passphrase, wraps the DEK, and POSTs everything to the cloud. The
/// DEK is cached locally in the OS keychain so subsequent
/// encrypt/decrypt calls are instant.
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

    // Use the local DEK if we already generated one (existing v0.1.14
    // users), otherwise generate a fresh one.
    let dek = match load_key().map_err(|e| api::ApiError::Decode(format!("vault: {e}")))? {
        Some(k) => k,
        None => {
            let k = crypto::generate_key();
            save_key(&k).map_err(|e| api::ApiError::Decode(format!("vault: {e}")))?;
            k
        }
    };

    let salt = crypto::generate_salt();
    let kek = crypto::derive_kek(&secret, &salt).map_err(api::ApiError::Decode)?;
    let wrapped = crypto::wrap_dek(&kek, &dek).map_err(api::ApiError::Decode)?;

    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Body {
        wrapped_dek: String,
        kek_salt: String,
        kek_params: crypto::KekParams,
        force: bool,
    }
    let body = Body {
        wrapped_dek: wrapped,
        kek_salt: base64::engine::general_purpose::STANDARD.encode(salt),
        kek_params: crypto::KekParams::defaults(),
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
/// secret → AES-GCM authentication fails → returns `wrong_secret` so
/// the UI can prompt again without locking anything out.
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
        // kek_params currently always scrypt; we'd use it for rotation later.
    }
    #[derive(serde::Deserialize)]
    struct Me {
        #[serde(rename = "syncKey")]
        sync_key: Option<SyncKey>,
    }
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
    let kek = crypto::derive_kek(&secret, &salt).map_err(api::ApiError::Decode)?;
    let dek = crypto::unwrap_dek(&kek, &sk.wrapped_dek).map_err(|e| api::ApiError::Server {
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
    struct SyncKey {
        /* fields ignored — only existence matters */
    }
    #[derive(serde::Deserialize)]
    struct Me {
        #[serde(rename = "syncKey")]
        sync_key: Option<SyncKey>,
    }
    let me: Me = api::get("/v1/account/me", Some(&token)).await?;
    match (me.sync_key.is_some(), local.is_some()) {
        (true, true) => Ok("unlocked"),
        (true, false) => Ok("locked"),
        (false, _) => Ok("not_set_up"),
    }
}
