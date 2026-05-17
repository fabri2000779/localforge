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
}

// ---------------------------------------------------------------------------
// File manager types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileEntryKind {
    File,
    Dir,
    Symlink,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub kind: FileEntryKind,
    pub size: u64,
    pub modified: Option<chrono::DateTime<chrono::Utc>>,
}
