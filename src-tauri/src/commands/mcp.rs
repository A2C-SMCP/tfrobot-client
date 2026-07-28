use crate::commands::runtime_error::RuntimeActionError;
use crate::services::computer::{
    ComputerRuntimeAction, ComputerRuntimeActionUnavailable, McpServerManagedBy,
};
use crate::AppState;
use a2c_smcp::smcp_computer::mcp_clients::bundle_id::resolve_bundle_id;
use a2c_smcp::smcp_computer::mcp_clients::model::BundleId;
use a2c_smcp::smcp_computer::settings::config::ProvenanceScope;
use serde::{Deserialize, Serialize};
use tauri::State;

/// Server status returned to frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerStatus {
    #[serde(rename = "bundleId")]
    pub bundle_id: BundleId,
    pub name: String,
    pub running: bool,
    pub status_message: String,
    pub disabled: bool,
    #[serde(rename = "managedBy")]
    pub managed_by: McpServerManagedBy,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct McpBatchFailure {
    #[serde(rename = "bundleId")]
    pub bundle_id: BundleId,
    pub name: String,
    pub error: RuntimeActionError,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct McpBatchOperationResult {
    pub candidate_count: usize,
    pub actual_operation_count: usize,
    pub unchanged_count: usize,
    pub excluded_plugin_owned_count: usize,
    pub failures: Vec<McpBatchFailure>,
}

#[derive(Debug, Clone)]
struct McpServerRuntimeMetadata {
    name: String,
    disabled: bool,
    managed_by: McpServerManagedBy,
}

#[derive(Debug, Clone)]
struct McpBatchCandidate {
    bundle_id: BundleId,
    name: String,
    running: bool,
}

#[derive(Debug, Clone)]
struct McpBatchInventory {
    candidates: Vec<McpBatchCandidate>,
    excluded_plugin_owned_count: usize,
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
        .map(|(bundle_id, _name, running, status_message)| (bundle_id, (running, status_message)))
        .collect();
    let diagnostics = runtime.mcp_start_diagnostics().await;
    let mut metadata = mcp_server_runtime_metadata(&runtime).await;
    for server in state.sdk_config.load(instance_id).mcp.servers {
        if server.origin == ProvenanceScope::Plugin {
            continue;
        }
        let bundle_id = resolve_bundle_id(&server.config);
        metadata
            .entry(bundle_id)
            .or_insert(McpServerRuntimeMetadata {
                name: server.name,
                disabled: server.config.disabled(),
                managed_by: McpServerManagedBy::User,
            });
    }
    let mut statuses: Vec<_> = metadata
        .into_iter()
        .filter(|(_, metadata)| metadata.managed_by.is_plugin_owned() || !metadata.disabled)
        .map(|(bundle_id, metadata)| {
            let (running, status_message) = runtime_statuses
                .get(&bundle_id)
                .cloned()
                .unwrap_or_else(|| {
                    (
                        false,
                        diagnostics
                            .get(&bundle_id)
                            .cloned()
                            .unwrap_or_else(|| "pending".to_string()),
                    )
                });
            McpServerStatus {
                disabled: metadata.disabled,
                bundle_id,
                name: metadata.name,
                running,
                status_message,
                managed_by: metadata.managed_by,
            }
        })
        .collect();
    statuses.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.bundle_id.cmp(&right.bundle_id))
    });

    Ok(statuses)
}

#[tauri::command]
pub async fn start_mcp_server(
    state: State<'_, AppState>,
    instance_id: String,
    bundle_id: BundleId,
) -> Result<(), RuntimeActionError> {
    start_mcp_server_core(&state, &instance_id, &bundle_id).await
}

pub async fn start_mcp_server_core(
    state: &AppState,
    instance_id: &str,
    bundle_id: &BundleId,
) -> Result<(), RuntimeActionError> {
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let instance_id = require_instance_id(instance_id).map_err(RuntimeActionError::runtime)?;
    log::info!(
        "Starting MCP server for instance {}: {}",
        instance_id,
        bundle_id
    );

    let runtime = require_runtime(state, instance_id)
        .await
        .map_err(RuntimeActionError::runtime)?;
    ensure_computer_started(&runtime)
        .await
        .map_err(RuntimeActionError::from)?;
    let server_name = ensure_user_managed_server(bundle_id, &runtime)
        .await
        .map_err(RuntimeActionError::runtime)?;
    runtime
        .start_mcp_server(bundle_id)
        .await
        .map_err(RuntimeActionError::from)?;

    log::info!(
        "MCP server started for instance {}: {}",
        instance_id,
        bundle_id
    );
    let _ = state.log_service.write_for_instance(
        "info",
        "mcp",
        &format!(
            "Server started for instance {}: {}",
            instance_id, server_name
        ),
        None,
        Some(instance_id),
    );
    Ok(())
}

#[tauri::command]
pub async fn stop_mcp_server(
    state: State<'_, AppState>,
    instance_id: String,
    bundle_id: BundleId,
) -> Result<(), RuntimeActionError> {
    stop_mcp_server_core(&state, &instance_id, &bundle_id).await
}

pub async fn stop_mcp_server_core(
    state: &AppState,
    instance_id: &str,
    bundle_id: &BundleId,
) -> Result<(), RuntimeActionError> {
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let instance_id = require_instance_id(instance_id).map_err(RuntimeActionError::runtime)?;
    log::info!(
        "Stopping MCP server for instance {}: {}",
        instance_id,
        bundle_id
    );
    let runtime = require_runtime(state, instance_id)
        .await
        .map_err(RuntimeActionError::runtime)?;
    ensure_computer_started(&runtime)
        .await
        .map_err(RuntimeActionError::from)?;
    let server_name = ensure_user_managed_server(bundle_id, &runtime)
        .await
        .map_err(RuntimeActionError::runtime)?;
    let stopped = runtime
        .stop_mcp_server(bundle_id)
        .await
        .map_err(RuntimeActionError::runtime)?;

    log::info!(
        "MCP server stop completed for instance {}: {} (changed={})",
        instance_id,
        bundle_id,
        stopped
    );
    let _ = state.log_service.write_for_instance(
        "info",
        "mcp",
        &format!(
            "Server stopped for instance {}: {}",
            instance_id, server_name
        ),
        None,
        Some(instance_id),
    );
    Ok(())
}

#[tauri::command]
pub async fn start_all_servers(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<McpBatchOperationResult, RuntimeActionError> {
    start_all_servers_core(&state, &instance_id).await
}

pub async fn start_all_servers_core(
    state: &AppState,
    instance_id: &str,
) -> Result<McpBatchOperationResult, RuntimeActionError> {
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let instance_id = require_instance_id(instance_id).map_err(RuntimeActionError::runtime)?;
    log::info!("Starting all MCP servers for instance {}", instance_id);

    let runtime = require_runtime(state, instance_id)
        .await
        .map_err(RuntimeActionError::runtime)?;
    ensure_computer_started(&runtime)
        .await
        .map_err(RuntimeActionError::from)?;
    let inventory = mcp_batch_inventory(&runtime).await;
    let candidate_count = inventory.candidates.len();
    let unchanged_count = inventory
        .candidates
        .iter()
        .filter(|candidate| candidate.running)
        .count();
    let operation_candidates: Vec<_> = inventory
        .candidates
        .into_iter()
        .filter(|candidate| !candidate.running)
        .collect();
    let operation_count = operation_candidates.len();
    let names: std::collections::HashMap<_, _> = operation_candidates
        .iter()
        .map(|candidate| (candidate.bundle_id.clone(), candidate.name.clone()))
        .collect();
    let failures = runtime
        .start_mcp_servers_best_effort(
            operation_candidates
                .into_iter()
                .map(|candidate| candidate.bundle_id)
                .collect(),
        )
        .await
        .into_iter()
        .map(|(bundle_id, error)| McpBatchFailure {
            name: names
                .get(&bundle_id)
                .cloned()
                .unwrap_or_else(|| bundle_id.to_string()),
            bundle_id,
            error: RuntimeActionError::from(error),
        })
        .collect::<Vec<_>>();
    let result = McpBatchOperationResult {
        candidate_count,
        actual_operation_count: operation_count.saturating_sub(failures.len()),
        unchanged_count,
        excluded_plugin_owned_count: inventory.excluded_plugin_owned_count,
        failures,
    };

    log::info!(
        "MCP start-all completed for instance {}: candidates={}, changed={}, unchanged={}, excluded_plugin_owned={}, failures={}",
        instance_id,
        result.candidate_count,
        result.actual_operation_count,
        result.unchanged_count,
        result.excluded_plugin_owned_count,
        result.failures.len()
    );
    Ok(result)
}

#[tauri::command]
pub async fn stop_all_servers(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<McpBatchOperationResult, RuntimeActionError> {
    stop_all_servers_core(&state, &instance_id).await
}

pub async fn stop_all_servers_core(
    state: &AppState,
    instance_id: &str,
) -> Result<McpBatchOperationResult, RuntimeActionError> {
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let instance_id = require_instance_id(instance_id).map_err(RuntimeActionError::runtime)?;
    log::info!("Stopping all MCP servers for instance {}", instance_id);

    let runtime = require_runtime(state, instance_id)
        .await
        .map_err(RuntimeActionError::runtime)?;
    ensure_computer_started(&runtime)
        .await
        .map_err(RuntimeActionError::from)?;
    let inventory = mcp_batch_inventory(&runtime).await;
    let operation_ids = inventory
        .candidates
        .iter()
        .filter(|candidate| candidate.running)
        .map(|candidate| candidate.bundle_id.clone())
        .collect();
    let operations = runtime.stop_mcp_servers_best_effort(operation_ids).await;
    let result = mcp_stop_batch_result(&inventory, operations);

    log::info!(
        "MCP stop-all completed for instance {}: candidates={}, changed={}, unchanged={}, excluded_plugin_owned={}, failures={}",
        instance_id,
        result.candidate_count,
        result.actual_operation_count,
        result.unchanged_count,
        result.excluded_plugin_owned_count,
        result.failures.len()
    );
    Ok(result)
}

fn mcp_stop_batch_result(
    inventory: &McpBatchInventory,
    operations: Vec<(BundleId, Result<bool, String>)>,
) -> McpBatchOperationResult {
    let names: std::collections::HashMap<_, _> = inventory
        .candidates
        .iter()
        .map(|candidate| (candidate.bundle_id.clone(), candidate.name.clone()))
        .collect();
    let mut result = McpBatchOperationResult {
        candidate_count: inventory.candidates.len(),
        actual_operation_count: 0,
        unchanged_count: inventory
            .candidates
            .iter()
            .filter(|candidate| !candidate.running)
            .count(),
        excluded_plugin_owned_count: inventory.excluded_plugin_owned_count,
        failures: Vec::new(),
    };
    for (bundle_id, operation) in operations {
        match operation {
            Ok(true) => result.actual_operation_count += 1,
            Ok(false) => result.unchanged_count += 1,
            Err(error) => result.failures.push(McpBatchFailure {
                name: names
                    .get(&bundle_id)
                    .cloned()
                    .unwrap_or_else(|| bundle_id.to_string()),
                bundle_id,
                error: RuntimeActionError::runtime(error),
            }),
        }
    }
    result
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
) -> Result<(), ComputerRuntimeActionUnavailable> {
    runtime
        .ensure_runtime_action(ComputerRuntimeAction::ManageMcp)
        .await
}

async fn ensure_user_managed_server(
    bundle_id: &BundleId,
    runtime: &crate::services::computer::ComputerInstanceRuntime,
) -> Result<String, String> {
    match mcp_server_runtime_metadata(runtime).await.get(bundle_id) {
        Some(metadata) if metadata.managed_by.is_plugin_owned() => Err(format!(
            "MCP server '{}' is managed by a Marketplace plugin; manage its lifecycle from Marketplace",
            metadata.name
        )),
        Some(metadata) => Ok(metadata.name.clone()),
        None => Err(format!("Server not found: {bundle_id}")),
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

async fn mcp_batch_inventory(
    runtime: &crate::services::computer::ComputerInstanceRuntime,
) -> McpBatchInventory {
    let running: std::collections::HashSet<_> = runtime
        .mcp_server_statuses()
        .await
        .into_iter()
        .filter_map(|(bundle_id, _name, running, _status)| running.then_some(bundle_id))
        .collect();
    let metadata = mcp_server_runtime_metadata(runtime).await;
    let excluded_plugin_owned_count = metadata
        .values()
        .filter(|metadata| metadata.managed_by.is_plugin_owned())
        .count();
    let mut candidates: Vec<_> = metadata
        .into_iter()
        .filter(|(_, metadata)| !metadata.managed_by.is_plugin_owned() && !metadata.disabled)
        .map(|(bundle_id, metadata)| McpBatchCandidate {
            running: running.contains(&bundle_id),
            bundle_id,
            name: metadata.name,
        })
        .collect();
    candidates.sort_by(|left, right| left.bundle_id.cmp(&right.bundle_id));
    McpBatchInventory {
        candidates,
        excluded_plugin_owned_count,
    }
}

async fn mcp_server_runtime_metadata(
    runtime: &crate::services::computer::ComputerInstanceRuntime,
) -> std::collections::HashMap<BundleId, McpServerRuntimeMetadata> {
    runtime
        .sdk_mcp_server_ownership()
        .await
        .into_iter()
        .filter_map(|entry| {
            let bundle_id = BundleId::try_from(entry.bundle_id.as_str()).ok()?;
            crate::services::computer::sdk_managed_by_to_client(entry.managed_by).map(
                |managed_by| {
                    (
                        bundle_id,
                        McpServerRuntimeMetadata {
                            name: entry.name,
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
    use super::*;
    use a2c_smcp::smcp_computer::errors::ComputerError;
    use a2c_smcp::smcp_computer::inputs::{InputKind, InputResolutionError};
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

    #[test]
    fn start_all_preserves_structured_missing_input_error() {
        let result = McpBatchOperationResult {
            candidate_count: 1,
            actual_operation_count: 0,
            unchanged_count: 0,
            excluded_plugin_owned_count: 0,
            failures: vec![McpBatchFailure {
                bundle_id: BundleId::try_from("input-backed").unwrap(),
                name: "Input-backed server".to_string(),
                error: RuntimeActionError::from(ComputerError::InputResolution(
                    InputResolutionError::Missing {
                        id: "runtime-token".to_string(),
                        kind: InputKind::Value,
                        env_hint: "A2C_SMCP_runtime_token".to_string(),
                    },
                )),
            }],
        };
        let serialized = serde_json::to_value(result).unwrap();

        assert_eq!(serialized["failures"][0]["error"]["code"], "missing_input");
        assert_eq!(
            serialized["failures"][0]["error"]["input_id"],
            "runtime-token"
        );
    }

    #[test]
    fn stop_all_summarizes_changed_unchanged_excluded_and_failed_candidates() {
        let bundle_id = |value: &str| BundleId::try_from(value).unwrap();
        let inventory = McpBatchInventory {
            candidates: vec![
                McpBatchCandidate {
                    bundle_id: bundle_id("stopped"),
                    name: "Stopped server".to_string(),
                    running: true,
                },
                McpBatchCandidate {
                    bundle_id: bundle_id("already-gone"),
                    name: "Already gone".to_string(),
                    running: true,
                },
                McpBatchCandidate {
                    bundle_id: bundle_id("broken"),
                    name: "Broken server".to_string(),
                    running: true,
                },
                McpBatchCandidate {
                    bundle_id: bundle_id("unchanged"),
                    name: "Unchanged server".to_string(),
                    running: false,
                },
            ],
            excluded_plugin_owned_count: 2,
        };

        let result = mcp_stop_batch_result(
            &inventory,
            vec![
                (bundle_id("stopped"), Ok(true)),
                (bundle_id("already-gone"), Ok(false)),
                (bundle_id("broken"), Err("disconnect failed".to_string())),
            ],
        );

        assert_eq!(result.candidate_count, 4);
        assert_eq!(result.actual_operation_count, 1);
        assert_eq!(result.unchanged_count, 2);
        assert_eq!(result.excluded_plugin_owned_count, 2);
        assert_eq!(result.failures.len(), 1);
        assert_eq!(result.failures[0].bundle_id, bundle_id("broken"));
        assert_eq!(result.failures[0].name, "Broken server");
        assert_eq!(result.failures[0].error.to_string(), "disconnect failed");
        assert_eq!(
            result.candidate_count,
            result.actual_operation_count + result.unchanged_count + result.failures.len()
        );
    }
}
