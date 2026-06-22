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
pub async fn get_mcp_servers(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Vec<McpServerStatus>, String> {
    get_mcp_servers_core(&state, &instance_id).await
}

pub async fn get_mcp_servers_core(
    state: &AppState,
    instance_id: &str,
) -> Result<Vec<McpServerStatus>, String> {
    let instance_id = require_instance_id(instance_id)?;
    let configs = state
        .config
        .load_configs_for_instance(instance_id)
        .map_err(|e| e.to_string())?;
    let runtime = state
        .computer_registry
        .runtime(instance_id)
        .await
        .ok_or_else(|| format!("Computer instance not found: {instance_id}"))?;
    let lock = runtime.manager.read().await;
    let mgr = lock
        .as_ref()
        .ok_or("MCP manager not initialized".to_string())?;
    let runtime_statuses: HashMap<_, _> = mgr
        .get_server_status()
        .await
        .into_iter()
        .map(|(name, running, status_message)| (name, (running, status_message)))
        .collect();

    Ok(configs
        .into_iter()
        .map(|config| {
            let name = config.name().to_string();
            let (running, status_message) = runtime_statuses
                .get(&name)
                .cloned()
                .unwrap_or_else(|| (false, "Stopped".to_string()));
            McpServerStatus {
                name,
                running,
                status_message,
                disabled: config.disabled(),
            }
        })
        .collect())
}

#[tauri::command]
pub async fn get_mcp_server_config(
    state: State<'_, AppState>,
    instance_id: String,
    name: String,
) -> Result<MCPServerConfig, String> {
    get_mcp_server_config_core(&state, &instance_id, &name)
}

pub fn get_mcp_server_config_core(
    state: &AppState,
    instance_id: &str,
    name: &str,
) -> Result<MCPServerConfig, String> {
    let instance_id = require_instance_id(instance_id)?;
    let configs = state
        .config
        .load_configs_for_instance(instance_id)
        .map_err(|e| e.to_string())?;
    configs
        .into_iter()
        .find(|c| c.name() == name)
        .ok_or(format!("Server not found: {}", name))
}

#[tauri::command]
pub async fn add_mcp_server(
    state: State<'_, AppState>,
    instance_id: String,
    config: MCPServerConfig,
) -> Result<(), String> {
    add_mcp_server_core(&state, &instance_id, config).await
}

pub async fn add_mcp_server_core(
    state: &AppState,
    instance_id: &str,
    config: MCPServerConfig,
) -> Result<(), String> {
    let instance_id = require_instance_id(instance_id)?;
    let name = config.name().to_string();
    log::info!("Adding MCP server for instance {}: {}", instance_id, name);

    let updated_instance = state
        .config
        .add_config_for_instance(instance_id, config.clone())
        .map_err(|e| e.to_string())?;

    let runtime = state
        .computer_registry
        .update_runtime_instance(updated_instance)
        .await;
    let lock = runtime.manager.read().await;
    let mgr = lock
        .as_ref()
        .ok_or("MCP manager not initialized".to_string())?;
    mgr.add_or_update_server(config)
        .await
        .map_err(|e| e.to_string())?;

    log::info!("MCP server added for instance {}: {}", instance_id, name);
    let _ = state.log_service.write(
        "info",
        "mcp",
        &format!("Server added for instance {}: {}", instance_id, name),
        None,
    );
    Ok(())
}

#[tauri::command]
pub async fn remove_mcp_server(
    state: State<'_, AppState>,
    instance_id: String,
    name: String,
) -> Result<(), String> {
    remove_mcp_server_core(&state, &instance_id, &name).await
}

pub async fn remove_mcp_server_core(
    state: &AppState,
    instance_id: &str,
    name: &str,
) -> Result<(), String> {
    let instance_id = require_instance_id(instance_id)?;
    log::info!("Removing MCP server for instance {}: {}", instance_id, name);

    let updated_instance = state
        .config
        .remove_config_for_instance(instance_id, name)
        .map_err(|e| e.to_string())?;
    let runtime = state
        .computer_registry
        .update_runtime_instance(updated_instance)
        .await;
    let lock = runtime.manager.read().await;
    let mgr = lock
        .as_ref()
        .ok_or("MCP manager not initialized".to_string())?;
    if let Err(error) = mgr.remove_server(name).await {
        log::warn!(
            "Failed to remove MCP server from runtime for instance {}: {}",
            instance_id,
            error
        );
    }

    log::info!("MCP server removed for instance {}: {}", instance_id, name);
    let _ = state.log_service.write(
        "info",
        "mcp",
        &format!("Server removed for instance {}: {}", instance_id, name),
        None,
    );
    Ok(())
}

#[tauri::command]
pub async fn update_mcp_server(
    state: State<'_, AppState>,
    instance_id: String,
    config: MCPServerConfig,
) -> Result<(), String> {
    update_mcp_server_core(&state, &instance_id, config).await
}

pub async fn update_mcp_server_core(
    state: &AppState,
    instance_id: &str,
    config: MCPServerConfig,
) -> Result<(), String> {
    let instance_id = require_instance_id(instance_id)?;
    let name = config.name().to_string();
    log::info!("Updating MCP server for instance {}: {}", instance_id, name);

    let updated_instance = state
        .config
        .add_config_for_instance(instance_id, config.clone())
        .map_err(|e| e.to_string())?;

    let runtime = state
        .computer_registry
        .update_runtime_instance(updated_instance)
        .await;
    let lock = runtime.manager.read().await;
    let mgr = lock
        .as_ref()
        .ok_or("MCP manager not initialized".to_string())?;
    mgr.add_or_update_server(config)
        .await
        .map_err(|e| e.to_string())?;

    log::info!("MCP server updated for instance {}: {}", instance_id, name);
    let _ = state.log_service.write(
        "info",
        "mcp",
        &format!("Server updated for instance {}: {}", instance_id, name),
        None,
    );
    Ok(())
}

#[tauri::command]
pub async fn start_mcp_server(
    state: State<'_, AppState>,
    instance_id: String,
    name: String,
) -> Result<(), String> {
    start_mcp_server_core(&state, &instance_id, &name).await
}

pub async fn start_mcp_server_core(
    state: &AppState,
    instance_id: &str,
    name: &str,
) -> Result<(), String> {
    let instance_id = require_instance_id(instance_id)?;
    log::info!("Starting MCP server for instance {}: {}", instance_id, name);

    let config = get_mcp_server_config_core(state, instance_id, name)?;
    let runtime = state
        .computer_registry
        .runtime(instance_id)
        .await
        .ok_or_else(|| format!("Computer instance not found: {instance_id}"))?;
    let lock = runtime.manager.read().await;
    let mgr = lock
        .as_ref()
        .ok_or("MCP manager not initialized".to_string())?;
    mgr.add_or_update_server(config)
        .await
        .map_err(|e| e.to_string())?;
    mgr.start_client(name).await.map_err(|e| e.to_string())?;

    log::info!("MCP server started for instance {}: {}", instance_id, name);
    let _ = state.log_service.write(
        "info",
        "mcp",
        &format!("Server started for instance {}: {}", instance_id, name),
        None,
    );
    Ok(())
}

#[tauri::command]
pub async fn stop_mcp_server(
    state: State<'_, AppState>,
    instance_id: String,
    name: String,
) -> Result<(), String> {
    stop_mcp_server_core(&state, &instance_id, &name).await
}

pub async fn stop_mcp_server_core(
    state: &AppState,
    instance_id: &str,
    name: &str,
) -> Result<(), String> {
    let instance_id = require_instance_id(instance_id)?;
    log::info!("Stopping MCP server for instance {}: {}", instance_id, name);

    let runtime = state
        .computer_registry
        .runtime(instance_id)
        .await
        .ok_or_else(|| format!("Computer instance not found: {instance_id}"))?;
    let lock = runtime.manager.read().await;
    let mgr = lock
        .as_ref()
        .ok_or("MCP manager not initialized".to_string())?;
    mgr.stop_client(name).await.map_err(|e| e.to_string())?;

    log::info!("MCP server stopped for instance {}: {}", instance_id, name);
    let _ = state.log_service.write(
        "info",
        "mcp",
        &format!("Server stopped for instance {}: {}", instance_id, name),
        None,
    );
    Ok(())
}

#[tauri::command]
pub async fn start_all_servers(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<(), String> {
    start_all_servers_core(&state, &instance_id).await
}

pub async fn start_all_servers_core(state: &AppState, instance_id: &str) -> Result<(), String> {
    let instance_id = require_instance_id(instance_id)?;
    log::info!("Starting all MCP servers for instance {}", instance_id);

    let instance = state
        .config
        .get_computer_instance(instance_id)
        .map_err(|e| e.to_string())?;
    let runtime = state
        .computer_registry
        .update_runtime_instance(instance.clone())
        .await;
    let lock = runtime.manager.read().await;
    let mgr = lock
        .as_ref()
        .ok_or("MCP manager not initialized".to_string())?;
    mgr.initialize(instance.mcp_servers)
        .await
        .map_err(|e| e.to_string())?;
    mgr.start_all().await.map_err(|e| e.to_string())?;

    log::info!("All MCP servers started for instance {}", instance_id);
    Ok(())
}

#[tauri::command]
pub async fn stop_all_servers(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<(), String> {
    stop_all_servers_core(&state, &instance_id).await
}

pub async fn stop_all_servers_core(state: &AppState, instance_id: &str) -> Result<(), String> {
    let instance_id = require_instance_id(instance_id)?;
    log::info!("Stopping all MCP servers for instance {}", instance_id);

    let runtime = state
        .computer_registry
        .runtime(instance_id)
        .await
        .ok_or_else(|| format!("Computer instance not found: {instance_id}"))?;
    let lock = runtime.manager.read().await;
    let mgr = lock
        .as_ref()
        .ok_or("MCP manager not initialized".to_string())?;
    mgr.stop_all().await.map_err(|e| e.to_string())?;

    log::info!("All MCP servers stopped for instance {}", instance_id);
    Ok(())
}

fn require_instance_id(instance_id: &str) -> Result<&str, String> {
    let instance_id = instance_id.trim();
    if instance_id.is_empty() {
        return Err("instance_id is required".to_string());
    }
    Ok(instance_id)
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
