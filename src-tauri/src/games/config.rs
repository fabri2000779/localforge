use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(transparent)]
pub struct GameType(pub String);

impl GameType {
    pub fn new(id: &str) -> Self {
        Self(id.to_string())
    }
}

impl fmt::Display for GameType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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

/// Configuration for modifying config files with variable substitution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigFile {
    /// File path relative to volume (e.g., "config.json")
    pub path: String,
    /// File format: "json", "yaml", "properties" (key=value)
    pub format: ConfigFileFormat,
    /// Variable mappings: config_key -> {{ENV_VAR}}
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PortProtocol {
    Tcp,
    Udp,
    Both,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            game_type: GameType::new("custom"),
            name: "Custom Game".to_string(),
            description: "A custom game server".to_string(),
            docker_image: "".to_string(),
            startup: "".to_string(),
            stop_command: "".to_string(),
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

// Resolve startup command by replacing {{VAR}} placeholders
#[allow(dead_code)]
pub fn resolve_startup(startup: &str, variables: &HashMap<String, String>) -> String {
    let mut result = startup.to_string();
    for (key, value) in variables {
        result = result.replace(&format!("{{{{{}}}}}", key), value);
    }
    result
}

// Build environment variables from game variables and server settings
pub fn build_env_vars(
    game: &GameConfig,
    ram_mb: u32,
    port: u16,
    user_overrides: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut env = HashMap::new();
    
    for var in &game.variables {
        let value = match &var.system_mapping {
            Some(SystemMapping::Ram) => format_ram(ram_mb, &var.default),
            Some(SystemMapping::Port) => port.to_string(),
            Some(SystemMapping::None) | None => {
                user_overrides.get(&var.env)
                    .cloned()
                    .unwrap_or_else(|| var.default.clone())
            }
        };
        env.insert(var.env.clone(), value);
    }
    
    env
}

// Format RAM based on the default format (e.g., "2G" -> "4G", "1024" -> "4096")
fn format_ram(ram_mb: u32, default_format: &str) -> String {
    if default_format.ends_with('G') || default_format.ends_with('g') {
        format!("{}G", ram_mb / 1024)
    } else if default_format.ends_with('M') || default_format.ends_with('m') {
        format!("{}M", ram_mb)
    } else {
        ram_mb.to_string()
    }
}

pub fn get_builtin_games() -> Vec<GameConfig> {
    vec![
        GameConfig {
            game_type: GameType::new("minecraft-java"),
            name: "Minecraft Java".to_string(),
            description: "The original Minecraft experience powered by Paper, a high performance Spigot fork.".to_string(),
            docker_image: "ghcr.io/serverwavehost/game-images:java_21".to_string(),
            startup: "java -Dcom.mojang.eula.agree=true -Xms128M -Xmx{{SERVER_MEMORY}}M -Dterminal.jline=false -Dterminal.ansi=true -jar {{SERVER_JARFILE}}".to_string(),
            stop_command: "stop".to_string(),
            variables: vec![
                Variable {
                    env: "SERVER_MEMORY".to_string(),
                    name: "Memory".to_string(),
                    description: "RAM allocation in MB".to_string(),
                    default: "2048".to_string(),
                    system_mapping: Some(SystemMapping::Ram),
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "SERVER_JARFILE".to_string(),
                    name: "Server JAR File".to_string(),
                    description: "The name of the server jarfile".to_string(),
                    default: "server.jar".to_string(),
                    system_mapping: None,
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "MINECRAFT_VERSION".to_string(),
                    name: "Minecraft Version".to_string(),
                    description: "The version of Minecraft. Leave at latest for newest version.".to_string(),
                    default: "latest".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "BUILD_NUMBER".to_string(),
                    name: "Build Number".to_string(),
                    description: "The build number for Paper. Leave at latest for newest build.".to_string(),
                    default: "latest".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "MC_DIFFICULTY".to_string(),
                    name: "Difficulty".to_string(),
                    description: "Game difficulty level".to_string(),
                    default: "normal".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: Some(vec![
                        SelectOption { value: "peaceful".to_string(), label: "Peaceful".to_string() },
                        SelectOption { value: "easy".to_string(), label: "Easy".to_string() },
                        SelectOption { value: "normal".to_string(), label: "Normal".to_string() },
                        SelectOption { value: "hard".to_string(), label: "Hard".to_string() },
                    ]),
                    field_type: FieldType::Select,
                },
                Variable {
                    env: "MC_GAMEMODE".to_string(),
                    name: "Game Mode".to_string(),
                    description: "Default game mode".to_string(),
                    default: "survival".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: Some(vec![
                        SelectOption { value: "survival".to_string(), label: "Survival".to_string() },
                        SelectOption { value: "creative".to_string(), label: "Creative".to_string() },
                        SelectOption { value: "adventure".to_string(), label: "Adventure".to_string() },
                        SelectOption { value: "spectator".to_string(), label: "Spectator".to_string() },
                    ]),
                    field_type: FieldType::Select,
                },
                Variable {
                    env: "MC_MAXPLAYERS".to_string(),
                    name: "Max Players".to_string(),
                    description: "Maximum number of players".to_string(),
                    default: "20".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "MC_ONLINEMODE".to_string(),
                    name: "Online Mode".to_string(),
                    description: "Verify players with Minecraft account database".to_string(),
                    default: "true".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: Some(vec![
                        SelectOption { value: "true".to_string(), label: "Yes (Recommended)".to_string() },
                        SelectOption { value: "false".to_string(), label: "No (Cracked)".to_string() },
                    ]),
                    field_type: FieldType::Select,
                },
                Variable {
                    env: "MC_WHITELIST".to_string(),
                    name: "Whitelist".to_string(),
                    description: "Enable whitelist for private servers".to_string(),
                    default: "false".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: Some(vec![
                        SelectOption { value: "true".to_string(), label: "Enabled".to_string() },
                        SelectOption { value: "false".to_string(), label: "Disabled".to_string() },
                    ]),
                    field_type: FieldType::Select,
                },
                Variable {
                    env: "MC_FLIGHT".to_string(),
                    name: "Allow Flight".to_string(),
                    description: "Allow flight in Survival mode".to_string(),
                    default: "false".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: Some(vec![
                        SelectOption { value: "true".to_string(), label: "Allowed".to_string() },
                        SelectOption { value: "false".to_string(), label: "Not Allowed".to_string() },
                    ]),
                    field_type: FieldType::Select,
                },
            ],
            ports: vec![
                PortConfig { container_port: 25565, protocol: PortProtocol::Both, description: Some("Game port".to_string()), env_var: None },
            ],
            volume_path: "/mnt/server".to_string(),
            min_ram_mb: 1024,
            recommended_ram_mb: 4096,
            icon: "🟫".to_string(),
            logo_url: Some("https://img.icons8.com/color/96/minecraft-grass-cube.png".to_string()),
            install_script: Some(r#"#!/bin/sh
# Paper Installation Script
# Using official Alpine with curl and jq
set -e

echo "[Serverwave] Installing required tools..."
apk add --no-cache curl jq

PROJECT=paper
SERVER_JARFILE="${SERVER_JARFILE:-server.jar}"
MINECRAFT_VERSION="${MINECRAFT_VERSION:-latest}"
BUILD_NUMBER="${BUILD_NUMBER:-latest}"

echo "[Serverwave] Starting Paper installation..."

# Get latest version if needed
if [ "$MINECRAFT_VERSION" = "latest" ]; then
    echo "[Serverwave] Fetching latest Minecraft version..."
    MINECRAFT_VERSION=$(curl -s https://api.papermc.io/v2/projects/${PROJECT} | jq -r '.versions[-1]')
    echo "[Serverwave] Latest version: ${MINECRAFT_VERSION}"
else
    # Verify version exists
    VER_EXISTS=$(curl -s https://api.papermc.io/v2/projects/${PROJECT} | jq -r --arg VERSION "$MINECRAFT_VERSION" '.versions[] | select(. == $VERSION)')
    if [ -z "$VER_EXISTS" ]; then
        echo "[Serverwave] Version ${MINECRAFT_VERSION} not found, using latest..."
        MINECRAFT_VERSION=$(curl -s https://api.papermc.io/v2/projects/${PROJECT} | jq -r '.versions[-1]')
    fi
    echo "[Serverwave] Using version: ${MINECRAFT_VERSION}"
fi

# Get latest build if needed
if [ "$BUILD_NUMBER" = "latest" ]; then
    echo "[Serverwave] Fetching latest build for ${MINECRAFT_VERSION}..."
    BUILD_NUMBER=$(curl -s https://api.papermc.io/v2/projects/${PROJECT}/versions/${MINECRAFT_VERSION} | jq -r '.builds[-1]')
    echo "[Serverwave] Latest build: ${BUILD_NUMBER}"
else
    # Verify build exists
    BUILD_EXISTS=$(curl -s https://api.papermc.io/v2/projects/${PROJECT}/versions/${MINECRAFT_VERSION} | jq -r --arg BUILD "$BUILD_NUMBER" '.builds[] | select(. == ($BUILD | tonumber))')
    if [ -z "$BUILD_EXISTS" ]; then
        echo "[Serverwave] Build ${BUILD_NUMBER} not found, using latest..."
        BUILD_NUMBER=$(curl -s https://api.papermc.io/v2/projects/${PROJECT}/versions/${MINECRAFT_VERSION} | jq -r '.builds[-1]')
    fi
    echo "[Serverwave] Using build: ${BUILD_NUMBER}"
fi

JAR_NAME=${PROJECT}-${MINECRAFT_VERSION}-${BUILD_NUMBER}.jar
DOWNLOAD_URL="https://api.papermc.io/v2/projects/${PROJECT}/versions/${MINECRAFT_VERSION}/builds/${BUILD_NUMBER}/downloads/${JAR_NAME}"

echo "[Serverwave] Download details:"
echo "  MC Version: ${MINECRAFT_VERSION}"
echo "  Build: ${BUILD_NUMBER}"
echo "  JAR: ${JAR_NAME}"
echo "  URL: ${DOWNLOAD_URL}"

# Backup old jar if exists
if [ -f "${SERVER_JARFILE}" ]; then
    echo "[Serverwave] Backing up existing ${SERVER_JARFILE}..."
    mv "${SERVER_JARFILE}" "${SERVER_JARFILE}.old"
fi

# Download the jar
echo "[Serverwave] Downloading Paper..."
curl -L --progress-bar -o "${SERVER_JARFILE}" "${DOWNLOAD_URL}"

# Download server.properties if it doesn't exist
if [ ! -f server.properties ]; then
    echo "[Serverwave] Creating default server.properties..."
    cat > server.properties << 'EOF'
#Minecraft server properties
enable-jmx-monitoring=false
rcon.port=25575
level-seed=
gamemode=survival
enable-command-block=false
enable-query=true
generator-settings={}
enforce-secure-profile=true
level-name=world
motd=A Serverwave Anywhere Server
query.port=25565
pvp=true
generate-structures=true
max-chained-neighbor-updates=1000000
difficulty=normal
network-compression-threshold=256
max-tick-time=60000
require-resource-pack=false
use-native-transport=true
max-players=20
online-mode=true
enable-status=true
allow-flight=false
initial-disabled-packs=
broadcast-rcon-to-ops=true
view-distance=10
server-ip=
resource-pack-prompt=
allow-nether=true
server-port=25565
enable-rcon=false
sync-chunk-writes=true
op-permission-level=4
prevent-proxy-connections=false
hide-online-players=false
resource-pack=
entity-broadcast-range-percentage=100
simulation-distance=10
rcon.password=
player-idle-timeout=0
force-gamemode=false
rate-limit=0
hardcore=false
white-list=false
broadcast-console-to-ops=true
spawn-npcs=true
spawn-animals=true
log-ips=true
function-permission-level=2
initial-enabled-packs=vanilla
level-type=minecraft\:normal
text-filtering-config=
spawn-monsters=true
enforce-whitelist=false
spawn-protection=16
resource-pack-sha1=
max-world-size=29999984
EOF
fi

# Accept EULA
echo "[Serverwave] Accepting EULA..."
echo "eula=true" > eula.txt

echo "[Serverwave] Paper ${MINECRAFT_VERSION} build ${BUILD_NUMBER} installed successfully!"
"#.to_string()),
            install_image: Some("alpine:latest".to_string()),
            config_files: vec![
                ConfigFile {
                    path: "server.properties".to_string(),
                    format: ConfigFileFormat::Properties,
                    variables: {
                        let mut m = HashMap::new();
                        m.insert("difficulty".to_string(), "{{MC_DIFFICULTY}}".to_string());
                        m.insert("gamemode".to_string(), "{{MC_GAMEMODE}}".to_string());
                        m.insert("max-players".to_string(), "{{MC_MAXPLAYERS}}".to_string());
                        m.insert("online-mode".to_string(), "{{MC_ONLINEMODE}}".to_string());
                        m.insert("white-list".to_string(), "{{MC_WHITELIST}}".to_string());
                        m.insert("allow-flight".to_string(), "{{MC_FLIGHT}}".to_string());
                        m
                    },
                },
            ],
            is_custom: false,
            console: true,
        },

        GameConfig {
            game_type: GameType::new("sons-of-the-forest"),
            name: "Sons of the Forest".to_string(),
            description: "Survival horror game. Survive on a remote island with mutants.".to_string(),
            docker_image: "ghcr.io/serverwavehost/game-images:wine_latest".to_string(),
            startup: "wine ./SonsOfTheForestDS.exe -userdatapath \"/home/container/serverconfig\" -dedicatedserver.IpAddress \"0.0.0.0\" -dedicatedserver.GamePort \"{{SERVER_PORT}}\" -dedicatedserver.QueryPort \"{{QUERY_PORT}}\" -dedicatedserver.BlobSyncPort \"{{SYNC_PORT}}\" -dedicatedserver.SkipNetworkAccessibilityTest \"{{SKIP_TESTS}}\"".to_string(),
            stop_command: "^C".to_string(),
            variables: vec![
                Variable {
                    env: "SRCDS_APPID".to_string(),
                    name: "Steam App ID".to_string(),
                    description: "Steam App ID for Sons of the Forest dedicated server".to_string(),
                    default: "2465200".to_string(),
                    system_mapping: None,
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "SERVER_PORT".to_string(),
                    name: "Game Port".to_string(),
                    description: "Main game port".to_string(),
                    default: "8766".to_string(),
                    system_mapping: Some(SystemMapping::Port),
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "QUERY_PORT".to_string(),
                    name: "Query Port".to_string(),
                    description: "Query port".to_string(),
                    default: "27016".to_string(),
                    system_mapping: None,
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "SYNC_PORT".to_string(),
                    name: "Blob Sync Port".to_string(),
                    description: "Blob sync port".to_string(),
                    default: "9700".to_string(),
                    system_mapping: None,
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "MAX_PLAYERS".to_string(),
                    name: "Max Players".to_string(),
                    description: "Maximum number of players".to_string(),
                    default: "8".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "SRV_NAME".to_string(),
                    name: "Server Name".to_string(),
                    description: "Name shown in server browser".to_string(),
                    default: "A SOTF server hosted by Serverwave".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "SRV_PW".to_string(),
                    name: "Server Password".to_string(),
                    description: "Password to join the server (leave empty for no password)".to_string(),
                    default: "".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Password,
                },
                Variable {
                    env: "GAME_MODE".to_string(),
                    name: "Game Mode".to_string(),
                    description: "Difficulty game mode for new saves".to_string(),
                    default: "Normal".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: Some(vec![
                        SelectOption { value: "Normal".to_string(), label: "Normal".to_string() },
                        SelectOption { value: "Hard".to_string(), label: "Hard".to_string() },
                        SelectOption { value: "HardSurvival".to_string(), label: "Hard Survival".to_string() },
                        SelectOption { value: "Peaceful".to_string(), label: "Peaceful".to_string() },
                        SelectOption { value: "Custom".to_string(), label: "Custom".to_string() },
                    ]),
                    field_type: FieldType::Select,
                },
                Variable {
                    env: "SAVE_SLOT".to_string(),
                    name: "Save Slot".to_string(),
                    description: "Save slot number (1-30)".to_string(),
                    default: "1".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "SKIP_TESTS".to_string(),
                    name: "Skip Network Test".to_string(),
                    description: "Skip network accessibility test (set to true if having connection issues)".to_string(),
                    default: "true".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: Some(vec![
                        SelectOption { value: "true".to_string(), label: "Yes".to_string() },
                        SelectOption { value: "false".to_string(), label: "No".to_string() },
                    ]),
                    field_type: FieldType::Select,
                },
                Variable {
                    env: "WINEDEBUG".to_string(),
                    name: "Wine Debug".to_string(),
                    description: "Wine debug mode".to_string(),
                    default: "-all".to_string(),
                    system_mapping: None,
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "WINEARCH".to_string(),
                    name: "Wine Architecture".to_string(),
                    description: "Wine architecture".to_string(),
                    default: "win64".to_string(),
                    system_mapping: None,
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "WINEPATH".to_string(),
                    name: "Wine Path".to_string(),
                    description: "Wine path".to_string(),
                    default: "/home/container".to_string(),
                    system_mapping: None,
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "WINETRICKS_RUN".to_string(),
                    name: "Winetricks".to_string(),
                    description: "Winetricks to run".to_string(),
                    default: "mono vcrun2019".to_string(),
                    system_mapping: None,
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "WINDOWS_INSTALL".to_string(),
                    name: "Windows Install".to_string(),
                    description: "Use Windows platform for SteamCMD".to_string(),
                    default: "1".to_string(),
                    system_mapping: None,
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "AUTO_UPDATE".to_string(),
                    name: "Auto Update".to_string(),
                    description: "Auto update the server on start".to_string(),
                    default: "1".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: Some(vec![
                        SelectOption { value: "1".to_string(), label: "Enabled".to_string() },
                        SelectOption { value: "0".to_string(), label: "Disabled".to_string() },
                    ]),
                    field_type: FieldType::Select,
                },
            ],
            ports: vec![
                PortConfig {
                    container_port: 8766,
                    protocol: PortProtocol::Both,
                    description: Some("Game port".to_string()),
                    env_var: Some("SERVER_PORT".to_string()),
                },
                PortConfig {
                    container_port: 27016,
                    protocol: PortProtocol::Both,
                    description: Some("Query port".to_string()),
                    env_var: Some("QUERY_PORT".to_string()),
                },
                PortConfig {
                    container_port: 9700,
                    protocol: PortProtocol::Both,
                    description: Some("Blob sync port".to_string()),
                    env_var: Some("SYNC_PORT".to_string()),
                },
            ],
            volume_path: "/home/container".to_string(),
            min_ram_mb: 4096,
            recommended_ram_mb: 8192,
            icon: "🌲".to_string(),
            logo_url: Some("https://cdn.cloudflare.steamstatic.com/steam/apps/1326470/header.jpg".to_string()),
            install_script: Some(r#"#!/bin/sh
# Sons of the Forest SteamCMD Installation Script
export DEBIAN_FRONTEND=noninteractive
apt -y update
apt -y --no-install-recommends install curl lib32gcc-s1 ca-certificates

echo "[Serverwave] Starting Sons of the Forest installation..."

SERVER_PATH=/home/container
SRCDS_APPID=2465200

# Download and setup steamcmd
cd /tmp
mkdir -p "${SERVER_PATH}/steamcmd"
curl -sSL -o steamcmd.tar.gz https://steamcdn-a.akamaihd.net/client/installer/steamcmd_linux.tar.gz
tar -xzvf steamcmd.tar.gz -C "${SERVER_PATH}/steamcmd"
mkdir -p "${SERVER_PATH}/steamapps"
cd "${SERVER_PATH}/steamcmd"

chown -R root:root "${SERVER_PATH}"
export HOME="${SERVER_PATH}"

echo "[Serverwave] Logging into Steam..."
./steamcmd.sh +login anonymous +quit

echo "[Serverwave] Installing Sons of the Forest dedicated server (Windows)..."
./steamcmd.sh +force_install_dir "${SERVER_PATH}" +login anonymous +@sSteamCmdForcePlatformType windows +app_update ${SRCDS_APPID} validate +quit

# Set up Steam libraries
echo "[Serverwave] Setting up Steam libraries..."
mkdir -p "${SERVER_PATH}/.steam/sdk32"
cp -v linux32/steamclient.so ../.steam/sdk32/steamclient.so

mkdir -p "${SERVER_PATH}/.steam/sdk64"
cp -v linux64/steamclient.so ../.steam/sdk64/steamclient.so

# Create serverconfig directory and download default configs
mkdir -p "${SERVER_PATH}/serverconfig"

if [ ! -f "${SERVER_PATH}/serverconfig/dedicatedserver.cfg" ]; then
    echo "[Serverwave] Downloading default dedicatedserver.cfg..."
    cd "${SERVER_PATH}/serverconfig/"
    curl -sSL -o dedicatedserver.cfg https://raw.githubusercontent.com/parkervcp/eggs/master/game_eggs/steamcmd_servers/sonsoftheforest/dedicatedserver.cfg
fi

if [ ! -f "${SERVER_PATH}/serverconfig/ownerswhitelist.txt" ]; then
    echo "[Serverwave] Downloading default ownerswhitelist.txt..."
    cd "${SERVER_PATH}/serverconfig/"
    curl -sSL -o ownerswhitelist.txt https://raw.githubusercontent.com/parkervcp/eggs/master/game_eggs/steamcmd_servers/sonsoftheforest/ownerswhitelist.txt
fi

echo "[Serverwave] Sons of the Forest installed successfully!"
"#.to_string()),
            install_image: Some("debian:bookworm".to_string()),
            config_files: vec![
                ConfigFile {
                    path: "serverconfig/dedicatedserver.cfg".to_string(),
                    format: ConfigFileFormat::Properties,
                    variables: {
                        let mut m = HashMap::new();
                        m.insert("GameMode".to_string(), "{{GAME_MODE}}".to_string());
                        m.insert("MaxPlayers".to_string(), "{{MAX_PLAYERS}}".to_string());
                        m.insert("Password".to_string(), "{{SRV_PW}}".to_string());
                        m.insert("SaveSlot".to_string(), "{{SAVE_SLOT}}".to_string());
                        m.insert("ServerName".to_string(), "{{SRV_NAME}}".to_string());
                        m
                    },
                },
            ],
            is_custom: false,
            console: true,
        },

        GameConfig {
            game_type: GameType::new("rust"),
            name: "Rust".to_string(),
            description: "Survival game. Gather, build, and fight to survive.".to_string(),
            docker_image: "ghcr.io/serverwavehost/game-images:rust_latest".to_string(),
            startup: "./RustDedicated -batchmode +server.port {{SERVER_PORT}} +server.queryport {{SERVER_PORT}} +server.identity \"rust\" +rcon.ip 0.0.0.0 +rcon.port {{RCON_PORT}} +rcon.web true +server.hostname \"{{HOSTNAME}}\" +server.level \"{{LEVEL}}\" +server.description \"{{DESCRIPTION}}\" +server.url \"{{SERVER_URL}}\" +server.headerimage \"{{SERVER_IMG}}\" +server.maxplayers {{MAX_PLAYERS}} +rcon.password \"{{RCON_PASS}}\" +server.saveinterval {{SAVEINTERVAL}} +server.worldsize {{WORLD_SIZE}} +server.seed {{WORLD_SEED}} {{ADDITIONAL_ARGS}}".to_string(),
            stop_command: "quit".to_string(),
            variables: vec![
                Variable {
                    env: "SRCDS_APPID".to_string(),
                    name: "Steam App ID".to_string(),
                    description: "Steam App ID for Rust dedicated server".to_string(),
                    default: "258550".to_string(),
                    system_mapping: None,
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "SERVER_PORT".to_string(),
                    name: "Server Port".to_string(),
                    description: "Game and query port".to_string(),
                    default: "28015".to_string(),
                    system_mapping: Some(SystemMapping::Port),
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "RCON_PORT".to_string(),
                    name: "RCON Port".to_string(),
                    description: "Port for RCON connections".to_string(),
                    default: "28016".to_string(),
                    system_mapping: None,
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "MAX_PLAYERS".to_string(),
                    name: "Max Players".to_string(),
                    description: "Maximum number of players".to_string(),
                    default: "40".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "HOSTNAME".to_string(),
                    name: "Server Name".to_string(),
                    description: "Name shown in server browser".to_string(),
                    default: "A Rust server hosted by Serverwave".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "DESCRIPTION".to_string(),
                    name: "Description".to_string(),
                    description: "Server description (use \\n for newlines)".to_string(),
                    default: "Powered by Serverwave".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "SERVER_URL".to_string(),
                    name: "Website URL".to_string(),
                    description: "URL shown when clicking Visit Website".to_string(),
                    default: "http://serverwave.com".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "SERVER_IMG".to_string(),
                    name: "Header Image".to_string(),
                    description: "Header image URL for server listing".to_string(),
                    default: "".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "LEVEL".to_string(),
                    name: "Map Level".to_string(),
                    description: "The world file for Rust to use".to_string(),
                    default: "Procedural Map".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "WORLD_SIZE".to_string(),
                    name: "World Size".to_string(),
                    description: "World size for procedural maps (3000-6000)".to_string(),
                    default: "3000".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "WORLD_SEED".to_string(),
                    name: "World Seed".to_string(),
                    description: "Seed for procedural maps (0 for random)".to_string(),
                    default: "0".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "RCON_PASS".to_string(),
                    name: "RCON Password".to_string(),
                    description: "Password for RCON access".to_string(),
                    default: "CHANGEME".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Password,
                },
                Variable {
                    env: "SAVEINTERVAL".to_string(),
                    name: "Save Interval".to_string(),
                    description: "Auto-save interval in seconds".to_string(),
                    default: "60".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "FRAMEWORK".to_string(),
                    name: "Modding Framework".to_string(),
                    description: "Modding framework to use".to_string(),
                    default: "vanilla".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: Some(vec![
                        SelectOption { value: "vanilla".to_string(), label: "Vanilla".to_string() },
                        SelectOption { value: "oxide".to_string(), label: "Oxide".to_string() },
                        SelectOption { value: "carbon".to_string(), label: "Carbon".to_string() },
                    ]),
                    field_type: FieldType::Select,
                },
                Variable {
                    env: "ADDITIONAL_ARGS".to_string(),
                    name: "Additional Arguments".to_string(),
                    description: "Additional startup parameters".to_string(),
                    default: "".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "AUTO_UPDATE".to_string(),
                    name: "Auto Update".to_string(),
                    description: "Auto update the server on start".to_string(),
                    default: "1".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: Some(vec![
                        SelectOption { value: "1".to_string(), label: "Enabled".to_string() },
                        SelectOption { value: "0".to_string(), label: "Disabled".to_string() },
                    ]),
                    field_type: FieldType::Select,
                },
            ],
            ports: vec![
                PortConfig {
                    container_port: 28015,
                    protocol: PortProtocol::Both,
                    description: Some("Game port".to_string()),
                    env_var: Some("SERVER_PORT".to_string()),
                },
                PortConfig {
                    container_port: 28016,
                    protocol: PortProtocol::Both,
                    description: Some("RCON port".to_string()),
                    env_var: Some("RCON_PORT".to_string()),
                },
            ],
            volume_path: "/home/container".to_string(),
            min_ram_mb: 8192,
            recommended_ram_mb: 16384,
            icon: "🛢️".to_string(),
            logo_url: Some("https://cdn.cloudflare.steamstatic.com/steam/apps/252490/header.jpg".to_string()),
            install_script: Some(r#"#!/bin/sh
# Rust SteamCMD Installation Script
export DEBIAN_FRONTEND=noninteractive
apt -y update
apt -y --no-install-recommends install curl lib32gcc-s1 ca-certificates

echo "[Serverwave] Starting Rust installation..."

SERVER_PATH=/home/container
SRCDS_APPID=258550

# Download and setup steamcmd
cd /tmp
mkdir -p "${SERVER_PATH}/steamcmd"
curl -sSL -o steamcmd.tar.gz https://steamcdn-a.akamaihd.net/client/installer/steamcmd_linux.tar.gz
tar -xzvf steamcmd.tar.gz -C "${SERVER_PATH}/steamcmd"
mkdir -p "${SERVER_PATH}/steamapps"
cd "${SERVER_PATH}/steamcmd"

chown -R root:root "${SERVER_PATH}"
export HOME="${SERVER_PATH}"

echo "[Serverwave] Logging into Steam..."
./steamcmd.sh +login anonymous +quit

echo "[Serverwave] Installing Rust dedicated server..."
./steamcmd.sh +force_install_dir "${SERVER_PATH}" +login anonymous +app_update ${SRCDS_APPID} validate +quit

# Set up Steam libraries
echo "[Serverwave] Setting up Steam libraries..."
mkdir -p "${SERVER_PATH}/.steam/sdk32"
cp -v linux32/steamclient.so ../.steam/sdk32/steamclient.so

mkdir -p "${SERVER_PATH}/.steam/sdk64"
cp -v linux64/steamclient.so ../.steam/sdk64/steamclient.so

# Generate random seed if needed
if [ ! -f "${SERVER_PATH}/seed.txt" ]; then
    cat /dev/urandom | tr -dc '1-9' | fold -w 5 | head -n 1 > "${SERVER_PATH}/seed.txt"
    echo "[Serverwave] Generated random seed: $(cat ${SERVER_PATH}/seed.txt)"
fi

echo "[Serverwave] Rust installed successfully!"
"#.to_string()),
            install_image: Some("debian:bookworm".to_string()),
            config_files: Vec::new(),
            is_custom: false,
            console: true,
        },

        GameConfig {
            game_type: GameType::new("minecraft-bedrock"),
            name: "Minecraft Bedrock".to_string(),
            description: "Cross-platform Minecraft for consoles, mobile, and Windows 10/11.".to_string(),
            docker_image: "ghcr.io/serverwavehost/game-images:debian".to_string(),
            startup: "./{{SERVER_BINARY}}".to_string(),
            stop_command: "stop".to_string(),
            variables: vec![
                Variable {
                    env: "SERVER_BINARY".to_string(),
                    name: "Server Binary".to_string(),
                    description: "The bedrock server executable".to_string(),
                    default: "bedrock_server".to_string(),
                    system_mapping: None,
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "BEDROCK_VERSION".to_string(),
                    name: "Bedrock Version".to_string(),
                    description: "The version of Minecraft Bedrock. Leave at latest for newest version.".to_string(),
                    default: "latest".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Text,
                },
            ],
            ports: vec![
                PortConfig { container_port: 19133, protocol: PortProtocol::Both, description: Some("Game port".to_string()), env_var: None },
            ],
            volume_path: "/mnt/server".to_string(),
            min_ram_mb: 512,
            recommended_ram_mb: 2048,
            icon: "🟩".to_string(),
            logo_url: Some("https://img.icons8.com/color/96/minecraft-logo.png".to_string()),
            install_script: Some(r#"#!/bin/sh
export DEBIAN_FRONTEND=noninteractive
apt update
apt install -y zip unzip wget curl

echo "[Serverwave] Starting Minecraft Bedrock installation..."

# Generate random number for user agent
RANDVERSION=$(awk 'BEGIN{srand(); print int(1 + rand() * 4000)}')

if [ -z "${BEDROCK_VERSION}" ] || [ "${BEDROCK_VERSION}" = "latest" ]; then
    echo "[Serverwave] Fetching latest Bedrock version..."
    DOWNLOAD_URL=$(curl -s -A "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/90.0.${RANDVERSION}.212 Safari/537.36" \
        -H "Accept-Language: en" \
        -H "Accept-Encoding: json" \
        -H "content-type: application/json" \
        "https://net-secondary.web.minecraft-services.net/api/v1.0/download/links" | grep -o 'https://www.minecraft.net/bedrockdedicatedserver/bin-linux/[^"]*')
else 
    echo "[Serverwave] Using Bedrock version: ${BEDROCK_VERSION}"
    DOWNLOAD_URL="https://www.minecraft.net/bedrockdedicatedserver/bin-linux/bedrock-server-${BEDROCK_VERSION}.zip"
fi

DOWNLOAD_FILE=$(echo "${DOWNLOAD_URL}" | cut -d"/" -f6)

echo "[Serverwave] Backing up config files..."
rm -f *.bak versions.html.gz 2>/dev/null
[ -f server.properties ] && cp server.properties server.properties.bak
[ -f permissions.json ] && cp permissions.json permissions.json.bak
[ -f allowlist.json ] && cp allowlist.json allowlist.json.bak

echo "[Serverwave] Downloading from: ${DOWNLOAD_URL}"
echo "[Serverwave] Saving to: ${DOWNLOAD_FILE}"

curl -L -A "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/90.0.${RANDVERSION}.212 Safari/537.36" \
    -H "Accept-Language: en" \
    -o "${DOWNLOAD_FILE}" \
    "${DOWNLOAD_URL}"

echo "[Serverwave] Extracting server files..."
unzip -o "${DOWNLOAD_FILE}"

echo "[Serverwave] Cleaning up..."
rm -f "${DOWNLOAD_FILE}"

echo "[Serverwave] Restoring config backups..."
[ -f server.properties.bak ] && cp -f server.properties.bak server.properties
[ -f permissions.json.bak ] && cp -f permissions.json.bak permissions.json
[ -f allowlist.json.bak ] && cp -f allowlist.json.bak allowlist.json

chmod +x bedrock_server 2>/dev/null

echo "[Serverwave] Minecraft Bedrock installed successfully!"
"#.to_string()),
            install_image: Some("debian:bookworm".to_string()),
            config_files: vec![
                ConfigFile {
                    path: "server.properties".to_string(),
                    format: ConfigFileFormat::Properties,
                    variables: {
                        let mut m = HashMap::new();
                        m.insert("enable-query".to_string(), "true".to_string());
                        m.insert("query.port".to_string(), "25565".to_string());
                        m
                    },
                },
            ],
            is_custom: false,
            console: true,
        },

        GameConfig {
            game_type: GameType::new("terraria"),
            name: "Terraria".to_string(),
            description: "2D sandbox adventure game. Dig, fight, explore, build!".to_string(),
            docker_image: "ghcr.io/serverwavehost/game-images:debian".to_string(),
            startup: "./TerrariaServer.bin.x86_64 -config serverconfig.txt".to_string(),
            stop_command: "exit".to_string(),
            variables: vec![
                Variable {
                    env: "WORLD_NAME".to_string(),
                    name: "World Name".to_string(),
                    description: "Name of the world file".to_string(),
                    default: "world".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "TERRARIA_VERSION".to_string(),
                    name: "Terraria Version".to_string(),
                    description: "Version to install. Leave at latest for newest version.".to_string(),
                    default: "latest".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "WORLD_SIZE".to_string(),
                    name: "World Size".to_string(),
                    description: "Size of auto-created world".to_string(),
                    default: "1".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: Some(vec![
                        SelectOption { value: "1".to_string(), label: "Small".to_string() },
                        SelectOption { value: "2".to_string(), label: "Medium".to_string() },
                        SelectOption { value: "3".to_string(), label: "Large".to_string() },
                    ]),
                    field_type: FieldType::Select,
                },
                Variable {
                    env: "WORLD_DIFFICULTY".to_string(),
                    name: "Difficulty".to_string(),
                    description: "World difficulty level".to_string(),
                    default: "0".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: Some(vec![
                        SelectOption { value: "0".to_string(), label: "Normal".to_string() },
                        SelectOption { value: "1".to_string(), label: "Expert".to_string() },
                        SelectOption { value: "2".to_string(), label: "Master".to_string() },
                        SelectOption { value: "3".to_string(), label: "Journey".to_string() },
                    ]),
                    field_type: FieldType::Select,
                },
                Variable {
                    env: "SERVER_MOTD".to_string(),
                    name: "MOTD".to_string(),
                    description: "Server message of the day".to_string(),
                    default: "Welcome!".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "MAX_PLAYERS".to_string(),
                    name: "Max Players".to_string(),
                    description: "Maximum number of players".to_string(),
                    default: "8".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Number,
                },
            ],
            ports: vec![
                PortConfig { container_port: 7777, protocol: PortProtocol::Both, description: Some("Game port".to_string()), env_var: None },
            ],
            volume_path: "/home/container".to_string(),
            min_ram_mb: 512,
            recommended_ram_mb: 1024,
            icon: "🌳".to_string(),
            logo_url: Some("https://cdn.cloudflare.steamstatic.com/steam/apps/105600/header.jpg".to_string()),
            install_script: Some(r#"#!/bin/sh
# Terraria Vanilla Installation Script
apt update
apt install -y curl wget file unzip

DOWNLOAD_LINK=invalid

echo "[Serverwave] Starting Terraria installation..."

if [ "${TERRARIA_VERSION}" = "latest" ] || [ -z "${TERRARIA_VERSION}" ]; then
    echo "[Serverwave] Fetching latest Terraria version..."
    DOWNLOAD_LINK=$(curl -sSL https://terraria.gamepedia.com/Server#Downloads | grep '>Terraria Server ' | grep -Eoi '<a [^>]+>' | grep -Eo 'href="[^"]+' | grep -Eo '(http|https)://[^"]+' | tail -1 | cut -d'?' -f1)
else
    CLEAN_VERSION=$(echo "${TERRARIA_VERSION}" | sed 's/\.//g')
    echo "[Serverwave] Downloading Terraria version ${TERRARIA_VERSION}..."
    DOWNLOAD_LINK=$(curl -sSL https://terraria.gamepedia.com/Server#Downloads | grep '>Terraria Server ' | grep -Eoi '<a [^>]+>' | grep -Eo 'href="[^"]+' | grep -Eo '(http|https)://[^"]+' | grep "${CLEAN_VERSION}" | cut -d'?' -f1)
fi

if [ -n "${DOWNLOAD_LINK}" ]; then
    if curl --output /dev/null --silent --head --fail "${DOWNLOAD_LINK}"; then
        echo "[Serverwave] Download link valid"
    else
        echo "[Serverwave] Invalid download link"
        exit 2
    fi
fi

CLEAN_VERSION=$(echo "${DOWNLOAD_LINK##*/}" | cut -d'-' -f3 | cut -d'.' -f1)

echo "[Serverwave] Downloading from ${DOWNLOAD_LINK}..."
curl -sSL "${DOWNLOAD_LINK}" -o "${DOWNLOAD_LINK##*/}"

echo "[Serverwave] Extracting server files..."
unzip "${DOWNLOAD_LINK##*/}"

cp -R "${CLEAN_VERSION}/Linux/"* ./
chmod +x TerrariaServer.bin.x86_64

echo "[Serverwave] Cleaning up..."
rm -rf "${CLEAN_VERSION}"
rm -f "${DOWNLOAD_LINK##*/}"

echo "[Serverwave] Creating config file..."
cat <<EOF > serverconfig.txt
worldpath=/home/container/saves/Worlds
worldname=world
world=/home/container/saves/Worlds/world.wld
difficulty=0
autocreate=1
port=7777
maxplayers=8
EOF

mkdir -p saves/Worlds

echo "[Serverwave] Terraria installed successfully!"
"#.to_string()),
            install_image: Some("debian:bookworm".to_string()),
            config_files: vec![
                ConfigFile {
                    path: "serverconfig.txt".to_string(),
                    format: ConfigFileFormat::Properties,
                    variables: {
                        let mut m = HashMap::new();
                        m.insert("autocreate".to_string(), "{{WORLD_SIZE}}".to_string());
                        m.insert("difficulty".to_string(), "{{WORLD_DIFFICULTY}}".to_string());
                        m.insert("motd".to_string(), "{{SERVER_MOTD}}".to_string());
                        m.insert("worldname".to_string(), "{{WORLD_NAME}}".to_string());
                        m.insert("world".to_string(), "/home/container/saves/Worlds/{{WORLD_NAME}}.wld".to_string());
                        m.insert("maxplayers".to_string(), "{{MAX_PLAYERS}}".to_string());
                        m
                    },
                },
            ],
            is_custom: false,
            console: true,
        },

        GameConfig {
            game_type: GameType::new("hytale"),
            name: "Hytale".to_string(),
            description: "Block-based adventure game from Hypixel Studios.".to_string(),
            docker_image: "ghcr.io/serverwavehost/game-images:java_25".to_string(),
            startup: "java -XX:+UnlockExperimentalVMOptions -XX:AOTCache=Server/HytaleServer.aot -Xms128M -Xmx{{SERVER_MEMORY}}M -XX:+UseG1GC -XX:MaxGCPauseMillis=200 -XX:G1HeapRegionSize=8M -XX:G1NewSizePercent=30 -XX:G1ReservePercent=20 -XX:InitiatingHeapOccupancyPercent=15 -XX:+UseStringDeduplication -XX:+AlwaysPreTouch -XX:MaxMetaspaceSize=512M -XX:+UseGCOverheadLimit -XX:+ExplicitGCInvokesConcurrent -jar {{SERVER_JARFILE}} --assets {{ASSETS_PATH}} {{EXTRA_ARGS}}".to_string(),
            stop_command: "stop".to_string(),
            variables: vec![
                Variable {
                    env: "SERVER_MEMORY".to_string(),
                    name: "Memory".to_string(),
                    description: "RAM in MB".to_string(),
                    default: "4096".to_string(),
                    system_mapping: Some(SystemMapping::Ram),
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "SERVER_PORT".to_string(),
                    name: "Port".to_string(),
                    description: "Server port".to_string(),
                    default: "5520".to_string(),
                    system_mapping: Some(SystemMapping::Port),
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "SERVER_JARFILE".to_string(),
                    name: "JAR File".to_string(),
                    description: "Server JAR path".to_string(),
                    default: "Server/HytaleServer.jar".to_string(),
                    system_mapping: None,
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "ASSETS_PATH".to_string(),
                    name: "Assets Path".to_string(),
                    description: "Path to Assets.zip".to_string(),
                    default: "Assets.zip".to_string(),
                    system_mapping: None,
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "EXTRA_ARGS".to_string(),
                    name: "Extra Arguments".to_string(),
                    description: "Additional server arguments".to_string(),
                    default: "".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "HT_MAXPLAYERS".to_string(),
                    name: "Max Players".to_string(),
                    description: "Maximum players".to_string(),
                    default: "20".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: Some(vec![
                        SelectOption { value: "10".to_string(), label: "10 players".to_string() },
                        SelectOption { value: "20".to_string(), label: "20 players".to_string() },
                        SelectOption { value: "50".to_string(), label: "50 players".to_string() },
                        SelectOption { value: "100".to_string(), label: "100 players".to_string() },
                    ]),
                    field_type: FieldType::Select,
                },
                Variable {
                    env: "HT_MAXVIEWRADIUS".to_string(),
                    name: "View Distance".to_string(),
                    description: "View distance in chunks".to_string(),
                    default: "12".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: Some(vec![
                        SelectOption { value: "8".to_string(), label: "8 chunks (Low)".to_string() },
                        SelectOption { value: "12".to_string(), label: "12 chunks (Default)".to_string() },
                        SelectOption { value: "16".to_string(), label: "16 chunks (High)".to_string() },
                        SelectOption { value: "20".to_string(), label: "20 chunks (Very High)".to_string() },
                    ]),
                    field_type: FieldType::Select,
                },
            ],
            ports: vec![
                PortConfig { 
                    container_port: 5520, 
                    protocol: PortProtocol::Both, 
                    description: Some("Game port".to_string()),
                    env_var: Some("SERVER_PORT".to_string()),
                },
            ],
            volume_path: "/home/container".to_string(),
            min_ram_mb: 4096,
            recommended_ram_mb: 8192,
            icon: "🏰".to_string(),
            logo_url: Some("https://upload.wikimedia.org/wikipedia/en/b/ba/Hytale_logo.png".to_string()),
            install_script: Some(r#"#!/bin/bash
# Force unbuffered output
exec 2>&1
set -e

echo "[Serverwave] Installing required tools..."
apt -y update
apt -y install unzip curl

echo "[Serverwave] Downloading Hytale downloader..."

# Download the downloader
curl -L --progress-bar -o hytale-downloader.zip https://downloader.hytale.com/hytale-downloader.zip
echo "[Serverwave] Download complete"

# Unzip it
echo "[Serverwave] Extracting downloader..."
unzip -o hytale-downloader.zip

# Make executable and run (this will prompt for OAuth if needed)
chmod +x hytale-downloader-linux-amd64
echo "[Serverwave] Running Hytale downloader (OAuth authentication may be required)..."
echo "[Serverwave] Check the popup if authentication is needed!"
./hytale-downloader-linux-amd64

# Find and extract the downloaded version zip
echo "[Serverwave] Looking for downloaded server files..."
VERSION_ZIP=$(ls -t *.zip 2>/dev/null | grep -E '^[0-9]{4}\.[0-9]{2}\.[0-9]{2}-' | head -1 || true)
if [ -n "$VERSION_ZIP" ]; then
    echo "[Serverwave] Found version: $VERSION_ZIP"
    echo "[Serverwave] Extracting server files..."
    unzip -o "$VERSION_ZIP"
    rm -f "$VERSION_ZIP"
    echo "[Serverwave] Server files extracted"
else
    echo "[Serverwave] Warning: No version zip found, server may already be extracted"
fi

# Cleanup downloader files (but keep .hytale-downloader-credentials.json for refresh token!)
echo "[Serverwave] Cleaning up..."
rm -f hytale-downloader.zip hytale-downloader-linux-amd64 hytale-downloader-windows-amd64.exe

echo "[Serverwave] Hytale server installed successfully!"
"#.to_string()),
            install_image: Some("debian:bookworm".to_string()),
            config_files: vec![
                ConfigFile {
                    path: "config.json".to_string(),
                    format: ConfigFileFormat::Json,
                    variables: {
                        let mut m = HashMap::new();
                        m.insert("MaxPlayers".to_string(), "{{HT_MAXPLAYERS}}".to_string());
                        m.insert("MaxViewRadius".to_string(), "{{HT_MAXVIEWRADIUS}}".to_string());
                        m
                    },
                },
            ],
            is_custom: false,
            console: true,
        },

        GameConfig {
            game_type: GameType::new("palworld"),
            name: "Palworld".to_string(),
            description: "Creature collecting survival game. Catch Pals, build bases, and survive.".to_string(),
            docker_image: "ghcr.io/serverwavehost/game-images:steamcmd_debian".to_string(),
            startup: "/home/container/Pal/Binaries/Linux/PalServer-Linux-Shipping Pal -port={{SERVER_PORT}} -players={{MAX_PLAYERS}} -useperfthreads -NoAsyncLoadingThread -UseMultithreadForDS -servername=\"{{SRV_NAME}}\" -serverpassword=\"{{SRV_PASSWORD}}\" -adminpassword=\"{{ADMIN_PASSWORD}}\"".to_string(),
            stop_command: "^C".to_string(),
            variables: vec![
                Variable {
                    env: "SRCDS_APPID".to_string(),
                    name: "Steam App ID".to_string(),
                    description: "Steam App ID for Palworld dedicated server".to_string(),
                    default: "2394010".to_string(),
                    system_mapping: None,
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "SERVER_PORT".to_string(),
                    name: "Server Port".to_string(),
                    description: "Server port".to_string(),
                    default: "8211".to_string(),
                    system_mapping: Some(SystemMapping::Port),
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "MAX_PLAYERS".to_string(),
                    name: "Max Players".to_string(),
                    description: "Maximum number of players (1-150)".to_string(),
                    default: "32".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "SRV_NAME".to_string(),
                    name: "Server Name".to_string(),
                    description: "Name shown in server browser".to_string(),
                    default: "A Palworld server hosted by Serverwave".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "SRV_PASSWORD".to_string(),
                    name: "Server Password".to_string(),
                    description: "Password to join the server (leave empty for no password)".to_string(),
                    default: "".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Password,
                },
                Variable {
                    env: "ADMIN_PASSWORD".to_string(),
                    name: "Admin Password".to_string(),
                    description: "Password for admin commands".to_string(),
                    default: "ChangeMe".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Password,
                },
                Variable {
                    env: "AUTO_UPDATE".to_string(),
                    name: "Auto Update".to_string(),
                    description: "Auto update the server on start".to_string(),
                    default: "1".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: Some(vec![
                        SelectOption { value: "1".to_string(), label: "Enabled".to_string() },
                        SelectOption { value: "0".to_string(), label: "Disabled".to_string() },
                    ]),
                    field_type: FieldType::Select,
                },
            ],
            ports: vec![
                PortConfig {
                    container_port: 8211,
                    protocol: PortProtocol::Both,
                    description: Some("Game port".to_string()),
                    env_var: Some("SERVER_PORT".to_string()),
                },
            ],
            volume_path: "/home/container".to_string(),
            min_ram_mb: 8192,
            recommended_ram_mb: 16384,
            icon: "🐾".to_string(),
            logo_url: Some("https://cdn.cloudflare.steamstatic.com/steam/apps/1623730/header.jpg".to_string()),
            install_script: Some(r#"#!/bin/sh
# Palworld SteamCMD Installation Script
export DEBIAN_FRONTEND=noninteractive
apt -y update
apt -y --no-install-recommends install curl lib32gcc-s1 ca-certificates

echo "[Serverwave] Starting Palworld installation..."

SERVER_PATH=/home/container
SRCDS_APPID=2394010

# Download and setup steamcmd
cd /tmp
mkdir -p ${SERVER_PATH}/steamcmd
curl -sSL -o steamcmd.tar.gz https://steamcdn-a.akamaihd.net/client/installer/steamcmd_linux.tar.gz
tar -xzvf steamcmd.tar.gz -C ${SERVER_PATH}/steamcmd
mkdir -p ${SERVER_PATH}/steamapps
cd ${SERVER_PATH}/steamcmd

chown -R root:root ${SERVER_PATH}
export HOME=${SERVER_PATH}

echo "[Serverwave] Logging into Steam..."
./steamcmd.sh +login anonymous +quit

echo "[Serverwave] Installing Palworld dedicated server..."
./steamcmd.sh +force_install_dir ${SERVER_PATH} +login anonymous +app_update ${SRCDS_APPID} validate +quit

# Set up Steam libraries
echo "[Serverwave] Setting up Steam libraries..."
mkdir -p ${SERVER_PATH}/.steam/sdk32
cp -v linux32/steamclient.so ../.steam/sdk32/steamclient.so

mkdir -p ${SERVER_PATH}/.steam/sdk64
cp -v linux64/steamclient.so ../.steam/sdk64/steamclient.so

# Copy template config file
echo "[Serverwave] Setting up config files..."
if [ -f "${SERVER_PATH}/Pal/Saved/Config/LinuxServer/PalWorldSettings.ini" ]; then
    echo "Config file already exists, backing up and creating new one"
    mv ${SERVER_PATH}/Pal/Saved/Config/LinuxServer/PalWorldSettings.ini "${SERVER_PATH}/Pal/Saved/Config/LinuxServer/PalWorldSettings_$(date +"%Y%m%d%H%M%S").ini"
    cp ${SERVER_PATH}/DefaultPalWorldSettings.ini ${SERVER_PATH}/Pal/Saved/Config/LinuxServer/PalWorldSettings.ini
else
    echo "Creating new config file"
    mkdir -p ${SERVER_PATH}/Pal/Saved/Config/LinuxServer
    cp ${SERVER_PATH}/DefaultPalWorldSettings.ini ${SERVER_PATH}/Pal/Saved/Config/LinuxServer/PalWorldSettings.ini
fi

echo "[Serverwave] Palworld installed successfully!"
"#.to_string()),
            install_image: Some("debian:bookworm".to_string()),
            config_files: vec![
                ConfigFile {
                    path: "Pal/Saved/Config/LinuxServer/PalWorldSettings.ini".to_string(),
                    format: ConfigFileFormat::Ini,
                    variables: {
                        let mut m = HashMap::new();
                        m.insert("RCONEnabled".to_string(), "True".to_string());
                        m
                    },
                },
            ],
            is_custom: false,
            console: true,
        },

        GameConfig {
            game_type: GameType::new("satisfactory"),
            name: "Satisfactory".to_string(),
            description: "Factory building game. Build massive factories and automate production.".to_string(),
            docker_image: "ghcr.io/serverwavehost/game-images:steamcmd_debian".to_string(),
            startup: "Engine/Binaries/Linux/*-Linux-Shipping FactoryGame -Port={{SERVER_PORT}} -ReliablePort={{RELIABLE_PORT}}".to_string(),
            stop_command: "^C".to_string(),
            variables: vec![
                Variable {
                    env: "SRCDS_APPID".to_string(),
                    name: "Steam App ID".to_string(),
                    description: "Steam App ID for Satisfactory dedicated server".to_string(),
                    default: "1690800".to_string(),
                    system_mapping: None,
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "SERVER_PORT".to_string(),
                    name: "Game Port".to_string(),
                    description: "Main game port".to_string(),
                    default: "7777".to_string(),
                    system_mapping: Some(SystemMapping::Port),
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "RELIABLE_PORT".to_string(),
                    name: "Reliable Port".to_string(),
                    description: "Reliable UDP port".to_string(),
                    default: "8888".to_string(),
                    system_mapping: None,
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "MAX_PLAYERS".to_string(),
                    name: "Max Players".to_string(),
                    description: "Maximum number of players".to_string(),
                    default: "4".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "NUM_AUTOSAVES".to_string(),
                    name: "Number of Autosaves".to_string(),
                    description: "Number of rotating autosaves to keep".to_string(),
                    default: "3".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "INIT_CONNECT_TIMEOUT".to_string(),
                    name: "Initial Connection Timeout".to_string(),
                    description: "Time in seconds for new client connection".to_string(),
                    default: "30".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "CONNECT_TIMEOUT".to_string(),
                    name: "Connection Timeout".to_string(),
                    description: "Time in seconds for established connection timeout".to_string(),
                    default: "20".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "AUTO_UPDATE".to_string(),
                    name: "Auto Update".to_string(),
                    description: "Auto update the server on start".to_string(),
                    default: "1".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: Some(vec![
                        SelectOption { value: "1".to_string(), label: "Enabled".to_string() },
                        SelectOption { value: "0".to_string(), label: "Disabled".to_string() },
                    ]),
                    field_type: FieldType::Select,
                },
            ],
            ports: vec![
                PortConfig {
                    container_port: 7777,
                    protocol: PortProtocol::Both,
                    description: Some("Game port".to_string()),
                    env_var: Some("SERVER_PORT".to_string()),
                },
                PortConfig {
                    container_port: 8888,
                    protocol: PortProtocol::Both,
                    description: Some("Reliable port".to_string()),
                    env_var: Some("RELIABLE_PORT".to_string()),
                },
            ],
            volume_path: "/home/container".to_string(),
            min_ram_mb: 8192,
            recommended_ram_mb: 16384,
            icon: "🏭".to_string(),
            logo_url: Some("https://cdn.cloudflare.steamstatic.com/steam/apps/526870/header.jpg".to_string()),
            install_script: Some(r#"#!/bin/sh
# Satisfactory SteamCMD Installation Script
export DEBIAN_FRONTEND=noninteractive
apt -y update
apt -y --no-install-recommends install curl lib32gcc-s1 ca-certificates

echo "[Serverwave] Starting Satisfactory installation..."

SERVER_PATH=/home/container
SRCDS_APPID=1690800

# Download and setup steamcmd
cd /tmp
curl -sSL -o steamcmd.tar.gz https://steamcdn-a.akamaihd.net/client/installer/steamcmd_linux.tar.gz
mkdir -p "${SERVER_PATH}/steamcmd"
tar -xzvf steamcmd.tar.gz -C "${SERVER_PATH}/steamcmd"
cd "${SERVER_PATH}/steamcmd"

chown -R root:root "${SERVER_PATH}"
export HOME="${SERVER_PATH}"

echo "[Serverwave] Logging into Steam..."
./steamcmd.sh +login anonymous +quit

echo "[Serverwave] Installing Satisfactory dedicated server..."
./steamcmd.sh +force_install_dir "${SERVER_PATH}" +login anonymous +app_update ${SRCDS_APPID} validate +exit

# Set up Steam libraries
echo "[Serverwave] Setting up Steam libraries..."
mkdir -p "${SERVER_PATH}/.steam/sdk32"
mkdir -p "${SERVER_PATH}/.steam/sdk64"
cp -v linux32/steamclient.so "${SERVER_PATH}/.steam/sdk32/steamclient.so"
cp -v linux64/steamclient.so "${SERVER_PATH}/.steam/sdk64/steamclient.so"

# Make server binary executable
cd "${SERVER_PATH}/Engine/Binaries/Linux"
chmod +x ./*-Linux-Shipping 2>/dev/null || true

# Create config directories and files
mkdir -p "${SERVER_PATH}/FactoryGame/Saved/Config/LinuxServer"

echo "[Serverwave] Creating Game.ini..."
cat > "${SERVER_PATH}/FactoryGame/Saved/Config/LinuxServer/Game.ini" << 'EOF'
[/Script/Engine.GameSession]
MaxPlayers=
EOF

echo "[Serverwave] Creating Engine.ini..."
cat > "${SERVER_PATH}/FactoryGame/Saved/Config/LinuxServer/Engine.ini" << 'EOF'
[/Script/FactoryGame.FGSaveSession]
mNumRotatingAutosaves=

[/Script/OnlineSubsystemUtils.IpNetDriver]
InitialConnectTimeout=
ConnectionTimeout=
EOF

echo "[Serverwave] Satisfactory installed successfully!"
"#.to_string()),
            install_image: Some("debian:bookworm".to_string()),
            config_files: vec![
                ConfigFile {
                    path: "FactoryGame/Saved/Config/LinuxServer/Game.ini".to_string(),
                    format: ConfigFileFormat::Ini,
                    variables: {
                        let mut m = HashMap::new();
                        m.insert("MaxPlayers".to_string(), "{{MAX_PLAYERS}}".to_string());
                        m
                    },
                },
                ConfigFile {
                    path: "FactoryGame/Saved/Config/LinuxServer/Engine.ini".to_string(),
                    format: ConfigFileFormat::Ini,
                    variables: {
                        let mut m = HashMap::new();
                        m.insert("mNumRotatingAutosaves".to_string(), "{{NUM_AUTOSAVES}}".to_string());
                        m.insert("InitialConnectTimeout".to_string(), "{{INIT_CONNECT_TIMEOUT}}".to_string());
                        m.insert("ConnectionTimeout".to_string(), "{{CONNECT_TIMEOUT}}".to_string());
                        m
                    },
                },
            ],
            is_custom: false,
            console: true,
        },

        GameConfig {
            game_type: GameType::new("project-zomboid"),
            name: "Project Zomboid".to_string(),
            description: "Zombie survival RPG. Survive the apocalypse and build your base.".to_string(),
            docker_image: "ghcr.io/serverwavehost/game-images:steamcmd_debian".to_string(),
            startup: "export PATH=\"./jre64/bin:$PATH\" ; export LD_LIBRARY_PATH=\"./linux64:./natives:.:./jre64/lib/amd64:${LD_LIBRARY_PATH}\" ; ./ProjectZomboid64 -port {{SERVER_PORT}} -udpport {{UDP_PORT}} -cachedir=/home/container/.cache -servername \"{{SERVER_NAME}}\" -adminusername {{ADMIN_USER}} -adminpassword \"{{ADMIN_PASSWORD}}\"".to_string(),
            stop_command: "^C".to_string(),
            variables: vec![
                Variable {
                    env: "SRCDS_APPID".to_string(),
                    name: "Steam App ID".to_string(),
                    description: "Steam App ID for Project Zomboid dedicated server".to_string(),
                    default: "380870".to_string(),
                    system_mapping: None,
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "SERVER_PORT".to_string(),
                    name: "Game Port".to_string(),
                    description: "Main game port".to_string(),
                    default: "16261".to_string(),
                    system_mapping: Some(SystemMapping::Port),
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "UDP_PORT".to_string(),
                    name: "UDP Port".to_string(),
                    description: "UDP port".to_string(),
                    default: "16262".to_string(),
                    system_mapping: None,
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "MAX_PLAYERS".to_string(),
                    name: "Max Players".to_string(),
                    description: "Maximum number of players".to_string(),
                    default: "10".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "SERVER_NAME".to_string(),
                    name: "Server Name".to_string(),
                    description: "Internal server name for save/config files".to_string(),
                    default: "Hosted by Serverwave".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "ADMIN_USER".to_string(),
                    name: "Admin Username".to_string(),
                    description: "Username for the admin account".to_string(),
                    default: "admin".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "ADMIN_PASSWORD".to_string(),
                    name: "Admin Password".to_string(),
                    description: "Password for the admin account".to_string(),
                    default: "ChangeMe".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Password,
                },
                Variable {
                    env: "AUTO_UPDATE".to_string(),
                    name: "Auto Update".to_string(),
                    description: "Auto update the server on start".to_string(),
                    default: "1".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: Some(vec![
                        SelectOption { value: "1".to_string(), label: "Enabled".to_string() },
                        SelectOption { value: "0".to_string(), label: "Disabled".to_string() },
                    ]),
                    field_type: FieldType::Select,
                },
            ],
            ports: vec![
                PortConfig {
                    container_port: 16261,
                    protocol: PortProtocol::Both,
                    description: Some("Game port".to_string()),
                    env_var: Some("SERVER_PORT".to_string()),
                },
                PortConfig {
                    container_port: 16262,
                    protocol: PortProtocol::Both,
                    description: Some("UDP port".to_string()),
                    env_var: Some("UDP_PORT".to_string()),
                },
            ],
            volume_path: "/home/container".to_string(),
            min_ram_mb: 4096,
            recommended_ram_mb: 8192,
            icon: "🧟".to_string(),
            logo_url: Some("https://cdn.cloudflare.steamstatic.com/steam/apps/108600/header.jpg".to_string()),
            install_script: Some(r#"#!/bin/sh
# Project Zomboid SteamCMD Installation Script
export DEBIAN_FRONTEND=noninteractive
apt -y update
apt -y --no-install-recommends install curl lib32gcc-s1 ca-certificates

echo "[Serverwave] Starting Project Zomboid installation..."

SERVER_PATH=/home/container
SRCDS_APPID=380870

# Download and setup steamcmd
cd /tmp
mkdir -p "${SERVER_PATH}/steamcmd"
curl -sSL -o steamcmd.tar.gz https://steamcdn-a.akamaihd.net/client/installer/steamcmd_linux.tar.gz
tar -xzvf steamcmd.tar.gz -C "${SERVER_PATH}/steamcmd"
mkdir -p "${SERVER_PATH}/steamapps"
cd "${SERVER_PATH}/steamcmd"

chown -R root:root "${SERVER_PATH}"
export HOME="${SERVER_PATH}"

echo "[Serverwave] Logging into Steam..."
./steamcmd.sh +login anonymous +quit

echo "[Serverwave] Installing Project Zomboid dedicated server..."
./steamcmd.sh +force_install_dir "${SERVER_PATH}" +login anonymous +app_update ${SRCDS_APPID} validate +quit

# Set up Steam libraries
echo "[Serverwave] Setting up Steam libraries..."
mkdir -p "${SERVER_PATH}/.steam/sdk32"
cp -v linux32/steamclient.so "${SERVER_PATH}/.steam/sdk32/steamclient.so"

mkdir -p "${SERVER_PATH}/.steam/sdk64"
cp -v linux64/steamclient.so "${SERVER_PATH}/.steam/sdk64/steamclient.so"

# Remove default start script
cd "${SERVER_PATH}"
rm -f start-server.sh

echo "[Serverwave] Project Zomboid installed successfully!"
"#.to_string()),
            install_image: Some("debian:bookworm".to_string()),
            config_files: Vec::new(),
            is_custom: false,
            console: true,
        },

        GameConfig {
            game_type: GameType::new("starrupture"),
            name: "StarRupture".to_string(),
            description: "Space survival game. Build bases and explore the cosmos.".to_string(),
            docker_image: "ghcr.io/serverwavehost/game-images:wine_latest".to_string(),
            startup: "wine ./StarRuptureServerEOS.exe -Log -port={{SERVER_PORT}} -QueryPort={{QUERY_PORT}} -ServerName=\"{{SRV_NAME}}\" MaxPlayers={{MAX_PLAYERS}}".to_string(),
            stop_command: "^C".to_string(),
            variables: vec![
                Variable {
                    env: "SRCDS_APPID".to_string(),
                    name: "Steam App ID".to_string(),
                    description: "Steam App ID for StarRupture dedicated server".to_string(),
                    default: "3809400".to_string(),
                    system_mapping: None,
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "SERVER_PORT".to_string(),
                    name: "Game Port".to_string(),
                    description: "Main game port".to_string(),
                    default: "7777".to_string(),
                    system_mapping: Some(SystemMapping::Port),
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "QUERY_PORT".to_string(),
                    name: "Query Port".to_string(),
                    description: "Query port".to_string(),
                    default: "27015".to_string(),
                    system_mapping: None,
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "MAX_PLAYERS".to_string(),
                    name: "Max Players".to_string(),
                    description: "Maximum number of players".to_string(),
                    default: "8".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Number,
                },
                Variable {
                    env: "SRV_NAME".to_string(),
                    name: "Server Name".to_string(),
                    description: "Name shown in server browser".to_string(),
                    default: "A StarRupture server hosted by Serverwave".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "WINEDEBUG".to_string(),
                    name: "Wine Debug".to_string(),
                    description: "Wine debug mode".to_string(),
                    default: "-all".to_string(),
                    system_mapping: None,
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "WINEARCH".to_string(),
                    name: "Wine Architecture".to_string(),
                    description: "Wine architecture".to_string(),
                    default: "win64".to_string(),
                    system_mapping: None,
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "WINEPATH".to_string(),
                    name: "Wine Path".to_string(),
                    description: "Wine path".to_string(),
                    default: "/home/container".to_string(),
                    system_mapping: None,
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "WINETRICKS_RUN".to_string(),
                    name: "Winetricks".to_string(),
                    description: "Winetricks to run".to_string(),
                    default: "mono vcrun2019".to_string(),
                    system_mapping: None,
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "WINDOWS_INSTALL".to_string(),
                    name: "Windows Install".to_string(),
                    description: "Use Windows platform for SteamCMD".to_string(),
                    default: "1".to_string(),
                    system_mapping: None,
                    user_editable: false,
                    options: None,
                    field_type: FieldType::Text,
                },
                Variable {
                    env: "AUTO_UPDATE".to_string(),
                    name: "Auto Update".to_string(),
                    description: "Auto update the server on start".to_string(),
                    default: "1".to_string(),
                    system_mapping: None,
                    user_editable: true,
                    options: Some(vec![
                        SelectOption { value: "1".to_string(), label: "Enabled".to_string() },
                        SelectOption { value: "0".to_string(), label: "Disabled".to_string() },
                    ]),
                    field_type: FieldType::Select,
                },
            ],
            ports: vec![
                PortConfig {
                    container_port: 7777,
                    protocol: PortProtocol::Both,
                    description: Some("Game port".to_string()),
                    env_var: Some("SERVER_PORT".to_string()),
                },
                PortConfig {
                    container_port: 27015,
                    protocol: PortProtocol::Both,
                    description: Some("Query port".to_string()),
                    env_var: Some("QUERY_PORT".to_string()),
                },
            ],
            volume_path: "/home/container".to_string(),
            min_ram_mb: 4096,
            recommended_ram_mb: 8192,
            icon: "🚀".to_string(),
            logo_url: Some("https://cdn.cloudflare.steamstatic.com/steam/apps/2080690/header.jpg".to_string()),
            install_script: Some(r#"#!/bin/sh
# StarRupture SteamCMD Installation Script
export DEBIAN_FRONTEND=noninteractive
apt -y update
apt -y --no-install-recommends install curl lib32gcc-s1 ca-certificates

echo "[Serverwave] Starting StarRupture installation..."

SERVER_PATH=/home/container
SRCDS_APPID=3809400

# Download and setup steamcmd
cd /tmp
mkdir -p "${SERVER_PATH}/steamcmd"
curl -sSL -o steamcmd.tar.gz https://steamcdn-a.akamaihd.net/client/installer/steamcmd_linux.tar.gz
tar -xzvf steamcmd.tar.gz -C "${SERVER_PATH}/steamcmd"
mkdir -p "${SERVER_PATH}/steamapps"
cd "${SERVER_PATH}/steamcmd"

chown -R root:root "${SERVER_PATH}"
export HOME="${SERVER_PATH}"

echo "[Serverwave] Logging into Steam..."
./steamcmd.sh +login anonymous +quit

echo "[Serverwave] Installing StarRupture dedicated server (Windows)..."
./steamcmd.sh +force_install_dir "${SERVER_PATH}" +login anonymous +@sSteamCmdForcePlatformType windows +app_update ${SRCDS_APPID} validate +quit

# Set up Steam libraries
echo "[Serverwave] Setting up Steam libraries..."
mkdir -p "${SERVER_PATH}/.steam/sdk32"
cp -v linux32/steamclient.so ../.steam/sdk32/steamclient.so

mkdir -p "${SERVER_PATH}/.steam/sdk64"
cp -v linux64/steamclient.so ../.steam/sdk64/steamclient.so

echo "[Serverwave] StarRupture installed successfully!"
"#.to_string()),
            install_image: Some("debian:bookworm".to_string()),
            config_files: Vec::new(),
            is_custom: false,
            console: true,
        },
    ]
}
