use std::path::PathBuf;
use tfrobot_client_lib::services::config::ConfigService;
use tfrobot_client_lib::services::keychain::InMemorySecretStore;
use tfrobot_client_lib::services::observability::ObservabilityService;
use tfrobot_client_lib::services::settings::SettingsService;
use tfrobot_client_lib::AppState;

/// Create an isolated test AppState with temp directories.
/// Each test gets its own tempdir for full isolation.
pub fn create_test_app_state(tmp_path: &std::path::Path) -> AppState {
    let config =
        ConfigService::new(tmp_path.to_path_buf()).expect("Failed to create ConfigService");
    let log_service =
        ObservabilityService::new(tmp_path).expect("Failed to create observability service");
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

/// Path to the environment-reading MCP fixture used to verify process materialization.
#[allow(dead_code)]
pub fn env_server_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/echo-mcp-server/index-env.js")
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

/// Path to the multi-tool MCP server used for alias-collision assertions.
#[allow(dead_code)]
pub fn multi_tool_server_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/echo-mcp-server/index-multi-tool.js")
}

/// Build an MCPServerConfig for the multi-tool test server.
#[allow(dead_code)]
pub fn multi_tool_server_config(
    name: &str,
) -> a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig {
    let server_path = multi_tool_server_path();
    serde_json::from_value(serde_json::json!({
        "type": "Stdio",
        "name": name,
        "server_parameters": {
            "command": "node",
            "args": [server_path.to_str().unwrap()],
            "env": {}
        }
    }))
    .expect("Failed to build multi-tool server config")
}

/// Compatibility helpers for runtime-focused integration tests.
///
/// Production config CRUD is intentionally exposed only through `commands::sdk_config`. These
/// test-only composites retain the old setup ergonomics while using the same live-apply boundary.
#[allow(dead_code)]
pub mod mcp {
    #[allow(unused_imports)]
    pub use tfrobot_client_lib::commands::mcp::*;

    use a2c_smcp::smcp_computer::mcp_clients::bundle_id::resolve_bundle_id;
    use a2c_smcp::smcp_computer::mcp_clients::model::BundleId;
    use a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig;
    use tfrobot_client_lib::commands::runtime_error::RuntimeActionError;
    use tfrobot_client_lib::AppState;

    pub fn get_mcp_server_config_core(
        state: &AppState,
        instance_id: &str,
        name: &str,
    ) -> Result<MCPServerConfig, String> {
        state
            .config
            .get_computer_instance(instance_id)
            .map_err(|error| error.to_string())?;
        state
            .sdk_config
            .load(instance_id)
            .mcp
            .servers
            .into_iter()
            .map(|server| server.config)
            .find(|config| config.name() == name)
            .ok_or_else(|| format!("Server not found: {name}"))
    }

    pub async fn add_mcp_server_core(
        state: &AppState,
        instance_id: &str,
        config: MCPServerConfig,
    ) -> Result<(), RuntimeActionError> {
        update_runtime_server_for_test(state, instance_id, config).await
    }

    pub async fn update_mcp_server_core(
        state: &AppState,
        instance_id: &str,
        config: MCPServerConfig,
    ) -> Result<(), RuntimeActionError> {
        update_runtime_server_for_test(state, instance_id, config).await
    }

    async fn update_runtime_server_for_test(
        state: &AppState,
        instance_id: &str,
        config: MCPServerConfig,
    ) -> Result<(), RuntimeActionError> {
        state
            .config
            .get_computer_instance(instance_id)
            .map_err(|error| RuntimeActionError::runtime(error.to_string()))?;
        let runtime = state
            .computer_registry
            .runtime(instance_id)
            .await
            .ok_or_else(|| RuntimeActionError::runtime("Computer runtime not found"))?;
        let bundle_id = resolve_bundle_id(&config);
        if runtime.plugin_mcp_server_owner(&bundle_id).await.is_some() {
            return Err(RuntimeActionError::runtime(format!(
                "MCP server '{}' is managed by a Marketplace plugin; manage its lifecycle from Marketplace",
                config.name()
            )));
        }
        tfrobot_client_lib::commands::sdk_config::upsert_computer_mcp_config_core(
            state,
            instance_id,
            config,
        )
        .await
    }

    pub async fn remove_mcp_server_core(
        state: &AppState,
        instance_id: &str,
        name: &str,
    ) -> Result<(), String> {
        state
            .config
            .get_computer_instance(instance_id)
            .map_err(|error| error.to_string())?;
        let runtime = state
            .computer_registry
            .runtime(instance_id)
            .await
            .ok_or_else(|| "Computer runtime not found".to_string())?;
        let bundle_id = runtime
            .sdk_mcp_server_ownership()
            .await
            .into_iter()
            .find(|server| server.name == name)
            .and_then(|server| BundleId::try_from(server.bundle_id).ok())
            .ok_or_else(|| format!("Server not found: {name}"))?;
        if runtime.plugin_mcp_server_owner(&bundle_id).await.is_some() {
            return Err(format!(
                "MCP server '{name}' is managed by a Marketplace plugin; manage its lifecycle from Marketplace"
            ));
        }
        tfrobot_client_lib::commands::sdk_config::remove_computer_mcp_config_core(
            state,
            instance_id,
            name,
        )
        .await?;
        if runtime.plugin_mcp_server_owner(&bundle_id).await.is_some() {
            runtime
                .start_mcp_server(&bundle_id)
                .await
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}
