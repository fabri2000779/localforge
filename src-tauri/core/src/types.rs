//! Pure data types shared between the desktop app and the agent.
//!
//! Anything serialisable that crosses a process boundary (Tauri IPC, agent
//! REST API, settings on disk) lives here.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Game types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(transparent)]
pub struct GameType(pub String);

impl GameType {
    pub fn new(id: &str) -> Self {
        Self(id.to_string())
    }
}

impl std::fmt::Display for GameType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for GameType {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameConfig {
    pub game_type: GameType,
    pub name: String,
    pub description: String,
    pub docker_image: String,
    pub startup: String,
    pub stop_command: String,
    pub variables: Vec<Variable>,
    pub ports: Vec<PortConfig>,
    pub volume_path: String,
    pub min_ram_mb: u32,
    pub recommended_ram_mb: u32,
    pub icon: String,
    #[serde(default)]
    pub logo_url: Option<String>,
    #[serde(default)]
    pub install_script: Option<String>,
    #[serde(default)]
    pub install_image: Option<String>,
    #[serde(default)]
    pub config_files: Vec<ConfigFile>,
    #[serde(default)]
    pub is_custom: bool,
    #[serde(default = "default_console")]
    pub console: bool,
}

fn default_console() -> bool {
    true
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            game_type: GameType::new("custom"),
            name: "Custom Game".to_string(),
            description: "A custom game server".to_string(),
            docker_image: String::new(),
            startup: String::new(),
            stop_command: String::new(),
            variables: Vec::new(),
            ports: Vec::new(),
            volume_path: "/data".to_string(),
            min_ram_mb: 512,
            recommended_ram_mb: 2048,
            icon: "🎮".to_string(),
            logo_url: None,
            install_script: None,
            install_image: None,
            config_files: Vec::new(),
            is_custom: true,
            console: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Variable {
    pub env: String,
    pub name: String,
    pub description: String,
    pub default: String,
    #[serde(default)]
    pub system_mapping: Option<SystemMapping>,
    #[serde(default)]
    pub user_editable: bool,
    #[serde(default)]
    pub options: Option<Vec<SelectOption>>,
    #[serde(default)]
    pub field_type: FieldType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SystemMapping {
    #[default]
    None,
    Ram,
    Port,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    #[default]
    Text,
    Number,
    Password,
    Select,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortConfig {
    pub container_port: u16,
    pub protocol: PortProtocol,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub env_var: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PortProtocol {
    Tcp,
    Udp,
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigFile {
    pub path: String,
    pub format: ConfigFileFormat,
    pub variables: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ConfigFileFormat {
    Json,
    Yaml,
    Properties,
    Ini,
}

// ---------------------------------------------------------------------------
// Server types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Server {
    pub id: String,
    pub name: String,
    pub game_type: GameType,
    pub status: ServerStatus,
    pub container_id: Option<String>,
    pub port: u16,
    pub memory_mb: u32,
    pub data_path: PathBuf,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub config: HashMap<String, String>,
    #[serde(default)]
    pub installed: bool,
    #[serde(default)]
    pub install_container_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ServerStatus {
    Stopped,
    Starting,
    Installing,
    Running,
    Stopping,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateServerRequest {
    pub name: String,
    pub game_type: GameType,
    pub port: Option<u16>,
    pub config: Option<HashMap<String, String>>,
    pub memory_mb: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerResponse {
    pub success: bool,
    pub server: Option<Server>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogsResponse {
    pub logs: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEvent {
    pub server_id: String,
    pub line: String,
}

// ---------------------------------------------------------------------------
// Docker / node telemetry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerStatus {
    pub available: bool,
    pub running: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerInfo {
    pub version: String,
    pub api_version: String,
    pub os: String,
    pub arch: String,
    pub containers_running: u64,
    pub containers_total: u64,
    pub images: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerStats {
    pub cpu_percent: f64,
    pub memory_usage_mb: f64,
    pub memory_limit_mb: f64,
    pub memory_percent: f64,
    /// Cumulative bytes received/sent across the container's interfaces.
    /// `#[serde(default)]` so an older agent's response (no net fields) still
    /// deserializes. The metrics sampler stores these; the chart derives a rate
    /// from the delta between samples. (Kept snake_case like the rest of this
    /// struct — the live-stats UI already reads it.)
    #[serde(default)]
    pub net_rx_bytes: u64,
    #[serde(default)]
    pub net_tx_bytes: u64,
}

/// One sampled point of a server's metrics history (stored host-side as
/// append-only JSONL under `~/LocalForge/metrics/`, never synced to the cloud).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricPoint {
    /// Unix ms.
    pub ts: i64,
    pub cpu_percent: f64,
    pub memory_mb: f64,
    pub net_rx_bytes: u64,
    pub net_tx_bytes: u64,
}

/// A player currently connected to a server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    pub name: String,
    /// Stable id when the game reports one (e.g. Minecraft UUID, Steam ID).
    /// Many games' console `list` output gives only names, so this is optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// A moderation action against a player. Internally tagged so it travels as
/// `{ "kind": "kick", "name": "...", "reason": "..." }` over the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PlayerAction {
    Kick {
        name: String,
        #[serde(default)]
        reason: Option<String>,
    },
    Ban {
        name: String,
        #[serde(default)]
        reason: Option<String>,
    },
    Unban {
        name: String,
    },
    Op {
        name: String,
    },
    Deop {
        name: String,
    },
}

/// Host-level metrics for a node — surfaces the underlying VPS health
/// (CPU/RAM/disk pressure) rather than just Docker's view of itself.
/// Disk numbers correspond to the filesystem that holds the configured
/// data_root.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStats {
    pub cpu_percent: f32,
    pub cpu_count: u32,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub swap_total_bytes: u64,
    pub disk_used_bytes: u64,
    pub disk_total_bytes: u64,
    pub uptime_secs: u64,
    /// 1-minute load average. `None` on Windows where load-avg isn't a
    /// thing the kernel exposes.
    pub load_avg_1m: Option<f64>,
}

// ---------------------------------------------------------------------------
// Backups (bring-your-own S3-compatible bucket)
// ---------------------------------------------------------------------------

/// A user-supplied S3-compatible backup destination (R2, B2, Wasabi, MinIO,
/// DO Spaces…). `secret_key` is sensitive — it lives in the OS keychain
/// (desktop) / a 0600 file (agent) and is NEVER returned to the frontend in a
/// listing (see [`BackupTargetView`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupTarget {
    /// S3 endpoint URL, e.g. `https://<acct>.r2.cloudflarestorage.com`. Empty
    /// string = AWS S3 default for the region.
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key: String,
    pub secret_key: String,
    /// Path-style addressing (MinIO + many S3-compat) vs virtual-hosted (AWS).
    #[serde(default)]
    pub path_style: bool,
}

impl BackupTarget {
    /// Redacted view safe to hand to the UI (drops the secret key).
    pub fn view(&self) -> BackupTargetView {
        BackupTargetView {
            endpoint: self.endpoint.clone(),
            region: self.region.clone(),
            bucket: self.bucket.clone(),
            access_key: self.access_key.clone(),
            path_style: self.path_style,
        }
    }
}

/// Non-secret projection of a [`BackupTarget`] for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupTargetView {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key: String,
    pub path_style: bool,
}

/// A named S3 backup destination belonging to an org. `id` is a stable
/// client-generated UUID; `name` is the user's label ("Production S3", …).
/// The org stores a list of these; any device in the org decrypts + caches
/// them with the org DEK, and relay backup cmds reference one by `targetId`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgBackupTarget {
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    pub credentials: BackupTarget,
}

impl OrgBackupTarget {
    pub fn view(&self) -> OrgBackupTargetView {
        OrgBackupTargetView {
            id: self.id.clone(),
            name: self.name.clone(),
            credentials: self.credentials.view(),
        }
    }
}

/// Non-secret projection of an [`OrgBackupTarget`] for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgBackupTargetView {
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    pub credentials: BackupTargetView,
}

// ---------------------------------------------------------------------------
// Scheduled actions
// ---------------------------------------------------------------------------

/// A cron-scheduled action against a server. Stored host-side
/// (`~/LocalForge/schedules.json`) and fired by the host's scheduler loop, so
/// it works on the desktop (while open) and 24/7 on an agent (headless).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    pub id: String,
    pub server_id: String,
    /// Standard 5-field cron expression (`min hour dom month dow`), evaluated
    /// in the host's LOCAL time.
    pub cron: String,
    pub action: ScheduleAction,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Unix ms of the last time this fired (None = never).
    #[serde(default)]
    pub last_run: Option<i64>,
}

fn default_true() -> bool {
    true
}

/// What a [`Schedule`] does when it fires.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ScheduleAction {
    /// Gracefully stop then start the server.
    Restart,
    /// Send a raw console command to the server.
    Command { command: String },
    /// Announce a message in-game (Minecraft `say`; other games vary).
    Broadcast { message: String },
}

/// One backup object found in the bucket (returned by `list_backups`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    /// Full S3 object key, e.g. `localforge/<serverId>/2026-05-29T10-00-00Z.tar.gz`.
    pub key: String,
    /// Object size in bytes.
    pub size: u64,
    /// Unix ms of the object's last-modified time (≈ when the backup ran).
    pub created_at: i64,
}

// ---------------------------------------------------------------------------
// File manager types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<u64>,
    pub extension: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryContents {
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<FileEntry>,
}

// ---------------------------------------------------------------------------
// Install pipeline events
// ---------------------------------------------------------------------------

/// Streamed event emitted while a server's install script runs. The
/// agent serialises these over its install WebSocket and the desktop's
/// run_install Tauri command fans them out: `Log` lines become
/// `server-log` Tauri events, `OAuthUrl` opens the user's local
/// browser, and `Done` signals end-of-install with the script's exit
/// code.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InstallEvent {
    Log {
        line: String,
    },
    OauthUrl {
        url: String,
    },
    Done {
        exit_code: i64,
    },
}
