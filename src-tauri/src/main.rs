// LocalForge - Main entry point
// Run game servers locally with a single click

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backend;
mod cloud;
mod commands;
mod games;
mod paths;

use backend::NodeRegistry;
use commands::games::GamesState;
use commands::server::ServerState;
use std::sync::Arc;
use tauri::{Emitter, Manager};
use tauri_plugin_deep_link::DeepLinkExt;
use tracing_subscriber::EnvFilter;

fn main() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new("info")
            .add_directive("tao=error".parse().unwrap())
            .add_directive("wry=error".parse().unwrap())
    });

    tracing_subscriber::fmt().with_env_filter(filter).init();

    // Install the OS-native credential store BEFORE anything reads the
    // keychain (cloud login, sync-key storage, relay). keyring 4 selects
    // the backend at runtime, not via Cargo features.
    cloud::keychain::init();

    // Tell the cloud-client crate which product+version to advertise
    // in the User-Agent. The shared crate can't bake this in via
    // `env!()` because that would resolve to its own version (always
    // 0.1.x), which is not what the cloud-side logs want to see.
    localforge_cloud_client::init_user_agent(format!(
        "LocalForge/{} ({} {})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    ));

    tauri::Builder::default()
        // single-instance MUST be the first plugin: if a second copy
        // of LocalForge launches (which is what happens on Win/Linux
        // when the OS hands a `localforge://…` URL to its handler),
        // we want to forward its args to the running instance and
        // exit before any other plugin spins up — otherwise we end up
        // with multiple processes fighting for the keychain entry,
        // duplicate D1 backend connections, etc.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            tracing::info!("[single-instance] second launch with args: {:?}", args);
            // Bring the main window to the front so the user sees the
            // result of whatever deep-link they just clicked.
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
            // Forward any localforge:// URL in argv to the deep-link
            // handler. Windows passes the URL as the last arg; Linux
            // sometimes splits it weirdly — we just scan.
            for arg in args {
                if arg.starts_with("localforge://") {
                    let h = app.clone();
                    let url = arg.clone();
                    tauri::async_runtime::spawn(async move {
                        cloud::oauth::handle_deep_link(h, url).await;
                    });
                }
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_deep_link::init())
        .manage(ServerState::default())
        .manage(GamesState::default())
        .manage(NodeRegistry::default())
        .manage(Arc::new(cloud::relay::RelayState::default()))
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir().expect("Failed to get app data dir");
            std::fs::create_dir_all(&app_data_dir).ok();

            // Bring up the local Docker backend and any persisted remote
            // nodes in the background. If Docker is offline that leaves
            // the local slot in the registry empty and the UI shows the
            // Docker-required screen.
            let data_root = paths::home_root();
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let state: tauri::State<NodeRegistry> = handle.state();
                match backend::LocalDockerBackend::connect(data_root).await {
                    Ok(b) => {
                        state.install_local(Arc::new(b)).await;
                        tracing::info!("Local Docker backend connected");
                    }
                    Err(e) => {
                        tracing::warn!("Local Docker backend unavailable at startup: {}", e);
                    }
                }
                if let Err(e) = state.load_remotes().await {
                    tracing::warn!("Failed to load remote nodes: {}", e);
                }
                // Tell the UI the persisted remote nodes are now in the
                // registry, so the node switcher (which fetched at mount,
                // before this finished) re-fetches and shows them without
                // the user having to open the Nodes page first.
                let _ = handle.emit("nodes-changed", ());
            });

            // Deep-link listener: the OS hands us localforge://… URLs
            // through this callback. On Linux + Windows we ALSO have to
            // explicitly register the scheme at runtime in dev — the
            // installer takes care of it in release. `register()` is
            // idempotent so calling it on prod is fine too.
            #[cfg(any(target_os = "linux", windows))]
            {
                let _ = app.deep_link().register("localforge");
            }
            let dl_handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    let url_string = url.to_string();
                    let h = dl_handle.clone();
                    tauri::async_runtime::spawn(async move {
                        cloud::oauth::handle_deep_link(h, url_string).await;
                    });
                }
            });

            tracing::info!("LocalForge initialized");
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
            commands::files::download_file_to_local,
            commands::files::upload_file_from_local,
            commands::nodes::list_nodes,
            commands::nodes::get_this_machine,
            commands::nodes::set_machine_name,
            commands::nodes::test_remote_node,
            commands::nodes::add_remote_node,
            commands::nodes::remove_node,
            commands::nodes::reconnect_node,
            commands::nodes::agent_install_command,
            commands::nodes::cluster_summary,
            commands::nodes::get_node_stats,
            cloud::auth::cloud_signup,
            cloud::auth::cloud_login,
            cloud::auth::cloud_logout,
            cloud::auth::cloud_me,
            cloud::auth::cloud_request_password_reset,
            cloud::auth::cloud_resend_verification,
            cloud::oauth::cloud_oauth_start,
            cloud::billing::cloud_open_checkout,
            cloud::billing::cloud_open_portal,
            cloud::vault::cloud_vault_export_key,
            cloud::vault::cloud_vault_import_key,
            cloud::vault::cloud_vault_has_key,
            cloud::vault::cloud_sync_key_setup,
            cloud::vault::cloud_sync_key_unlock,
            cloud::vault::cloud_sync_key_status,
            cloud::sync::cloud_sync_now,
            cloud::sync::cloud_sync_pull,
            cloud::sync::cloud_sync_nodes_now,
            cloud::relay::cloud_relay_start,
            cloud::relay::cloud_relay_stop,
            cloud::relay::cloud_relay_send_cmd,
            cloud::relay::cloud_relay_send_event,
            cloud::orgs::cloud_orgs_list,
            cloud::orgs::cloud_orgs_me,
            cloud::orgs::cloud_orgs_invite,
            cloud::orgs::cloud_orgs_list_invitations,
            cloud::orgs::cloud_orgs_revoke_invitation,
            cloud::orgs::cloud_orgs_remove_member,
            cloud::orgs::cloud_orgs_accept_invite,
            cloud::nodes::cloud_node_create,
            cloud::nodes::cloud_node_list,
            cloud::nodes::cloud_list_machines,
            cloud::nodes::cloud_node_revoke,
            cloud::nodes::cloud_claim_desktop,
            cloud::audit::cloud_audit_emit,
            cloud::auth::cloud_export_data,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
