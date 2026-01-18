// Games manager - handles custom game definitions

use crate::games::config::{get_builtin_games, GameConfig, GameType};
use std::collections::HashMap;
use std::path::PathBuf;

pub struct GamesManager {
    builtin_games: HashMap<String, GameConfig>,
    custom_games: HashMap<String, GameConfig>,
    custom_games_path: PathBuf,
}

impl GamesManager {
    pub fn new() -> Self {
        let custom_games_path = get_games_config_path();
        let mut manager = Self {
            builtin_games: HashMap::new(),
            custom_games: HashMap::new(),
            custom_games_path,
        };
        manager.load_all_games();
        manager
    }

    fn load_all_games(&mut self) {
        // Load built-in games
        for game in get_builtin_games() {
            self.builtin_games.insert(game.game_type.0.clone(), game);
        }

        // Load custom games
        if let Ok(content) = std::fs::read_to_string(&self.custom_games_path) {
            if let Ok(custom_games) = serde_json::from_str::<Vec<GameConfig>>(&content) {
                for game in custom_games {
                    self.custom_games.insert(game.game_type.0.clone(), game);
                }
            }
        }
    }

    pub fn get_all_games(&self) -> Vec<GameConfig> {
        let mut games: Vec<GameConfig> = Vec::new();
        
        // Add built-in games (unless overridden by custom)
        for (id, game) in &self.builtin_games {
            if !self.custom_games.contains_key(id) {
                games.push(game.clone());
            }
        }
        
        // Add all custom games (including overrides)
        for game in self.custom_games.values() {
            games.push(game.clone());
        }
        
        // Sort: built-in first, then custom, alphabetically
        games.sort_by(|a, b| {
            match (a.is_custom, b.is_custom) {
                (false, true) => std::cmp::Ordering::Less,
                (true, false) => std::cmp::Ordering::Greater,
                _ => a.name.cmp(&b.name),
            }
        });
        
        games
    }

    pub fn get_game(&self, game_type: &GameType) -> Option<GameConfig> {
        // Custom games take precedence (overrides)
        if let Some(game) = self.custom_games.get(&game_type.0) {
            return Some(game.clone());
        }
        self.builtin_games.get(&game_type.0).cloned()
    }

    pub fn add_game(&mut self, mut game: GameConfig) -> Result<(), String> {
        if game.game_type.0.is_empty() {
            return Err("Game ID cannot be empty".to_string());
        }
        if game.docker_image.is_empty() {
            return Err("Docker image cannot be empty".to_string());
        }

        // Always mark as custom when adding
        game.is_custom = true;
        self.custom_games.insert(game.game_type.0.clone(), game);
        self.save_custom_games()
    }

    pub fn update_game(&mut self, mut game: GameConfig) -> Result<(), String> {
        // Check if game exists (in either built-in or custom)
        let exists = self.builtin_games.contains_key(&game.game_type.0) 
            || self.custom_games.contains_key(&game.game_type.0);
        
        if !exists {
            return Err("Game not found".to_string());
        }
        
        // Always save as custom (this creates an override for built-in games)
        game.is_custom = true;
        self.custom_games.insert(game.game_type.0.clone(), game);
        self.save_custom_games()
    }

    pub fn delete_game(&mut self, game_type: &GameType) -> Result<(), String> {
        // Can only delete from custom games
        if !self.custom_games.contains_key(&game_type.0) {
            // If it's a built-in game without custom override, can't delete
            if self.builtin_games.contains_key(&game_type.0) {
                return Err("Cannot delete built-in games. Edit it to create an override, or reset to defaults.".to_string());
            }
            return Err("Game not found".to_string());
        }
        
        self.custom_games.remove(&game_type.0);
        self.save_custom_games()
    }

    pub fn export_game(&self, game_type: &GameType) -> Result<String, String> {
        // Try custom first, then built-in
        let game = self.custom_games.get(&game_type.0)
            .or_else(|| self.builtin_games.get(&game_type.0))
            .ok_or("Game not found")?;
        serde_json::to_string_pretty(game).map_err(|e| e.to_string())
    }

    pub fn export_all_custom_games(&self) -> Result<String, String> {
        let custom_games: Vec<_> = self.custom_games.values().cloned().collect();
        serde_json::to_string_pretty(&custom_games).map_err(|e| e.to_string())
    }

    pub fn import_game(&mut self, json: &str) -> Result<GameConfig, String> {
        let mut game: GameConfig = serde_json::from_str(json)
            .map_err(|e| format!("Invalid JSON: {}", e))?;
        
        // Mark as custom
        game.is_custom = true;
        
        // Validate
        if game.game_type.0.is_empty() {
            return Err("Game ID cannot be empty".to_string());
        }
        if game.docker_image.is_empty() {
            return Err("Docker image cannot be empty".to_string());
        }

        self.custom_games.insert(game.game_type.0.clone(), game.clone());
        self.save_custom_games()?;
        Ok(game)
    }

    pub fn import_games(&mut self, json: &str) -> Result<Vec<GameConfig>, String> {
        let games: Vec<GameConfig> = serde_json::from_str(json)
            .map_err(|e| format!("Invalid JSON: {}", e))?;
        
        let mut imported = Vec::new();
        for mut game in games {
            game.is_custom = true;
            if !game.game_type.0.is_empty() && !game.docker_image.is_empty() {
                self.custom_games.insert(game.game_type.0.clone(), game.clone());
                imported.push(game);
            }
        }
        
        self.save_custom_games()?;
        Ok(imported)
    }

    fn save_custom_games(&self) -> Result<(), String> {
        let custom_games: Vec<_> = self.custom_games.values().cloned().collect();

        let content = serde_json::to_string_pretty(&custom_games)
            .map_err(|e| e.to_string())?;

        // Ensure directory exists
        if let Some(parent) = self.custom_games_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        std::fs::write(&self.custom_games_path, content)
            .map_err(|e| e.to_string())
    }

    pub fn reset_to_defaults(&mut self) -> Result<(), String> {
        // Clear all custom games
        self.custom_games.clear();

        // Delete custom games file
        if self.custom_games_path.exists() {
            std::fs::remove_file(&self.custom_games_path).ok();
        }

        Ok(())
    }
}

fn get_games_config_path() -> PathBuf {
    directories::UserDirs::new()
        .map(|d| d.home_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ServerWaveAnywhere")
        .join("games")
        .join("custom_games.json")
}
