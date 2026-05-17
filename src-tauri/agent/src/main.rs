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
mod config;
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

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

    let app_state = routes::AppState {
        backend,
        token: cfg.token.clone(),
    };

    let tls_config = tls::rustls_config(&cfg.tls_cert_pem, &cfg.tls_key_pem).await?;
    let addr: std::net::SocketAddr = format!("{}:{}", cfg.bind, cfg.port).parse()?;

    let router = routes::router(app_state);
    tracing::info!("listening on https://{}", addr);

    axum_server::bind_rustls(addr, tls_config)
        .serve(router.into_make_service())
        .await?;
    Ok(())
}
