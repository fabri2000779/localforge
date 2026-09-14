//! Self-signed cert generation (install) and rustls server config (runtime).

use axum_server::tls_rustls::RustlsConfig;

/// Self-signed cert for the bind address; returns `(cert_pem, key_pem, "SHA256:…" fingerprint)`.
pub fn generate_self_signed(bind: &str) -> anyhow::Result<(String, String, String)> {
    use rcgen::{CertificateParams, DistinguishedName, DnType, SanType};

    let mut params = CertificateParams::new(vec![bind.to_string()])?;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "LocalForge Agent");
    params.distinguished_name = dn;

    // A literal IP bind also needs an IP SAN.
    if let Ok(ip) = bind.parse::<std::net::IpAddr>() {
        params.subject_alt_names.push(SanType::IpAddress(ip));
    }

    // Backdated one day, valid ~5 years.
    let now = std::time::SystemTime::now();
    let day = std::time::Duration::from_secs(86_400);
    params.not_before = (now - day).into();
    params.not_after = (now + day * 365 * 5).into();

    let key_pair = rcgen::KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;

    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();

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

/// rustls server config from the inline PEM stored in the agent config.
pub async fn rustls_config(cert_pem: &str, key_pem: &str) -> anyhow::Result<RustlsConfig> {
    let cfg = RustlsConfig::from_pem(
        cert_pem.as_bytes().to_vec(),
        key_pem.as_bytes().to_vec(),
    )
    .await?;
    Ok(cfg)
}
