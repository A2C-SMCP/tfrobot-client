use crate::services::computer::McpServerManagedBy;
use crate::AppState;
use a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig;
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
    let snapshot = state.sdk_config.load(instance_id);
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
    let runtime_metadata = mcp_server_runtime_metadata(&runtime).await;

    let mut statuses: Vec<_> = snapshot
        .mcp
        .servers
        .into_iter()
        .map(|server| {
            let config = server.config;
            let name = config.name().to_string();
            let (running, status_message) = runtime_statuses
                .get(&name)
                .cloned()
                .unwrap_or_else(|| (false, "Stopped".to_string()));
            let managed_by = runtime_metadata
                .get(&name)
                .map(|metadata| metadata.managed_by.clone())
                .unwrap_or(McpServerManagedBy::User);
            McpServerStatus {
                disabled: runtime_metadata
                    .get(&name)
                    .map(|metadata| metadata.disabled)
                    .unwrap_or(config.disabled()),
                name: name.clone(),
                running,
                status_message,
                managed_by,
            }
        })
        .collect();

    let configured_names: std::collections::HashSet<_> =
        statuses.iter().map(|status| status.name.clone()).collect();
    for (name, metadata) in runtime_metadata {
        if configured_names.contains(&name) {
            continue;
        }
        let (running, status_message) = runtime_statuses
            .get(&name)
            .cloned()
            .unwrap_or_else(|| (false, "Stopped".to_string()));
        statuses.push(McpServerStatus {
            name,
            running,
            status_message,
            disabled: metadata.disabled,
            managed_by: metadata.managed_by,
        });
    }

    Ok(statuses)
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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    add_mcp_server_locked(state, instance_id, config).await
}

pub(crate) async fn add_mcp_server_locked(
    state: &AppState,
    instance_id: &str,
    config: MCPServerConfig,
) -> Result<(), String> {
    let instance_id = require_instance_id(instance_id)?;
    let name = config.name().to_string();
    log::info!("Adding MCP server for instance {}: {}", instance_id, name);
    let runtime = require_runtime(state, instance_id).await?;
    ensure_no_plugin_managed_runtime_server(&runtime, &name).await?;
    // Computer validates/render-checks before its SDK-owned CRUD persist + runtime reload.
    runtime.add_or_update_server(config).await?;

    log::info!("MCP server added for instance {}: {}", instance_id, name);
    let _ = state.log_service.write_for_instance(
        "info",
        "mcp",
        &format!("Server added for instance {}: {}", instance_id, name),
        None,
        Some(instance_id),
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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let instance_id = require_instance_id(instance_id)?;
    log::info!("Removing MCP server for instance {}: {}", instance_id, name);
    let runtime = require_runtime(state, instance_id).await?;
    ensure_user_managed_server(state, instance_id, name, &runtime).await?;
    runtime.remove_server(name).await?;

    log::info!("MCP server removed for instance {}: {}", instance_id, name);
    let _ = state.log_service.write_for_instance(
        "info",
        "mcp",
        &format!("Server removed for instance {}: {}", instance_id, name),
        None,
        Some(instance_id),
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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let instance_id = require_instance_id(instance_id)?;
    let name = config.name().to_string();
    log::info!("Updating MCP server for instance {}: {}", instance_id, name);
    let runtime = require_runtime(state, instance_id).await?;
    ensure_user_managed_server(state, instance_id, &name, &runtime).await?;
    // Computer validates/render-checks before its SDK-owned CRUD persist + runtime reload.
    runtime.add_or_update_server(config).await?;

    log::info!("MCP server updated for instance {}: {}", instance_id, name);
    let _ = state.log_service.write_for_instance(
        "info",
        "mcp",
        &format!("Server updated for instance {}: {}", instance_id, name),
        None,
        Some(instance_id),
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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let instance_id = require_instance_id(instance_id)?;
    log::info!("Starting MCP server for instance {}: {}", instance_id, name);

    let runtime = require_runtime(state, instance_id).await?;
    ensure_user_managed_server(state, instance_id, name, &runtime).await?;
    ensure_computer_started(&runtime).await?;
    runtime.start_mcp_server(name).await?;

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
    ensure_user_managed_server(state, instance_id, name, &runtime).await?;
    ensure_computer_started(&runtime).await?;
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
) -> Result<(), String> {
    start_all_servers_core(&state, &instance_id).await
}

pub async fn start_all_servers_core(state: &AppState, instance_id: &str) -> Result<(), String> {
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let instance_id = require_instance_id(instance_id)?;
    log::info!("Starting all MCP servers for instance {}", instance_id);

    let runtime = require_runtime(state, instance_id).await?;
    ensure_computer_started(&runtime).await?;
    for server in user_managed_servers(state, instance_id, &runtime).await {
        runtime.start_mcp_server(server.name()).await?;
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
    for server in user_managed_servers(state, instance_id, &runtime).await {
        runtime.stop_mcp_server(server.name()).await?;
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
    if runtime.is_running().await {
        Ok(())
    } else {
        Err("请先启动 Computer".to_string())
    }
}

async fn ensure_user_managed_server(
    state: &AppState,
    instance_id: &str,
    name: &str,
    runtime: &crate::services::computer::ComputerInstanceRuntime,
) -> Result<(), String> {
    ensure_no_plugin_managed_runtime_server(runtime, name).await?;
    state
        .sdk_config
        .load(instance_id)
        .mcp
        .servers
        .into_iter()
        .find(|server| server.name == name)
        .ok_or_else(|| format!("Server not found: {name}"))?;
    Ok(())
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

async fn ensure_no_plugin_managed_runtime_server(
    runtime: &crate::services::computer::ComputerInstanceRuntime,
    name: &str,
) -> Result<(), String> {
    let metadata = mcp_server_runtime_metadata(runtime).await;
    if metadata
        .get(name)
        .is_some_and(|metadata| metadata.managed_by.is_plugin_owned())
    {
        return Err(format!(
            "MCP server '{}' is managed by a Marketplace plugin; manage its lifecycle from Marketplace",
            name
        ));
    }
    Ok(())
}

async fn user_managed_servers(
    state: &AppState,
    instance_id: &str,
    runtime: &crate::services::computer::ComputerInstanceRuntime,
) -> Vec<MCPServerConfig> {
    let metadata = mcp_server_runtime_metadata(runtime).await;
    state
        .sdk_config
        .load(instance_id)
        .mcp
        .servers
        .into_iter()
        .filter(|server| {
            !metadata
                .get(&server.name)
                .is_some_and(|metadata| metadata.managed_by.is_plugin_owned())
        })
        .map(|server| server.config)
        .collect()
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
