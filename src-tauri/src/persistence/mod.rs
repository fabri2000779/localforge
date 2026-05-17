//! On-disk persistence of server records.
//!
//! Server data and config files live under `~/LocalForge/`:
//!   - `~/LocalForge/servers/<game>/<id>/`  — game world / config / saves
//!   - `~/LocalForge/config/<id>.json`     — serialised [`Server`] record
//!   - `~/LocalForge/games/custom_games.json` — user-authored game catalogue
//!
//! This module owns those paths and the JSON read/write logic; both the
//! local backend and any future migration tools should go through it.

use localforge_core::Server;
use std::path::PathBuf;

const APP_DIR: &str = "LocalForge";

fn home_root() -> PathBuf {
    directories::UserDirs::new()
        .map(|d| d.home_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR)
}

pub fn servers_data_root() -> PathBuf {
    home_root().join("servers")
}

pub fn servers_config_dir() -> PathBuf {
    home_root().join("config")
}

pub fn server_config_path(server_id: &str) -> PathBuf {
    servers_config_dir().join(format!("{}.json", server_id))
}

pub fn server_data_path(server: &Server) -> PathBuf {
    servers_data_root()
        .join(server.game_type.to_string())
        .join(&server.id)
}

pub fn load_server(server_id: &str) -> std::io::Result<Server> {
    let path = server_config_path(server_id);
    let body = std::fs::read_to_string(path)?;
    serde_json::from_str(&body)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

pub fn list_servers() -> std::io::Result<Vec<Server>> {
    let dir = servers_config_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            match std::fs::read_to_string(&path).and_then(|body| {
                serde_json::from_str::<Server>(&body)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
            }) {
                Ok(server) => out.push(server),
                Err(e) => tracing::warn!("Skipping malformed server config {:?}: {}", path, e),
            }
        }
    }
    out.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Ok(out)
}

/// Recursively compute the size of a path on disk.
pub fn directory_size(path: &std::path::Path) -> std::io::Result<u64> {
    if path.is_file() {
        return Ok(std::fs::metadata(path)?.len());
    }
    let mut total: u64 = 0;
    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let entry_path = entry.path();
            if entry_path.is_file() {
                total += std::fs::metadata(&entry_path)?.len();
            } else if entry_path.is_dir() {
                total += directory_size(&entry_path).unwrap_or(0);
            }
        }
    }
    Ok(total)
}
