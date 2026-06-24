//! LocalForge agent — a small HTTPS daemon that exposes the
//! [`NodeBackend`] contract so the desktop app can drive a remote VPS
//! as if it were a local Docker host.
//!
//! Layout:
//!   - `config`  — agent.toml load/save + first-run defaults
//!   - `tls`     — rustls + self-signed cert generation
//!   - `auth`    — bearer-token middleware
//!   - `routes`  — axum router that maps HTTP/WS onto the trait
//!
//! Run modes:
//!   - `localforge-agent install` — generate token + self-signed TLS cert,
//!     write `/etc/localforge/agent.toml`, print pairing data, exit
//!   - `localforge-agent serve`   — load config and start the HTTPS server
//!   - `localforge-agent`          — same as `serve` (the default)

use clap::{Parser, Subcommand};
use localforge_backend_local::LocalDockerBackend;
use localforge_core::NodeBackend;
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

mod auth;
mod backup_target;
mod config;
mod relay;
mod routes;
mod tls;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Path to the agent config file (default: /etc/localforge/agent.toml).
    #[arg(long, global = true)]
    config: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// One-shot setup: generate a fresh token (+ self-signed TLS cert
    /// by default) and write the config file. Prints the pairing
    /// URL/token/fingerprint to stdout.
    Install {
        /// Where to write the config (default: /etc/localforge/agent.toml).
        #[arg(long)]
        config_path: Option<PathBuf>,

        /// Where to put the server data (default: /var/lib/localforge).
        #[arg(long)]
        data_root: Option<PathBuf>,

        /// Network interface to bind to (default: 0.0.0.0).
        #[arg(long, default_value = "0.0.0.0")]
        bind: String,

        /// TCP port to listen on (default: 7878).
        #[arg(long, default_value_t = 7878)]
        port: u16,

        /// Path to a CA-signed cert (e.g. Let's Encrypt fullchain.pem)
        /// to use instead of generating a self-signed one. Requires
        /// --key-pem too. When set, the desktop trusts the cert via
        /// WebPKI and doesn't need fingerprint pinning.
        #[arg(long, requires = "key_pem")]
        cert_pem: Option<PathBuf>,

        /// Path to the matching private key (PEM, PKCS#8).
        #[arg(long, requires = "cert_pem")]
        key_pem: Option<PathBuf>,
    },

    /// Serve HTTPS using the configured token + TLS cert.
    Serve,

    /// Link this agent to a LocalForge cloud org for direct relay control,
    /// so mobile / another desktop can drive it with the owner's desktop
    /// offline. Paste the one-time enrollment blob from the desktop's
    /// "Link node" action (or the dashboard). Purely additive — the
    /// standalone HTTPS surface is unchanged.
    Link {
        /// The one-time enrollment blob (base64url) issued by the cloud.
        blob: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    // rustls 0.23 can't auto-select a crypto provider when BOTH `ring` (our
    // HTTPS server) and `aws-lc-rs` (pulled transitively by the relay's
    // tokio-tungstenite) end up in the dependency tree. Install ring as the
    // process default before ANY TLS work — otherwise building the server's
    // ServerConfig (or a relay ClientConfig) panics with "Could not
    // automatically determine the process-level CryptoProvider". Idempotent;
    // ignore the Err if something already installed one.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let cli = Cli::parse();
    let config_path = cli
        .config
        .unwrap_or_else(|| PathBuf::from("/etc/localforge/agent.toml"));

    match cli.command.unwrap_or(Command::Serve) {
        Command::Install {
            config_path: cp,
            data_root,
            bind,
            port,
            cert_pem,
            key_pem,
        } => {
            let target = cp.unwrap_or(config_path);
            let data = data_root.unwrap_or_else(|| PathBuf::from("/var/lib/localforge"));
            let outcome = config::install(config::InstallOptions {
                config_path: &target,
                data_root: &data,
                bind: &bind,
                port,
                cert_pem_path: cert_pem.as_deref(),
                key_pem_path: key_pem.as_deref(),
            })?;
            println!("{}", outcome.pairing_summary());
            Ok(())
        }
        Command::Link { blob } => {
            use base64::Engine;
            let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(blob.trim())
                .map_err(|e| anyhow::anyhow!("invalid enrollment blob: {}", e))?;
            let link: config::CloudLink = serde_json::from_slice(&raw)
                .map_err(|e| anyhow::anyhow!("malformed enrollment blob: {}", e))?;
            let node_id = link.node_id.clone();
            config::save_cloud_link(&config_path, link)?;
            // Best-effort: bounce the systemd service so the agent picks up
            // the cloud link and connects to the relay right away — the user
            // shouldn't have to run a second command. Writing the 0600 config
            // above already required root, so `systemctl` will have the
            // privileges it needs. If we're not under systemd (foreground /
            // WSL without systemd), fall back to printing instructions.
            let restarted = std::process::Command::new("systemctl")
                .args(["restart", "localforge-agent"])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if restarted {
                println!(
                    "Linked to cloud (node {node_id}) and restarted the agent — \
                     it's connecting to the relay now."
                );
            } else {
                println!(
                    "Linked to cloud (node {node_id}). Restart the agent to connect now:\n  \
                     sudo systemctl restart localforge-agent\n\
                     (or, if you run it in the foreground, stop it and re-run `serve`)."
                );
            }
            Ok(())
        }
        Command::Serve => serve(&config_path).await,
    }
}

async fn serve(config_path: &std::path::Path) -> anyhow::Result<()> {
    let cfg = config::Config::load(config_path)?;
    tracing::info!(
        "starting localforge-agent on {}:{} (data_root: {})",
        cfg.bind,
        cfg.port,
        cfg.data_root.display()
    );

    let backend = LocalDockerBackend::connect(cfg.data_root.clone())
        .await
        .map_err(|e| anyhow::anyhow!("local backend unreachable: {}", e))?;
    let backend: Arc<dyn NodeBackend> = Arc::new(backend);

    // Start the host scheduler so cron actions fire 24/7 (the agent runs
    // headless as a service — this is the intended home for schedules). Backup
    // schedules resolve their target id against the provisioned target file.
    let sched_data_root = cfg.data_root.clone();
    let resolver: localforge_backend_local::BackupTargetResolver =
        Arc::new(move |id| crate::backup_target::find(&sched_data_root, id));
    localforge_backend_local::spawn_scheduler(backend.clone(), cfg.data_root.clone(), resolver);
    // Watch for unexpected container exits 24/7 and auto-restart per policy.
    localforge_backend_local::spawn_crash_watcher(backend.clone(), cfg.data_root.clone());

    let app_state = routes::AppState {
        backend,
        token: cfg.token.clone(),
        config_path: config_path.to_path_buf(),
        data_root: cfg.data_root.clone(),
        relay_started: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };

    // If this agent has been linked to a cloud org, start the relay client
    // alongside the HTTPS server so mobile / another desktop can drive it
    // with the owner's desktop offline. Un-linked agents skip this entirely
    // (standalone, no cloud).
    if let Some(link) = cfg.cloud.clone() {
        tracing::info!(
            "cloud link present (node {} / org {}); starting relay client",
            link.node_id,
            link.org_id,
        );
        app_state
            .relay_started
            .store(true, std::sync::atomic::Ordering::SeqCst);
        relay::spawn(app_state.backend.clone(), link, app_state.data_root.clone());
    }

    let tls_config = tls::rustls_config(&cfg.tls_cert_pem, &cfg.tls_key_pem).await?;
    let addr: std::net::SocketAddr = format!("{}:{}", cfg.bind, cfg.port).parse()?;

    let router = routes::router(app_state);
    tracing::info!("listening on https://{}", addr);

    axum_server::bind_rustls(addr, tls_config)
        .serve(router.into_make_service())
        .await?;
    Ok(())
}
