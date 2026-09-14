//! Serialisable types shared by the desktop app and the agent.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

// Game types

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

// Server types

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
    /// Crash-watcher behaviour on an unexpected container exit (defaults to `OnCrash`).
    #[serde(default)]
    pub restart_policy: RestartPolicy,
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
    /// Exited unexpectedly while believed running (set by the crash-watcher).
    Crashed,
}

/// Crash-watcher behaviour when a `Running` server's container exits unexpectedly.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum RestartPolicy {
    /// Detect + record the crash, but leave the server stopped.
    Off,
    /// Auto-restart on an unexpected exit, with crash-loop backoff (default).
    #[default]
    OnCrash,
    /// Bring it back up after any unexpected exit, with the same backoff.
    Always,
}

/// Crash-journal entry; metadata only, safe to show and notify on.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashEvent {
    /// Unix ms when the host noticed the unexpected exit.
    pub ts: i64,
    pub server_id: String,
    pub server_name: String,
    pub kind: CrashEventKind,
    /// Last few log lines before the exit, for quick diagnosis (best-effort).
    #[serde(default)]
    pub log_tail: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CrashEventKind {
    /// The container exited unexpectedly.
    Crashed,
    /// We auto-restarted it per the restart policy.
    Restarted,
    /// Crash-loop backoff tripped — we stopped auto-restarting.
    Backoff,
}

/// Alert destination; the host POSTs directly, so the webhook URL never reaches the cloud.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WebhookKind {
    /// Discord incoming webhook (`{ content, embeds }`).
    Discord,
    /// Slack incoming webhook (`{ text }`).
    Slack,
    /// Any endpoint — receives the raw event JSON.
    Generic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookConfig {
    pub id: String,
    pub name: String,
    pub kind: WebhookKind,
    /// The full endpoint URL (secret — carries the Discord/Slack token).
    pub url: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl WebhookConfig {
    /// Non-secret projection for the frontend (URL masked).
    pub fn view(&self) -> WebhookConfigView {
        WebhookConfigView {
            id: self.id.clone(),
            name: self.name.clone(),
            kind: self.kind,
            url_hint: mask_url(&self.url),
            enabled: self.enabled,
        }
    }
}

/// Redacted [`WebhookConfig`] for display (no secret URL).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookConfigView {
    pub id: String,
    pub name: String,
    pub kind: WebhookKind,
    /// Masked URL, e.g. `https://discord.com/api/webhooks/…1a2b`.
    pub url_hint: String,
    pub enabled: bool,
}

/// Mask a webhook URL: keep the scheme+host and the last 4 chars, hide the rest.
fn mask_url(url: &str) -> String {
    let scheme_host: String = url.split('/').take(3).collect::<Vec<_>>().join("/");
    let tail: String = url.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    format!("{scheme_host}/…{tail}")
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

// Docker / node telemetry

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
    /// Cumulative bytes; `default` so older agents (no net fields) still deserialize.
    #[serde(default)]
    pub net_rx_bytes: u64,
    #[serde(default)]
    pub net_tx_bytes: u64,
}

/// One sample of a server's metrics history (host-side JSONL, never synced).
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
    /// Stable id when the game reports one; console `list` output often has only names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// Moderation action, tagged as `{ "kind": "kick", "name", "reason" }` on the wire.
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

/// Host-level metrics; disk numbers refer to the filesystem holding data_root.
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
    /// 1-minute load average (`None` on Windows).
    pub load_avg_1m: Option<f64>,
}

// Backups (bring-your-own S3-compatible bucket)

/// User-supplied S3-compatible destination; `secret_key` never reaches the frontend (see [`BackupTargetView`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupTarget {
    /// Endpoint URL; empty means the AWS S3 default for the region.
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

/// Named org backup destination; `id` is a stable client UUID that relay backup cmds reference.
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

// Scheduled actions

/// Cron-scheduled action, stored and fired host-side (desktop while open, agent 24/7).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    pub id: String,
    pub server_id: String,
    /// 5-field cron expression evaluated in the host's local time.
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
    /// Back up to the named target (`None` = first). Retention: the `keep_last` newest are always
    /// kept; `max_age_days` prunes older objects beyond them.
    Backup {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keep_last: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_age_days: Option<u32>,
    },
}

/// One backup object found in the bucket (returned by `list_backups`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    /// Full S3 object key, e.g. `localforge/<serverId>/2026-05-29T10-00-00Z.tar.gz`.
    pub key: String,
    pub size: u64,
    /// Unix ms of the object's last-modified time (≈ when the backup ran).
    pub created_at: i64,
}

// File manager types

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

// Install pipeline events

/// Event streamed while an install script runs: a log line, an OAuth URL to open, or `Done` with the exit code.
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
