mod commands;
mod state;

use sr_core::account::AccountStore;
use sr_core::diagnostics;
use sr_core::download::DownloadManager;
use sr_core::java;
use sr_core::launch::LaunchManager;
use sr_core::modrinth::ModrinthService;
use sr_core::multiplayer::{GuestMonitor, MultiplayerManager};
use sr_core::paths::{AppPaths, GamePaths};
use sr_core::settings;
use sr_core::skin::SkinLibrary;
use sr_core::version::VersionService;
use state::AppState;
use std::sync::OnceLock;
use tauri::Manager;
use tokio::sync::RwLock;
use tracing_subscriber::EnvFilter;

static LOG_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

fn init_logging(logs_dir: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(logs_dir)?;
    let file_appender = tracing_appender::rolling::daily(logs_dir, "launcher.log");
    let (writer, guard) = tracing_appender::non_blocking(file_appender);
    let _ = LOG_GUARD.set(guard);
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,sr_core=debug,sr_launcher_lib=debug"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .try_init();
    Ok(())
}

pub fn run() {
    let builder = tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            init_logging(&data_dir.join("logs"))?;
            tracing::info!(?data_dir, "starting SR Launcher");

            let paths = AppPaths::new(data_dir);
            let http = reqwest::Client::builder()
                .user_agent(sr_core::USER_AGENT)
                .connect_timeout(std::time::Duration::from_secs(15))
                .timeout(std::time::Duration::from_secs(1800))
                .pool_max_idle_per_host(8)
                .build()?;

            let (settings_value, game) = tauri::async_runtime::block_on(async {
                paths.ensure_layout().await?;
                let settings = settings::load_or_default(&paths).await?;
                let game = GamePaths::new(settings.game_dir.clone());
                game.ensure_layout().await?;
                for message in sr_core::resources::recover_import_transactions(&game) {
                    tracing::warn!(%message, "recovered an interrupted modpack transaction");
                }
                Ok::<_, sr_core::AppError>((settings, game))
            })?;
            let multiplayer = std::sync::Arc::new(tauri::async_runtime::block_on(
                MultiplayerManager::new_with_bundle(
                    paths.clone(),
                    http.clone(),
                    state::multiplayer_sink(handle.clone()),
                    app.path()
                        .resource_dir()
                        .ok()
                        .map(|dir| dir.join("frpc"))
                        .filter(|dir| dir.is_dir()),
                ),
            ));

            let versions = VersionService::new(http.clone());
            let downloads = DownloadManager::with_store(
                state::task_sink(handle.clone()),
                paths.tasks_file(),
                settings_value.max_concurrent_downloads,
            );
            let accounts = AccountStore::new(paths.clone());
            let skins = SkinLibrary::new(paths.clone(), http.clone());
            let modrinth = ModrinthService::new(http.clone());
            let launch_sink = state::launch_sink(handle.clone());
            let launches = LaunchManager::new(
                game.clone(),
                versions.clone(),
                paths.logs_dir(),
                launch_sink.clone(),
            );

            let state = AppState {
                paths,
                settings: std::sync::Arc::new(RwLock::new(settings_value)),
                game: std::sync::Arc::new(RwLock::new(game)),
                versions,
                downloads,
                accounts,
                skins,
                modrinth,
                multiplayer,
                guest: GuestMonitor::new(),
                java: std::sync::Arc::new(RwLock::new(Vec::new())),
                launches: std::sync::Arc::new(RwLock::new(launches)),
                http,
                installing: std::sync::Arc::new(tokio::sync::Mutex::new(
                    std::collections::HashSet::new(),
                )),
                lifecycle: std::sync::Arc::new(tokio::sync::Mutex::new(
                    std::collections::HashMap::new(),
                )),
            };
            app.manage(state.clone());

            // Persist task history periodically; active tasks become
            // "interrupted" on the next start instead of being replayed.
            {
                let downloads = state.downloads.clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                        if let Err(e) = downloads.save().await {
                            tracing::warn!(error = %e, "failed to persist task history");
                        }
                    }
                });
            }

            // Discover Java in the background; the UI can refresh explicitly.
            let log_paths = state.paths.clone();
            tauri::async_runtime::spawn(async move {
                let settings = state.settings_snapshot().await;
                let found = java::discover_with_managed(
                    settings.java_path.as_deref(),
                    Some(&state.paths.java_dir()),
                )
                .await;
                tracing::info!(count = found.len(), "java discovery finished");
                *state.java.write().await = found;
            });

            // Keep launcher logs bounded: drop files older than the retention
            // window and trim the oldest ones if the directory is too large.
            {
                let paths = log_paths;
                tauri::async_runtime::spawn(async move {
                    match diagnostics::cleanup_logs(
                        &paths,
                        diagnostics::LOG_RETENTION_DAYS,
                        diagnostics::LOG_MAX_TOTAL_BYTES,
                    )
                    .await
                    {
                        Ok(report) if report.removed > 0 => {
                            tracing::info!(
                                removed = report.removed,
                                bytes = report.bytes,
                                "old logs cleaned"
                            )
                        }
                        Ok(_) => {}
                        Err(error) => tracing::warn!(%error, "log cleanup failed"),
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_bootstrap,
            commands::get_settings,
            commands::update_settings,
            commands::list_versions,
            commands::install_version,
            commands::list_tasks,
            commands::cancel_task,
            commands::clear_finished_tasks,
            commands::get_version_detail,
            commands::verify_version,
            commands::list_installed_versions,
            commands::get_version_delete_plan,
            commands::delete_version,
            commands::open_launcher_path,
            commands::list_instances,
            commands::create_instance,
            commands::update_instance,
            commands::delete_instance,
            commands::duplicate_instance,
            commands::export_instance_archive,
            commands::export_instance_modpack,
            commands::export_instance_curseforge_pack,
            commands::inspect_instance_archive,
            commands::import_instance_archive,
            commands::list_mods,
            commands::toggle_mod,
            commands::delete_mod,
            commands::import_mod,
            commands::import_mods,
            commands::import_mod_bytes,
            commands::list_worlds,
            commands::open_world_folder,
            commands::list_resourcepacks,
            commands::import_resourcepacks,
            commands::import_resourcepack_bytes,
            commands::set_resourcepack_enabled,
            commands::delete_resourcepack,
            commands::list_shaders,
            commands::toggle_shader,
            commands::delete_shader,
            commands::import_shaders,
            commands::import_shader_bytes,
            commands::shader_support,
            commands::install_iris,
            commands::install_optifine,
            commands::launch_instance,
            commands::stop_instance,
            commands::running_instances,
            commands::list_java,
            commands::add_java_path,
            commands::java_runtime_options,
            commands::install_java_runtime,
            commands::install_java_for_version,
            commands::java_launch_check,
            commands::check_java_for_version,
            commands::list_accounts,
            commands::add_offline_account,
            commands::remove_account,
            commands::set_active_account,
            commands::search_resources,
            commands::resource_game_versions,
            commands::get_resource_project,
            commands::get_resource_versions,
            commands::install_resource,
            commands::import_modpack,
            commands::import_modpack_bytes,
            commands::search_curseforge,
            commands::get_curseforge_project,
            commands::get_curseforge_files,
            commands::install_curseforge,
            commands::curseforge_key_status,
            commands::set_curseforge_api_key,
            commands::clear_curseforge_api_key,
            commands::test_curseforge_api_key,
            commands::default_resource_dir,
            commands::analyze_mod_dependencies,
            commands::install_mod_dependencies,
            commands::list_skins,
            commands::import_skin_file,
            commands::import_skin_bytes,
            commands::fetch_skin_by_name,
            commands::delete_skin,
            commands::set_account_skin,
            commands::apply_account_skin,
            commands::refresh_microsoft_account,
            commands::begin_microsoft_login,
            commands::poll_microsoft_login,
            commands::get_loader_recommendations,
            commands::open_trusted_url,
            commands::list_fabric_loaders,
            commands::install_fabric,
            commands::list_quilt_loaders,
            commands::install_quilt,
            commands::list_forge_versions,
            commands::list_neoforge_versions,
            commands::install_forge,
            commands::install_neoforge,
            commands::loader_resource_versions,
            commands::download_loader_resource,
            commands::install_loader_resource,
            commands::assistant_status,
            commands::assistant_conversations,
            commands::assistant_conversation,
            commands::assistant_delete_conversation,
            commands::set_deepseek_api_key,
            commands::clear_deepseek_api_key,
            commands::test_deepseek_api_key,
            commands::assistant_local_report,
            commands::assistant_chat,
            commands::assistant_repair_plan,
            commands::assistant_apply_repair,
            commands::assistant_rollback_repair,
            commands::assistant_mod_plan,
            commands::assistant_apply_mod_plan,
            commands::assistant_rollback_mod_install,
            commands::get_diagnostics,
            commands::list_logs,
            commands::read_log,
            commands::cleanup_logs,
            commands::delete_log,
            commands::export_diagnostics,
            commands::list_crash_reports,
            commands::analyze_crash,
            commands::delete_crash_report,
            commands::multiplayer_status,
            commands::multiplayer_connect,
            commands::multiplayer_logout,
            commands::multiplayer_nodes,
            commands::multiplayer_detect,
            commands::multiplayer_create_room,
            commands::multiplayer_stop_room,
            commands::multiplayer_restart_frpc,
            commands::multiplayer_orphans,
            commands::multiplayer_delete_orphan,
            commands::multiplayer_members,
            commands::multiplayer_logs,
            commands::multiplayer_clear_logs,
            commands::multiplayer_guest_ping,
            commands::multiplayer_guest_monitor_start,
            commands::multiplayer_guest_monitor_stop,
            commands::multiplayer_guest_monitor_status,
        ]);

    builder
        .build(tauri::generate_context!())
        .expect("failed to build SR Launcher")
        .run(|handle, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                let state = handle.state::<AppState>();
                tauri::async_runtime::block_on(async {
                    state.downloads.cancel_all().await;
                    let _ = state.downloads.save().await;
                    state.multiplayer.shutdown().await;
                });
                tracing::info!("application exiting");
            }
        });
}
