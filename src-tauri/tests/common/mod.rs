use std::path::PathBuf;
use tfrobot_client_lib::services::config::ConfigService;
use tfrobot_client_lib::services::logger::LogService;
use tfrobot_client_lib::services::settings::SettingsService;
use tfrobot_client_lib::AppState;

/// Create an isolated test AppState with temp directories.
/// Each test gets its own tempdir for full isolation.
pub fn create_test_app_state(tmp_path: &std::path::Path) -> AppState {
    let config = ConfigService::new(tmp_path.to_path_buf())
        .expect("Failed to create ConfigService");
    let log_service = LogService::new(tmp_path)
        .expect("Failed to create LogService");
    let settings_service = SettingsService::new(tmp_path.to_path_buf());

    AppState::new(config, log_service, settings_service)
}

/// Path to the echo MCP server index.js
pub fn echo_server_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/echo-mcp-server/index.js")
}

/// Build an MCPServerConfig for the echo server
pub fn echo_server_config(name: &str) -> smcp_computer::mcp_clients::MCPServerConfig {
    let server_path = echo_server_path();
    serde_json::from_value(serde_json::json!({
        "type": "Stdio",
        "name": name,
        "server_parameters": {
            "command": "node",
            "args": [server_path.to_str().unwrap()],
            "env": {}
        }
    }))
    .expect("Failed to build echo server config")
}
