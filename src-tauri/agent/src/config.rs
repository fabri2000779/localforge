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
    pub fingerprint: String,
}

impl InstallOutcome {
    pub fn pairing_summary(&self) -> String {
        format!(
            "\n\
             localforge-agent installed.\n\
             Pairing data — copy these three lines into the desktop's \"Add Node\" form:\n\
             \n\
               URL:         https://{bind}:{port}\n\
               Token:       {token}\n\
               Fingerprint: {fp}\n\
             \n\
             Config saved to {cfg}. Keep it readable only by root.\n\
             Start the service with: systemctl enable --now localforge-agent\n",
            bind = if self.bind == "0.0.0.0" {
                "<server-public-ip>"
            } else {
                &self.bind
            },
            port = self.port,
            token = self.token,
            fp = self.fingerprint,
            cfg = self.config_path.display()
        )
    }
}

pub fn install(
    config_path: &Path,
    data_root: &Path,
    bind: &str,
    port: u16,
) -> anyhow::Result<InstallOutcome> {
    // Token: 32 random hex chars prefixed for easy recognition.
    let raw = Uuid::new_v4().simple().to_string();
    let token = format!("lf_agent_{}", raw);

    // Self-signed cert valid for ~5 years, scoped to the bind address.
    let (cert_pem, key_pem, fingerprint) = tls::generate_self_signed(bind)?;

    let cfg = Config {
        token: token.clone(),
        bind: bind.to_string(),
        port,
        data_root: data_root.to_path_buf(),
        tls_cert_pem: cert_pem,
        tls_key_pem: key_pem,
    };
    cfg.save(config_path)?;

    std::fs::create_dir_all(data_root)?;

    Ok(InstallOutcome {
        config_path: config_path.to_path_buf(),
        bind: bind.to_string(),
        port,
        token,
        fingerprint,
    })
}
