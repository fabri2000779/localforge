// Game-related commands

use crate::games::{GameConfig, GameType, GamesManager};
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
        .join("ServerWaveAnywhere")
        .join("games");
    
    // Create directory if it doesn't exist
    if !path.exists() {
        let _ = std::fs::create_dir_all(&path);
    }
    
    path.to_string_lossy().to_string()
}
