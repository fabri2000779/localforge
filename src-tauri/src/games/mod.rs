//! Game catalogue: pulls shared types from `localforge_core` and adds a
//! local `GamesManager` that persists user-authored custom games.

mod manager;

pub use localforge_core::{build_env_vars, GameConfig, GameType};
pub use manager::GamesManager;
