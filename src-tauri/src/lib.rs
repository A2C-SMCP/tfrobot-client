pub mod commands;
pub mod services;

use services::config::ConfigService;
use smcp_computer::mcp_clients::MCPServerManager;
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::RwLock;

/// Application state shared across all Tauri commands
pub struct AppState {
    /// MCP Server manager from smcp-computer
    pub manager: Arc<RwLock<MCPServerManager>>,
    /// Configuration persistence service
    pub config: Arc<ConfigService>,
}

impl AppState {
    pub fn new(config: ConfigService) -> Self {
        Self {
            manager: Arc::new(RwLock::new(MCPServerManager::new())),
            config: Arc::new(config),
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            // Initialize config service with app data directory
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to get app data directory");

            let config_service = ConfigService::new(app_data_dir.clone())
                .expect("Failed to initialize config service");

            // Load saved configurations
            let saved_configs = config_service.load_configs().unwrap_or_default();
            tracing::info!("Loaded {} MCP server configurations", saved_configs.len());

            // Create app state
            let state = AppState::new(config_service);

            // Initialize manager with saved configs in background
            let manager = state.manager.clone();
            let configs = saved_configs.clone();
            tauri::async_runtime::spawn(async move {
                let mgr = manager.read().await;
                if let Err(e) = mgr.initialize(configs).await {
                    tracing::error!("Failed to initialize MCP servers: {}", e);
                }
                tracing::info!("MCP servers initialized");
            });

            // Manage state
            app.manage(state);

            #[cfg(debug_assertions)]
            {
                let window = app.get_webview_window("main").unwrap();
                window.open_devtools();
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::mcp::get_mcp_servers,
            commands::mcp::add_mcp_server,
            commands::mcp::remove_mcp_server,
            commands::mcp::update_mcp_server,
            commands::mcp::start_mcp_server,
            commands::mcp::stop_mcp_server,
            commands::mcp::start_all_servers,
            commands::mcp::stop_all_servers,
            commands::connection::connect_smcp,
            commands::connection::disconnect_smcp,
            commands::connection::get_connection_status,
            commands::logs::get_logs,
            commands::logs::export_logs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
