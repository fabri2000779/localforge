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
    BackupEntry, BackupTarget, ContainerStats, CreateServerRequest, DirectoryContents, DockerInfo,
    FileEntry, GameConfig, InstallEvent, MetricPoint, NodeStats, OrgBackupTarget, Player,
    PlayerAction, Schedule, Server, ServerStatus,
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

/// Boxed install-event stream. Each item is an [`InstallEvent`] yielded
/// while the install script runs; the stream ends with a `Done` event
/// (carrying the exit code) or an `Err` if the underlying transport
/// died.
pub type InstallStream = BoxStream<'static, Result<InstallEvent>>;

/// Stream of file chunks for upload/download. Uses [`bytes::Bytes`] for
/// zero-copy chunk passing — chunks are typically 8-64 KB depending on
/// who feeds the stream.
pub type ByteStream = BoxStream<'static, Result<bytes::Bytes>>;

/// Detect an OAuth-style URL embedded in an install-script log line.
/// Matches whitespace-separated tokens that start with `https://` and
/// contain one of the common auth keywords. Returns the first match
/// stripped of surrounding quotes/brackets.
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

    /// Host-level metrics (CPU%, mem, disk, uptime) for the node.
    /// Implementations cache a sysinfo snapshot in the background so
    /// readings are accurate without per-call delays.
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

    /// Run the game's install script in a one-shot container, streaming
    /// progress events. Game metadata (script, image, volume path) is
    /// supplied by the caller because the agent doesn't keep a copy of
    /// the desktop's game catalogue.
    async fn run_install(&self, id: &str, game: GameConfig) -> Result<InstallStream>;

    /// Stop the server (if running), wipe its on-disk data dir, and
    /// mark it as not-installed so the next start re-runs the install
    /// script. The persisted [`Server`] record is preserved.
    async fn reset_server_data(&self, id: &str) -> Result<()>;

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

    /// Stream a file's contents in chunks. Used by the desktop's
    /// "Download" action to pull large worlds from a remote node
    /// without loading the whole file into memory.
    async fn download_file(&self, path: &str) -> Result<ByteStream>;

    /// Stream a file's contents into `path`, replacing it if it exists.
    /// Counterpart to [`download_file`].
    async fn upload_file(&self, path: &str, body: ByteStream) -> Result<()>;

    // ----- backups (bring-your-own S3) -----------------------------------
    //
    // A backup archives the server's data dir and uploads it to the
    // user-supplied S3-compatible bucket; restore pulls it back. These run ON
    // THE HOST (local Docker or the agent) — the cloud never touches S3.
    //
    // Default impls return "unsupported" so a backend that hasn't wired this
    // yet (the remote client until the agent route lands) degrades gracefully
    // instead of failing to compile.

    /// Archive the server's data dir (tar + gzip) and upload it to `target`.
    /// Returns the object key written.
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

    /// Download `key` and restore it over the server's data dir. The server is
    /// stopped first and the previous data is moved aside before extraction.
    async fn restore_backup(&self, id: &str, target: &BackupTarget, key: &str) -> Result<()> {
        let _ = (id, target, key);
        Err(BackendError::Other(
            "backups are not supported on this backend".into(),
        ))
    }

    /// Delete a backup object from the bucket.
    async fn delete_backup(&self, target: &BackupTarget, key: &str) -> Result<()> {
        let _ = (target, key);
        Err(BackendError::Other(
            "backups are not supported on this backend".into(),
        ))
    }

    /// Provision the full org backup-target list onto this node so it can run
    /// relay-triggered backups by itself (e.g. when the mobile app fires one
    /// and the owner's desktop is offline). Default is a no-op: the local
    /// backend resolves credentials from the keychain directly. Only the
    /// remote-agent client overrides this — pushes over direct HTTPS, never
    /// the relay, so the secret never transits Cloudflare.
    async fn set_backup_targets(&self, targets: &[OrgBackupTarget]) -> Result<()> {
        let _ = targets;
        Ok(())
    }

    // ----- scheduled actions ---------------------------------------------
    //
    // Schedules are stored host-side and fired by the host's scheduler loop.
    // These CRUD the persisted defs; execution is automatic. Default impls keep
    // a backend that hasn't wired this (the remote client until its route
    // lands) compiling.

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

    /// Sampled metrics for a server since `since_ms` (unix ms), oldest first.
    /// Read from the host's local store; the cloud never sees this.
    async fn query_metrics(&self, server_id: &str, since_ms: i64) -> Result<Vec<MetricPoint>> {
        let _ = (server_id, since_ms);
        Ok(Vec::new())
    }

    // ----- player administration -----------------------------------------
    //
    // Live player list + moderation (kick/ban/op). Per-game: only servers whose
    // game exposes a console/RCON/REST admin surface support this; everything
    // else falls through to the defaults (empty list / unsupported action). The
    // host talks to the running container directly — the cloud is never involved.

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
