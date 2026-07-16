use crate::AppState;
use a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig;
use a2c_smcp::smcp_computer::settings::config::ProjectConfigDoc;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;

    let mut servers_imported = 0;
    let mut servers_skipped = Vec::new();
    let inputs_imported = config.inputs.len();

    // Import inputs before servers so SDK Computer can render ${input:...}
    // placeholders while each imported server is synchronized into runtime.
    if !config.inputs.is_empty() {
        let _mutation_guard = state.input_mutation_lock.lock().await;
        let mut existing = state
            .config
            .load_inputs_for_instance(instance_id)
            .map_err(|e| e.to_string())?;
        for input in config.inputs {
            let id = input.id().to_string();
            existing.retain(|i| i.id() != id);
            existing.push(input);
        }
        crate::commands::inputs::replace_global_input_definitions_with_parts_locked(
            state.config.as_ref(),
            state.computer_registry.as_ref(),
            state.secret_store.as_ref(),
            instance_id,
            &existing,
        )
        .await?;
    }

    for server in &config.servers {
        if is_plugin_owned_server(state, instance_id, server.name()).await? {
            servers_skipped.push(server.name().to_string());
            continue;
        }

        super::mcp::add_mcp_server_locked(state, instance_id, server.clone())
            .await
            .map_err(|error| error.to_string())?;
        servers_imported += 1;
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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;

    let mut servers_imported = 0;
    let mut servers_skipped = Vec::new();

    for (name, server) in config.mcp_servers {
        if is_plugin_owned_server(state, instance_id, &name).await? {
            servers_skipped.push(name);
            continue;
        }

        let mcp_config = build_stdio_config(&name, &server);
        super::mcp::add_mcp_server_locked(state, instance_id, mcp_config)
            .await
            .map_err(|error| error.to_string())?;
        servers_imported += 1;
    }

    Ok(ImportResult {
        servers_imported,
        inputs_imported: 0,
        servers_skipped,
    })
}

fn build_stdio_config(name: &str, server: &ClaudeDesktopServer) -> MCPServerConfig {
    use a2c_smcp::smcp_computer::mcp_clients::model::{StdioServerConfig, StdioServerParameters};

    MCPServerConfig::Stdio(StdioServerConfig::new(
        name,
        StdioServerParameters {
            command: server.command.clone(),
            args: server.args.clone(),
            env: server.env.clone(),
            cwd: None,
        },
    ))
}

async fn is_plugin_owned_server(
    state: &AppState,
    instance_id: &str,
    name: &str,
) -> Result<bool, String> {
    if let Some(runtime) = state.computer_registry.runtime(instance_id).await {
        return Ok(runtime.plugin_mcp_server_owner(name).await.is_some());
    }
    Ok(false)
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
    state
        .config
        .get_computer_instance(instance_id)
        .map_err(|error| error.to_string())?;
    let portable = state
        .sdk_config
        .export_cli_native_mcp(instance_id)
        .map_err(|error| error.to_string())?;
    let validation = state.sdk_config.validate(&portable);
    if !validation.is_valid() {
        let details = validation
            .errors
            .iter()
            .map(|error| {
                format!(
                    "{}:{}: {}",
                    error.source_path.as_deref().unwrap_or("project config"),
                    error.field,
                    error.reason
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!(
            "Cannot export invalid portable SDK configuration: {details}"
        ));
    }
    let servers = cli_native_servers_from_project_config(portable)?;
    let inputs = state
        .config
        .load_inputs_for_instance(instance_id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(sanitize_portable_input_definition)
        .collect();

    let filtered_servers = match server_names {
        Some(names) => {
            let requested: HashSet<_> = names.into_iter().collect();
            let available: HashSet<_> = servers
                .iter()
                .map(|server| server.name().to_string())
                .collect();
            let mut missing: Vec<_> = requested.difference(&available).cloned().collect();
            if !missing.is_empty() {
                missing.sort();
                return Err(format!(
                    "Cannot export unknown MCP server(s): {}",
                    missing.join(", ")
                ));
            }
            servers
                .into_iter()
                .filter(|server| requested.contains(server.name()))
                .collect()
        }
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

fn cli_native_servers_from_project_config(
    document: ProjectConfigDoc,
) -> Result<Vec<MCPServerConfig>, String> {
    let Some(mcp) = document.mcp else {
        return Ok(Vec::new());
    };
    let Some(servers) = mcp.get("servers") else {
        return Ok(Vec::new());
    };
    let servers = servers
        .as_object()
        .ok_or_else(|| "Portable SDK mcp.servers must be an object".to_string())?;

    let mut configs = servers
        .iter()
        .map(|(name, value)| {
            let mut body = value
                .as_object()
                .cloned()
                .ok_or_else(|| format!("Portable SDK MCP server '{name}' must be an object"))?;
            if let Some(explicit_name) = body.get("name") {
                if explicit_name.as_str() != Some(name) {
                    return Err(format!(
                        "Portable SDK MCP server key '{name}' conflicts with its name field"
                    ));
                }
            }
            body.insert("name".to_string(), serde_json::Value::String(name.clone()));
            serde_json::from_value(serde_json::Value::Object(body))
                .map_err(|error| format!("Invalid portable SDK MCP server '{name}': {error}"))
        })
        .collect::<Result<Vec<MCPServerConfig>, String>>()?;
    configs.sort_by(|left, right| left.name().cmp(right.name()));
    Ok(configs)
}

fn sanitize_portable_input_definition(
    mut input: super::inputs::InputDefinition,
) -> super::inputs::InputDefinition {
    if let super::inputs::InputDefinition::PromptString {
        default, password, ..
    } = &mut input
    {
        if *password == Some(true) {
            *default = None;
        }
    }
    input
}

fn require_instance_id(instance_id: &str) -> Result<&str, String> {
    let instance_id = instance_id.trim();
    if instance_id.is_empty() {
        return Err("instance_id is required".to_string());
    }
    Ok(instance_id)
}
