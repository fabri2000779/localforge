//! Game catalogue: shared core types plus the persisted custom-game manager.

mod manager;

pub use localforge_core::{GameConfig, GameType};
pub use manager::GamesManager;
