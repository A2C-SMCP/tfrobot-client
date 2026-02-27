pub mod commands;
pub mod services;

use commands::connection::ConnectionState;
use services::config::ConfigService;
use smcp_computer::mcp_clients::model::MCPServerInput;
use smcp_computer::mcp_clients::MCPServerManager;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::Manager;
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
}

impl AppState {
    pub fn new(config: ConfigService) -> Self {
        Self {
            manager: Arc::new(RwLock::new(Some(MCPServerManager::new()))),
            config: Arc::new(config),
            inputs: Arc::new(RwLock::new(HashMap::new())),
            connection: Arc::new(RwLock::new(None)),
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
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to get app data directory");

            let config_service = ConfigService::new(app_data_dir.clone())
                .expect("Failed to initialize config service");

            let saved_configs = config_service.load_configs().unwrap_or_default();
            tracing::info!("Loaded {} MCP server configurations", saved_configs.len());

            let state = AppState::new(config_service);

            // Initialize manager with saved configs in background
            let manager = state.manager.clone();
            let configs = saved_configs.clone();
            tauri::async_runtime::spawn(async move {
                let lock = manager.read().await;
                if let Some(mgr) = lock.as_ref() {
                    if let Err(e) = mgr.initialize(configs).await {
                        tracing::error!("Failed to initialize MCP servers: {}", e);
                    }
                    tracing::info!("MCP servers initialized");
                }
            });

            app.manage(state);

            #[cfg(debug_assertions)]
            {
                let window = app.get_webview_window("main").unwrap();
                window.open_devtools();
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // MCP server management
            commands::mcp::get_mcp_servers,
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
            commands::connection::disconnect_smcp,
            commands::connection::get_connection_status,
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
            // Logs
            commands::logs::get_logs,
            commands::logs::export_logs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
