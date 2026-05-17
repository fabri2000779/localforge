//! Desktop-specific path resolution. The agent has its own data root;
//! the desktop hardcodes `~/LocalForge`. These helpers are just thin
//! wrappers around `localforge_backend_local::persistence` with that
//! root pre-applied so the install scripts in `commands::server` don't
//! have to thread a `&Path` through every call.

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

pub fn save_server(server: &Server) -> std::io::Result<()> {
    persistence::save_server(&home_root(), server)
}

pub fn delete_server_record(id: &str) -> std::io::Result<()> {
    persistence::delete_server_record(&home_root(), id)
}
