use crate::AppState;
use serde::{Deserialize, Serialize};
use smcp_computer::mcp_clients::MCPServerConfig;
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
    let lock = state.manager.read().await;
    let mgr = lock
        .as_ref()
        .ok_or("MCP manager not initialized".to_string())?;
    let statuses = mgr.get_server_status().await;

    Ok(statuses
        .into_iter()
        .map(|(name, running, status_message)| McpServerStatus {
            name,
            running,
            status_message,
            disabled: false,
        })
        .collect())
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
    let name = config.name().to_string();
    log::info!("Adding MCP server: {}", name);

    state
        .config
        .add_config(config.clone())
        .map_err(|e| e.to_string())?;

    let lock = state.manager.read().await;
    let mgr = lock
        .as_ref()
        .ok_or("MCP manager not initialized".to_string())?;
    mgr.add_or_update_server(config)
        .await
        .map_err(|e| e.to_string())?;

    log::info!("MCP server added: {}", name);
    let _ = state
        .log_service
        .write("info", "mcp", &format!("Server added: {}", name), None);
    Ok(())
}

#[tauri::command]
pub async fn remove_mcp_server(state: State<'_, AppState>, name: String) -> Result<(), String> {
    log::info!("Removing MCP server: {}", name);

    let lock = state.manager.read().await;
    let mgr = lock
        .as_ref()
        .ok_or("MCP manager not initialized".to_string())?;
    mgr.remove_server(&name).await.map_err(|e| e.to_string())?;

    state
        .config
        .remove_config(&name)
        .map_err(|e| e.to_string())?;

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
    let name = config.name().to_string();
    log::info!("Updating MCP server: {}", name);

    state
        .config
        .add_config(config.clone())
        .map_err(|e| e.to_string())?;

    let lock = state.manager.read().await;
    let mgr = lock
        .as_ref()
        .ok_or("MCP manager not initialized".to_string())?;
    mgr.add_or_update_server(config)
        .await
        .map_err(|e| e.to_string())?;

    log::info!("MCP server updated: {}", name);
    let _ = state
        .log_service
        .write("info", "mcp", &format!("Server updated: {}", name), None);
    Ok(())
}

#[tauri::command]
pub async fn start_mcp_server(state: State<'_, AppState>, name: String) -> Result<(), String> {
    log::info!("Starting MCP server: {}", name);

    let lock = state.manager.read().await;
    let mgr = lock
        .as_ref()
        .ok_or("MCP manager not initialized".to_string())?;
    mgr.start_client(&name).await.map_err(|e| e.to_string())?;

    log::info!("MCP server started: {}", name);
    let _ = state
        .log_service
        .write("info", "mcp", &format!("Server started: {}", name), None);
    Ok(())
}

#[tauri::command]
pub async fn stop_mcp_server(state: State<'_, AppState>, name: String) -> Result<(), String> {
    log::info!("Stopping MCP server: {}", name);

    let lock = state.manager.read().await;
    let mgr = lock
        .as_ref()
        .ok_or("MCP manager not initialized".to_string())?;
    mgr.stop_client(&name).await.map_err(|e| e.to_string())?;

    log::info!("MCP server stopped: {}", name);
    let _ = state
        .log_service
        .write("info", "mcp", &format!("Server stopped: {}", name), None);
    Ok(())
}

#[tauri::command]
pub async fn start_all_servers(state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Starting all MCP servers");

    let lock = state.manager.read().await;
    let mgr = lock
        .as_ref()
        .ok_or("MCP manager not initialized".to_string())?;
    mgr.start_all().await.map_err(|e| e.to_string())?;

    log::info!("All MCP servers started");
    Ok(())
}

#[tauri::command]
pub async fn stop_all_servers(state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Stopping all MCP servers");

    let lock = state.manager.read().await;
    let mgr = lock
        .as_ref()
        .ok_or("MCP manager not initialized".to_string())?;
    mgr.stop_all().await.map_err(|e| e.to_string())?;

    log::info!("All MCP servers stopped");
    Ok(())
}

#[cfg(test)]
mod tests {
    use smcp_computer::mcp_clients::MCPServerConfig;

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
