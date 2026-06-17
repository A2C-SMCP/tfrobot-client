use crate::AppState;
use serde::{Deserialize, Serialize};
use smcp_computer::mcp_clients::MCPServerConfig;
use std::collections::HashMap;
use tauri::State;

/// Server status returned to frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerStatus {
    pub name: String,
    pub running: bool,
    pub status_message: String,
    pub disabled: bool,
}

#[tauri::command]
pub async fn get_mcp_servers(state: State<'_, AppState>) -> Result<Vec<McpServerStatus>, String> {
    let statuses = state.runtime.computer().get_server_status().await;
    let configs = state.config.load_configs().map_err(|e| e.to_string())?;

    Ok(build_mcp_server_statuses(statuses, &configs))
}

fn build_mcp_server_statuses(
    statuses: Vec<(String, bool, String)>,
    configs: &[MCPServerConfig],
) -> Vec<McpServerStatus> {
    let disabled_by_name: HashMap<&str, bool> = configs
        .iter()
        .map(|config| (config.name(), config_disabled(config)))
        .collect();

    statuses
        .into_iter()
        .map(|(name, running, status_message)| McpServerStatus {
            disabled: disabled_by_name
                .get(name.as_str())
                .copied()
                .unwrap_or(false),
            name,
            running,
            status_message,
        })
        .collect()
}

fn config_disabled(config: &MCPServerConfig) -> bool {
    match config {
        MCPServerConfig::Stdio(config) => config.disabled,
        MCPServerConfig::Http(config) => config.disabled,
        MCPServerConfig::Sse(config) => config.disabled,
    }
}

#[tauri::command]
pub async fn get_mcp_server_config(
    state: State<'_, AppState>,
    name: String,
) -> Result<MCPServerConfig, String> {
    let configs = state.config.load_configs().map_err(|e| e.to_string())?;
    configs
        .into_iter()
        .find(|c| c.name() == name)
        .ok_or(format!("Server not found: {}", name))
}

#[tauri::command]
pub async fn add_mcp_server(
    state: State<'_, AppState>,
    config: MCPServerConfig,
) -> Result<(), String> {
    add_mcp_server_core(&state, config).await
}

pub async fn add_mcp_server_core(state: &AppState, config: MCPServerConfig) -> Result<(), String> {
    let name = config.name().to_string();
    log::info!("Adding MCP server: {}", name);

    upsert_mcp_server_config(state, config).await?;

    log::info!("MCP server added: {}", name);
    let _ = state
        .log_service
        .write("info", "mcp", &format!("Server added: {}", name), None);
    Ok(())
}

#[tauri::command]
pub async fn remove_mcp_server(state: State<'_, AppState>, name: String) -> Result<(), String> {
    remove_mcp_server_core(&state, name).await
}

pub async fn remove_mcp_server_core(state: &AppState, name: String) -> Result<(), String> {
    log::info!("Removing MCP server: {}", name);

    let previous_config = state
        .config
        .load_configs()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|config| config.name() == name)
        .ok_or_else(|| format!("Server not found: {}", name))?;

    state
        .runtime
        .computer()
        .remove_server(&name)
        .await
        .map_err(|e| e.to_string())?;

    if let Err(e) = state.config.remove_config(&name) {
        restore_runtime_server(state, &name, Some(previous_config)).await;
        return Err(e.to_string());
    }
    sync_runtime_configs(state)?;

    log::info!("MCP server removed: {}", name);
    let _ = state
        .log_service
        .write("info", "mcp", &format!("Server removed: {}", name), None);
    Ok(())
}

#[tauri::command]
pub async fn update_mcp_server(
    state: State<'_, AppState>,
    config: MCPServerConfig,
) -> Result<(), String> {
    update_mcp_server_core(&state, config).await
}

pub async fn update_mcp_server_core(
    state: &AppState,
    config: MCPServerConfig,
) -> Result<(), String> {
    let name = config.name().to_string();
    log::info!("Updating MCP server: {}", name);

    upsert_mcp_server_config(state, config).await?;

    log::info!("MCP server updated: {}", name);
    let _ = state
        .log_service
        .write("info", "mcp", &format!("Server updated: {}", name), None);
    Ok(())
}

async fn upsert_mcp_server_config(state: &AppState, config: MCPServerConfig) -> Result<(), String> {
    let name = config.name().to_string();
    let previous_config = state
        .config
        .load_configs()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|existing| existing.name() == name);

    state
        .runtime
        .computer()
        .add_or_update_server(config.clone())
        .await
        .map_err(|e| e.to_string())?;

    if let Err(e) = state.config.add_config(config) {
        restore_runtime_server(state, &name, previous_config).await;
        return Err(e.to_string());
    }

    sync_runtime_configs(state)?;
    Ok(())
}

async fn restore_runtime_server(
    state: &AppState,
    name: &str,
    previous_config: Option<MCPServerConfig>,
) {
    let computer = state.runtime.computer();
    if let Some(previous_config) = previous_config {
        let _ = computer.add_or_update_server(previous_config).await;
    } else {
        let _ = computer.remove_server(name).await;
    }
}

fn sync_runtime_configs(state: &AppState) -> Result<(), String> {
    state
        .runtime
        .store_configs(state.config.load_configs().map_err(|e| e.to_string())?);
    Ok(())
}

#[tauri::command]
pub async fn start_mcp_server(state: State<'_, AppState>, name: String) -> Result<(), String> {
    log::info!("Starting MCP server: {}", name);

    state
        .runtime
        .computer()
        .start_mcp_client(&name)
        .await
        .map_err(|e| e.to_string())?;

    log::info!("MCP server started: {}", name);
    let _ = state
        .log_service
        .write("info", "mcp", &format!("Server started: {}", name), None);
    Ok(())
}

#[tauri::command]
pub async fn stop_mcp_server(state: State<'_, AppState>, name: String) -> Result<(), String> {
    log::info!("Stopping MCP server: {}", name);

    state
        .runtime
        .computer()
        .stop_mcp_client(&name)
        .await
        .map_err(|e| e.to_string())?;

    log::info!("MCP server stopped: {}", name);
    let _ = state
        .log_service
        .write("info", "mcp", &format!("Server stopped: {}", name), None);
    Ok(())
}

#[tauri::command]
pub async fn start_all_servers(state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Starting all MCP servers");

    state
        .runtime
        .computer()
        .start_mcp_client("all")
        .await
        .map_err(|e| e.to_string())?;

    log::info!("All MCP servers started");
    Ok(())
}

#[tauri::command]
pub async fn stop_all_servers(state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Stopping all MCP servers");

    state
        .runtime
        .computer()
        .stop_mcp_client("all")
        .await
        .map_err(|e| e.to_string())?;

    log::info!("All MCP servers stopped");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::config::ConfigService;
    use crate::services::logger::LogService;
    use crate::services::settings::SettingsService;
    use smcp_computer::mcp_clients::MCPServerConfig;

    fn test_state(tmp_path: &std::path::Path) -> AppState {
        AppState::new(
            ConfigService::new(tmp_path.to_path_buf()).expect("config service"),
            LogService::new(tmp_path).expect("log service"),
            SettingsService::new(tmp_path.to_path_buf()),
        )
    }

    fn stdio_config(name: &str, command: &str) -> MCPServerConfig {
        serde_json::from_value(serde_json::json!({
            "type": "Stdio",
            "name": name,
            "server_parameters": {
                "command": command,
                "args": [],
                "env": {}
            }
        }))
        .expect("stdio config")
    }

    #[cfg(unix)]
    fn make_servers_file_readonly(tmp_path: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;

        let servers_file = tmp_path.join("mcp_servers.json");
        if !servers_file.exists() {
            std::fs::write(&servers_file, "[]").expect("write servers file");
        }
        std::fs::set_permissions(&servers_file, std::fs::Permissions::from_mode(0o444))
            .expect("readonly servers file");
    }

    #[cfg(unix)]
    fn restore_servers_file_permissions(tmp_path: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;

        let _ = std::fs::set_permissions(
            tmp_path.join("mcp_servers.json"),
            std::fs::Permissions::from_mode(0o644),
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn add_mcp_server_persist_failure_rolls_back_runtime_config() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let state = test_state(tmp.path());
        make_servers_file_readonly(tmp.path());

        let err = add_mcp_server_core(&state, stdio_config("rollback-add", "node"))
            .await
            .expect_err("readonly config file should fail add");
        let active_configs = state.runtime.computer().list_mcp_servers().await;

        restore_servers_file_permissions(tmp.path());
        assert!(err.contains("Permission denied") || err.contains("permission denied"));
        assert!(active_configs.is_empty());
        assert_eq!(state.runtime.stored_config_count(), 0);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn update_mcp_server_persist_failure_restores_previous_runtime_config() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let state = test_state(tmp.path());
        add_mcp_server_core(&state, stdio_config("rollback-update", "node"))
            .await
            .expect("initial add");
        make_servers_file_readonly(tmp.path());

        let err = update_mcp_server_core(&state, stdio_config("rollback-update", "python"))
            .await
            .expect_err("readonly config file should fail update");
        let active_configs = state.runtime.computer().list_mcp_servers().await;

        restore_servers_file_permissions(tmp.path());
        assert!(err.contains("Permission denied") || err.contains("permission denied"));
        assert_eq!(active_configs.len(), 1);
        match &active_configs[0] {
            MCPServerConfig::Stdio(config) => {
                assert_eq!(config.server_parameters.command, "node");
            }
            other => panic!("expected stdio config, got {other:?}"),
        }
        assert_eq!(state.runtime.stored_config_count(), 1);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn remove_mcp_server_persist_failure_restores_runtime_config() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let state = test_state(tmp.path());
        add_mcp_server_core(&state, stdio_config("rollback-remove", "node"))
            .await
            .expect("initial add");
        make_servers_file_readonly(tmp.path());

        let err = remove_mcp_server_core(&state, "rollback-remove".to_string())
            .await
            .expect_err("readonly config file should fail remove");
        let active_configs = state.runtime.computer().list_mcp_servers().await;

        restore_servers_file_permissions(tmp.path());
        assert!(err.contains("Permission denied") || err.contains("permission denied"));
        assert_eq!(active_configs.len(), 1);
        assert_eq!(active_configs[0].name(), "rollback-remove");
        assert_eq!(state.runtime.stored_config_count(), 1);
    }

    #[test]
    fn test_stdio_config_from_frontend_json() {
        let json = serde_json::json!({
            "type": "Stdio",
            "name": "test-server",
            "disabled": false,
            "forbidden_tools": [],
            "tool_meta": {},
            "default_tool_meta": null,
            "vrl": null,
            "server_parameters": {
                "command": "npx",
                "args": ["-y", "@modelcontextprotocol/server-filesystem"],
                "env": { "HOME": "/tmp" },
                "cwd": "/workspace"
            }
        });

        let config: MCPServerConfig = serde_json::from_value(json)
            .expect("should deserialize Stdio config from frontend JSON");
        assert_eq!(config.name(), "test-server");
        match &config {
            MCPServerConfig::Stdio(c) => {
                assert_eq!(c.server_parameters.command, "npx");
                assert_eq!(
                    c.server_parameters.args,
                    vec!["-y", "@modelcontextprotocol/server-filesystem"]
                );
                assert_eq!(c.server_parameters.env.get("HOME").unwrap(), "/tmp");
                assert_eq!(c.server_parameters.cwd.as_deref(), Some("/workspace"));
                assert!(!c.disabled);
            }
            _ => panic!("expected Stdio variant"),
        }
    }

    #[test]
    fn test_http_config_from_frontend_json() {
        let json = serde_json::json!({
            "type": "Http",
            "name": "http-server",
            "disabled": false,
            "forbidden_tools": [],
            "tool_meta": {},
            "default_tool_meta": null,
            "vrl": null,
            "server_parameters": {
                "url": "https://api.example.com/mcp",
                "headers": { "Authorization": "Bearer token123" }
            }
        });

        let config: MCPServerConfig = serde_json::from_value(json)
            .expect("should deserialize Http config from frontend JSON");
        assert_eq!(config.name(), "http-server");
        match &config {
            MCPServerConfig::Http(c) => {
                assert_eq!(c.server_parameters.url, "https://api.example.com/mcp");
                assert_eq!(
                    c.server_parameters.headers.get("Authorization").unwrap(),
                    "Bearer token123"
                );
            }
            _ => panic!("expected Http variant"),
        }
    }

    #[test]
    fn test_sse_config_from_frontend_json() {
        let json = serde_json::json!({
            "type": "Sse",
            "name": "sse-server",
            "disabled": false,
            "forbidden_tools": [],
            "tool_meta": {},
            "default_tool_meta": null,
            "vrl": null,
            "server_parameters": {
                "url": "https://sse.example.com/events",
                "headers": {}
            }
        });

        let config: MCPServerConfig =
            serde_json::from_value(json).expect("should deserialize Sse config from frontend JSON");
        assert_eq!(config.name(), "sse-server");
        match &config {
            MCPServerConfig::Sse(c) => {
                assert_eq!(c.server_parameters.url, "https://sse.example.com/events");
            }
            _ => panic!("expected Sse variant"),
        }
    }

    #[test]
    fn test_lowercase_type_alias() {
        let json = serde_json::json!({
            "type": "stdio",
            "name": "lowercase-test",
            "server_parameters": {
                "command": "node",
                "args": ["server.js"],
                "env": {}
            }
        });

        let config: MCPServerConfig =
            serde_json::from_value(json).expect("should deserialize lowercase 'stdio' type alias");
        assert_eq!(config.name(), "lowercase-test");
        assert!(matches!(config, MCPServerConfig::Stdio(_)));
    }

    #[test]
    fn test_serde_round_trip() {
        let json = serde_json::json!({
            "type": "Stdio",
            "name": "roundtrip",
            "disabled": true,
            "forbidden_tools": ["dangerous_tool"],
            "tool_meta": {},
            "default_tool_meta": null,
            "vrl": null,
            "server_parameters": {
                "command": "python",
                "args": ["-m", "mcp_server"],
                "env": { "PYTHONPATH": "/lib" },
                "cwd": "/app"
            }
        });

        let config: MCPServerConfig = serde_json::from_value(json).expect("deserialize");
        let serialized = serde_json::to_value(&config).expect("serialize");
        let roundtrip: MCPServerConfig =
            serde_json::from_value(serialized.clone()).expect("deserialize again");

        assert_eq!(config, roundtrip);
        // Verify the serialized JSON has the expected structure
        assert_eq!(serialized["type"], "Stdio");
        assert_eq!(serialized["name"], "roundtrip");
        assert_eq!(serialized["server_parameters"]["command"], "python");
    }

    #[test]
    fn test_build_mcp_server_statuses_preserves_disabled_flag_from_config() {
        let disabled: MCPServerConfig = serde_json::from_value(serde_json::json!({
            "type": "Stdio",
            "name": "disabled-server",
            "disabled": true,
            "server_parameters": {
                "command": "node",
                "args": [],
                "env": {}
            }
        }))
        .expect("disabled config");
        let enabled: MCPServerConfig = serde_json::from_value(serde_json::json!({
            "type": "Http",
            "name": "enabled-server",
            "disabled": false,
            "server_parameters": {
                "url": "https://api.example.com/mcp",
                "headers": {}
            }
        }))
        .expect("enabled config");

        assert!(config_disabled(&disabled));
        assert!(!config_disabled(&enabled));

        let statuses = build_mcp_server_statuses(
            vec![
                ("disabled-server".to_string(), false, "Stopped".to_string()),
                ("enabled-server".to_string(), true, "Running".to_string()),
                ("runtime-only".to_string(), true, "Running".to_string()),
            ],
            &[disabled, enabled],
        );

        assert!(statuses[0].disabled);
        assert!(!statuses[1].disabled);
        assert!(!statuses[2].disabled);
    }

    #[test]
    fn test_deserialize_missing_required_fields() {
        // Missing server_parameters
        let json = serde_json::json!({
            "type": "Stdio",
            "name": "incomplete"
        });
        let result: Result<MCPServerConfig, _> = serde_json::from_value(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_http_serde_roundtrip() {
        let json = serde_json::json!({
            "type": "Http",
            "name": "http-roundtrip",
            "server_parameters": {
                "url": "https://api.example.com/mcp",
                "headers": { "X-Custom": "value" }
            }
        });
        let config: MCPServerConfig = serde_json::from_value(json).unwrap();
        let serialized = serde_json::to_value(&config).unwrap();
        let roundtrip: MCPServerConfig = serde_json::from_value(serialized).unwrap();
        assert_eq!(config, roundtrip);
    }

    #[test]
    fn test_sse_serde_roundtrip() {
        let json = serde_json::json!({
            "type": "Sse",
            "name": "sse-roundtrip",
            "server_parameters": {
                "url": "https://sse.example.com/events",
                "headers": {}
            }
        });
        let config: MCPServerConfig = serde_json::from_value(json).unwrap();
        let serialized = serde_json::to_value(&config).unwrap();
        let roundtrip: MCPServerConfig = serde_json::from_value(serialized).unwrap();
        assert_eq!(config, roundtrip);
    }
}
