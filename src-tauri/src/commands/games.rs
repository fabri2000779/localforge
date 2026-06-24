// Game-related commands

use crate::backend::NodeRegistry;
use crate::commands::require_backend;
use crate::games::{GameConfig, GameType, GamesManager};
use localforge_core::types::SystemMapping;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

pub struct GamesState {
    pub manager: Arc<Mutex<GamesManager>>,
}

impl Default for GamesState {
    fn default() -> Self {
        Self {
            manager: Arc::new(Mutex::new(GamesManager::new())),
        }
    }
}

/// List all available games
#[tauri::command]
pub async fn list_available_games(state: State<'_, GamesState>) -> Result<Vec<GameConfig>, String> {
    let manager = state.manager.lock().await;
    Ok(manager.get_all_games())
}

/// Get configuration for a specific game
#[tauri::command(rename_all = "camelCase")]
pub async fn get_game_config(
    game_type: String,
    state: State<'_, GamesState>,
) -> Result<Option<GameConfig>, String> {
    let manager = state.manager.lock().await;
    Ok(manager.get_game(&GameType::new(&game_type)))
}

/// Add a new custom game
#[tauri::command]
pub async fn add_custom_game(
    game: GameConfig,
    state: State<'_, GamesState>,
) -> Result<GameConfig, String> {
    let mut manager = state.manager.lock().await;
    let mut game = game;
    game.is_custom = true;
    manager.add_game(game.clone())?;
    Ok(game)
}

/// Update an existing game
#[tauri::command]
pub async fn update_game(
    game: GameConfig,
    state: State<'_, GamesState>,
) -> Result<GameConfig, String> {
    let mut manager = state.manager.lock().await;
    manager.update_game(game.clone())?;
    Ok(game)
}

/// Delete a custom game
#[tauri::command(rename_all = "camelCase")]
pub async fn delete_game(
    game_type: String,
    state: State<'_, GamesState>,
) -> Result<(), String> {
    let mut manager = state.manager.lock().await;
    manager.delete_game(&GameType::new(&game_type))
}

/// Export a game definition as JSON
#[tauri::command(rename_all = "camelCase")]
pub async fn export_game(
    game_type: String,
    state: State<'_, GamesState>,
) -> Result<String, String> {
    let manager = state.manager.lock().await;
    manager.export_game(&GameType::new(&game_type))
}

/// Export all custom games as JSON
#[tauri::command]
pub async fn export_all_custom_games(
    state: State<'_, GamesState>,
) -> Result<String, String> {
    let manager = state.manager.lock().await;
    manager.export_all_custom_games()
}

/// Import a game from JSON
#[tauri::command]
pub async fn import_game(
    json: String,
    state: State<'_, GamesState>,
) -> Result<GameConfig, String> {
    let mut manager = state.manager.lock().await;
    manager.import_game(&json)
}

/// Import multiple games from JSON
#[tauri::command]
pub async fn import_games(
    json: String,
    state: State<'_, GamesState>,
) -> Result<Vec<GameConfig>, String> {
    let mut manager = state.manager.lock().await;
    manager.import_games(&json)
}

/// Reset games to defaults (removes all custom games)
#[tauri::command]
pub async fn reset_games_to_defaults(
    state: State<'_, GamesState>,
) -> Result<(), String> {
    let mut manager = state.manager.lock().await;
    manager.reset_to_defaults()
}

/// Get the path to the games config folder (creates it if it doesn't exist)
#[tauri::command]
pub fn get_games_config_path() -> String {
    let path = directories::UserDirs::new()
        .map(|d| d.home_dir().to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("LocalForge")
        .join("games");
    
    // Create directory if it doesn't exist
    if !path.exists() {
        let _ = std::fs::create_dir_all(&path);
    }

    path.to_string_lossy().to_string()
}

/// Capture an existing server's current setup as a reusable Custom Game
/// template. Clones the server's game definition, bakes its current config
/// values in as the variable defaults, and saves it as a new custom game —
/// which the Games library can then one-click deploy (locally or to any node).
#[tauri::command(rename_all = "camelCase")]
pub async fn save_server_as_template(
    server_id: String,
    template_name: String,
    node_id: Option<String>,
    games: State<'_, GamesState>,
    registry: State<'_, NodeRegistry>,
) -> Result<GameConfig, String> {
    let backend = require_backend(&registry, node_id.as_deref()).await?;
    let server = backend
        .get_server(&server_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "server not found".to_string())?;

    let mut manager = games.manager.lock().await;
    let mut tpl = manager
        .get_game(&server.game_type)
        .ok_or_else(|| "no game definition for this server".to_string())?;

    // Fresh unique id so it never collides with the base game or other
    // templates; mark custom, name it, carry the server's RAM as the rec.
    tpl.game_type = GameType::new(&format!("tpl-{}", uuid::Uuid::new_v4()));
    tpl.name = template_name;
    tpl.is_custom = true;
    tpl.recommended_ram_mb = server.memory_mb;

    // Bake the server's chosen values in as the variable defaults — but leave
    // RAM/port-mapped variables alone (those are filled per-server at create).
    for v in tpl.variables.iter_mut() {
        if matches!(v.system_mapping, Some(SystemMapping::Ram) | Some(SystemMapping::Port)) {
            continue;
        }
        if let Some(val) = server.config.get(&v.env) {
            v.default = val.clone();
        }
    }

    manager.add_game(tpl.clone())?;
    Ok(tpl)
}
