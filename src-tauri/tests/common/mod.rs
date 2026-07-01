use std::path::PathBuf;
use tfrobot_client_lib::services::config::ConfigService;
use tfrobot_client_lib::services::keychain::InMemorySecretStore;
use tfrobot_client_lib::services::logger::LogService;
use tfrobot_client_lib::services::settings::SettingsService;
use tfrobot_client_lib::AppState;

/// Create an isolated test AppState with temp directories.
/// Each test gets its own tempdir for full isolation.
pub fn create_test_app_state(tmp_path: &std::path::Path) -> AppState {
    let config =
        ConfigService::new(tmp_path.to_path_buf()).expect("Failed to create ConfigService");
    let log_service = LogService::new(tmp_path).expect("Failed to create LogService");
    let settings_service = SettingsService::new(tmp_path.to_path_buf());

    AppState::new_with_secret_store(
        config,
        log_service,
        settings_service,
        InMemorySecretStore::shared(),
    )
}

/// Path to the echo MCP server index.js
#[allow(dead_code)]
pub fn echo_server_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/echo-mcp-server/index.js")
}

/// Build an MCPServerConfig for the echo server
#[allow(dead_code)]
pub fn echo_server_config(name: &str) -> a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig {
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

/// Path to the stderr-flood echo MCP server (Issue #19 regression)
#[allow(dead_code)]
pub fn stderr_flood_server_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/echo-mcp-server/index-stderr-flood.js")
}

/// Build an MCPServerConfig for the stderr-flood echo server.
/// This server writes >64 KB to stderr on startup and on each tool call,
/// reproducing the pipe-buffer deadlock from Issue #19.
#[allow(dead_code)]
pub fn stderr_flood_server_config(
    name: &str,
) -> a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig {
    let server_path = stderr_flood_server_path();
    serde_json::from_value(serde_json::json!({
        "type": "Stdio",
        "name": name,
        "server_parameters": {
            "command": "node",
            "args": [server_path.to_str().unwrap()],
            "env": {}
        }
    }))
    .expect("Failed to build stderr-flood server config")
}

/// Path to the slow echo MCP server used for timeout assertions.
#[allow(dead_code)]
pub fn slow_echo_server_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/echo-mcp-server/index-slow.js")
}

/// Build an MCPServerConfig for the slow echo server.
#[allow(dead_code)]
pub fn slow_echo_server_config(
    name: &str,
) -> a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig {
    let server_path = slow_echo_server_path();
    serde_json::from_value(serde_json::json!({
        "type": "Stdio",
        "name": name,
        "server_parameters": {
            "command": "node",
            "args": [server_path.to_str().unwrap()],
            "env": {}
        }
    }))
    .expect("Failed to build slow echo server config")
}

/// Build an MCPServerConfig for the official server-everything package.
#[allow(dead_code)]
pub fn everything_server_config(
    name: &str,
) -> a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig {
    everything_server_config_with_forbidden_tools(name, &[])
}

/// Build an MCPServerConfig for server-everything with metadata changes.
#[allow(dead_code)]
pub fn everything_server_config_with_forbidden_tools(
    name: &str,
    forbidden_tools: &[&str],
) -> a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig {
    let forbidden_tools: Vec<String> = forbidden_tools
        .iter()
        .map(|tool| (*tool).to_string())
        .collect();

    serde_json::from_value(serde_json::json!({
        "type": "Stdio",
        "name": name,
        "forbidden_tools": forbidden_tools,
        "server_parameters": {
            "command": "npx",
            "args": ["-y", "@modelcontextprotocol/server-everything"],
            "env": {}
        }
    }))
    .expect("Failed to build server-everything config")
}
