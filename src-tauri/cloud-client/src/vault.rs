//! Envelope-encryption primitives (pure Rust, no platform code).
//! DEK: AES-256-GCM key that encrypts every server/node blob on-device.
//! KEK: derived on demand from the password/passphrase via scrypt and used only to
//! wrap/unwrap the DEK; the cloud stores the wrap plus the salt and params.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine;
use rand::TryRng;

pub const KEY_LEN: usize = 32;
pub const NONCE_LEN: usize = 12;

/// Fresh 256-bit key (the DEK, or a new one on rotation).
pub fn generate_key() -> [u8; KEY_LEN] {
    let mut k = [0u8; KEY_LEN];
    rand::rngs::SysRng
        .try_fill_bytes(&mut k)
        .expect("OS RNG must be available");
    k
}

/// Fresh scrypt salt; stored beside the wrapped DEK (unique, not secret).
pub fn generate_salt() -> [u8; 16] {
    let mut s = [0u8; 16];
    rand::rngs::SysRng
        .try_fill_bytes(&mut s)
        .expect("OS RNG must be available");
    s
}

// Envelope format: `v1.<base64(nonce)>.<base64(ciphertext+tag)>`.

pub fn encrypt(key: &[u8; KEY_LEN], plaintext: &[u8]) -> Result<String, String> {
    let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from(*key));
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::rngs::SysRng
        .try_fill_bytes(&mut nonce_bytes)
        .map_err(|e| format!("rng: {e}"))?;
    let nonce = Nonce::from(nonce_bytes);
    let ct = cipher
        .encrypt(&nonce, plaintext)
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
    let nonce_arr: [u8; NONCE_LEN] = nonce_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "nonce wrong length".to_string())?;
    let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from(*key));
    let nonce = Nonce::from(nonce_arr);
    cipher
        .decrypt(&nonce, ciphertext.as_slice())
        .map_err(|e| format!("decrypt (key mismatch?): {e}"))
}

/// Scrypt parameters (N=2^15, r=8, p=1, 32-byte key): ~150 ms on a modern CPU. Changing them
/// invalidates every existing wrap; the cloud stores them beside the wrap for rotation.
pub const KEK_LOG_N: u8 = 15;
pub const KEK_R: u32 = 8;
pub const KEK_P: u32 = 1;
pub const KEK_LEN: usize = 32;

/// scrypt(password_or_passphrase, salt) → 32-byte KEK.
pub fn derive_kek(password: &str, salt: &[u8]) -> Result<[u8; KEK_LEN], String> {
    let params = scrypt::Params::new(KEK_LOG_N, KEK_R, KEK_P)
        .map_err(|e| format!("scrypt params: {e}"))?;
    let mut out = [0u8; KEK_LEN];
    scrypt::scrypt(password.as_bytes(), salt, &params, &mut out)
        .map_err(|e| format!("scrypt: {e}"))?;
    Ok(out)
}

/// Wrap the DEK with the KEK for server-side storage (same v1 envelope).
pub fn wrap_dek(kek: &[u8; KEY_LEN], dek: &[u8; KEY_LEN]) -> Result<String, String> {
    encrypt(kek, dek)
}

/// Unwrap a stored DEK; an AES-GCM failure means a wrong passphrase.
pub fn unwrap_dek(kek: &[u8; KEY_LEN], wrapped: &str) -> Result<[u8; KEY_LEN], String> {
    let plain = decrypt(kek, wrapped)?;
    if plain.len() != KEY_LEN {
        return Err(format!("unwrapped DEK wrong length: {}", plain.len()));
    }
    let mut out = [0u8; KEY_LEN];
    out.copy_from_slice(&plain);
    Ok(out)
}

/// Stored beside the wrap so a fresh device can re-derive the KEK.
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

// X25519 sealed box for Team org-DEK distribution (libsodium crypto_box_seal style):
// ephemeral ECDH with the recipient's pubkey, SHA-256 KDF bound to both public keys,
// then the v1 AES-GCM envelope. Confidentiality-only; the cloud never sees the raw DEK.

use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};

/// Length of an X25519 public key, in bytes.
pub const X25519_PK_LEN: usize = 32;

/// Fresh X25519 keypair as `(secret, public)`; the secret comes from our own OS RNG.
pub fn generate_keypair() -> ([u8; KEY_LEN], [u8; X25519_PK_LEN]) {
    let sk_bytes = generate_key();
    let pk = PublicKey::from(&StaticSecret::from(sk_bytes));
    (sk_bytes, pk.to_bytes())
}

/// Recover the public key from a stored secret (e.g. to (re)publish it).
pub fn public_from_secret(sk_bytes: &[u8; KEY_LEN]) -> [u8; X25519_PK_LEN] {
    PublicKey::from(&StaticSecret::from(*sk_bytes)).to_bytes()
}

/// KDF bound to both public keys, so a sealed blob can't be replayed to another recipient.
fn seal_kdf(shared: &[u8], epk: &[u8], recipient_pk: &[u8]) -> [u8; KEY_LEN] {
    let mut h = Sha256::new();
    h.update(b"localforge-sealed-dek-v1");
    h.update(shared);
    h.update(epk);
    h.update(recipient_pk);
    let digest = h.finalize();
    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(&digest);
    key
}

/// Seal `dek` to `recipient_pk`; returns `(ephemeral_pubkey_b64, sealed_envelope)`.
pub fn seal_to(
    recipient_pk: &[u8; X25519_PK_LEN],
    dek: &[u8; KEY_LEN],
) -> Result<(String, String), String> {
    let eph = StaticSecret::from(generate_key());
    let epk = PublicKey::from(&eph);
    let shared = eph.diffie_hellman(&PublicKey::from(*recipient_pk));
    let key = seal_kdf(shared.as_bytes(), &epk.to_bytes(), recipient_pk);
    let sealed = encrypt(&key, dek)?;
    Ok((
        base64::engine::general_purpose::STANDARD.encode(epk.to_bytes()),
        sealed,
    ))
}

/// Open a sealed DEK; an AES-GCM failure means a wrong key or a tampered blob.
pub fn open_sealed(
    my_sk: &[u8; KEY_LEN],
    epk_b64: &str,
    sealed: &str,
) -> Result<[u8; KEY_LEN], String> {
    let epk_bytes = base64::engine::general_purpose::STANDARD
        .decode(epk_b64)
        .map_err(|e| format!("epk decode: {e}"))?;
    if epk_bytes.len() != X25519_PK_LEN {
        return Err("epk wrong length".into());
    }
    let mut epk_arr = [0u8; X25519_PK_LEN];
    epk_arr.copy_from_slice(&epk_bytes);
    let sk = StaticSecret::from(*my_sk);
    let my_pk = PublicKey::from(&sk);
    let shared = sk.diffie_hellman(&PublicKey::from(epk_arr));
    let key = seal_kdf(shared.as_bytes(), &epk_arr, &my_pk.to_bytes());
    let plain = decrypt(&key, sealed)?;
    if plain.len() != KEY_LEN {
        return Err(format!("sealed DEK wrong length: {}", plain.len()));
    }
    let mut out = [0u8; KEY_LEN];
    out.copy_from_slice(&plain);
    Ok(out)
}

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
    fn sealed_dek_round_trip() {
        let (member_sk, member_pk) = generate_keypair();
        let dek = generate_key();
        let (epk_b64, sealed) = seal_to(&member_pk, &dek).unwrap();
        let opened = open_sealed(&member_sk, &epk_b64, &sealed).unwrap();
        assert_eq!(opened, dek);
    }

    #[test]
    fn sealed_dek_wrong_recipient_fails() {
        let (_alice_sk, alice_pk) = generate_keypair();
        let (mallory_sk, _mallory_pk) = generate_keypair();
        let dek = generate_key();
        let (epk_b64, sealed) = seal_to(&alice_pk, &dek).unwrap();
        assert!(open_sealed(&mallory_sk, &epk_b64, &sealed).is_err());
    }

    #[test]
    fn public_from_secret_matches_generated() {
        let (sk, pk) = generate_keypair();
        assert_eq!(public_from_secret(&sk), pk);
    }

    #[test]
    fn kek_params_defaults_stable() {
        let p = KekParams::defaults();
        // Changing these means existing wraps stop unwrapping.
        assert_eq!(p.algo, "scrypt");
        assert_eq!(p.n, 32768);
        assert_eq!(p.r, 8);
        assert_eq!(p.p, 1);
        assert_eq!(p.len, 32);
    }
}

/// Known-answer tests pinning the byte formats every stored blob depends on; a crypto
/// dependency bump that changed any output would lock every user out of their vault.
#[cfg(test)]
mod known_answer {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{:02x}", x)).collect()
    }

    const SALT: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    const PASSPHRASE: &str = "correct horse battery staple";
    const KEK_HEX: &str = "1aeba192e2389376c87334c305e9acc97a492085a776c11c0af751e1b42e2d13";

    fn fixed_key() -> [u8; 32] {
        core::array::from_fn(|i| (i as u8).wrapping_mul(7).wrapping_add(3))
    }
    fn fixed_sk() -> [u8; 32] {
        core::array::from_fn(|i| 200u8.wrapping_sub(i as u8 * 3))
    }
    fn fixed_dek() -> [u8; 32] {
        core::array::from_fn(|i| (i as u8) ^ 0x5a)
    }

    #[test]
    fn scrypt_kek_derivation_is_stable() {
        let kek = derive_kek(PASSPHRASE, &SALT).unwrap();
        assert_eq!(hex(&kek), KEK_HEX);
    }

    #[test]
    fn v1_envelope_decrypts_fixed_vector() {
        let env = "v1.cl34Zm4+riIyg2sN.aG/zMhCRurLcpqNZWDp0ELfkYo7NHTxIYdNZ+YrgshSEfcGNNCviVdTCr/vIYMo07Q==";
        let plain = decrypt(&fixed_key(), env).unwrap();
        assert_eq!(plain, b"localforge known-answer plaintext");
    }

    #[test]
    fn wrapped_dek_unwraps_with_derived_kek() {
        let kek = derive_kek(PASSPHRASE, &SALT).unwrap();
        let wrapped = "v1.vwmgq2wiQO/BJVJ5.BOxmMMwmrTBZfK4LRoo77or1kLe0suO8NAuzvI7SCPmaoIiVNIx71J64vk/7yYi7";
        assert_eq!(unwrap_dek(&kek, wrapped).unwrap(), fixed_dek());
    }

    #[test]
    fn x25519_public_key_derivation_is_stable() {
        assert_eq!(
            hex(&public_from_secret(&fixed_sk())),
            "36d6b4567333d248860d3aa36e4bd1a0cc3d5ab570c378c58133a85c43bd9d14"
        );
    }

    #[test]
    fn sealed_grant_opens_fixed_vector() {
        let epk = "5FiO6TRSoyE+wM+VjMH3fu0aFk+85Cy846FLt0dCDQU=";
        let sealed = "v1.WzTj1wCkUL4Sax6i.QE8nnR7zDSL1Z+TOhd9b6U6HN0r7J5mfSHQ+7eg/QDrP1eVN8t6SW7zP54Igh/lG";
        assert_eq!(open_sealed(&fixed_sk(), epk, sealed).unwrap(), fixed_dek());
    }
}
