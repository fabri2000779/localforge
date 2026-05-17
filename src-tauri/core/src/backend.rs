//! Backend contract: every node (local Docker or remote agent) must expose
//! this surface for the desktop app to drive it.
//!
//! The trait is intentionally crate-agnostic: it knows nothing about Tauri,
//! bollard, axum, or HTTPS. Implementations live in the desktop crate (local
//! Docker via bollard) and in the agent / remote-client crates (Phase 2/3).
//!
//! The trait is intentionally **incremental** — only methods with a working
//! implementation today are listed. As lifecycle/streaming/install support
//! lands on every backend, methods get added here.

use crate::types::{
    ContainerStats, CreateServerRequest, DirectoryContents, DockerInfo, FileEntry, GameConfig,
    Server, ServerStatus,
};
use async_trait::async_trait;
use futures_util::stream::BoxStream;
use std::collections::HashMap;

pub type Result<T> = std::result::Result<T, BackendError>;

/// Errors surfaced by a [`NodeBackend`] implementation.
///
/// The variants are deliberately coarse — implementations stringify their
/// internal causes so the trait can stay free of bollard / hyper / etc.
#[derive(thiserror::Error, Debug)]
pub enum BackendError {
    #[error("node is not reachable: {0}")]
    NotConnected(String),

    #[error("Docker error: {0}")]
    Docker(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("I/O error: {0}")]
    Io(String),

    #[error("transport error: {0}")]
    Transport(String),

    #[error("authentication failed")]
    Unauthorized,

    #[error("{0}")]
    Other(String),
}

impl BackendError {
    pub fn docker<E: std::fmt::Display>(e: E) -> Self {
        Self::Docker(e.to_string())
    }
    pub fn io<E: std::fmt::Display>(e: E) -> Self {
        Self::Io(e.to_string())
    }
    pub fn not_found<S: Into<String>>(s: S) -> Self {
        Self::NotFound(s.into())
    }
    pub fn invalid<S: Into<String>>(s: S) -> Self {
        Self::InvalidInput(s.into())
    }
    pub fn other<E: std::fmt::Display>(e: E) -> Self {
        Self::Other(e.to_string())
    }
}

/// A single log line streamed from a server's stdout/stderr.
#[derive(Debug, Clone)]
pub struct LogLine {
    pub server_id: String,
    pub line: String,
}

/// Boxed log stream, `'static` so it can outlive the backend Arc.
pub type LogStream = BoxStream<'static, Result<LogLine>>;

/// The node operations contract. This is the read-only / file-management
/// surface — server lifecycle (start/stop/install) and live streaming
/// land in subsequent phases.
#[async_trait]
pub trait NodeBackend: Send + Sync {
    // ----- health & metadata ----------------------------------------------

    /// Cheap reachability check.
    async fn ping(&self) -> Result<()>;

    /// Docker daemon information (or equivalent on the remote node).
    async fn docker_info(&self) -> Result<DockerInfo>;

    // ----- server read-side -----------------------------------------------

    async fn list_servers(&self) -> Result<Vec<Server>>;

    async fn get_server(&self, id: &str) -> Result<Option<Server>>;

    async fn server_status(&self, id: &str) -> Result<ServerStatus>;

    async fn get_stats(&self, id: &str) -> Result<ContainerStats>;

    /// Total disk usage of the server data directory in bytes.
    async fn get_disk_usage(&self, id: &str) -> Result<u64>;

    /// Fetch the last `lines` log lines from the running container.
    async fn get_logs(&self, id: &str, lines: usize) -> Result<Vec<String>>;

    // ----- server lifecycle ----------------------------------------------

    /// Create a server record and prepare its data directory. Does **not**
    /// start the container — call [`start_server`] for that.
    async fn create_server(
        &self,
        request: CreateServerRequest,
        game: GameConfig,
    ) -> Result<Server>;

    async fn update_server_config(
        &self,
        id: &str,
        config: HashMap<String, String>,
    ) -> Result<Server>;

    /// Stop the container (if running), remove it, and delete the on-disk
    /// data directory + persisted record.
    async fn delete_server(&self, id: &str) -> Result<()>;

    /// Start the server's container. Returns the new status after starting.
    async fn start_server(&self, id: &str) -> Result<ServerStatus>;

    /// Stop the container gracefully (sending the configured stop_command
    /// where available before SIGTERM).
    async fn stop_server(&self, id: &str) -> Result<ServerStatus>;

    /// Send a single console command to the running container's stdin.
    async fn send_command(&self, id: &str, command: &str) -> Result<()>;

    /// Continuous stream of new log lines for the running container.
    /// The stream ends when the container stops or the caller drops it.
    async fn stream_logs(&self, id: &str) -> Result<LogStream>;

    // ----- file operations on the host -----------------------------------
    //
    // Paths are absolute. For the local backend that's the user's
    // filesystem; for remote agents the path lives on the remote host.
    // Both implementations are expected to enforce that the path is under
    // one of the known server data directories.

    async fn list_files(&self, path: &str) -> Result<DirectoryContents>;
    async fn read_file_text(&self, path: &str) -> Result<String>;
    async fn write_file_text(&self, path: &str, content: &str) -> Result<()>;
    async fn create_file(&self, path: &str) -> Result<()>;
    async fn create_directory(&self, path: &str) -> Result<()>;
    async fn delete_path(&self, path: &str) -> Result<()>;
    async fn rename_path(&self, from: &str, to: &str) -> Result<()>;
    async fn move_path(&self, from: &str, to: &str) -> Result<()>;
    async fn copy_path(&self, from: &str, to: &str) -> Result<()>;
    async fn file_info(&self, path: &str) -> Result<FileEntry>;
}
