//! Backend contract every node (local Docker or remote agent) implements; crate-agnostic.

use crate::types::{
    BackupEntry, BackupTarget, ContainerStats, CreateServerRequest, DirectoryContents, DockerInfo,
    FileEntry, GameConfig, InstallEvent, MetricPoint, NodeStats, OrgBackupTarget, Player,
    PlayerAction, Schedule, Server, ServerStatus,
};
use async_trait::async_trait;
use futures_util::stream::BoxStream;
use std::collections::HashMap;

pub type Result<T> = std::result::Result<T, BackendError>;

/// Errors from a [`NodeBackend`]; deliberately coarse, causes are stringified.
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

/// Install-event stream; ends with `Done` (exit code) or an `Err`.
pub type InstallStream = BoxStream<'static, Result<InstallEvent>>;

/// Chunked file stream for upload/download.
pub type ByteStream = BoxStream<'static, Result<bytes::Bytes>>;

/// First `https://` token in a log line that looks like an auth/device-login URL.
pub fn detect_oauth_url(line: &str) -> Option<String> {
    for word in line.split_whitespace() {
        if !word.starts_with("https://") {
            continue;
        }
        let lower = word.to_ascii_lowercase();
        if lower.contains("oauth")
            || lower.contains("auth")
            || lower.contains("login")
            || lower.contains("verify")
            || lower.contains("device")
        {
            return Some(
                word.trim_matches(|c| c == '"' || c == '\'' || c == '<' || c == '>' || c == ',')
                    .to_string(),
            );
        }
    }
    None
}

/// The node operations contract.
#[async_trait]
pub trait NodeBackend: Send + Sync {
    // ----- health & metadata ----------------------------------------------

    /// Cheap reachability check.
    async fn ping(&self) -> Result<()>;

    /// Docker daemon information (or equivalent on the remote node).
    async fn docker_info(&self) -> Result<DockerInfo>;

    /// Host-level metrics; implementations cache a background sysinfo snapshot.
    async fn node_stats(&self) -> Result<NodeStats>;

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

    /// Create the record and data directory without starting the container.
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

    /// Make the saved config take effect: render the game's config files into the data dir and, while
    /// the server is stopped, recreate the container when its env / command / port binding differ.
    async fn apply_server_config(&self, id: &str, game: GameConfig) -> Result<Server>;

    /// Stop and remove the container, then delete the data directory and record.
    async fn delete_server(&self, id: &str) -> Result<()>;

    /// Like [`delete_server`] but keeps the on-disk data. Backends without support must
    /// refuse (the default) rather than fall back to a full delete.
    async fn delete_server_keep_data(&self, id: &str) -> Result<()> {
        let _ = id;
        Err(BackendError::Other(
            "keeping world data on delete is not supported by this node — upgrade the agent, or delete everything".into(),
        ))
    }

    /// Start the server's container. Returns the new status after starting.
    async fn start_server(&self, id: &str) -> Result<ServerStatus>;

    /// Stop gracefully (configured stop command before SIGTERM).
    async fn stop_server(&self, id: &str) -> Result<ServerStatus>;

    /// Send a single console command to the running container's stdin.
    async fn send_command(&self, id: &str, command: &str) -> Result<()>;

    /// Live log stream; ends when the container stops or the caller drops it.
    async fn stream_logs(&self, id: &str) -> Result<LogStream>;

    /// Run the install script in a one-shot container, streaming events; the caller supplies the game.
    async fn run_install(&self, id: &str, game: GameConfig) -> Result<InstallStream>;

    /// Stop, wipe the data dir and mark not-installed; the [`Server`] record is kept.
    async fn reset_server_data(&self, id: &str) -> Result<()>;

    // ----- file operations on the host -----------------------------------

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

    /// Chunked file download.
    async fn download_file(&self, path: &str) -> Result<ByteStream>;

    /// Chunked upload replacing `path`.
    async fn upload_file(&self, path: &str, body: ByteStream) -> Result<()>;

    // ----- backups (bring-your-own S3) -----------------------------------
    // Backups run on the host (local Docker or the agent); the cloud never touches S3.

    /// Tar+gzip the data dir and upload it; returns the object key.
    async fn create_backup(&self, id: &str, target: &BackupTarget) -> Result<String> {
        let _ = (id, target);
        Err(BackendError::Other(
            "backups are not supported on this backend".into(),
        ))
    }

    /// List the server's backup objects in the bucket, newest first.
    async fn list_backups(&self, id: &str, target: &BackupTarget) -> Result<Vec<BackupEntry>> {
        let _ = (id, target);
        Err(BackendError::Other(
            "backups are not supported on this backend".into(),
        ))
    }

    /// Stop the server, move the old data aside and extract `key` over it.
    async fn restore_backup(&self, id: &str, target: &BackupTarget, key: &str) -> Result<()> {
        let _ = (id, target, key);
        Err(BackendError::Other(
            "backups are not supported on this backend".into(),
        ))
    }

    /// Delete a backup object from the bucket.
    async fn delete_backup(&self, id: &str, target: &BackupTarget, key: &str) -> Result<()> {
        let _ = (id, target, key);
        Err(BackendError::Other(
            "backups are not supported on this backend".into(),
        ))
    }

    /// Push the org's backup targets to this node over direct HTTPS (remote agents only; local is a no-op).
    async fn set_backup_targets(&self, targets: &[OrgBackupTarget]) -> Result<()> {
        let _ = targets;
        Ok(())
    }

    // ----- scheduled actions ---------------------------------------------

    /// All schedules for a server.
    async fn list_schedules(&self, server_id: &str) -> Result<Vec<Schedule>> {
        let _ = server_id;
        Ok(Vec::new())
    }

    /// Create or replace a schedule (matched by `schedule.id`).
    async fn upsert_schedule(&self, schedule: Schedule) -> Result<()> {
        let _ = schedule;
        Err(BackendError::Other(
            "schedules are not supported on this backend".into(),
        ))
    }

    /// Delete a schedule by id.
    async fn delete_schedule(&self, id: &str) -> Result<()> {
        let _ = id;
        Err(BackendError::Other(
            "schedules are not supported on this backend".into(),
        ))
    }

    // ----- metrics history -----------------------------------------------

    /// Sampled metrics since `since_ms`, oldest first.
    async fn query_metrics(&self, server_id: &str, since_ms: i64) -> Result<Vec<MetricPoint>> {
        let _ = (server_id, since_ms);
        Ok(Vec::new())
    }

    // ----- player administration -----------------------------------------

    /// Players currently online. Empty if the game/server can't report them.
    async fn list_players(&self, server_id: &str) -> Result<Vec<Player>> {
        let _ = server_id;
        Ok(Vec::new())
    }

    /// Apply a moderation action (kick/ban/op/…) to a player.
    async fn player_action(&self, server_id: &str, action: PlayerAction) -> Result<()> {
        let _ = (server_id, action);
        Err(BackendError::Other(
            "player administration is not supported on this backend".into(),
        ))
    }
}
