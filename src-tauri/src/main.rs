//! LocalForge desktop entry point.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backend;
mod backups;
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

    // Must run before anything reads the keychain; keyring 4 picks the backend at runtime.
    cloud::keychain::init();

    // The shared crate can't use env!() for the version (it would resolve to its own).
    localforge_cloud_client::init_user_agent(format!(
        "LocalForge/{} ({} {})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    ));

    tauri::Builder::default()
        // single-instance MUST be first: a second launch (how Win/Linux deliver `localforge://` URLs)
        // forwards its args here and exits before any other plugin starts.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            tracing::info!("[single-instance] second launch with args: {:?}", args);
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
            // Forward any localforge:// URL in argv to the deep-link handler.
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
            let app_data_dir = app.path().app_data_dir().unwrap_or_else(|_| {
                tracing::warn!("app_data_dir unavailable; falling back to current dir");
                std::path::PathBuf::from(".")
            });
            std::fs::create_dir_all(&app_data_dir).ok();

            // Bring up the local Docker backend and persisted remote nodes in the background.
            let data_root = paths::home_root();
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let state: tauri::State<NodeRegistry> = handle.state();
                match backend::LocalDockerBackend::connect(data_root).await {
                    Ok(b) => {
                        let arc = Arc::new(b);
                        state.install_local(arc.clone()).await;
                        // Host scheduler (idempotent); backup schedules resolve targets from the keychain at fire time.
                        let resolver: localforge_backend_local::BackupTargetResolver =
                            Arc::new(|id| crate::backups::find_target(id).map(|(_, t)| t));
                        localforge_backend_local::spawn_scheduler(arc.clone(), paths::home_root(), resolver);
                        localforge_backend_local::spawn_crash_watcher(arc, paths::home_root());
                        tracing::info!("Local Docker backend connected");
                    }
                    Err(e) => {
                        tracing::warn!("Local Docker backend unavailable at startup: {}", e);
                    }
                }
                if let Err(e) = state.load_remotes().await {
                    tracing::warn!("Failed to load remote nodes: {}", e);
                }
                // The node switcher fetched at mount, before this finished; tell it to re-fetch.
                let _ = handle.emit("nodes-changed", ());
            });

            // Deep-link listener. Linux/Windows also register the scheme at runtime for dev (idempotent).
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
            commands::server::send_command,
            commands::server::get_server_logs,
            commands::server::get_server_stats,
            commands::server::get_server_disk_usage,
            commands::server::attach_server,
            commands::server::detach_server,
            commands::server::update_server_config,
            commands::server::reinstall_server,
            commands::server::update_server_game,
            commands::server::check_needs_install,
            commands::docker::check_docker_status,
            commands::docker::get_docker_info,
            commands::games::list_available_games,
            commands::games::add_custom_game,
            commands::games::update_game,
            commands::games::delete_game,
            commands::games::export_game,
            commands::games::export_all_custom_games,
            commands::games::import_game,
            commands::games::import_games,
            commands::games::save_server_as_template,
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
            commands::files::download_file_to_local,
            commands::files::upload_file_from_local,
            commands::nodes::list_nodes,
            commands::nodes::get_this_machine,
            commands::nodes::set_machine_name,
            commands::nodes::set_machine_name_prompt_dismissed,
            commands::backups::cloud_list_backup_targets,
            commands::backups::cloud_add_backup_target,
            commands::backups::cloud_remove_backup_target,
            commands::backups::cloud_pull_backup_targets,
            commands::backups::cloud_backup_now,
            commands::backups::cloud_list_backups,
            commands::backups::cloud_restore_backup,
            commands::backups::cloud_delete_backup,
            commands::schedules::list_schedules,
            commands::schedules::upsert_schedule,
            commands::schedules::delete_schedule,
            commands::metrics::query_metrics,
            commands::crash::query_crash_events,
            commands::webhooks::list_webhooks,
            commands::webhooks::add_webhook,
            commands::webhooks::remove_webhook,
            commands::webhooks::set_webhook_enabled,
            commands::webhooks::test_webhook,
            commands::players::list_players,
            commands::players::player_action,
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
            cloud::vault::cloud_unlock_org_dek,
            cloud::vault::cloud_clear_org_dek,
            cloud::vault::cloud_process_grants,
            cloud::sync::cloud_sync_now,
            cloud::sync::cloud_sync_pull,
            cloud::sync::cloud_sync_delete_server,
            cloud::sync::cloud_sync_nodes_now,
            cloud::sync::cloud_sync_delete_node,
            cloud::sync::cloud_rotate_org_dek,
            cloud::relay::cloud_relay_start,
            cloud::relay::cloud_relay_stop,
            cloud::relay::cloud_relay_send_cmd,
            cloud::relay::cloud_relay_send_event,
            cloud::orgs::cloud_orgs_list,
            cloud::orgs::cloud_set_active_org,
            cloud::orgs::cloud_orgs_me,
            cloud::orgs::cloud_orgs_invite,
            cloud::orgs::cloud_orgs_list_invitations,
            cloud::orgs::cloud_orgs_revoke_invitation,
            cloud::orgs::cloud_orgs_remove_member,
            cloud::orgs::cloud_member_scopes_get,
            cloud::orgs::cloud_member_scopes_set,
            cloud::orgs::cloud_orgs_accept_invite,
            cloud::nodes::cloud_node_create,
            cloud::nodes::cloud_node_list,
            cloud::nodes::cloud_list_machines,
            cloud::nodes::cloud_node_revoke,
            cloud::nodes::cloud_claim_desktop,
            cloud::audit::cloud_audit_emit,
            cloud::audit::cloud_audit_list,
            cloud::templates::cloud_template_publish,
            cloud::templates::cloud_templates_list,
            cloud::templates::cloud_template_get,
            cloud::templates::cloud_template_delete,
            cloud::push::cloud_push_notify,
            cloud::auth::cloud_export_data,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
