//! Desktop path helpers: persistence wrappers with `~/LocalForge` pre-applied.

use localforge_backend_local::persistence;
use localforge_core::Server;
use std::path::PathBuf;

pub fn home_root() -> PathBuf {
    directories::UserDirs::new()
        .map(|d| d.home_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
        .join("LocalForge")
}

pub fn load_server(id: &str) -> std::io::Result<Server> {
    persistence::load_server(&home_root(), id)
}
