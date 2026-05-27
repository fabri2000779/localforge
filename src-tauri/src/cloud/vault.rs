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
/// Keychain slot for this user's X25519 SECRET (Team key sharing). Lets the
/// member open org-DEK grants sealed to them. Same backend as the DEK.
const ACCOUNT_X25519: &str = "x25519-sk";
const KEY_LEN: usize = crypto::KEY_LEN;

/// DEK to use for decrypting the ACTIVE org's blobs when it isn't ours. A
/// sub-user viewing the owner's org sets this to the org DEK they opened from
/// their sealed grant; cleared (back to our own keychain DEK) for our own org.
/// `cloud_sync_pull` consults `active_dek()` so a member's pull decrypts the
/// owner's data without ever overwriting the member's own cached DEK.
static ACTIVE_DEK_OVERRIDE: std::sync::RwLock<Option<[u8; KEY_LEN]>> =
    std::sync::RwLock::new(None);

fn set_active_dek_override(dek: Option<[u8; KEY_LEN]>) {
    if let Ok(mut g) = ACTIVE_DEK_OVERRIDE.write() {
        *g = dek;
    }
}

/// The DEK the sync/pull path should decrypt with: the active-org override if
/// set (sub-user viewing another org), else our own keychain DEK.
pub fn active_dek() -> Result<[u8; KEY_LEN], String> {
    if let Ok(g) = ACTIVE_DEK_OVERRIDE.read() {
        if let Some(dek) = *g {
            return Ok(dek);
        }
    }
    ensure_key()
}

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

// --- X25519 secret (Team key sharing) -------------------------------------

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

/// Make sure this device holds the user's X25519 secret, given the KEK
/// (available at setup/unlock time). Recovery order:
///   1. already cached locally → done.
///   2. cloud has a KEK-wrapped secret (another device published it) →
///      unwrap with the KEK + cache.
///   3. neither → mint a fresh keypair, wrap the secret with the KEK,
///      publish the public key + wrapped secret, cache locally.
/// The public key is stable per user; owners seal the org DEK to it.
async fn ensure_keypair(kek: &[u8; KEY_LEN], token: &str) -> Result<(), super::api::ApiError> {
    use super::api;
    if load_x25519_sk()
        .map_err(|e| api::ApiError::Decode(format!("x25519: {e}")))?
        .is_some()
    {
        return Ok(());
    }
    // Try to recover a secret another device already published.
    let me = localforge_cloud_client::auth::fetch_me(token).await?;
    if let Some(wrapped) = me.wrapped_x25519_sk {
        if let Ok(sk) = crypto::unwrap_dek(kek, &wrapped) {
            save_x25519_sk(&sk).map_err(|e| api::ApiError::Decode(format!("x25519: {e}")))?;
            return Ok(());
        }
        // Wrapped present but unwrap failed — fall through to mint a fresh
        // one (the KEK is correct here since the DEK unwrap succeeded; a
        // mismatch means a legacy/corrupt blob, so replace it).
    }
    let (sk, pk) = crypto::generate_keypair();
    let wrapped_sk = crypto::wrap_dek(kek, &sk).map_err(api::ApiError::Decode)?;
    let pk_b64 = base64::engine::general_purpose::STANDARD.encode(pk);
    localforge_cloud_client::keys::publish_pubkey(&pk_b64, &wrapped_sk, token).await?;
    save_x25519_sk(&sk).map_err(|e| api::ApiError::Decode(format!("x25519: {e}")))?;
    Ok(())
}

// --- Per-org DEK cache (a member's borrowed org key) ----------------------
//
// When a member obtains an org's DEK — via an invite handoff or by opening a
// sealed grant — we cache it in the keychain keyed by org. That makes access
// durable across restarts WITHOUT needing the grant re-opened (or even a
// keypair, for the invite-handoff path) every time. Never the user's OWN org
// (that's the plain `vault-key` DEK).

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

/// Adopt an org DEK we just obtained (invite handoff): cache it durably +
/// make it the active decryption key. Used by the accept-invite command.
pub fn adopt_org_dek(org_id: &str, dek: &[u8; KEY_LEN]) {
    let _ = save_org_dek(org_id, dek);
    set_active_dek_override(Some(*dek));
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
    // Also establish + publish the X25519 keypair so this user can be granted
    // (and grant) access to org-shared data. Best-effort: a failure here
    // doesn't undo the sync-key setup.
    let _ = ensure_keypair(&kek, &token).await;
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
    // Recover (or mint + publish) the X25519 keypair on this device too.
    let _ = ensure_keypair(&kek, &token).await;
    Ok(())
}

/// Unlock the DEK for an org we DON'T own (a sub-user viewing the owner's
/// org): fetch our sealed grant, open it with our X25519 secret, and stash it
/// as the active-org decryption DEK. `cloud_sync_pull` then decrypts the
/// owner's blobs with it. Returns:
///   - `granted`   — DEK unlocked, ready to decrypt.
///   - `no_grant`  — the owner hasn't sealed us in yet (they're offline, or
///                   we just joined). The UI shows "waiting for access".
///   - `no_keypair`— we have no X25519 secret yet (set up sync first).
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_unlock_org_dek(org_id: String) -> Result<&'static str, super::api::ApiError> {
    use super::api;
    let token = super::auth::current_token().ok_or_else(|| api::ApiError::Server {
        status: 401,
        code: "unauthenticated".into(),
        message: None,
    })?;
    // Prefer the CURRENT grant from the cloud so a rotated DEK is picked up
    // (the owner re-seals a fresh grant after rotating). The cached DEK is
    // only a fallback — for the invite-handoff member (no grant yet) and for
    // offline use. This costs one small GET per org switch, which is fine for
    // a user-initiated action and is the price of correctness across rotation.
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
                // A grant exists but won't open with our key (our keypair was
                // regenerated) — fall through to the cache / no_grant.
            }
            Ok(None) => { /* no grant yet — invite-handoff member uses the cache */ }
            Err(_) => { /* offline / transient — use the cache if we have one */ }
        }
    }
    // Fallback: a DEK we cached earlier (invite handoff, or a previously
    // opened grant). Durable across restarts + works offline.
    if let Some(dek) = load_org_dek(&org_id) {
        set_active_dek_override(Some(dek));
        return Ok("granted");
    }
    if have_keypair.is_none() {
        return Ok("no_keypair");
    }
    Ok("no_grant")
}

/// Clear the active-org DEK override — call when switching back to an org we
/// own, so sync/pull goes back to our own keychain DEK.
#[tauri::command]
pub async fn cloud_clear_org_dek() -> Result<(), String> {
    set_active_dek_override(None);
    Ok(())
}

/// Owner side: seal OUR org DEK to every member of `org_id` who has published
/// a key but doesn't have a grant yet. Idempotent + safe to call often (on
/// sign-in, when a member joins). No-op / 403 if we don't own the org.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_process_grants(org_id: String) -> Result<usize, super::api::ApiError> {
    let token = super::auth::current_token().ok_or_else(|| super::api::ApiError::Server {
        status: 401,
        code: "unauthenticated".into(),
        message: None,
    })?;
    process_grants(&org_id, &token).await
}

/// Seal OUR org DEK to every member of `org_id` who has a published key but no
/// grant yet. Reused by the grant-on-presence flow AND by DEK rotation (after
/// a rotation, all grants were wiped → everyone is "pending" → re-sealed with
/// the new DEK). 403 (not the owner) → no-op.
pub async fn process_grants(org_id: &str, token: &str) -> Result<usize, super::api::ApiError> {
    use super::api;
    let pending = match localforge_cloud_client::keys::pending_grants(org_id, token).await {
        Ok(p) => p,
        // Not the owner of this org → nothing for us to do.
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
        if localforge_cloud_client::keys::put_grant(org_id, &m.user_id, &sealed, &epk_b64, token)
            .await
            .is_ok()
        {
            granted += 1;
        }
    }
    Ok(granted)
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
