//! On-disk agent configuration: `/etc/localforge/agent.toml` (by default).
//!
//! The file holds the bearer token, TLS material (PEM, inline so a single
//! file is sufficient), bind address, port, and data root. `install` is
//! the one-shot bootstrap that generates a fresh token + self-signed
//! cert and writes the file.

use crate::tls;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Hex-encoded bearer token required on every request.
    pub token: String,

    /// Address to bind to (e.g. `0.0.0.0`).
    pub bind: String,

    /// TCP port to listen on.
    pub port: u16,

    /// Path under which servers/, config/ etc. live on the agent host.
    pub data_root: PathBuf,

    /// PEM-encoded TLS certificate.
    pub tls_cert_pem: String,

    /// PEM-encoded TLS private key (PKCS#8).
    pub tls_key_pem: String,
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let body = std::fs::read_to_string(path).map_err(|e| {
            anyhow::anyhow!(
                "failed to read agent config {}: {}. Run `localforge-agent install` first.",
                path.display(),
                e
            )
        })?;
        Ok(toml::from_str(&body)?)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = toml::to_string_pretty(self)?;
        std::fs::write(path, body)?;
        // Best-effort tighten permissions to 0600 on Unix (the file holds
        // a token + private key).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }
}

pub struct InstallOutcome {
    pub config_path: PathBuf,
    pub bind: String,
    pub port: u16,
    pub token: String,
    /// `None` when the operator brought their own CA-signed cert
    /// (typically Let's Encrypt) — the desktop falls back to WebPKI
    /// roots and doesn't need fingerprint pinning.
    pub fingerprint: Option<String>,
}

impl InstallOutcome {
    pub fn pairing_summary(&self) -> String {
        let bind = if self.bind == "0.0.0.0" {
            "<server-public-ip>"
        } else {
            &self.bind
        };
        let fp_line = match &self.fingerprint {
            Some(fp) => format!("  Fingerprint: {}\n", fp),
            None => "  Fingerprint: (skipped — using a CA-signed certificate)\n".to_string(),
        };
        format!(
            "\n\
             localforge-agent installed.\n\
             Pairing data — paste these into the desktop's \"Add Node\" form:\n\
             \n  URL:         https://{bind}:{port}\n  Token:       {token}\n{fp}\n\
             Config saved to {cfg}. Keep it readable only by root.\n\
             Start the service with: systemctl enable --now localforge-agent\n",
            bind = bind,
            port = self.port,
            token = self.token,
            fp = fp_line,
            cfg = self.config_path.display(),
        )
    }
}

pub struct InstallOptions<'a> {
    pub config_path: &'a Path,
    pub data_root: &'a Path,
    pub bind: &'a str,
    pub port: u16,
    /// Path to an existing PEM-encoded cert (Let's Encrypt fullchain,
    /// Cloudflare origin cert, etc.). When `Some`, `key_pem_path` must
    /// also be provided. When `None`, a self-signed cert is generated.
    pub cert_pem_path: Option<&'a Path>,
    pub key_pem_path: Option<&'a Path>,
}

pub fn install(opts: InstallOptions<'_>) -> anyhow::Result<InstallOutcome> {
    // Token: 32 random hex chars prefixed for easy recognition.
    let raw = Uuid::new_v4().simple().to_string();
    let token = format!("lf_agent_{}", raw);

    let (cert_pem, key_pem, fingerprint) =
        match (opts.cert_pem_path, opts.key_pem_path) {
            (Some(cert), Some(key)) => {
                let cert_pem = std::fs::read_to_string(cert).map_err(|e| {
                    anyhow::anyhow!("failed to read cert {}: {}", cert.display(), e)
                })?;
                let key_pem = std::fs::read_to_string(key).map_err(|e| {
                    anyhow::anyhow!("failed to read key {}: {}", key.display(), e)
                })?;
                (cert_pem, key_pem, None)
            }
            (None, None) => {
                let (cert, key, fp) = tls::generate_self_signed(opts.bind)?;
                (cert, key, Some(fp))
            }
            _ => anyhow::bail!(
                "both --cert-pem and --key-pem must be provided together (or neither, to auto-generate)"
            ),
        };

    let cfg = Config {
        token: token.clone(),
        bind: opts.bind.to_string(),
        port: opts.port,
        data_root: opts.data_root.to_path_buf(),
        tls_cert_pem: cert_pem,
        tls_key_pem: key_pem,
    };
    cfg.save(opts.config_path)?;

    std::fs::create_dir_all(opts.data_root)?;

    Ok(InstallOutcome {
        config_path: opts.config_path.to_path_buf(),
        bind: opts.bind.to_string(),
        port: opts.port,
        token,
        fingerprint,
    })
}
