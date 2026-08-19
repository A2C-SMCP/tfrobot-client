#![recursion_limit = "512"]

pub mod commands;
pub mod services;
pub mod tray;

use services::chat_session::ChatSessionService;
use services::client_computers::ClientComputersPaths;
use services::client_control::{
    ClientControlHost, ClientControlPlane, ObservabilityControlAuditSink,
};
use services::computer::{ComputerInstance, ComputerInstancesConfig, ComputerRegistry};
use services::config::ConfigService;
use services::config_migration::{migrate_legacy_config, MigrationError};
use services::keychain::{SecretStore, SystemSecretStore};
use services::manager_client::ManagerClient;
use services::manager_context::ManagerContextCoordinator;
use services::observability::{
    initialize_tracing, ActivityEventDraft, ActivityLevel, ActivityOutcome, Diagnostics,
    ObservabilityRetention, ObservabilityService,
};
use services::sdk_config::SdkConfigService;
use services::settings::SettingsService;
use std::path::Path;
use std::sync::Arc;
use tauri::Manager;
use tauri_plugin_log::{Target, TargetKind, TimezoneStrategy};
use tokio::sync::Mutex;

/// Application state shared across all Tauri commands
#[derive(Clone)]
pub struct AppState {
    /// Configuration persistence service
    pub config: Arc<ConfigService>,
    /// Adapter for SDK-owned per-Computer configuration.
    pub sdk_config: Arc<SdkConfigService>,
    /// Runtime registry for all configured Computer instances
    pub computer_registry: Arc<ComputerRegistry>,
    /// Protocol-neutral domain boundary shared by Tauri and the embedded Robot adapter.
    pub client_control: Arc<ClientControlPlane>,
    /// Secret persistence backend. Production uses the OS keychain; tests can inject memory.
    pub secret_store: Arc<dyn SecretStore>,
    /// Short-lived cross-Computer reservations for Robot/Office identities. Network connection
    /// work runs under each Computer's own lifecycle coordinator, never under this map lock.
    pub connection_target_reservations:
        Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>,
    /// Serializes Computer lifecycle transactions across profile, SDK storage, and runtime state.
    /// These operations are infrequent and must not observe one another half-committed.
    pub computer_lifecycle_lock: Arc<Mutex<()>>,
    /// Serializes per-Computer input definition/value mutations through runtime compensation.
    pub input_mutation_lock: Arc<Mutex<()>>,
    /// User-visible activity journal and durable tool-call audit history.
    pub observability: Arc<ObservabilityService>,
    /// Runtime diagnostic verbosity controller. Diagnostics never enter the activity database.
    pub diagnostics: Arc<Diagnostics>,
    /// Settings persistence service
    pub settings_service: Arc<SettingsService>,
    /// Backend-authoritative TFRSManager identity context and HTTP coordinator.
    pub manager_context: Arc<ManagerContextCoordinator>,
    /// Context-bound in-memory chat leases and narrow RobotServer HTTP BFF.
    pub chat_sessions: Arc<ChatSessionService>,
}

#[derive(Debug, thiserror::Error)]
pub enum AppStateInitError {
    #[error(transparent)]
    Migration(#[from] MigrationError),
    #[error("failed to discover Computer profiles: {0}")]
    Config(#[from] services::config::ConfigError),
    #[error("failed to access the system Keychain: {0}")]
    Keychain(#[from] services::keychain::KeychainError),
    #[error("failed to recover an interrupted configuration import: {0}")]
    ConfigImportRecovery(String),
    #[error("failed to migrate removed rust-sdk HTTP OAuth fields: {0}")]
    RemovedHttpOAuthMigration(String),
    #[error("failed to recover an interrupted Client Control Skill transaction: {0}")]
    SkillPackageRecovery(String),
}

impl AppState {
    pub fn new(
        config: ConfigService,
        observability: ObservabilityService,
        settings_service: SettingsService,
    ) -> Self {
        Self::try_new(config, observability, settings_service)
            .expect("failed to initialize application state")
    }

    pub fn try_new(
        config: ConfigService,
        observability: ObservabilityService,
        settings_service: SettingsService,
    ) -> Result<Self, AppStateInitError> {
        Self::try_new_with_secret_store(
            config,
            observability,
            settings_service,
            Arc::new(SystemSecretStore),
        )
    }

    pub fn new_with_secret_store(
        config: ConfigService,
        observability: ObservabilityService,
        settings_service: SettingsService,
        secret_store: Arc<dyn SecretStore>,
    ) -> Self {
        Self::try_new_with_secret_store(config, observability, settings_service, secret_store)
            .expect("failed to initialize application state")
    }

    pub fn try_new_with_secret_store(
        config: ConfigService,
        observability: ObservabilityService,
        settings_service: SettingsService,
        secret_store: Arc<dyn SecretStore>,
    ) -> Result<Self, AppStateInitError> {
        let config = Arc::new(config);
        let sdk_config = Arc::new(SdkConfigService::new(config.clone()));
        let initial_settings = settings_service.load();
        let diagnostics = Arc::new(Diagnostics::new(initial_settings.diagnostic_log_level));
        let settings_service = Arc::new(settings_service);
        let manager_client = Arc::new(ManagerClient::new_with_secret_store(secret_store.clone()));
        let manager_context = Arc::new(ManagerContextCoordinator::new(
            manager_client,
            settings_service.clone(),
        ));
        let chat_sessions = Arc::new(ChatSessionService::new_with_settings(
            Arc::downgrade(&manager_context),
            settings_service.clone(),
        ));

        migrate_legacy_config(
            config.as_ref(),
            sdk_config.as_ref(),
            settings_service.as_ref(),
            secret_store.as_ref(),
        )?;
        let stored_instances = config.load_computer_instances()?;
        for instance in &stored_instances.instances {
            let migration = sdk_config
                .migrate_removed_http_oauth_fields(&instance.id)
                .map_err(|error| {
                    AppStateInitError::RemovedHttpOAuthMigration(format!(
                        "Computer '{}': {error}",
                        instance.id
                    ))
                })?;
            if migration.removed_fields > 0 {
                log::info!(
                    "Migrated {} removed rust-sdk HTTP OAuth field(s) for Computer '{}'",
                    migration.removed_fields,
                    instance.id
                );
            }
            if migration.disabled_opt_out_servers > 0 {
                log::warn!(
                    "Disabled {} MCP server(s) for Computer '{}' because their legacy explicit OAuth opt-out cannot be represented by the automatic-only rust-sdk",
                    migration.disabled_opt_out_servers,
                    instance.id
                );
            }
        }
        commands::config_io::recover_pending_config_imports(
            config.as_ref(),
            sdk_config.as_ref(),
            stored_instances
                .instances
                .iter()
                .map(|instance| instance.id.clone()),
        )
        .map_err(AppStateInitError::ConfigImportRecovery)?;
        // Recovery may update per-Computer input definitions. Reload the profiles so every runtime is
        // hydrated from the recovered storage state rather than the pre-recovery discovery copy.
        let stored_instances = config.load_computer_instances()?;
        let instances = hydrate_computer_instances(stored_instances, secret_store.as_ref())?;
        let skill_home_base = config.computer_skill_home_base();
        let skill_packages = services::client_control::SkillPackageService::new();
        for instance in &instances.instances {
            let skill_home =
                services::computer::configured_skill_home_for_instance(instance, &skill_home_base);
            skill_packages
                .recover_home(&skill_home, &skill_home)
                .map_err(|error| AppStateInitError::SkillPackageRecovery(error.to_string()))?;
        }
        let computer_registry = Arc::new(
            ComputerRegistry::from_config_with_skill_home_base_and_secret_store(
                instances,
                skill_home_base,
                secret_store.clone(),
            ),
        );
        let observability = Arc::new(observability);
        let connection_target_reservations =
            Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let computer_lifecycle_lock = Arc::new(Mutex::new(()));
        let input_mutation_lock = Arc::new(Mutex::new(()));
        let client_control = Arc::new(ClientControlPlane::new_with_host(
            config.clone(),
            computer_registry.clone(),
            Arc::new(ObservabilityControlAuditSink::new(observability.clone())),
            ClientControlHost {
                sdk_config: sdk_config.clone(),
                secret_store: secret_store.clone(),
                connection_target_reservations: connection_target_reservations.clone(),
                computer_lifecycle_lock: computer_lifecycle_lock.clone(),
                input_mutation_lock: input_mutation_lock.clone(),
                observability: observability.clone(),
                diagnostics: diagnostics.clone(),
                settings_service: settings_service.clone(),
                manager_context: manager_context.clone(),
                chat_sessions: chat_sessions.clone(),
            },
        ));
        computer_registry.bind_client_control(&client_control);

        Ok(Self {
            config,
            sdk_config,
            computer_registry,
            client_control,
            secret_store: secret_store.clone(),
            connection_target_reservations,
            computer_lifecycle_lock,
            input_mutation_lock,
            observability,
            diagnostics,
            settings_service,
            manager_context,
            chat_sessions,
        })
    }

    pub fn hydrate_computer_instance(
        &self,
        instance: ComputerInstance,
    ) -> Result<ComputerInstance, services::keychain::KeychainError> {
        hydrate_computer_instance(instance, self.secret_store.as_ref())
    }

    pub fn load_hydrated_computer_instances(
        &self,
    ) -> Result<ComputerInstancesConfig, AppStateInitError> {
        Ok(hydrate_computer_instances(
            self.config.load_computer_instances()?,
            self.secret_store.as_ref(),
        )?)
    }
}

fn hydrate_computer_instances(
    mut config: ComputerInstancesConfig,
    secret_store: &dyn SecretStore,
) -> Result<ComputerInstancesConfig, services::keychain::KeychainError> {
    config.instances = config
        .instances
        .into_iter()
        .map(|instance| hydrate_computer_instance(instance, secret_store))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(config)
}

fn hydrate_computer_instance(
    mut instance: ComputerInstance,
    _secret_store: &dyn SecretStore,
) -> Result<ComputerInstance, services::keychain::KeychainError> {
    // Resolved values are intentionally never hydrated into the runtime profile. The SDK
    // requests them through RuntimeInputResolver only while rendering a referenced input.
    instance.input_values.clear();
    Ok(instance)
}

/// Remove log files older than `retention_days` from the given directory.
pub(crate) fn cleanup_old_log_files(log_dir: &Path, retention_days: u64) {
    if let Ok(entries) = std::fs::read_dir(log_dir) {
        let retention_days = retention_days.max(1);
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
    if let Err(error) = initialize_tracing() {
        eprintln!("failed to initialize structured diagnostics: {error}");
    }
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
                // The runtime max level is controlled by Diagnostics from persisted settings.
                .level(log::LevelFilter::Trace)
                .max_file_size(5_000_000) // 5MB per file
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepAll)
                .build(),
        )
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
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
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to get app data directory");

            let client_computers_paths = ClientComputersPaths::from_app_data_dir(&app_data_dir);
            let config_service = ConfigService::new_with_client_computers_paths(
                app_data_dir.clone(),
                client_computers_paths.clone(),
            )
            .expect("Failed to initialize config service");

            let observability = ObservabilityService::new(&app_data_dir)
                .expect("Failed to initialize observability database");

            let settings_service = SettingsService::new_with_client_computers_paths(
                app_data_dir.clone(),
                client_computers_paths,
            );

            // Use configured log retention days for cleanup
            let settings = settings_service.load();
            if let Ok(log_dir) = app.path().app_log_dir() {
                cleanup_old_log_files(&log_dir, settings.diagnostic_retention_days as u64);
            }

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

            let state = AppState::try_new(config_service, observability, settings_service)?;
            state.diagnostics.set_level(settings.diagnostic_log_level);
            tauri::async_runtime::block_on(state.manager_context.set_lifecycle_sink(Arc::new(
                commands::manager::TauriManagerContextLifecycleSink::new(
                    state.config.clone(),
                    state.computer_registry.clone(),
                    state.computer_lifecycle_lock.clone(),
                    state.chat_sessions.clone(),
                ),
            )));
            tauri::async_runtime::block_on(state.manager_context.set_event_sink(Arc::new(
                commands::manager::TauriManagerContextEventSink::new(app.handle().clone()),
            )));
            tauri::async_runtime::block_on(state.manager_context.set_token_bridge_sink(Arc::new(
                commands::manager::TauriManagerTokenBridgeSink::new(app.handle().clone()),
            )));
            commands::runtime_input::install_runtime_input_sink(app.handle().clone(), &state);

            // Write startup log and cleanup old entries
            if let Err(error) = state
                .observability
                .record_activity(&ActivityEventDraft::client(
                    ActivityLevel::Info,
                    "system",
                    "application_lifecycle",
                    "start",
                    ActivityOutcome::Succeeded,
                    "Application started",
                ))
            {
                log::error!("failed to persist startup activity: {error}");
            }
            if let Err(error) = state.observability.apply_retention(ObservabilityRetention {
                activity_days: settings.activity_retention_days,
                tool_history_days: settings.tool_history_retention_days,
            }) {
                log::error!("failed to apply observability retention: {error}");
            }

            log::info!("Configured Computer runtimes loaded; instances remain stopped");

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
            commands::mcp::authorize_mcp_server,
            commands::mcp::cancel_mcp_authorization,
            commands::mcp::clear_mcp_authorization,
            commands::sdk_config::get_computer_config_state,
            commands::sdk_config::upsert_computer_mcp_config,
            commands::sdk_config::upsert_computer_mcp_config_with_inputs,
            commands::sdk_config::remove_computer_mcp_config,
            commands::mcp::start_mcp_server,
            commands::mcp::stop_mcp_server,
            commands::mcp::start_all_servers,
            commands::mcp::stop_all_servers,
            // Skills marketplace governance
            commands::marketplace::get_marketplace_capabilities,
            commands::marketplace::get_marketplace_governance,
            commands::marketplace::add_marketplace,
            commands::marketplace::refresh_marketplace,
            commands::marketplace::remove_marketplace,
            commands::marketplace::update_marketplace,
            commands::marketplace::install_plugin,
            commands::marketplace::enable_plugin,
            commands::marketplace::disable_plugin,
            commands::marketplace::uninstall_plugin,
            // Skills inventory and content
            commands::skills::list_skills,
            commands::skills::get_skill,
            commands::skills::refresh_skills,
            commands::skills::open_local_skills_root,
            commands::skills::open_configured_local_skills_root,
            // Input variable management
            commands::inputs::list_inputs,
            commands::inputs::get_input,
            commands::inputs::get_runtime_input,
            commands::inputs::add_or_update_input,
            commands::inputs::remove_input,
            commands::inputs::list_input_reference_issues,
            commands::inputs::list_input_values,
            commands::inputs::list_input_entries,
            commands::inputs::upsert_input_entry,
            commands::inputs::delete_input_entry,
            commands::inputs::get_input_value,
            commands::inputs::set_input_value,
            commands::inputs::set_runtime_input_value,
            commands::inputs::remove_input_value,
            commands::inputs::clear_input_values,
            commands::inputs::import_inputs,
            commands::inputs::preview_command_input,
            commands::runtime_input::runtime_input_bridge_ready,
            commands::runtime_input::complete_runtime_input_request,
            // SMCP connection management
            commands::connection::list_manual_smcp_targets,
            commands::connection::save_manual_smcp_target,
            commands::connection::delete_manual_smcp_target,
            commands::connection::connect_connection_target,
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
            commands::computer::restart_computer_instance,
            commands::computer::update_computer_connection_policy,
            commands::computer::update_computer_skill_home,
            commands::computer::connect_computer_connection_target,
            commands::computer::disconnect_computer_connection_target,
            commands::client_control::get_client_control_catalog,
            commands::client_control::get_remote_control_policy,
            commands::client_control::update_remote_control_policy,
            commands::computer_runtime::enable_computer_runtime_events,
            commands::computer_runtime::get_computer_runtime_snapshots,
            // Config import/export
            commands::config_io::detect_config_format,
            commands::config_io::import_config,
            commands::config_io::export_config,
            // Debug & tools
            commands::debug::get_available_tools,
            commands::debug::get_debug_resources,
            commands::debug::execute_tool,
            commands::debug::get_tool_history,
            // Desktop resources
            commands::desktop::get_desktop,
            commands::desktop::get_window_detail,
            // Activity journal and diagnostics
            commands::activity::get_activity,
            commands::activity::export_activity,
            commands::activity::clear_activity,
            commands::diagnostics::set_diagnostic_log_level,
            // Dashboard
            commands::dashboard::get_dashboard_data,
            // Settings
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::settings::detect_runtimes,
            commands::settings::get_app_info,
            commands::settings::get_detected_path,
            // TFRSManager HTTP client (issue #23)
            commands::manager::manager_get_context,
            commands::manager::manager_restore_session,
            commands::manager::manager_login,
            commands::manager::manager_select_account,
            commands::manager::manager_list_accounts,
            commands::manager::manager_switch_account,
            commands::manager::manager_list_digital_employees,
            commands::manager::manager_logout,
            commands::manager::manager_token_bridge_ready,
            commands::manager::manager_token_bridge_http_request,
            commands::manager::manager_token_bridge_complete,
            // Context-bound Chat Kit session and HTTP BFF
            commands::chat::chat_open_session,
            commands::chat::chat_get_recent_robot,
            commands::chat::chat_remember_robot,
            commands::chat::chat_get_session_token,
            commands::chat::chat_invalidate_session,
            commands::chat::chat_http_request,
            commands::chat::chat_close_session,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                // Graceful shutdown: close connections and log exit
                let state = app_handle.state::<AppState>();
                tauri::async_runtime::block_on(async {
                    state.manager_context.force_token_bridge_not_ready().await;
                    state.chat_sessions.close_for_context(None).await;
                    state.computer_registry.shutdown_all().await;
                    if let Err(error) = state
                        .observability
                        .record_activity_async(ActivityEventDraft::client(
                            ActivityLevel::Info,
                            "system",
                            "application_lifecycle",
                            "shutdown",
                            ActivityOutcome::Succeeded,
                            "Application shutting down",
                        ))
                        .await
                    {
                        log::error!("failed to persist shutdown activity: {error}");
                    }
                });
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
    async fn app_state_allows_empty_computer_registry() {
        let dir = TempDir::new().unwrap();
        let config = ConfigService::new(dir.path().to_path_buf()).unwrap();
        let log_service = ObservabilityService::new(dir.path()).unwrap();
        let settings_service = SettingsService::new(dir.path().to_path_buf());

        let state = AppState::new(config, log_service, settings_service);

        assert!(state.computer_registry.list_runtimes().await.is_empty());
    }

    #[tokio::test]
    async fn app_state_loads_configured_computer_runtimes_stopped() {
        let dir = TempDir::new().unwrap();
        let config = ConfigService::new(dir.path().to_path_buf()).unwrap();
        config
            .add_computer_instance(services::computer::ComputerInstance::new("one", "One"))
            .unwrap();
        let log_service = ObservabilityService::new(dir.path()).unwrap();
        let settings_service = SettingsService::new(dir.path().to_path_buf());

        let state = AppState::new(config, log_service, settings_service);
        let runtime = state.computer_registry.runtime("one").await.unwrap();

        assert!(!runtime.is_running().await);
    }

    #[tokio::test]
    async fn app_state_migrates_removed_http_oauth_fields_before_runtime_discovery() {
        let dir = TempDir::new().unwrap();
        let config = ConfigService::new(dir.path().to_path_buf()).unwrap();
        config
            .add_computer_instance(services::computer::ComputerInstance::new("one", "One"))
            .unwrap();
        let sdk_config = SdkConfigService::new(Arc::new(
            ConfigService::new(dir.path().to_path_buf()).unwrap(),
        ));
        sdk_config
            .save(
                "one",
                &a2c_smcp::smcp_computer::settings::config::ProjectConfigDoc {
                    mcp: Some(
                        serde_json::json!({
                            "servers": {
                                "legacy": {
                                    "type": "streamable",
                                    "authPolicy": "auto",
                                    "oauth": {"client_name": "TFRobot"},
                                    "futureField": {"preserve": true},
                                    "server_parameters": {
                                        "url": "https://mcp.example/mcp"
                                    }
                                }
                            }
                        })
                        .as_object()
                        .unwrap()
                        .clone(),
                    ),
                    ..Default::default()
                },
            )
            .unwrap();

        let state = AppState::new(
            config,
            ObservabilityService::new(dir.path()).unwrap(),
            SettingsService::new(dir.path().to_path_buf()),
        );
        let migrated = state.sdk_config.load_raw_project_config("one").unwrap();
        let server = &migrated.mcp.as_ref().unwrap()["servers"]["legacy"];
        assert!(server.get("oauth").is_none());
        assert!(server.get("authPolicy").is_none());
        assert_eq!(server["futureField"], serde_json::json!({"preserve": true}));
        assert!(state.computer_registry.runtime("one").await.is_some());
    }

    #[tokio::test]
    async fn app_state_recovers_skill_transaction_before_runtime_discovery() {
        let dir = TempDir::new().unwrap();
        let config = ConfigService::new(dir.path().to_path_buf()).unwrap();
        config
            .add_computer_instance(services::computer::ComputerInstance::new("one", "One"))
            .unwrap();
        let skill_home = config
            .computer_skill_home_base()
            .join("one")
            .join("skill_home");
        let transaction = skill_home
            .join(".client_control_transactions")
            .join("interrupted-update");
        let previous = transaction.join("previous");
        fs::create_dir_all(&previous).unwrap();
        fs::write(
            transaction.join("transaction.json"),
            r#"{"name":"recovered-skill","operation":"update"}"#,
        )
        .unwrap();
        fs::write(
            previous.join("SKILL.md"),
            "---\nname: recovered-skill\ndescription: recovered at startup\n---\nBody\n",
        )
        .unwrap();

        let state = AppState::new(
            config,
            ObservabilityService::new(dir.path()).unwrap(),
            SettingsService::new(dir.path().to_path_buf()),
        );
        assert!(skill_home.join("user/recovered-skill/SKILL.md").is_file());
        assert!(!transaction.exists());

        let runtime = state.computer_registry.runtime("one").await.unwrap();
        runtime.start().await.unwrap();
        let reader = runtime.sdk_skill_reader().unwrap();
        assert!(reader
            .skills()
            .await
            .iter()
            .any(|skill| skill.name == "recovered-skill"));
        drop(reader);
        runtime.shutdown().await;
    }
}
