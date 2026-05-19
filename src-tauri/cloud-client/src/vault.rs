//! Envelope encryption — pure crypto primitives.
//!
//! Two layers, both used by the cloud sync stack:
//!
//!   **DEK** (Data Encryption Key) — 256-bit AES-GCM key that
//!   encrypts every server / node blob before it leaves the device.
//!   Cached locally (in the OS keychain on desktop, app-data dir on
//!   mobile) so day-to-day operation is instant.
//!
//!   **KEK** (Key Encryption Key) — 256-bit AES-GCM key NEVER stored
//!   anywhere; re-derived on demand from the user's password (for
//!   email/pwd accounts) or sync passphrase (for OAuth accounts) via
//!   scrypt. Used to wrap/unwrap the DEK.
//!
//! The cloud stores `wrapped_dek = AES-GCM(DEK, KEK)` plus the scrypt
//! salt + params. On a new device the user authenticates as normal,
//! fetches the wrap from /me, re-derives the KEK from their secret,
//! unwraps the DEK, caches it locally, and decryption proceeds. The
//! cloud never sees an unwrapped DEK.
//!
//! Everything here is pure Rust — no keyring, no Tauri, nothing
//! platform-specific. Storage (loading + saving the DEK and the
//! token) lives in the consuming app behind the `DekStore` /
//! `TokenStore` traits defined in `lib.rs`.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine;
use rand::TryRngCore;

pub const KEY_LEN: usize = 32;
pub const NONCE_LEN: usize = 12;

// ---------------------------------------------------------------------------
// Random material
// ---------------------------------------------------------------------------

/// Generate a fresh 256-bit AES key — DEK on first cloud-sync setup,
/// or a fresh wrap on rotation.
pub fn generate_key() -> [u8; KEY_LEN] {
    let mut k = [0u8; KEY_LEN];
    rand::rngs::OsRng
        .try_fill_bytes(&mut k)
        .expect("OS RNG must be available");
    k
}

/// Generate a fresh 16-byte salt for scrypt. Public — stored alongside
/// the wrapped DEK on the cloud (it doesn't have to be secret, only
/// unique per user).
pub fn generate_salt() -> [u8; 16] {
    let mut s = [0u8; 16];
    rand::rngs::OsRng
        .try_fill_bytes(&mut s)
        .expect("OS RNG must be available");
    s
}

// ---------------------------------------------------------------------------
// AES-256-GCM envelope.
// Format: `v1.<base64(nonce)>.<base64(ciphertext+tag)>`
// Bumping the prefix is how we'd migrate the format later.
// ---------------------------------------------------------------------------

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
// KEK derivation (scrypt) + DEK wrap/unwrap (AES-GCM)
// ---------------------------------------------------------------------------

/// Scrypt parameters. log2(N)=15 (N=32768), r=8, p=1, key_len=32.
/// ~150ms on a modern desktop CPU and roughly equivalent on a current
/// flagship phone — slow enough that a stolen-DB brute force takes
/// years per password, fast enough that a real user doesn't notice.
///
/// Bumping these would invalidate every existing wrap, so the cloud
/// stores them alongside the wrap (`kek_params` field on /me) and we
/// only consult them on rotation.
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

/// Wrap the DEK with the KEK so it's safe to store server-side. Same
/// envelope format as the rest of the vault — reuses `encrypt()`.
pub fn wrap_dek(kek: &[u8; KEY_LEN], dek: &[u8; KEY_LEN]) -> Result<String, String> {
    encrypt(kek, dek)
}

/// Unwrap a server-stored wrapped_dek with a freshly-derived KEK.
/// AES-GCM authentication failure here means a wrong passphrase —
/// callers should surface that gently rather than treating it like a
/// data-corruption error.
pub fn unwrap_dek(kek: &[u8; KEY_LEN], wrapped: &str) -> Result<[u8; KEY_LEN], String> {
    let plain = decrypt(kek, wrapped)?;
    if plain.len() != KEY_LEN {
        return Err(format!("unwrapped DEK wrong length: {}", plain.len()));
    }
    let mut out = [0u8; KEY_LEN];
    out.copy_from_slice(&plain);
    Ok(out)
}

/// What the cloud stores alongside the wrap so the desktop / mobile
/// can re-derive the KEK on a fresh device. Round-trips through JSON.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_round_trip() {
        let key = generate_key();
        let plaintext = b"hello, localforge cloud";
        let env = encrypt(&key, plaintext).unwrap();
        assert!(env.starts_with("v1."));
        let decrypted = decrypt(&key, &env).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn wrong_key_fails() {
        let key1 = generate_key();
        let key2 = generate_key();
        let env = encrypt(&key1, b"secret").unwrap();
        assert!(decrypt(&key2, &env).is_err());
    }

    #[test]
    fn bad_envelope_rejected() {
        let key = generate_key();
        assert!(decrypt(&key, "v2.foo.bar").is_err());
        assert!(decrypt(&key, "not.an.envelope.shape").is_err());
        assert!(decrypt(&key, "v1.").is_err());
    }

    #[test]
    fn dek_wrap_unwrap_round_trip() {
        let salt = generate_salt();
        let kek = derive_kek("correct horse battery staple", &salt).unwrap();
        let dek = generate_key();
        let wrapped = wrap_dek(&kek, &dek).unwrap();
        let unwrapped = unwrap_dek(&kek, &wrapped).unwrap();
        assert_eq!(unwrapped, dek);
    }

    #[test]
    fn wrong_passphrase_unwrap_fails() {
        let salt = generate_salt();
        let kek_a = derive_kek("right one", &salt).unwrap();
        let kek_b = derive_kek("wrong one", &salt).unwrap();
        let dek = generate_key();
        let wrapped = wrap_dek(&kek_a, &dek).unwrap();
        assert!(unwrap_dek(&kek_b, &wrapped).is_err());
    }

    #[test]
    fn kek_params_defaults_stable() {
        let p = KekParams::defaults();
        // Pinned values — changing these means existing wraps stop
        // unwrapping. The test exists to catch unintended bumps.
        assert_eq!(p.algo, "scrypt");
        assert_eq!(p.n, 32768);
        assert_eq!(p.r, 8);
        assert_eq!(p.p, 1);
        assert_eq!(p.len, 32);
    }
}
