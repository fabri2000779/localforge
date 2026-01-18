// Serverwave Anywhere - Main entry point
// Run game servers anywhere, locally

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod docker;
mod games;

use commands::games::GamesState;
use commands::server::ServerState;
use tauri::Manager;
use tracing_subscriber::EnvFilter;

fn main() {
    // Configure logging to filter out noisy tao warnings
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| {
            EnvFilter::new("info")
                .add_directive("tao=error".parse().unwrap())
                .add_directive("wry=error".parse().unwrap())
        });
    
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(ServerState::default())
        .manage(GamesState::default())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir().expect("Failed to get app data dir");
            std::fs::create_dir_all(&app_data_dir).ok();

            if let Some(home) = directories::UserDirs::new() {
                let servers_dir = home.home_dir().join("ServerWaveAnywhere").join("servers");
                std::fs::create_dir_all(&servers_dir).ok();

                let config_dir = home.home_dir().join("ServerWaveAnywhere").join("config");
                std::fs::create_dir_all(&config_dir).ok();
            }

            tracing::info!("Serverwave Anywhere initialized");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::server::create_server,
            commands::server::start_server,
            commands::server::stop_server,
            commands::server::delete_server,
            commands::server::list_servers,
            commands::server::get_server_status,
            commands::server::send_command,
            commands::server::get_server_logs,
            commands::server::get_server_stats,
            commands::server::get_server_disk_usage,
            commands::server::attach_server,
            commands::server::detach_server,
            commands::server::update_server_config,
            commands::server::run_install_script,
            commands::server::reinstall_server,
            commands::server::update_server_game,
            commands::server::check_needs_install,
            commands::docker::check_docker_status,
            commands::docker::get_docker_info,
            commands::games::list_available_games,
            commands::games::get_game_config,
            commands::games::add_custom_game,
            commands::games::update_game,
            commands::games::delete_game,
            commands::games::export_game,
            commands::games::export_all_custom_games,
            commands::games::import_game,
            commands::games::import_games,
            commands::games::reset_games_to_defaults,
            commands::games::get_games_config_path,
            commands::files::list_directory,
            commands::files::read_file_text,
            commands::files::write_file_text,
            commands::files::create_file,
            commands::files::create_directory,
            commands::files::delete_path,
            commands::files::rename_path,
            commands::files::move_path,
            commands::files::copy_path,
            commands::files::get_file_info,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
