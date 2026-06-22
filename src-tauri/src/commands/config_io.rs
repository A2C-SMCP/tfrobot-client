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
    instance_id: String,
    format: Option<ConfigFormat>,
) -> Result<ImportResult, String> {
    import_config_core(&state, path, instance_id, format).await
}

pub async fn import_config_core(
    state: &AppState,
    path: String,
    instance_id: String,
    format: Option<ConfigFormat>,
) -> Result<ImportResult, String> {
    let instance_id = require_instance_id(&instance_id)?.to_string();
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
        ConfigFormat::CliNative => import_cli_native(state, &instance_id, &content).await,
        ConfigFormat::ClaudeDesktop => import_claude_desktop(state, &instance_id, &content).await,
    }
}

async fn import_cli_native(
    state: &AppState,
    instance_id: &str,
    content: &str,
) -> Result<ImportResult, String> {
    let config: CliNativeConfig = serde_json::from_str(content).map_err(|e| e.to_string())?;

    let mut servers_imported = 0;
    let servers_skipped = Vec::new();

    // Import servers
    for server in &config.servers {
        let updated_instance = state
            .config
            .add_config_for_instance(instance_id, server.clone())
            .map_err(|e| e.to_string())?;
        let runtime = state
            .computer_registry
            .update_runtime_instance(updated_instance)
            .await;
        let lock = runtime.manager.read().await;
        let mgr = lock
            .as_ref()
            .ok_or("MCP manager not initialized".to_string())?;
        let _ = mgr.add_or_update_server(server.clone()).await;
        servers_imported += 1;
    }

    // Import inputs
    let inputs_imported = config.inputs.len();
    if !config.inputs.is_empty() {
        let mut existing = state
            .config
            .load_inputs_for_instance(instance_id)
            .map_err(|e| e.to_string())?;
        for input in config.inputs {
            let id = input.id().to_string();
            existing.retain(|i| i.id() != id);
            existing.push(input);
        }
        state
            .config
            .save_inputs_for_instance(instance_id, &existing)
            .map_err(|e| e.to_string())?;
    }

    Ok(ImportResult {
        servers_imported,
        inputs_imported,
        servers_skipped,
    })
}

async fn import_claude_desktop(
    state: &AppState,
    instance_id: &str,
    content: &str,
) -> Result<ImportResult, String> {
    let config: ClaudeDesktopConfig = serde_json::from_str(content).map_err(|e| e.to_string())?;

    let mut servers_imported = 0;
    let servers_skipped = Vec::new();

    for (name, server) in config.mcp_servers {
        let mcp_config = build_stdio_config(&name, &server);
        let updated_instance = state
            .config
            .add_config_for_instance(instance_id, mcp_config.clone())
            .map_err(|e| e.to_string())?;
        let runtime = state
            .computer_registry
            .update_runtime_instance(updated_instance)
            .await;
        let lock = runtime.manager.read().await;
        let mgr = lock
            .as_ref()
            .ok_or("MCP manager not initialized".to_string())?;
        let _ = mgr.add_or_update_server(mcp_config).await;
        servers_imported += 1;
    }

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

/// Export configuration to file
#[tauri::command]
pub async fn export_config(
    state: State<'_, AppState>,
    path: String,
    instance_id: String,
    server_names: Option<Vec<String>>,
) -> Result<(), String> {
    export_config_core(&state, path, instance_id, server_names).await
}

pub async fn export_config_core(
    state: &AppState,
    path: String,
    instance_id: String,
    server_names: Option<Vec<String>>,
) -> Result<(), String> {
    let instance_id = require_instance_id(&instance_id)?;
    let servers = state
        .config
        .load_configs_for_instance(instance_id)
        .map_err(|e| e.to_string())?;
    let inputs = state
        .config
        .load_inputs_for_instance(instance_id)
        .map_err(|e| e.to_string())?;

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

fn require_instance_id(instance_id: &str) -> Result<&str, String> {
    let instance_id = instance_id.trim();
    if instance_id.is_empty() {
        return Err("instance_id is required".to_string());
    }
    Ok(instance_id)
}
