//! TLS material — self-signed cert generation on install, and rustls
//! [`ServerConfig`] construction at runtime.

use axum_server::tls_rustls::RustlsConfig;

/// Generate a self-signed certificate scoped to the agent's bind address.
///
/// Returns `(cert_pem, key_pem, sha256_fingerprint)`. The fingerprint is
/// what the desktop pins; the user copies it into the "Add Node" form so
/// the client can verify the cert without involving a public CA.
pub fn generate_self_signed(bind: &str) -> anyhow::Result<(String, String, String)> {
    use rcgen::{CertificateParams, DistinguishedName, DnType, SanType};

    let mut params = CertificateParams::new(vec![bind.to_string()])?;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "LocalForge Agent");
    params.distinguished_name = dn;

    // If the bind is a literal IP, also expose it as an IP SAN; otherwise
    // it's a DNS SAN (already covered by the constructor above).
    if let Ok(ip) = bind.parse::<std::net::IpAddr>() {
        params.subject_alt_names.push(SanType::IpAddress(ip));
    }

    // Validity: backdated 1 day, valid ~5 years. rcgen wants `time`
    // timestamps so we convert through SystemTime which both crates accept.
    let now = std::time::SystemTime::now();
    let day = std::time::Duration::from_secs(86_400);
    params.not_before = (now - day).into();
    params.not_after = (now + day * 365 * 5).into();

    let key_pair = rcgen::KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;

    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();

    // SHA-256 fingerprint of the DER-encoded cert.
    let der = cert.der();
    let digest = sha256_hex(der.as_ref());
    let formatted = digest
        .as_bytes()
        .chunks(2)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect::<Vec<_>>()
        .join(":")
        .to_uppercase();

    Ok((cert_pem, key_pem, format!("SHA256:{}", formatted)))
}

fn sha256_hex(input: &[u8]) -> String {
    use ring::digest::{digest, SHA256};
    let d = digest(&SHA256, input);
    hex_lower(d.as_ref())
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

/// Build a rustls [`RustlsConfig`] from inline PEM (as stored in the
/// agent config) suitable for `axum_server::bind_rustls`.
pub async fn rustls_config(cert_pem: &str, key_pem: &str) -> anyhow::Result<RustlsConfig> {
    let cfg = RustlsConfig::from_pem(
        cert_pem.as_bytes().to_vec(),
        key_pem.as_bytes().to_vec(),
    )
    .await?;
    Ok(cfg)
}
