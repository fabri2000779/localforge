//! On-disk persistence of server records, scoped to a configurable root.
//!
//! Under `<root>/`:
//!   - `servers/<game>/<id>/`  — game world / config / saves
//!   - `config/<id>.json`      — serialised [`Server`] record
//!
//! The desktop app uses `~/LocalForge` as the root; the agent uses
//! whatever path is configured at install time (typically
//! `/var/lib/localforge`).

use localforge_core::Server;
use std::path::{Path, PathBuf};

pub fn servers_data_root(root: &Path) -> PathBuf {
    root.join("servers")
}

pub fn servers_config_dir(root: &Path) -> PathBuf {
    root.join("config")
}

pub fn server_config_path(root: &Path, server_id: &str) -> PathBuf {
    servers_config_dir(root).join(format!("{}.json", server_id))
}

pub fn server_data_path(root: &Path, server: &Server) -> PathBuf {
    servers_data_root(root)
        .join(server.game_type.to_string())
        .join(&server.id)
}

pub fn load_server(root: &Path, server_id: &str) -> std::io::Result<Server> {
    let path = server_config_path(root, server_id);
    let body = std::fs::read_to_string(path)?;
    serde_json::from_str(&body)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

pub fn save_server(root: &Path, server: &Server) -> std::io::Result<()> {
    let dir = servers_config_dir(root);
    std::fs::create_dir_all(&dir)?;
    let path = server_config_path(root, &server.id);
    let body = serde_json::to_string_pretty(server)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, body)
}

pub fn delete_server_record(root: &Path, server_id: &str) -> std::io::Result<()> {
    let path = server_config_path(root, server_id);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

pub fn list_servers(root: &Path) -> std::io::Result<Vec<Server>> {
    let dir = servers_config_dir(root);
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
    out.sort_by_key(|a| a.created_at);
    Ok(out)
}

/// Recursively compute the size of a path on disk.
pub fn directory_size(path: &Path) -> std::io::Result<u64> {
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
