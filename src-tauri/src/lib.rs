pub mod commands;
pub mod services;
pub mod tray;

use commands::connection::ConnectionState;
use services::computer::ComputerRegistry;
use services::config::ConfigService;
use services::logger::LogService;
use services::manager_client::ManagerClient;
use services::settings::SettingsService;
use smcp_computer::mcp_clients::model::MCPServerInput;
use smcp_computer::mcp_clients::MCPServerManager;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tauri::Manager;
use tauri_plugin_log::{Target, TargetKind, TimezoneStrategy};
use tokio::sync::RwLock;

/// Application state shared across all Tauri commands
pub struct AppState {
    /// MCP Server manager from smcp-computer (wrapped in Option for SmcpComputerClient compatibility)
    pub manager: Arc<RwLock<Option<MCPServerManager>>>,
    /// Configuration persistence service
    pub config: Arc<ConfigService>,
    /// Input definitions for SMCP (shared with SmcpComputerClient)
    pub inputs: Arc<RwLock<HashMap<String, MCPServerInput>>>,
    /// Active SMCP connection
    pub connection: Arc<RwLock<Option<ConnectionState>>>,
    /// Runtime registry for all configured Computer instances
    pub computer_registry: Arc<ComputerRegistry>,
    /// Log service for SQLite-backed logging
    pub log_service: Arc<LogService>,
    /// Settings persistence service
    pub settings_service: Arc<SettingsService>,
    /// TFRSManager HTTP client (login / list / connection-info)
    pub manager_client: Arc<ManagerClient>,
}

impl AppState {
    pub fn new(
        config: ConfigService,
        log_service: LogService,
        settings_service: SettingsService,
    ) -> Self {
        let instances = config.load_computer_instances().unwrap_or_else(|error| {
            log::error!(
                "Failed to load ComputerInstance configuration; starting with default instance: {}",
                error
            );
            Default::default()
        });
        let (computer_registry, default_runtime) =
            ComputerRegistry::from_config_with_default_runtime(instances);

        Self {
            manager: default_runtime.manager,
            config: Arc::new(config),
            inputs: default_runtime.inputs,
            connection: default_runtime.connection,
            computer_registry: Arc::new(computer_registry),
            log_service: Arc::new(log_service),
            settings_service: Arc::new(settings_service),
            manager_client: Arc::new(ManagerClient::new()),
        }
    }
}

/// Remove log files older than `retention_days` from the given directory.
fn cleanup_old_log_files(log_dir: &Path, retention_days: u64) {
    if let Ok(entries) = std::fs::read_dir(log_dir) {
        let cutoff =
            std::time::SystemTime::now() - std::time::Duration::from_secs(retention_days * 86400);
        for entry in entries.flatten() {
            if let Ok(metadata) = entry.metadata() {
                if let Ok(modified) = metadata.modified() {
                    if modified < cutoff {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .targets([
                    Target::new(TargetKind::Stdout),
                    Target::new(TargetKind::LogDir {
                        file_name: Some("tfrobot-client".into()),
                    }),
                    Target::new(TargetKind::Webview),
                ])
                .timezone_strategy(TimezoneStrategy::UseLocal)
                .level(log::LevelFilter::Info)
                .max_file_size(5_000_000) // 5MB per file
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepAll)
                .build(),
        )
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // When a second instance launches, focus the existing window
            if let Some(window) = app.get_webview_window("main") {
                window.show().ok();
                window.set_focus().ok();
            }
        }))
        .setup(|app| {
            // Clean up old log files from the system log directory
            if let Ok(log_dir) = app.path().app_log_dir() {
                cleanup_old_log_files(&log_dir, 3);
            }

            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to get app data directory");

            let config_service = ConfigService::new(app_data_dir.clone())
                .expect("Failed to initialize config service");

            let log_service =
                LogService::new(&app_data_dir).expect("Failed to initialize log service");

            let settings_service = SettingsService::new(app_data_dir.clone());

            // Use configured log retention days for cleanup
            let settings = settings_service.load();

            // Apply user's custom PATH override if configured
            if let Some(ref custom_path) = settings.custom_path {
                if !custom_path.is_empty() {
                    std::env::set_var("PATH", custom_path);
                    log::info!("PATH overridden by user setting");
                }
            } else {
                log::info!(
                    "PATH auto-detected ({} entries)",
                    std::env::var("PATH").unwrap_or_default().split(':').count()
                );
            }

            let saved_configs = config_service.load_configs().unwrap_or_else(|error| {
                log::error!(
                    "Failed to load MCP server configurations during startup: {}",
                    error
                );
                Vec::new()
            });
            log::info!("Loaded {} MCP server configurations", saved_configs.len());

            let state = AppState::new(config_service, log_service, settings_service);

            // Write startup log and cleanup old entries
            let _ = state
                .log_service
                .write("info", "system", "Application started", None);
            let _ = state
                .log_service
                .cleanup(settings.log_retention_days as i64);

            // Initialize the default Computer runtime with saved configs in background.
            let computer_registry = state.computer_registry.clone();
            tauri::async_runtime::spawn(async move {
                if let Some(runtime) = computer_registry.default_runtime().await {
                    if let Err(e) = runtime.start().await {
                        log::error!("Failed to initialize MCP servers: {}", e);
                        return;
                    }
                    log::info!("Default Computer runtime initialized");
                }
            });

            app.manage(state);

            // Setup system tray
            tray::setup_tray(app.handle())?;

            // Minimize to tray on window close
            let window = app.get_webview_window("main").unwrap();

            #[cfg(debug_assertions)]
            window.open_devtools();

            let app_handle = app.handle().clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    if let Some(w) = app_handle.get_webview_window("main") {
                        w.hide().ok();
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // MCP server management
            commands::mcp::get_mcp_servers,
            commands::mcp::get_mcp_server_config,
            commands::mcp::add_mcp_server,
            commands::mcp::remove_mcp_server,
            commands::mcp::update_mcp_server,
            commands::mcp::start_mcp_server,
            commands::mcp::stop_mcp_server,
            commands::mcp::start_all_servers,
            commands::mcp::stop_all_servers,
            // Input variable management
            commands::inputs::list_inputs,
            commands::inputs::get_input,
            commands::inputs::add_or_update_input,
            commands::inputs::remove_input,
            commands::inputs::list_input_values,
            commands::inputs::get_input_value,
            commands::inputs::set_input_value,
            commands::inputs::remove_input_value,
            commands::inputs::clear_input_values,
            commands::inputs::import_inputs,
            // SMCP connection management
            commands::connection::list_profiles,
            commands::connection::save_profile,
            commands::connection::delete_profile,
            commands::connection::connect_smcp,
            commands::connection::manager_connect_smcp,
            commands::connection::disconnect_smcp,
            commands::connection::get_connection_status,
            // Computer instance management
            commands::computer::list_computer_instances,
            commands::computer::get_computer_instance_status,
            commands::computer::create_computer_instance,
            commands::computer::rename_computer_instance,
            commands::computer::duplicate_computer_instance,
            commands::computer::delete_computer_instance,
            commands::computer::start_computer_instance,
            commands::computer::stop_computer_instance,
            // Config import/export
            commands::config_io::detect_config_format,
            commands::config_io::import_config,
            commands::config_io::export_config,
            // Debug & tools
            commands::debug::get_available_tools,
            commands::debug::execute_tool,
            commands::debug::get_tool_history,
            // Desktop resources
            commands::desktop::get_desktop,
            commands::desktop::get_window_detail,
            // Logs
            commands::logs::get_logs,
            commands::logs::export_logs,
            commands::logs::clear_logs,
            // Dashboard
            commands::dashboard::get_dashboard_data,
            // Settings
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::settings::detect_runtimes,
            commands::settings::get_app_info,
            commands::settings::get_detected_path,
            // TFRSManager HTTP client (issue #23)
            commands::manager::manager_login,
            commands::manager::manager_select_account,
            commands::manager::manager_list_digital_employees,
            commands::manager::manager_logout,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                // Graceful shutdown: close connections and log exit
                let state = app_handle.state::<AppState>();
                tauri::async_runtime::block_on(async {
                    state.computer_registry.shutdown_all().await;
                });
                let _ =
                    state
                        .log_service
                        .write("info", "system", "Application shutting down", None);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, SystemTime};
    use tempfile::TempDir;

    fn set_file_modified_time(path: &std::path::Path, time: SystemTime) {
        let since_epoch = time.duration_since(SystemTime::UNIX_EPOCH).unwrap();
        let ft = filetime::FileTime::from_unix_time(since_epoch.as_secs() as i64, 0);
        filetime::set_file_mtime(path, ft).unwrap();
    }

    #[test]
    fn test_cleanup_removes_old_log_files() {
        let dir = TempDir::new().unwrap();

        // Create a file modified 5 days ago
        let old_file = dir.path().join("old.log");
        fs::write(&old_file, "old log content").unwrap();
        let five_days_ago = SystemTime::now() - Duration::from_secs(5 * 86400);
        set_file_modified_time(&old_file, five_days_ago);

        // Create a recent file
        let new_file = dir.path().join("new.log");
        fs::write(&new_file, "new log content").unwrap();

        cleanup_old_log_files(dir.path(), 3);

        assert!(!old_file.exists(), "Old log file should be removed");
        assert!(new_file.exists(), "Recent log file should be kept");
    }

    #[test]
    fn test_cleanup_keeps_files_within_retention() {
        let dir = TempDir::new().unwrap();

        let file_1day = dir.path().join("recent.log");
        fs::write(&file_1day, "recent").unwrap();
        let one_day_ago = SystemTime::now() - Duration::from_secs(86400);
        set_file_modified_time(&file_1day, one_day_ago);

        let file_now = dir.path().join("now.log");
        fs::write(&file_now, "now").unwrap();

        cleanup_old_log_files(dir.path(), 3);

        assert!(file_1day.exists(), "1-day-old file should be kept");
        assert!(file_now.exists(), "Current file should be kept");
    }

    #[test]
    fn test_cleanup_handles_empty_directory() {
        let dir = TempDir::new().unwrap();
        // Should not panic
        cleanup_old_log_files(dir.path(), 3);
    }

    #[test]
    fn test_cleanup_handles_nonexistent_directory() {
        let path = std::path::Path::new("/tmp/nonexistent_log_dir_test_12345");
        // Should not panic
        cleanup_old_log_files(path, 3);
    }

    #[tokio::test]
    async fn app_state_uses_registry_default_runtime_handles() {
        let dir = TempDir::new().unwrap();
        let config = ConfigService::new(dir.path().to_path_buf()).unwrap();
        let log_service = LogService::new(dir.path()).unwrap();
        let settings_service = SettingsService::new(dir.path().to_path_buf());

        let state = AppState::new(config, log_service, settings_service);
        let default_runtime = state.computer_registry.default_runtime().await.unwrap();

        assert!(Arc::ptr_eq(&state.manager, &default_runtime.manager));
        assert!(Arc::ptr_eq(&state.inputs, &default_runtime.inputs));
        assert!(Arc::ptr_eq(&state.connection, &default_runtime.connection));
    }

    #[tokio::test]
    async fn default_runtime_start_marks_legacy_default_state_running() {
        let dir = TempDir::new().unwrap();
        let config = ConfigService::new(dir.path().to_path_buf()).unwrap();
        let log_service = LogService::new(dir.path()).unwrap();
        let settings_service = SettingsService::new(dir.path().to_path_buf());

        let state = AppState::new(config, log_service, settings_service);
        let default_runtime = state.computer_registry.default_runtime().await.unwrap();

        default_runtime.start().await.unwrap();

        assert!(default_runtime.is_running().await);
        assert!(Arc::ptr_eq(&state.manager, &default_runtime.manager));
    }
}
