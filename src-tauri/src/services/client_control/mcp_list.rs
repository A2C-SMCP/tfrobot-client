//! Remote-only projection of runtime status plus sanitized configuration declarations.
use crate::commands::mcp::{get_mcp_servers_core, McpServerStatus};
use crate::services::computer::McpServerManagedBy;
use crate::services::sdk_config::SdkConfigService;
use crate::AppState;
use a2c_smcp::smcp_computer::mcp_clients::bundle_id::resolve_bundle_id;
use a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig;
use a2c_smcp::smcp_computer::settings::config::{ComputerConfigSnapshot, ProvenanceScope};
use serde::Serialize;
use std::collections::HashMap;

pub(super) const TOOL_DESCRIPTION: &str = "List MCP server runtime statuses. Each item preserves the status fields and adds connection_config: {basis: 'configuration', transport: 'http'|'sse', url: string, headers: object} or null. These are current configuration declarations, NOT resolved secrets or proof of the active connection; unapplied edits may differ from runtime. Header literals and URL credentials are redacted; input references are preserved. Applies equally to user and Plugin HTTP/SSE servers. Missing headers are {}. Stdio, built-in servers and entries without a matching declaration have connection_config: null.";

#[derive(Serialize)]
pub(super) struct ServerView {
    #[serde(flatten)]
    status: McpServerStatus,
    connection_config: Option<ConnectionConfig>,
}

#[derive(Serialize)]
struct ConnectionConfig {
    basis: &'static str,
    transport: &'static str,
    url: String,
    headers: HashMap<String, String>,
}

pub(super) async fn list(state: &AppState, instance_id: &str) -> Result<Vec<ServerView>, String> {
    let statuses = get_mcp_servers_core(state, instance_id).await?;
    let (snapshot, _) = state
        .sdk_config
        .load_with_validation(instance_id)
        .map_err(|_| "Unable to read MCP configuration declarations".to_string())?;
    project(&state.sdk_config, statuses, snapshot)
}

fn project(
    service: &SdkConfigService,
    statuses: Vec<McpServerStatus>,
    snapshot: ComputerConfigSnapshot,
) -> Result<Vec<ServerView>, String> {
    // Resolve identity BEFORE sanitizing: fallback IDs hash connection parameters.
    // Names are not unique across user and Plugin declarations.
    let configs: HashMap<_, _> = snapshot
        .mcp
        .servers
        .into_iter()
        .map(|server| {
            (
                (
                    resolve_bundle_id(&server.config),
                    server.origin == ProvenanceScope::Plugin,
                ),
                server.config,
            )
        })
        .collect();
    statuses
        .into_iter()
        .map(|status| {
            let config = match &status.managed_by {
                McpServerManagedBy::BuiltIn { .. } => None,
                owner => configs.get(&(status.bundle_id.clone(), owner.is_plugin_owned())),
            };
            let connection_config = match config {
                Some(config @ (MCPServerConfig::Http(_) | MCPServerConfig::Sse(_))) => {
                    // One declaration per SDK portability document avoids name collisions.
                    // Never fall back to raw data (including raw sanitizer errors).
                    let sanitized = service
                        .prepare_portable_mcp_configs(std::slice::from_ref(config))
                        .map_err(|_| {
                            "Unable to safely project MCP connection configuration".to_string()
                        })?;
                    let (transport, url, headers) = match sanitized.into_iter().next() {
                        Some(MCPServerConfig::Http(c)) => {
                            ("http", c.server_parameters.url, c.server_parameters.headers)
                        }
                        Some(MCPServerConfig::Sse(c)) => {
                            ("sse", c.server_parameters.url, c.server_parameters.headers)
                        }
                        _ => {
                            return Err(
                                "MCP sanitizer did not return a remote declaration".to_string()
                            )
                        }
                    };
                    Some(ConnectionConfig {
                        basis: "configuration",
                        transport,
                        url,
                        headers,
                    })
                }
                _ => None,
            };
            Ok(ServerView {
                status,
                connection_config,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{
        client_control::{InvocationContext, RemoteControlPolicy, TargetScope, ToolId, ToolScope},
        computer::ComputerInstance,
        config::ConfigService,
        keychain::InMemorySecretStore,
        observability::ObservabilityService,
        settings::SettingsService,
    };

    #[tokio::test]
    async fn tool_returns_redacted_declarations_and_preserves_status() {
        let temp = tempfile::tempdir().unwrap();
        let config = ConfigService::new(temp.path().to_path_buf()).unwrap();
        for id in ["source", "target"] {
            config
                .add_computer_instance(ComputerInstance::new(id, id))
                .unwrap();
        }
        let state = AppState::new_with_secret_store(
            config,
            ObservabilityService::new(temp.path()).unwrap(),
            SettingsService::new(temp.path().to_path_buf()),
            InMemorySecretStore::shared(),
        );
        state
            .client_control
            .update_policy_local(
                "source",
                RemoteControlPolicy {
                    enabled: true,
                    tool_scope: ToolScope::All,
                    target_scope: TargetScope::All,
                },
            )
            .await
            .unwrap();
        state
            .client_control
            .update_policy_local(
                "target",
                RemoteControlPolicy {
                    enabled: true,
                    tool_scope: ToolScope::All,
                    target_scope: TargetScope::All,
                },
            )
            .await
            .unwrap();
        let runtime = state.computer_registry.runtime("target").await.unwrap();
        runtime.start().await.unwrap();
        let configs: Vec<MCPServerConfig> = serde_json::from_value(serde_json::json!([
            {"type":"http", "name":"中文", "server_parameters": {
                "url":"https://literal-user:literal-password@example.com/mcp",
                "headers":{"Authorization":"Bearer literal-secret", "X-Region":"${input:REGION}"}
            }},
            {"type":"sse", "name":"events", "server_parameters": {
                "url":"https://example.com/events", "headers":{}
            }},
            {"type":"stdio", "name":"local", "server_parameters": {
                "command":"helper", "args":[], "env":{"TOKEN":"local-secret"}
            }}
        ]))
        .unwrap();
        let original_id = resolve_bundle_id(&configs[0]);
        state
            .sdk_config
            .upsert_mcp_configs("target", &configs)
            .unwrap();
        let before = get_mcp_servers_core(&state, "target").await.unwrap();
        let mut missing = state.sdk_config.load("target");
        missing.mcp.servers.clear();
        let missing = project(&state.sdk_config, before.clone(), missing).unwrap();
        assert!(missing.iter().all(|row| row.connection_config.is_none()));

        let call = || {
            state.client_control.dispatch(
                InvocationContext {
                    source_computer_id: "source".into(),
                    request_id: "list-config".into(),
                },
                ToolId::McpServerList,
                serde_json::json!({"computer_id":"target"}),
            )
        };
        let remote = call().await.unwrap();
        let rows = remote.as_array().unwrap();
        assert_eq!(rows.len(), before.len());
        for (row, status) in rows.iter().zip(before.iter()) {
            let mut unchanged = row.clone();
            unchanged
                .as_object_mut()
                .unwrap()
                .remove("connection_config");
            assert_eq!(unchanged, serde_json::to_value(status).unwrap());
        }
        let http = rows
            .iter()
            .find(|row| row["bundleId"] == original_id.as_str())
            .unwrap();
        assert_eq!(http["connection_config"]["basis"], "configuration");
        assert_eq!(http["connection_config"]["transport"], "http");
        assert_eq!(
            http["connection_config"]["headers"]["X-Region"],
            "${input:REGION}"
        );
        assert_eq!(
            http["connection_config"]["headers"]["Authorization"],
            "${REDACTED}"
        );
        assert!(http["connection_config"]["url"]
            .as_str()
            .unwrap()
            .contains("example.com/mcp"));
        let sse = rows.iter().find(|row| row["name"] == "events").unwrap();
        assert_eq!(sse["connection_config"]["transport"], "sse");
        assert_eq!(sse["connection_config"]["headers"], serde_json::json!({}));
        assert!(
            rows.iter().find(|row| row["name"] == "local").unwrap()["connection_config"].is_null()
        );
        let builtins: Vec<_> = rows
            .iter()
            .filter(|row| row["managedBy"]["type"] == "built_in")
            .collect();
        assert!(!builtins.is_empty());
        for row in builtins {
            assert!(row["connection_config"].is_null());
        }
        for secret in [
            "literal-user",
            "literal-password",
            "literal-secret",
            "local-secret",
        ] {
            assert!(!remote.to_string().contains(secret));
        }
        // The declaration changes without applying it to or starting the runtime.
        let mut edited = serde_json::to_value(&configs[1]).unwrap();
        edited["server_parameters"]["url"] = serde_json::json!("https://new.example.com/events");
        state
            .sdk_config
            .upsert_mcp_configs("target", &[serde_json::from_value(edited).unwrap()])
            .unwrap();
        let remote = call().await.unwrap();
        let sse = remote
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == "events")
            .unwrap();
        assert_eq!(
            sse["connection_config"]["url"],
            "https://new.example.com/events"
        );
        assert_eq!(sse["connection_config"]["basis"], "configuration");
        assert_eq!(sse["running"], false);

        // A raw on-disk secret in a guarded query fails closed, without echoing it.
        let path = state
            .sdk_config
            .project_anchor("target")
            .join(".tfrobot/mcp.local.json");
        let raw = std::fs::read_to_string(&path).unwrap();
        std::fs::write(
            &path,
            raw.replace(
                "https://new.example.com/events",
                "https://new.example.com/events?api_key=query-secret",
            ),
        )
        .unwrap();
        let error = call().await.unwrap_err();
        assert!(!serde_json::to_string(&error)
            .unwrap()
            .contains("query-secret"));
        runtime.shutdown().await;
    }
}
