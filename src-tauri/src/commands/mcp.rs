use crate::commands::runtime_error::RuntimeActionError;
use crate::services::computer::{ComputerRuntimeAction, McpServerManagedBy};
use crate::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;

/// Server status returned to frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerStatus {
    pub name: String,
    pub running: bool,
    pub status_message: String,
    pub disabled: bool,
    #[serde(rename = "managedBy")]
    pub managed_by: McpServerManagedBy,
}

#[derive(Debug, Clone)]
struct McpServerRuntimeMetadata {
    disabled: bool,
    managed_by: McpServerManagedBy,
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
    state
        .config
        .get_computer_instance(instance_id)
        .map_err(|error| error.to_string())?;
    let runtime = state
        .computer_registry
        .runtime(instance_id)
        .await
        .ok_or_else(|| format!("Computer instance not found: {instance_id}"))?;
    let runtime_statuses: std::collections::HashMap<_, _> = runtime
        .mcp_server_statuses()
        .await
        .into_iter()
        .map(|(name, running, status_message)| (name, (running, status_message)))
        .collect();
    let mut statuses: Vec<_> = mcp_server_runtime_metadata(&runtime)
        .await
        .into_iter()
        .map(|(name, metadata)| {
            let (running, status_message) = runtime_statuses
                .get(&name)
                .cloned()
                .unwrap_or_else(|| (false, "Stopped".to_string()));
            McpServerStatus {
                disabled: metadata.disabled,
                name,
                running,
                status_message,
                managed_by: metadata.managed_by,
            }
        })
        .collect();
    statuses.sort_by(|left, right| left.name.cmp(&right.name));

    Ok(statuses)
}

#[tauri::command]
pub async fn start_mcp_server(
    state: State<'_, AppState>,
    instance_id: String,
    name: String,
) -> Result<(), RuntimeActionError> {
    start_mcp_server_core(&state, &instance_id, &name).await
}

pub async fn start_mcp_server_core(
    state: &AppState,
    instance_id: &str,
    name: &str,
) -> Result<(), RuntimeActionError> {
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let instance_id = require_instance_id(instance_id).map_err(RuntimeActionError::runtime)?;
    log::info!("Starting MCP server for instance {}: {}", instance_id, name);

    let runtime = require_runtime(state, instance_id)
        .await
        .map_err(RuntimeActionError::runtime)?;
    ensure_computer_started(&runtime)
        .await
        .map_err(RuntimeActionError::runtime)?;
    ensure_user_managed_server(name, &runtime)
        .await
        .map_err(RuntimeActionError::runtime)?;
    runtime
        .start_mcp_server(name)
        .await
        .map_err(RuntimeActionError::from)?;

    log::info!("MCP server started for instance {}: {}", instance_id, name);
    let _ = state.log_service.write_for_instance(
        "info",
        "mcp",
        &format!("Server started for instance {}: {}", instance_id, name),
        None,
        Some(instance_id),
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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let instance_id = require_instance_id(instance_id)?;
    log::info!("Stopping MCP server for instance {}: {}", instance_id, name);
    let runtime = require_runtime(state, instance_id).await?;
    ensure_computer_started(&runtime).await?;
    ensure_user_managed_server(name, &runtime).await?;
    runtime.stop_mcp_server(name).await?;

    log::info!("MCP server stopped for instance {}: {}", instance_id, name);
    let _ = state.log_service.write_for_instance(
        "info",
        "mcp",
        &format!("Server stopped for instance {}: {}", instance_id, name),
        None,
        Some(instance_id),
    );
    Ok(())
}

#[tauri::command]
pub async fn start_all_servers(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<(), RuntimeActionError> {
    start_all_servers_core(&state, &instance_id).await
}

pub async fn start_all_servers_core(
    state: &AppState,
    instance_id: &str,
) -> Result<(), RuntimeActionError> {
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let instance_id = require_instance_id(instance_id).map_err(RuntimeActionError::runtime)?;
    log::info!("Starting all MCP servers for instance {}", instance_id);

    let runtime = require_runtime(state, instance_id)
        .await
        .map_err(RuntimeActionError::runtime)?;
    ensure_computer_started(&runtime)
        .await
        .map_err(RuntimeActionError::runtime)?;
    for name in user_managed_server_names(&runtime).await {
        runtime
            .start_mcp_server(&name)
            .await
            .map_err(RuntimeActionError::from)?;
    }

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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let instance_id = require_instance_id(instance_id)?;
    log::info!("Stopping all MCP servers for instance {}", instance_id);

    let runtime = require_runtime(state, instance_id).await?;
    ensure_computer_started(&runtime).await?;
    for name in user_managed_server_names(&runtime).await {
        runtime.stop_mcp_server(&name).await?;
    }

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

async fn ensure_computer_started(
    runtime: &crate::services::computer::ComputerInstanceRuntime,
) -> Result<(), String> {
    runtime
        .ensure_runtime_action(ComputerRuntimeAction::ManageMcp)
        .await
        .map_err(|error| error.to_string())
}

async fn ensure_user_managed_server(
    name: &str,
    runtime: &crate::services::computer::ComputerInstanceRuntime,
) -> Result<(), String> {
    match mcp_server_runtime_metadata(runtime).await.get(name) {
        Some(metadata) if metadata.managed_by.is_plugin_owned() => Err(format!(
            "MCP server '{name}' is managed by a Marketplace plugin; manage its lifecycle from Marketplace"
        )),
        Some(_) => Ok(()),
        None => Err(format!("Server not found: {name}")),
    }
}

async fn require_runtime(
    state: &AppState,
    instance_id: &str,
) -> Result<crate::services::computer::ComputerInstanceRuntime, String> {
    state
        .computer_registry
        .runtime(instance_id)
        .await
        .ok_or_else(|| format!("Computer instance not found: {instance_id}"))
}

async fn user_managed_server_names(
    runtime: &crate::services::computer::ComputerInstanceRuntime,
) -> Vec<String> {
    let mut names: Vec<_> = mcp_server_runtime_metadata(runtime)
        .await
        .into_iter()
        .filter(|(_, metadata)| !metadata.managed_by.is_plugin_owned())
        .map(|(name, _)| name)
        .collect();
    names.sort();
    names
}

async fn mcp_server_runtime_metadata(
    runtime: &crate::services::computer::ComputerInstanceRuntime,
) -> std::collections::HashMap<String, McpServerRuntimeMetadata> {
    runtime
        .sdk_mcp_server_ownership()
        .await
        .into_iter()
        .filter_map(|entry| {
            crate::services::computer::sdk_managed_by_to_client(entry.managed_by).map(
                |managed_by| {
                    (
                        entry.name,
                        McpServerRuntimeMetadata {
                            disabled: entry.disabled,
                            managed_by,
                        },
                    )
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig;

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
