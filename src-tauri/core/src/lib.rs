//! LocalForge core: shared types, traits, and game definitions.
//!
//! This crate is consumed by both the desktop app (Tauri) and the remote
//! agent binary. It must not depend on Tauri, Docker, or any other backend-
//! specific implementation — only on serialisable types and trait contracts.

pub mod backend;
pub mod games;
pub mod node;
pub mod types;

mod config_processor;
pub use config_processor::apply_config_variables;

pub use backend::{BackendError, LogLine, LogStream, NodeBackend, Result as BackendResult};
pub use games::{build_env_vars, get_builtin_games, resolve_startup};
pub use node::{NodeId, NodeKind};
pub use types::*;
