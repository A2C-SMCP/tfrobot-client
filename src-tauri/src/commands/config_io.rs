use crate::services::sdk_config::SdkConfigService;
use crate::services::storage::write_json_atomically;
use crate::AppState;
use a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig;
use a2c_smcp::smcp_computer::settings::config::ValidationReport;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tauri::State;

const CONFIG_IMPORT_TRANSACTION_VERSION: u32 = 1;
const CONFIG_IMPORT_TRANSACTION_FILE: &str = "config_import_transaction.json";

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ConfigImportTransactionPhase {
    #[default]
    Pending,
    Aborted,
    Committed,
}

/// Result of an import operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    pub servers_imported: usize,
    pub inputs_imported: usize,
    pub servers_skipped: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigImportTransaction {
    version: u32,
    #[serde(default)]
    phase: ConfigImportTransactionPhase,
    instance_id: String,
    servers: Vec<MCPServerConfig>,
    inputs: Vec<super::inputs::InputDefinition>,
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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let _mutation_guard = state.input_mutation_lock.lock().await;
    let instance_id = require_instance_id(&instance_id)?.to_string();
    state
        .config
        .get_computer_instance(&instance_id)
        .map_err(|error| error.to_string())?;
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
    import_servers_and_inputs(state, instance_id, config.servers, config.inputs).await
}

async fn import_claude_desktop(
    state: &AppState,
    instance_id: &str,
    content: &str,
) -> Result<ImportResult, String> {
    let config: ClaudeDesktopConfig = serde_json::from_str(content).map_err(|e| e.to_string())?;
    let servers = config
        .mcp_servers
        .into_iter()
        .map(|(name, server)| build_stdio_config(&name, &server))
        .collect();
    import_servers_and_inputs(state, instance_id, servers, Vec::new()).await
}

async fn import_servers_and_inputs(
    state: &AppState,
    instance_id: &str,
    servers: Vec<MCPServerConfig>,
    inputs: Vec<super::inputs::InputDefinition>,
) -> Result<ImportResult, String> {
    let inputs = super::inputs::prepare_portable_input_definitions(&inputs)?;
    let servers = state
        .sdk_config
        .prepare_portable_mcp_configs(&servers)
        .map_err(|error| error.to_string())?;
    let transaction_inputs = inputs.clone();

    recover_pending_config_import_for_instance(
        state.config.as_ref(),
        state.sdk_config.as_ref(),
        state.secret_store.as_ref(),
        instance_id,
    )?;
    let inputs_imported = inputs.len();
    let servers_imported = servers.len();

    let merged_inputs = if inputs.is_empty() {
        None
    } else {
        let mut existing = state
            .config
            .load_inputs_for_instance(instance_id)
            .map_err(|e| e.to_string())?;
        for input in inputs {
            let id = input.id().to_string();
            existing.retain(|item| item.id() != id);
            existing.push(input);
        }
        Some(existing)
    };

    if servers.is_empty() && merged_inputs.is_none() {
        return Ok(ImportResult {
            servers_imported,
            inputs_imported,
            servers_skipped: Vec::new(),
        });
    }

    // Complete every deterministic SDK target check before creating the durable redo journal or
    // mutating client-owned inputs. Recovery must never be born with a transaction that can only
    // replay into a known read-only provenance conflict or malformed local target document.
    state
        .sdk_config
        .preflight_import_mcp_configs(instance_id, &servers)
        .map_err(|error| error.to_string())?;

    let transaction = ConfigImportTransaction {
        version: CONFIG_IMPORT_TRANSACTION_VERSION,
        phase: ConfigImportTransactionPhase::Pending,
        instance_id: instance_id.to_string(),
        servers: servers.clone(),
        inputs: transaction_inputs,
    };
    persist_config_import_transaction(state.config.as_ref(), &transaction)?;

    // The durable, secret-free transaction makes a crash between the two stores recoverable.
    let mut transaction = transaction;
    let input_snapshot = if let Some(merged_inputs) = &merged_inputs {
        match crate::commands::inputs::replace_input_definitions_config_only_locked(
            state.config.as_ref(),
            state.secret_store.as_ref(),
            instance_id,
            merged_inputs,
        ) {
            Ok(snapshot) => Some(snapshot),
            Err(error) => {
                let safe_to_abort = error.is_safe_to_abort();
                let message = error.to_string();
                if safe_to_abort {
                    return Err(abort_config_import_transaction(
                        state.config.as_ref(),
                        &mut transaction,
                        message,
                    ));
                }
                return Err(format!(
                    "{message}; the pending import journal was retained for deterministic recovery"
                ));
            }
        }
    } else {
        None
    };

    if let Err(error) = state
        .sdk_config
        .import_mcp_configs_atomically(instance_id, &servers)
    {
        let rollback_error = input_snapshot.as_ref().and_then(|snapshot| {
            crate::commands::inputs::restore_input_definitions_config_only_locked(
                state.config.as_ref(),
                state.secret_store.as_ref(),
                instance_id,
                snapshot,
            )
            .err()
        });
        let message = match rollback_error.as_ref() {
            Some(rollback_error) => format!(
                "Failed to persist imported SDK configuration: {error}; input rollback also failed: {rollback_error}"
            ),
            None => format!("Failed to persist imported SDK configuration: {error}"),
        };
        if rollback_error.is_none() {
            return Err(abort_config_import_transaction(
                state.config.as_ref(),
                &mut transaction,
                message,
            ));
        }
        return Err(format!(
            "{message}; the pending import journal was retained for deterministic recovery"
        ));
    }

    finish_config_import_transaction(state.config.as_ref(), &mut transaction);

    if merged_inputs.is_some() {
        let instance = state
            .config
            .get_computer_instance(instance_id)
            .map_err(|error| error.to_string())?;
        state
            .computer_registry
            .update_runtime_instance(instance)
            .await
            .map_err(|error| {
                format!(
                    "Configuration was imported, but the target Computer runtime could not synchronize its Inputs: {error}"
                )
            })?;
    }

    Ok(ImportResult {
        servers_imported,
        inputs_imported,
        servers_skipped: Vec::new(),
    })
}

fn config_import_transaction_path(
    config: &crate::services::config::ConfigService,
    instance_id: &str,
) -> PathBuf {
    config
        .computer_instance_storage_root(instance_id)
        .join(CONFIG_IMPORT_TRANSACTION_FILE)
}

fn persist_config_import_transaction(
    config: &crate::services::config::ConfigService,
    transaction: &ConfigImportTransaction,
) -> Result<(), String> {
    write_json_atomically(
        &config_import_transaction_path(config, &transaction.instance_id),
        transaction,
    )
    .map_err(|error| format!("Failed to prepare recoverable config import: {error}"))
}

fn set_config_import_transaction_phase(
    config: &crate::services::config::ConfigService,
    transaction: &mut ConfigImportTransaction,
    phase: ConfigImportTransactionPhase,
) -> Result<(), String> {
    transaction.phase = phase;
    write_json_atomically(
        &config_import_transaction_path(config, &transaction.instance_id),
        transaction,
    )
    .map_err(|error| format!("Failed to record config import phase '{phase:?}': {error}"))
}

fn abort_config_import_transaction(
    config: &crate::services::config::ConfigService,
    transaction: &mut ConfigImportTransaction,
    primary_error: String,
) -> String {
    if let Err(error) = set_config_import_transaction_phase(
        config,
        transaction,
        ConfigImportTransactionPhase::Aborted,
    ) {
        return format!(
            "{primary_error}; failed to record the rollback, so the pending import may be replayed during recovery: {error}"
        );
    }
    match clear_config_import_transaction(config, &transaction.instance_id) {
        Ok(()) => primary_error,
        Err(error) => format!(
            "{primary_error}; changes were reverted and an aborted recovery marker remains because cleanup failed: {error}"
        ),
    }
}

fn finish_config_import_transaction(
    config: &crate::services::config::ConfigService,
    transaction: &mut ConfigImportTransaction,
) {
    if let Err(error) = set_config_import_transaction_phase(
        config,
        transaction,
        ConfigImportTransactionPhase::Committed,
    ) {
        log::warn!(
            "Config import committed, but its recovery phase could not be updated; the pending journal remains safe to replay: {error}"
        );
        return;
    }
    if let Err(error) = clear_config_import_transaction(config, &transaction.instance_id) {
        log::warn!(
            "Config import committed, but its committed recovery marker could not be removed: {error}"
        );
    }
}

fn clear_config_import_transaction(
    config: &crate::services::config::ConfigService,
    instance_id: &str,
) -> Result<(), String> {
    let path = config_import_transaction_path(config, instance_id);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "Failed to remove config import transaction {}: {error}",
            path.display()
        )),
    }
}

fn clear_terminal_config_import_transaction(
    config: &crate::services::config::ConfigService,
    instance_id: &str,
) {
    if let Err(error) = clear_config_import_transaction(config, instance_id) {
        log::warn!(
            "A terminal config import recovery marker could not be removed; startup can continue without replaying it: {error}"
        );
    }
}

fn load_config_import_transaction(
    config: &crate::services::config::ConfigService,
    instance_id: &str,
) -> Result<Option<ConfigImportTransaction>, String> {
    let path = config_import_transaction_path(config, instance_id);
    let content = match std::fs::read(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "Failed to read pending config import {}: {error}",
                path.display()
            ));
        }
    };
    let transaction: ConfigImportTransaction = serde_json::from_slice(&content)
        .map_err(|error| format!("Invalid pending config import {}: {error}", path.display()))?;
    if transaction.version != CONFIG_IMPORT_TRANSACTION_VERSION
        || transaction.instance_id != instance_id
    {
        return Err(format!(
            "Pending config import {} has an incompatible version or instance id",
            path.display()
        ));
    }
    Ok(Some(transaction))
}

fn recover_pending_config_import_for_instance(
    config: &crate::services::config::ConfigService,
    sdk_config: &SdkConfigService,
    secret_store: &dyn crate::services::keychain::SecretStore,
    instance_id: &str,
) -> Result<(), String> {
    let Some(mut transaction) = load_config_import_transaction(config, instance_id)? else {
        return Ok(());
    };

    if matches!(
        transaction.phase,
        ConfigImportTransactionPhase::Aborted | ConfigImportTransactionPhase::Committed
    ) {
        clear_terminal_config_import_transaction(config, instance_id);
        return Ok(());
    }

    transaction.inputs = super::inputs::prepare_portable_input_definitions(&transaction.inputs)?;
    transaction.servers = sdk_config
        .prepare_portable_mcp_configs(&transaction.servers)
        .map_err(|error| error.to_string())?;
    sdk_config
        .preflight_import_mcp_configs(instance_id, &transaction.servers)
        .map_err(|error| error.to_string())?;

    if !transaction.inputs.is_empty() {
        let mut merged_inputs = config
            .load_inputs_for_instance(instance_id)
            .map_err(|error| error.to_string())?;
        for input in &transaction.inputs {
            let id = input.id().to_string();
            merged_inputs.retain(|existing| existing.id() != id);
            merged_inputs.push(input.clone());
        }
        crate::commands::inputs::replace_input_definitions_config_only_locked(
            config,
            secret_store,
            instance_id,
            &merged_inputs,
        )
        .map_err(|error| error.to_string())?;
    }
    sdk_config
        .import_mcp_configs_atomically(instance_id, &transaction.servers)
        .map_err(|error| error.to_string())?;
    set_config_import_transaction_phase(
        config,
        &mut transaction,
        ConfigImportTransactionPhase::Committed,
    )?;
    clear_terminal_config_import_transaction(config, instance_id);
    Ok(())
}

pub(crate) fn recover_pending_config_imports(
    config: &crate::services::config::ConfigService,
    sdk_config: &SdkConfigService,
    secret_store: &dyn crate::services::keychain::SecretStore,
    instance_ids: impl IntoIterator<Item = String>,
) -> Result<(), String> {
    for instance_id in instance_ids {
        recover_pending_config_import_for_instance(config, sdk_config, secret_store, &instance_id)
            .map_err(|error| {
                format!("Failed to recover config import for Computer '{instance_id}': {error}")
            })?;
    }
    Ok(())
}

fn format_validation_errors(validation: &ValidationReport) -> String {
    validation
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
        .join("; ")
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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let _mutation_guard = state.input_mutation_lock.lock().await;
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
        let details = format_validation_errors(&validation);
        return Err(format!(
            "Cannot export invalid portable SDK configuration: {details}"
        ));
    }
    let servers = SdkConfigService::mcp_configs_from_portable_document(portable)
        .map_err(|error| error.to_string())?;
    let inputs = state
        .config
        .load_inputs_for_instance(instance_id)
        .map_err(|e| e.to_string())?;
    let inputs = super::inputs::prepare_portable_input_definitions(&inputs)?;

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

    write_json_atomically(Path::new(&path), &export).map_err(|error| error.to_string())?;

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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn terminal_transaction_cleanup_failure_does_not_become_an_error() {
        let temp = tempfile::tempdir().unwrap();
        let config =
            crate::services::config::ConfigService::new(temp.path().to_path_buf()).unwrap();
        let path = config_import_transaction_path(&config, "cleanup-test");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"terminal-marker").unwrap();
        let parent = path.parent().unwrap();
        let original_permissions = std::fs::metadata(parent).unwrap().permissions();
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o500)).unwrap();

        clear_terminal_config_import_transaction(&config, "cleanup-test");

        assert!(path.exists());
        std::fs::set_permissions(parent, original_permissions).unwrap();
    }
}
