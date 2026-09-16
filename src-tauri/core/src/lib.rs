//! Shared types, traits and game definitions; no Tauri, Docker or backend-specific deps.

pub mod backend;
pub mod config_files;
pub mod games;
pub mod node;
pub mod types;

pub use backend::{
    detect_oauth_url, BackendError, ByteStream, InstallStream, LogLine, LogStream, NodeBackend,
    Result as BackendResult,
};
pub use config_files::apply_config_files;
pub use games::{build_env_vars, get_builtin_games};
pub use node::{NodeId, NodeKind};
pub use types::*;
