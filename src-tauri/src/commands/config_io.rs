use crate::AppState;
use serde::{Deserialize, Serialize};
use smcp_computer::mcp_clients::MCPServerConfig;
use std::collections::HashMap;
use tauri::State;

/// Result of an import operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    pub servers_imported: usize,
    pub inputs_imported: usize,
    pub servers_skipped: Vec<String>,
}

/// Detected config format
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigFormat {
    CliNative,
    ClaudeDesktop,
}

/// Claude Desktop config format
#[derive(Debug, Deserialize)]
struct ClaudeDesktopConfig {
    #[serde(rename = "mcpServers")]
    mcp_servers: HashMap<String, ClaudeDesktopServer>,
}

#[derive(Debug, Deserialize)]
struct ClaudeDesktopServer {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
}

/// CLI native config format
#[derive(Debug, Serialize, Deserialize)]
struct CliNativeConfig {
    #[serde(default)]
    servers: Vec<MCPServerConfig>,
    #[serde(default)]
    inputs: Vec<super::inputs::InputDefinition>,
}

/// Detect the format of a config file
#[tauri::command]
pub async fn detect_config_format(path: String) -> Result<ConfigFormat, String> {
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;

    if value.get("mcpServers").is_some() {
        Ok(ConfigFormat::ClaudeDesktop)
    } else if value.get("servers").is_some() {
        Ok(ConfigFormat::CliNative)
    } else {
        Err("Unknown config format: expected 'servers' or 'mcpServers' key".to_string())
    }
}

/// Import configuration from file (auto-detect or specified format)
#[tauri::command]
pub async fn import_config(
    state: State<'_, AppState>,
    path: String,
    format: Option<ConfigFormat>,
) -> Result<ImportResult, String> {
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;

    // Detect format if not specified
    let fmt = match format {
        Some(f) => f,
        None => {
            let value: serde_json::Value =
                serde_json::from_str(&content).map_err(|e| e.to_string())?;
            if value.get("mcpServers").is_some() {
                ConfigFormat::ClaudeDesktop
            } else {
                ConfigFormat::CliNative
            }
        }
    };

    match fmt {
        ConfigFormat::CliNative => import_cli_native(&state, &content).await,
        ConfigFormat::ClaudeDesktop => import_claude_desktop(&state, &content).await,
    }
}

async fn import_cli_native(state: &AppState, content: &str) -> Result<ImportResult, String> {
    let config: CliNativeConfig = serde_json::from_str(content).map_err(|e| e.to_string())?;

    let mut servers_imported = 0;
    let servers_skipped = Vec::new();

    // Import servers
    for server in &config.servers {
        upsert_imported_server_config(state, server.clone())
            .await
            .map_err(|e| e.to_string())?;
        servers_imported += 1;
    }
    sync_runtime_configs(state)?;

    // Import inputs
    let inputs_imported = config.inputs.len();
    if !config.inputs.is_empty() {
        let mut existing = state.config.load_inputs().map_err(|e| e.to_string())?;
        for input in config.inputs {
            let id = input.id().to_string();
            existing.retain(|i| i.id() != id);
            existing.push(input);
        }
        state
            .config
            .save_inputs(&existing)
            .map_err(|e| e.to_string())?;
    }

    Ok(ImportResult {
        servers_imported,
        inputs_imported,
        servers_skipped,
    })
}

async fn import_claude_desktop(state: &AppState, content: &str) -> Result<ImportResult, String> {
    let config: ClaudeDesktopConfig = serde_json::from_str(content).map_err(|e| e.to_string())?;

    let mut servers_imported = 0;
    let mut servers_skipped = Vec::new();
    for (name, server) in config.mcp_servers {
        let mcp_config = build_stdio_config(&name, &server);
        match upsert_imported_server_config(state, mcp_config).await {
            Ok(()) => {
                servers_imported += 1;
            }
            Err(ImportServerError::Runtime(message)) => {
                servers_skipped.push(format!("{}: {}", name, message));
                continue;
            }
            Err(ImportServerError::Persistence(message)) => {
                return Err(message);
            }
        }
    }
    sync_runtime_configs(state)?;

    Ok(ImportResult {
        servers_imported,
        inputs_imported: 0,
        servers_skipped,
    })
}

fn build_stdio_config(name: &str, server: &ClaudeDesktopServer) -> MCPServerConfig {
    use smcp_computer::mcp_clients::model::{StdioServerConfig, StdioServerParameters};

    MCPServerConfig::Stdio(StdioServerConfig {
        name: name.to_string(),
        disabled: false,
        forbidden_tools: vec![],
        tool_meta: HashMap::new(),
        default_tool_meta: None,
        vrl: None,
        env_file: None,
        server_parameters: StdioServerParameters {
            command: server.command.clone(),
            args: server.args.clone(),
            env: server.env.clone(),
            cwd: None,
        },
    })
}

async fn upsert_imported_server_config(
    state: &AppState,
    config: MCPServerConfig,
) -> Result<(), ImportServerError> {
    let name = config.name().to_string();
    let previous_config = state
        .config
        .load_configs()
        .map_err(|e| ImportServerError::Persistence(e.to_string()))?
        .into_iter()
        .find(|existing| existing.name() == name);

    state
        .runtime
        .computer()
        .add_or_update_server(config.clone())
        .await
        .map_err(|e| ImportServerError::Runtime(e.to_string()))?;

    if let Err(e) = state.config.add_config(config) {
        restore_runtime_server(state, &name, previous_config).await;
        return Err(ImportServerError::Persistence(e.to_string()));
    }

    Ok(())
}

#[derive(Debug)]
enum ImportServerError {
    Runtime(String),
    Persistence(String),
}

impl std::fmt::Display for ImportServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportServerError::Runtime(message) | ImportServerError::Persistence(message) => {
                f.write_str(message)
            }
        }
    }
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
    let configs = state.config.load_configs().map_err(|e| e.to_string())?;
    state.runtime.store_configs(configs);
    Ok(())
}

/// Export configuration to file
#[tauri::command]
pub async fn export_config(
    state: State<'_, AppState>,
    path: String,
    server_names: Option<Vec<String>>,
) -> Result<(), String> {
    let servers = state.config.load_configs().map_err(|e| e.to_string())?;
    let inputs = state.config.load_inputs().map_err(|e| e.to_string())?;

    let filtered_servers = match server_names {
        Some(names) => servers
            .into_iter()
            .filter(|s| names.contains(&s.name().to_string()))
            .collect(),
        None => servers,
    };

    let export = CliNativeConfig {
        servers: filtered_servers,
        inputs,
    };

    let content = serde_json::to_string_pretty(&export).map_err(|e| e.to_string())?;
    std::fs::write(&path, content).map_err(|e| e.to_string())?;

    log::info!("Configuration exported to: {}", path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::config::ConfigService;
    use crate::services::logger::LogService;
    use crate::services::settings::SettingsService;

    fn test_state(tmp_path: &std::path::Path) -> AppState {
        AppState::new(
            ConfigService::new(tmp_path.to_path_buf()).expect("config service"),
            LogService::new(tmp_path).expect("log service"),
            SettingsService::new(tmp_path.to_path_buf()),
        )
    }

    #[tokio::test]
    async fn import_cli_native_syncs_runtime_config_snapshot() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let state = test_state(tmp.path());
        let content = r#"{
            "servers": [{
                "type": "Stdio",
                "name": "imported-cli",
                "server_parameters": {
                    "command": "node",
                    "args": ["--version"],
                    "env": {}
                }
            }],
            "inputs": []
        }"#;

        let result = import_cli_native(&state, content).await.expect("import");

        assert_eq!(result.servers_imported, 1);
        assert_eq!(state.runtime.stored_config_count(), 1);
    }

    #[tokio::test]
    async fn import_cli_native_persist_failure_rolls_back_runtime_config() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let state = test_state(tmp.path());
        std::fs::create_dir(tmp.path().join("mcp_servers.json"))
            .expect("block config file writes with directory");
        let content = r#"{
            "servers": [{
                "type": "Stdio",
                "name": "rollback-server",
                "server_parameters": {
                    "command": "node",
                    "args": [],
                    "env": {}
                }
            }],
            "inputs": []
        }"#;

        let err = import_cli_native(&state, content)
            .await
            .expect_err("blocked config file should fail import");
        let active_configs = state.runtime.computer().list_mcp_servers().await;

        assert!(err.contains("directory") || err.contains("Is a directory"));
        assert!(active_configs.is_empty());
        assert_eq!(state.runtime.stored_config_count(), 0);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn import_cli_native_persist_failure_restores_existing_runtime_config() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().expect("tempdir");
        let state = test_state(tmp.path());
        let existing: MCPServerConfig = serde_json::from_value(serde_json::json!({
            "type": "Stdio",
            "name": "same-name",
            "server_parameters": {
                "command": "node",
                "args": [],
                "env": {}
            }
        }))
        .expect("existing config");
        upsert_imported_server_config(&state, existing)
            .await
            .expect("initial import");
        sync_runtime_configs(&state).expect("sync initial config snapshot");
        std::fs::set_permissions(
            tmp.path().join("mcp_servers.json"),
            std::fs::Permissions::from_mode(0o444),
        )
        .expect("make config file readonly");

        let content = r#"{
            "servers": [{
                "type": "Stdio",
                "name": "same-name",
                "server_parameters": {
                    "command": "python",
                    "args": [],
                    "env": {}
                }
            }],
            "inputs": []
        }"#;

        let err = import_cli_native(&state, content)
            .await
            .expect_err("blocked config file should fail import");
        let active_configs = state.runtime.computer().list_mcp_servers().await;

        let _ = std::fs::set_permissions(
            tmp.path().join("mcp_servers.json"),
            std::fs::Permissions::from_mode(0o644),
        );
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

    #[tokio::test]
    async fn import_claude_desktop_syncs_runtime_config_snapshot() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let state = test_state(tmp.path());
        let content = r#"{
            "mcpServers": {
                "imported-claude": {
                    "command": "node",
                    "args": ["--version"],
                    "env": {}
                }
            }
        }"#;

        let result = import_claude_desktop(&state, content)
            .await
            .expect("import");

        assert_eq!(result.servers_imported, 1);
        assert_eq!(state.runtime.stored_config_count(), 1);
    }

    #[tokio::test]
    async fn import_claude_desktop_persist_failure_returns_error() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let state = test_state(tmp.path());
        std::fs::create_dir(tmp.path().join("mcp_servers.json"))
            .expect("block config file writes with directory");
        let content = r#"{
            "mcpServers": {
                "blocked-claude": {
                    "command": "node",
                    "args": [],
                    "env": {}
                }
            }
        }"#;

        let err = import_claude_desktop(&state, content)
            .await
            .expect_err("blocked config file should fail import");

        assert!(err.contains("directory") || err.contains("Is a directory"));
        assert_eq!(state.runtime.stored_config_count(), 0);
    }
}
