//! Player-administration Tauri commands.
//!
//! Live roster + moderation (kick/ban/op) for the active node's backend — local
//! Docker or an agent over HTTPS. Only games with a console/RCON/REST admin
//! surface report players; the rest return an empty list. The cloud is never
//! involved (the host talks to the running container directly).

use crate::backend::NodeRegistry;
use crate::commands::require_backend;
use localforge_core::types::{Player, PlayerAction};
use tauri::State;

#[tauri::command(rename_all = "camelCase")]
pub async fn list_players(
    server_id: String,
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<Vec<Player>, String> {
    let backend = require_backend(&state, node_id.as_deref()).await?;
    backend
        .list_players(&server_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn player_action(
    server_id: String,
    action: PlayerAction,
    node_id: Option<String>,
    state: State<'_, NodeRegistry>,
) -> Result<(), String> {
    let backend = require_backend(&state, node_id.as_deref()).await?;
    backend
        .player_action(&server_id, action)
        .await
        .map_err(|e| e.to_string())
}
