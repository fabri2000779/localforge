//! Cert-pinning rustls verifier.
//!
//! The agent ships with a self-signed certificate generated at install
//! time; the desktop trusts it solely because its SHA-256 fingerprint
//! matches what was copy-pasted into the "Add Node" form. We bypass the
//! whole chain-of-trust verification — the fingerprint *is* the trust
//! anchor — but still validate that the cert actually presents the
//! expected key (no MITM can fake the fingerprint without breaking
//! SHA-256).

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use std::sync::Arc;

/// Normalised SHA-256 fingerprint: 64 lowercase hex chars, no separators.
#[derive(Debug, Clone)]
pub struct PinnedFingerprint(String);

impl PinnedFingerprint {
    /// Parse a fingerprint in any of the common shapes:
    ///   `SHA256:AB:CD:...`  /  `ab:cd:...`  /  `abcd...`
    pub fn parse(input: &str) -> Result<Self, FingerprintError> {
        let cleaned: String = input
            .trim()
            .trim_start_matches("SHA256:")
            .trim_start_matches("sha256:")
            .chars()
            .filter(|c| !c.is_whitespace() && *c != ':')
            .collect();
        let lower = cleaned.to_lowercase();
        if lower.len() != 64 || !lower.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(FingerprintError::InvalidFormat);
        }
        Ok(Self(lower))
    }

    pub fn as_hex(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FingerprintError {
    #[error("fingerprint is not 64 lowercase hex characters")]
    InvalidFormat,
}

#[derive(Debug)]
pub struct FingerprintVerifier {
    pinned: PinnedFingerprint,
    provider: Arc<CryptoProvider>,
}

impl FingerprintVerifier {
    pub fn new(pinned: PinnedFingerprint) -> Arc<Self> {
        Arc::new(Self {
            pinned,
            provider: Arc::new(rustls::crypto::ring::default_provider()),
        })
    }
}

impl ServerCertVerifier for FingerprintVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let actual = sha256_hex(end_entity.as_ref());
        if constant_time_eq(actual.as_bytes(), self.pinned.0.as_bytes()) {
            Ok(ServerCertVerified::assertion())
        } else {
            tracing::warn!(
                "cert fingerprint mismatch: expected {} got {}",
                self.pinned.0,
                actual
            );
            Err(rustls::Error::General(
                "certificate fingerprint does not match pinned value".into(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

fn sha256_hex(input: &[u8]) -> String {
    use ring::digest::{digest, SHA256};
    let d = digest(&SHA256, input);
    let mut out = String::with_capacity(64);
    for b in d.as_ref() {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}
