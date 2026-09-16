//! Server records under `<root>/config/<id>.json`; world data lives in `<root>/servers/<game>/<id>/`;
//! the game definition each server was last created/installed/configured with under `<root>/games/<id>.json`.

use localforge_core::{get_builtin_games, GameConfig, Server};
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
    // Temp file + rename: atomic (no torn records) and needs only directory write permission,
    // so a record owned by another uid (the agent once ran as root) can still be replaced.
    let tmp = dir.join(format!(".{}.json.tmp", server.id));
    let _ = std::fs::remove_file(&tmp);
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, &path)
}

pub fn delete_server_record(root: &Path, server_id: &str) -> std::io::Result<()> {
    let path = server_config_path(root, server_id);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    let _ = std::fs::remove_file(game_snapshot_path(root, server_id));
    Ok(())
}

fn game_snapshot_path(root: &Path, server_id: &str) -> PathBuf {
    root.join("games").join(format!("{}.json", server_id))
}

/// Remember the game definition so a plain `start_server(id)` (agent REST/relay, schedules, crash
/// restarts) can re-apply the saved configuration without the caller knowing the game.
pub fn save_game_snapshot(root: &Path, server_id: &str, game: &GameConfig) -> std::io::Result<()> {
    let dir = root.join("games");
    std::fs::create_dir_all(&dir)?;
    let body = serde_json::to_string_pretty(game)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let tmp = dir.join(format!(".{}.json.tmp", server_id));
    let _ = std::fs::remove_file(&tmp);
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, game_snapshot_path(root, server_id))
}

/// Where a server's game definition came from. Only a snapshot was handed over by a client that
/// knows the server's real game (custom overrides included); the other two are educated guesses for
/// records created before snapshots existed, good enough to render config files but not to rebuild
/// the container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameSource {
    Snapshot,
    /// The desktop's own custom-games catalog next to the snapshots.
    Catalog,
    Builtin,
}

#[derive(Debug)]
pub struct ResolvedGame {
    pub game: GameConfig,
    pub source: GameSource,
}

/// Resolve the definition for a server, including records created before snapshots existed: the
/// snapshot, else the desktop's catalog at `<root>/games/custom_games.json` (custom definitions
/// shadow built-ins there, like GamesManager), else the built-in game. Never guesses an unknown
/// custom game, and never replaces an unreadable definition with a built-in one. Guesses are not
/// persisted: the next client-driven create / install / apply writes the real snapshot.
pub fn resolve_game(root: &Path, server: &Server) -> std::io::Result<ResolvedGame> {
    let snapshot = game_snapshot_path(root, &server.id);
    match std::fs::read_to_string(&snapshot) {
        Ok(body) => {
            let game: GameConfig = serde_json::from_str(&body).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Invalid game definition {}: {e}", snapshot.display()),
                )
            })?;
            if game.game_type != server.game_type {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "Game definition {} belongs to {}, but server {} uses {}",
                        snapshot.display(),
                        game.game_type,
                        server.id,
                        server.game_type
                    ),
                ));
            }
            return Ok(ResolvedGame { game, source: GameSource::Snapshot });
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }

    let catalog = root.join("games").join("custom_games.json");
    let custom_game = match std::fs::read_to_string(&catalog) {
        Ok(body) => {
            let games: Vec<GameConfig> = serde_json::from_str(&body).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Invalid game catalog {}: {e}", catalog.display()),
                )
            })?;
            // GamesManager loads the list into a map, so its last definition for an id wins.
            games
                .into_iter()
                .rev()
                .find(|game| game.game_type == server.game_type)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    if let Some(game) = custom_game {
        return Ok(ResolvedGame { game, source: GameSource::Catalog });
    }
    get_builtin_games()
        .into_iter()
        .find(|game| game.game_type == server.game_type)
        .map(|game| ResolvedGame { game, source: GameSource::Builtin })
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "no game definition for server {} ({}); save or apply its configuration from the desktop once",
                    server.id, server.game_type
                ),
            )
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use localforge_core::{GameType, ServerStatus};

    struct TestRoot {
        path: PathBuf,
    }

    impl TestRoot {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("localforge-persistence-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn catalog(&self, games: &[GameConfig]) {
            std::fs::create_dir_all(self.path.join("games")).unwrap();
            std::fs::write(
                self.path.join("games/custom_games.json"),
                serde_json::to_string(games).unwrap(),
            )
            .unwrap();
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn load_game_snapshot(root: &Path, server_id: &str) -> Option<GameConfig> {
        let body = std::fs::read_to_string(game_snapshot_path(root, server_id)).ok()?;
        serde_json::from_str(&body).ok()
    }

    fn server(root: &Path, game_type: &str) -> Server {
        Server {
            id: "old-server".to_string(),
            name: "Old server".to_string(),
            game_type: GameType::new(game_type),
            status: ServerStatus::Stopped,
            container_id: Some("existing-container".to_string()),
            port: 25565,
            memory_mb: 2048,
            data_path: root.join("servers").join(game_type).join("old-server"),
            created_at: chrono::Utc::now(),
            config: Default::default(),
            installed: true,
            install_container_id: None,
            restart_policy: Default::default(),
        }
    }

    #[test]
    fn old_builtin_server_without_a_catalog_resolves_to_the_builtin_as_a_guess() {
        let root = TestRoot::new();
        let server = server(&root.path, "minecraft-java");
        save_server(&root.path, &server).unwrap();

        let resolved = resolve_game(&root.path, &server).unwrap();
        assert_eq!(resolved.source, GameSource::Builtin);
        assert_eq!(resolved.game.game_type, server.game_type);
        assert!(!resolved.game.docker_image.is_empty());
        // Guesses are never persisted as the server's definition.
        assert!(load_game_snapshot(&root.path, &server.id).is_none());
        assert_eq!(load_server(&root.path, &server.id).unwrap().container_id, server.container_id);
    }

    #[test]
    fn catalog_definition_shadows_the_builtin_and_the_last_entry_wins() {
        let root = TestRoot::new();
        let server = server(&root.path, "minecraft-java");
        let first = GameConfig { game_type: server.game_type.clone(), docker_image: "custom:old".into(), ..Default::default() };
        let game = GameConfig { docker_image: "custom:latest".into(), ..first.clone() };
        root.catalog(&[first, game]);

        let resolved = resolve_game(&root.path, &server).unwrap();
        assert_eq!(resolved.source, GameSource::Catalog);
        assert_eq!(resolved.game.docker_image, "custom:latest");
        assert!(load_game_snapshot(&root.path, &server.id).is_none());
    }

    #[test]
    fn snapshot_takes_precedence_over_a_changed_or_invalid_catalog() {
        let root = TestRoot::new();
        let server = server(&root.path, "custom-game");
        let game = GameConfig { game_type: server.game_type.clone(), docker_image: "custom:pinned".into(), ..Default::default() };
        save_game_snapshot(&root.path, &server.id, &game).unwrap();
        std::fs::write(root.path.join("games/custom_games.json"), "not json").unwrap();

        let resolved = resolve_game(&root.path, &server).unwrap();
        assert_eq!(resolved.source, GameSource::Snapshot);
        assert_eq!(resolved.game.docker_image, "custom:pinned");
    }

    #[test]
    fn unknown_custom_game_is_an_error_and_writes_nothing() {
        let root = TestRoot::new();
        let server = server(&root.path, "missing-custom-game");

        let error = resolve_game(&root.path, &server).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert!(!game_snapshot_path(&root.path, &server.id).exists());
    }

    #[test]
    fn malformed_catalog_does_not_silently_fall_back_to_the_builtin() {
        let root = TestRoot::new();
        let server = server(&root.path, "minecraft-java");
        root.catalog(&[]);
        std::fs::write(root.path.join("games/custom_games.json"), "[").unwrap();

        let error = resolve_game(&root.path, &server).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(!game_snapshot_path(&root.path, &server.id).exists());
    }

    #[test]
    fn invalid_or_mismatched_snapshot_is_reported_without_overwriting_it() {
        let root = TestRoot::new();
        let server = server(&root.path, "minecraft-java");
        let other_game = GameConfig::default();
        save_game_snapshot(&root.path, &server.id, &other_game).unwrap();

        let error = resolve_game(&root.path, &server).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(load_game_snapshot(&root.path, &server.id).unwrap().game_type, other_game.game_type);

        std::fs::write(game_snapshot_path(&root.path, &server.id), "{").unwrap();
        assert_eq!(resolve_game(&root.path, &server).unwrap_err().kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(std::fs::read_to_string(game_snapshot_path(&root.path, &server.id)).unwrap(), "{");
    }
}
