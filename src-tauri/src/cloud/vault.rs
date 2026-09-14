//! Desktop vault glue: DEK/X25519 storage in the OS keychain plus the Tauri commands; the
//! crypto lives in `localforge-cloud-client::vault`.

use base64::Engine;

use localforge_cloud_client::vault as crypto;

pub use crypto::{decrypt, derive_kek, encrypt, generate_key, unwrap_dek, wrap_dek};

const SERVICE: &str = "LocalForge Cloud";
const ACCOUNT: &str = "vault-key";
/// Keychain slot for the user's X25519 secret (Team key sharing).
const ACCOUNT_X25519: &str = "x25519-sk";
const KEY_LEN: usize = crypto::KEY_LEN;

/// Borrowed org DEK used to decrypt the active org's blobs when we don't own it; `None`
/// means our own keychain DEK.
static ACTIVE_DEK_OVERRIDE: std::sync::RwLock<Option<[u8; KEY_LEN]>> =
    std::sync::RwLock::new(None);

fn set_active_dek_override(dek: Option<[u8; KEY_LEN]>) {
    // Recover from a poisoned lock: a dropped write would leave the previous org's DEK installed.
    let mut g = ACTIVE_DEK_OVERRIDE.write().unwrap_or_else(|e| e.into_inner());
    *g = dek;
}

/// True while a borrowed-org DEK is installed (we're viewing an org we don't own).
pub fn has_active_override() -> bool {
    ACTIVE_DEK_OVERRIDE
        .read()
        .map(|g| g.is_some())
        .unwrap_or_else(|e| e.into_inner().is_some())
}

/// The DEK the sync/pull path decrypts with: the override if set, else our own.
pub fn active_dek() -> Result<[u8; KEY_LEN], String> {
    {
        let g = ACTIVE_DEK_OVERRIDE.read().unwrap_or_else(|e| e.into_inner());
        if let Some(dek) = *g {
            return Ok(dek);
        }
    }
    ensure_key()
}

// OS-keychain-backed DEK storage.

fn entry() -> Result<keyring_core::Entry, keyring_core::Error> {
    keyring_core::Entry::new(SERVICE, ACCOUNT)
}

/// Get-or-generate the local DEK (stored in the OS keychain).
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

/// Wipe this device's key material (DEK, X25519 secret, override, the given borrowed org DEKs)
/// on sign-out so the next account can't inherit it.
pub fn clear_local_keys(borrowed_org_ids: &[String]) {
    set_active_dek_override(None);
    if let Ok(e) = entry() {
        let _ = e.delete_credential();
    }
    if let Ok(e) = x25519_entry() {
        let _ = e.delete_credential();
    }
    for org_id in borrowed_org_ids {
        if let Ok(e) = org_dek_entry(org_id) {
            let _ = e.delete_credential();
        }
    }
}

// X25519 secret (Team key sharing)

fn x25519_entry() -> Result<keyring_core::Entry, keyring_core::Error> {
    keyring_core::Entry::new(SERVICE, ACCOUNT_X25519)
}

fn load_x25519_sk() -> Result<Option<[u8; KEY_LEN]>, String> {
    let e = x25519_entry().map_err(|x| x.to_string())?;
    match e.get_password() {
        Ok(b64) => {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|x| format!("x25519 sk decode: {x}"))?;
            if bytes.len() != KEY_LEN {
                return Err(format!("x25519 sk wrong length: {}", bytes.len()));
            }
            let mut out = [0u8; KEY_LEN];
            out.copy_from_slice(&bytes);
            Ok(Some(out))
        }
        Err(keyring_core::Error::NoEntry) => Ok(None),
        Err(err) => Err(err.to_string()),
    }
}

fn save_x25519_sk(sk: &[u8; KEY_LEN]) -> Result<(), String> {
    let b64 = base64::engine::general_purpose::STANDARD.encode(sk);
    x25519_entry()
        .map_err(|e| e.to_string())?
        .set_password(&b64)
        .map_err(|e| e.to_string())
}

/// Ensure this device holds the user's X25519 secret: cached, or recovered from the cloud's
/// KEK-wrapped copy, or freshly minted and published. The pubkey is stable per user.
async fn ensure_keypair(kek: &[u8; KEY_LEN], token: &str) -> Result<(), super::api::ApiError> {
    use super::api;
    let local_sk =
        load_x25519_sk().map_err(|e| api::ApiError::Decode(format!("x25519: {e}")))?;
    let me = localforge_cloud_client::auth::fetch_me(token).await?;

    if let Some(sk) = local_sk {
        // Re-wrap with the CURRENT KEK after a passphrase change; minting a new keypair instead
        // would orphan every existing grant.
        let needs_rewrap = match &me.wrapped_x25519_sk {
            Some(w) => crypto::unwrap_dek(kek, w).is_err(),
            None => true,
        };
        if needs_rewrap {
            let pk = crypto::public_from_secret(&sk);
            let wrapped_sk = crypto::wrap_dek(kek, &sk).map_err(api::ApiError::Decode)?;
            let pk_b64 = base64::engine::general_purpose::STANDARD.encode(pk);
            localforge_cloud_client::keys::publish_pubkey(&pk_b64, &wrapped_sk, token).await?;
        }
        return Ok(());
    }

    // No local secret. Recover the one another device published, if it unwraps.
    if let Some(wrapped) = me.wrapped_x25519_sk {
        if let Ok(sk) = crypto::unwrap_dek(kek, &wrapped) {
            save_x25519_sk(&sk).map_err(|e| api::ApiError::Decode(format!("x25519: {e}")))?;
            return Ok(());
        }
        // Unwrap failed (legacy/corrupt blob): mint a new keypair; the cloud re-lists us for re-sealing.
    }
    let (sk, pk) = crypto::generate_keypair();
    let wrapped_sk = crypto::wrap_dek(kek, &sk).map_err(api::ApiError::Decode)?;
    let pk_b64 = base64::engine::general_purpose::STANDARD.encode(pk);
    localforge_cloud_client::keys::publish_pubkey(&pk_b64, &wrapped_sk, token).await?;
    save_x25519_sk(&sk).map_err(|e| api::ApiError::Decode(format!("x25519: {e}")))?;
    Ok(())
}

// Per-org DEK cache: a member's borrowed org key, kept in the keychain so access survives
// restarts without re-opening the grant. Never the user's own org.

fn org_dek_entry(org_id: &str) -> Result<keyring_core::Entry, keyring_core::Error> {
    keyring_core::Entry::new(SERVICE, &format!("org-dek:{org_id}"))
}

fn load_org_dek(org_id: &str) -> Option<[u8; KEY_LEN]> {
    let e = org_dek_entry(org_id).ok()?;
    let b64 = e.get_password().ok()?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    if bytes.len() != KEY_LEN {
        return None;
    }
    let mut out = [0u8; KEY_LEN];
    out.copy_from_slice(&bytes);
    Some(out)
}

fn save_org_dek(org_id: &str, dek: &[u8; KEY_LEN]) -> Result<(), String> {
    let b64 = base64::engine::general_purpose::STANDARD.encode(dek);
    org_dek_entry(org_id)
        .map_err(|e| e.to_string())?
        .set_password(&b64)
        .map_err(|e| e.to_string())
}

/// Adopt an org DEK obtained via invite handoff: cache it and make it the active key.
pub fn adopt_org_dek(org_id: &str, dek: &[u8; KEY_LEN]) {
    let _ = save_org_dek(org_id, dek);
    set_active_dek_override(Some(*dek));
}

/// Member side: seal a handoff DEK to our own pubkey as a durable grant for our other devices.
/// `Ok(false)` when this device has no keypair yet (the owner's grant will cover us).
pub async fn self_seal_grant(
    org_id: &str,
    my_user_id: &str,
    dek: &[u8; KEY_LEN],
    token: &str,
) -> Result<bool, super::api::ApiError> {
    use super::api;
    let Some(sk) =
        load_x25519_sk().map_err(|e| api::ApiError::Decode(format!("x25519: {e}")))?
    else {
        return Ok(false);
    };
    let pk = crypto::public_from_secret(&sk);
    let (epk_b64, sealed) = crypto::seal_to(&pk, dek).map_err(api::ApiError::Decode)?;
    localforge_cloud_client::keys::put_grant(org_id, my_user_id, &sealed, &epk_b64, token).await?;
    Ok(true)
}

// Tauri commands

/// Base64 DEK for the user to copy to a second device (generated if absent).
#[tauri::command]
pub async fn cloud_vault_export_key() -> Result<String, String> {
    let key = ensure_key()?;
    Ok(base64::engine::general_purpose::STANDARD.encode(key))
}

/// Replace the local DEK with a pasted recovery key; bad input leaves the existing key untouched.
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

/// Whether a vault key is stored on this device.
#[tauri::command]
pub async fn cloud_vault_has_key() -> Result<bool, String> {
    Ok(load_key()?.is_some())
}

/// Set up envelope encryption: derive the KEK from the password/passphrase, wrap the DEK and
/// POST it. 409 if a wrap exists unless `force` (which orphans existing blobs).
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

    // A fresh DEK is persisted only after the cloud accepts the wrap, so the keychain can never
    // disagree with the server.
    let (dek, freshly_generated) =
        match load_key().map_err(|e| api::ApiError::Decode(format!("vault: {e}")))? {
            Some(k) => (k, false),
            None => (crypto::generate_key(), true),
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
    if freshly_generated {
        save_key(&dek).map_err(|e| api::ApiError::Decode(format!("vault: {e}")))?;
    }
    // Best-effort: publish the X25519 keypair so this user can receive and grant org access.
    let _ = ensure_keypair(&kek, &token).await;
    Ok(())
}

/// Unlock the DEK on a fresh device from the cloud's wrap; a wrong secret returns `wrong_secret`.
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
    // Recover (or mint + publish) the X25519 keypair on this device too.
    let _ = ensure_keypair(&kek, &token).await;
    Ok(())
}

/// Unlock the DEK of an org we don't own from our sealed grant. Returns `granted`, `no_grant`
/// (the owner hasn't sealed us yet) or `no_keypair` (set up sync first).
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_unlock_org_dek(org_id: String) -> Result<&'static str, super::api::ApiError> {
    use super::api;
    let token = super::auth::current_token().ok_or_else(|| api::ApiError::Server {
        status: 401,
        code: "unauthenticated".into(),
        message: None,
    })?;
    // Prefer the current cloud grant so a rotated DEK is picked up; the cache is the fallback.
    let have_keypair = match load_x25519_sk() {
        Ok(opt) => opt,
        Err(e) => return Err(api::ApiError::Decode(format!("x25519: {e}"))),
    };
    if let Some(sk) = have_keypair {
        match localforge_cloud_client::keys::my_grant(&org_id, &token).await {
            Ok(Some(grant)) => {
                if let Ok(dek) = crypto::open_sealed(&sk, &grant.sealed_epk, &grant.sealed_dek) {
                    let _ = save_org_dek(&org_id, &dek);
                    set_active_dek_override(Some(dek));
                    return Ok("granted");
                }
                // Grant won't open with our key (keypair regenerated): fall through to the cache.
            }
            Ok(None) => { /* no grant yet — invite-handoff member uses the cache */ }
            Err(_) => { /* offline / transient — use the cache if we have one */ }
        }
    }
    // Fallback: a cached DEK (invite handoff or a previously opened grant); works offline.
    if let Some(dek) = load_org_dek(&org_id) {
        set_active_dek_override(Some(dek));
        return Ok("granted");
    }
    if have_keypair.is_none() {
        return Ok("no_keypair");
    }
    Ok("no_grant")
}

/// Clear the borrowed-org DEK override (switching back to an org we own).
#[tauri::command]
pub async fn cloud_clear_org_dek() -> Result<(), String> {
    set_active_dek_override(None);
    Ok(())
}

/// Owner side: seal our org DEK to every member with a published key but no grant.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_process_grants(org_id: String) -> Result<usize, super::api::ApiError> {
    let token = super::auth::current_token().ok_or_else(|| super::api::ApiError::Server {
        status: 401,
        code: "unauthenticated".into(),
        message: None,
    })?;
    process_grants(&org_id, &token).await
}

/// Seal our org DEK to every pending member; also used after rotation (all grants wiped). 403 = no-op.
pub async fn process_grants(org_id: &str, token: &str) -> Result<usize, super::api::ApiError> {
    use super::api;
    let pending = match localforge_cloud_client::keys::pending_grants(org_id, token).await {
        Ok(p) => p,
        Err(api::ApiError::Server { status: 403, .. }) => return Ok(0),
        Err(e) => return Err(e),
    };
    if pending.is_empty() {
        return Ok(0);
    }
    let dek = ensure_key().map_err(|e| api::ApiError::Decode(format!("vault: {e}")))?;
    let mut granted = 0usize;
    for m in pending {
        let pk_bytes = match base64::engine::general_purpose::STANDARD.decode(&m.pubkey) {
            Ok(b) if b.len() == crypto::X25519_PK_LEN => {
                let mut arr = [0u8; crypto::X25519_PK_LEN];
                arr.copy_from_slice(&b);
                arr
            }
            _ => continue, // skip a malformed pubkey rather than abort the batch
        };
        let Ok((epk_b64, sealed)) = crypto::seal_to(&pk_bytes, &dek) else {
            continue;
        };
        // Log per-member failures: a silent skip after rotation would leave that member locked out.
        match localforge_cloud_client::keys::put_grant(org_id, &m.user_id, &sealed, &epk_b64, token)
            .await
        {
            Ok(_) => granted += 1,
            Err(e) => tracing::warn!(
                "[grants] failed to seal org {} grant for member {}: {:?}",
                org_id,
                m.user_id,
                e
            ),
        }
    }
    Ok(granted)
}

/// `not_set_up` (no wrap on the server), `locked` (wrap exists, DEK not cached) or `unlocked`.
#[tauri::command]
pub async fn cloud_sync_key_status() -> Result<&'static str, super::api::ApiError> {
    use super::api;
    let Some(token) = super::auth::current_token() else {
        return Ok("not_set_up");
    };
    let local = load_key().ok().flatten();

    #[derive(serde::Deserialize)]
    struct SyncKey {
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
