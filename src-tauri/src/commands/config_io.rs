use crate::services::sdk_config::SdkConfigService;
use crate::services::storage::write_json_atomically;
use crate::AppState;
use a2c_smcp::smcp_computer::mcp_clients::model::{
    HttpServerConfig, HttpServerParameters, StdioServerConfig, StdioServerParameters,
};
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

#[derive(Debug, Default)]
struct RecoveredConfigImport {
    servers: Vec<MCPServerConfig>,
    inputs_changed: bool,
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
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default, alias = "serverUrl")]
    url: Option<String>,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    oauth: Option<bool>,
    #[serde(rename = "type", default)]
    transport_type: Option<String>,
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
        .map(|(name, server)| build_external_mcp_config(&name, server))
        .collect::<Result<Vec<_>, _>>()?;
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

    let recovered_import = recover_pending_config_import_for_instance(
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

    if !recovered_import.servers.is_empty() || recovered_import.inputs_changed {
        // Recovery is a completed prior transaction. Synchronize it independently so a
        // preflight failure in this new import cannot leave recovered declarations stale in an
        // already-created runtime.
        synchronize_imported_runtime(
            state,
            instance_id,
            &recovered_import.servers,
            recovered_import.inputs_changed,
        )
        .await?;
    }

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

    synchronize_imported_runtime(state, instance_id, &servers, merged_inputs.is_some()).await?;

    Ok(ImportResult {
        servers_imported,
        inputs_imported,
        servers_skipped: Vec::new(),
    })
}

async fn synchronize_imported_runtime(
    state: &AppState,
    instance_id: &str,
    servers: &[MCPServerConfig],
    inputs_changed: bool,
) -> Result<(), String> {
    let Some(mut runtime) = state.computer_registry.runtime(instance_id).await else {
        return Ok(());
    };

    if inputs_changed {
        let instance = state
            .config
            .get_computer_instance(instance_id)
            .map_err(|error| error.to_string())?;
        runtime = state
            .computer_registry
            .update_runtime_instance(instance)
            .await
            .map_err(|error| {
                format!(
                    "Configuration was imported, but the target Computer runtime could not synchronize its Inputs: {error}"
                )
            })?;
    }

    for server in servers {
        if let Err(error) = runtime.apply_user_mcp_server_config(server.clone()).await {
            // The SDK declaration is already committed. A runtime may still be unable to mount
            // it until a required input, secret, or authorization is supplied; the runtime keeps
            // that failure as a per-server diagnostic, but import remains a configuration success.
            log::warn!(
                "Imported MCP config for instance {}, but active runtime application is pending for {}: {}",
                instance_id,
                server.name(),
                error
            );
        }
    }
    Ok(())
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
) -> Result<RecoveredConfigImport, String> {
    let Some(mut transaction) = load_config_import_transaction(config, instance_id)? else {
        return Ok(RecoveredConfigImport::default());
    };

    if matches!(
        transaction.phase,
        ConfigImportTransactionPhase::Aborted | ConfigImportTransactionPhase::Committed
    ) {
        clear_terminal_config_import_transaction(config, instance_id);
        return Ok(RecoveredConfigImport::default());
    }

    transaction.inputs = super::inputs::prepare_portable_input_definitions(&transaction.inputs)?;
    transaction.servers = sdk_config
        .prepare_portable_mcp_configs(&transaction.servers)
        .map_err(|error| error.to_string())?;
    sdk_config
        .preflight_import_mcp_configs(instance_id, &transaction.servers)
        .map_err(|error| error.to_string())?;

    let inputs_changed = !transaction.inputs.is_empty();
    if inputs_changed {
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
    Ok(RecoveredConfigImport {
        servers: transaction.servers,
        inputs_changed,
    })
}

pub(crate) fn recover_pending_config_imports(
    config: &crate::services::config::ConfigService,
    sdk_config: &SdkConfigService,
    secret_store: &dyn crate::services::keychain::SecretStore,
    instance_ids: impl IntoIterator<Item = String>,
) -> Result<(), String> {
    for instance_id in instance_ids {
        let _recovered = recover_pending_config_import_for_instance(
            config,
            sdk_config,
            secret_store,
            &instance_id,
        )
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

fn build_external_mcp_config(
    name: &str,
    server: ClaudeDesktopServer,
) -> Result<MCPServerConfig, String> {
    let command = normalize_optional_non_empty(name, "command", server.command)?;
    let url = normalize_optional_non_empty(name, "url", server.url)?;

    match (command, url) {
        (Some(command), None) => {
            validate_transport_type(name, server.transport_type.as_deref(), &["stdio"])?;
            if !server.headers.is_empty() || server.oauth.is_some() {
                return Err(format!(
                    "MCP server '{name}' mixes stdio 'command' with HTTP-only headers or OAuth settings"
                ));
            }
            Ok(MCPServerConfig::Stdio(StdioServerConfig::new(
                name,
                StdioServerParameters {
                    command,
                    args: server.args,
                    env: server.env,
                    cwd: server.cwd,
                },
            )))
        }
        (None, Some(url)) => {
            validate_transport_type(
                name,
                server.transport_type.as_deref(),
                &["http", "streamable", "streamable-http", "streamable_http"],
            )?;
            if !server.args.is_empty() || !server.env.is_empty() || server.cwd.is_some() {
                return Err(format!(
                    "MCP server '{name}' mixes HTTP URL with stdio-only args, env, or cwd settings"
                ));
            }
            let has_authorization = server
                .headers
                .keys()
                .any(|header| header.eq_ignore_ascii_case("authorization"));
            if server.oauth == Some(true) && has_authorization {
                return Err(format!(
                    "MCP server '{name}' cannot enable OAuth while providing an Authorization header"
                ));
            }
            if server.oauth == Some(false) && !has_authorization {
                return Err(format!(
                    "MCP server '{name}' requests oauth=false, but the rust-sdk now uses automatic-only OAuth negotiation and cannot preserve that opt-out"
                ));
            }

            let config = HttpServerConfig::new(
                name,
                HttpServerParameters {
                    url,
                    headers: server.headers,
                },
            );
            Ok(MCPServerConfig::Http(config))
        }
        (Some(_), Some(_)) => Err(format!(
            "MCP server '{name}' must configure exactly one transport: command or URL, not both"
        )),
        (None, None) => Err(format!(
            "MCP server '{name}' must configure either a stdio command or an HTTP URL"
        )),
    }
}

fn normalize_optional_non_empty(
    server_name: &str,
    field: &str,
    value: Option<String>,
) -> Result<Option<String>, String> {
    value
        .map(|value| {
            let value = value.trim();
            if value.is_empty() {
                Err(format!(
                    "MCP server '{server_name}' has an empty '{field}' field"
                ))
            } else {
                Ok(value.to_string())
            }
        })
        .transpose()
}

fn validate_transport_type(
    server_name: &str,
    transport_type: Option<&str>,
    allowed: &[&str],
) -> Result<(), String> {
    let Some(transport_type) = transport_type else {
        return Ok(());
    };
    let normalized = transport_type.trim().to_ascii_lowercase();
    if allowed.contains(&normalized.as_str()) {
        Ok(())
    } else {
        Err(format!(
            "MCP server '{server_name}' has transport type '{transport_type}' that conflicts with its configured transport"
        ))
    }
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

    fn parse_external_server(value: serde_json::Value) -> ClaudeDesktopServer {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn official_remote_url_maps_to_automatic_oauth_http_without_legacy_fields() {
        let config = build_external_mcp_config(
            "atlassian",
            parse_external_server(serde_json::json!({
                "url": "https://mcp.atlassian.com/v1/mcp/authv2"
            })),
        )
        .unwrap();

        let MCPServerConfig::Http(http) = config else {
            panic!("URL entry must import as Streamable HTTP");
        };
        assert_eq!(
            http.server_parameters.url,
            "https://mcp.atlassian.com/v1/mcp/authv2"
        );
        let encoded = serde_json::to_value(http).unwrap();
        assert!(encoded.get("authPolicy").is_none());
        assert!(encoded.get("oauth").is_none());
    }

    #[test]
    fn server_url_alias_and_explicit_oauth_opt_out_is_rejected() {
        let error = build_external_mcp_config(
            "public",
            parse_external_server(serde_json::json!({
                "serverUrl": "https://public.example.com/mcp",
                "oauth": false
            })),
        )
        .unwrap_err();
        assert!(error.contains("cannot preserve that opt-out"));
    }

    #[test]
    fn authorization_header_selects_static_http_auth_and_conflicts_with_explicit_oauth() {
        let static_auth = build_external_mcp_config(
            "static-auth",
            parse_external_server(serde_json::json!({
                "url": "https://api.example.com/mcp",
                "headers": {"authorization": "Bearer ${input:token}"}
            })),
        )
        .unwrap();
        let MCPServerConfig::Http(static_auth) = static_auth else {
            panic!("URL entry must import as Streamable HTTP");
        };
        assert!(static_auth
            .server_parameters
            .headers
            .keys()
            .any(|header| header.eq_ignore_ascii_case("authorization")));

        let static_opt_out = build_external_mcp_config(
            "static-opt-out",
            parse_external_server(serde_json::json!({
                "url": "https://api.example.com/mcp",
                "oauth": false,
                "headers": {"Authorization": "Bearer ${input:token}"}
            })),
        )
        .unwrap();
        let MCPServerConfig::Http(static_opt_out) = static_opt_out else {
            panic!("static opt-out entry must import as Streamable HTTP");
        };
        assert!(static_opt_out
            .server_parameters
            .headers
            .keys()
            .any(|header| header.eq_ignore_ascii_case("authorization")));

        let proactive = build_external_mcp_config(
            "proactive",
            parse_external_server(serde_json::json!({
                "url": "https://api.example.com/mcp",
                "oauth": true
            })),
        )
        .unwrap();
        let MCPServerConfig::Http(proactive) = proactive else {
            panic!("URL entry must import as Streamable HTTP");
        };
        let proactive = serde_json::to_value(proactive).unwrap();
        assert!(proactive.get("authPolicy").is_none());
        assert!(proactive.get("oauth").is_none());

        let conflict = build_external_mcp_config(
            "conflict",
            parse_external_server(serde_json::json!({
                "url": "https://api.example.com/mcp",
                "headers": {"Authorization": "Bearer ${input:token}"},
                "oauth": true
            })),
        )
        .unwrap_err();
        assert!(conflict.contains("cannot enable OAuth"));
    }

    #[test]
    fn stdio_import_is_preserved_and_mixed_transports_are_rejected() {
        let stdio = build_external_mcp_config(
            "stdio",
            parse_external_server(serde_json::json!({
                "command": "node",
                "args": ["server.js"],
                "env": {"MODE": "test"},
                "cwd": "/tmp"
            })),
        )
        .unwrap();
        let MCPServerConfig::Stdio(stdio) = stdio else {
            panic!("command entry must import as stdio");
        };
        assert_eq!(stdio.server_parameters.command, "node");
        assert_eq!(stdio.server_parameters.args, ["server.js"]);
        assert_eq!(stdio.server_parameters.cwd.as_deref(), Some("/tmp"));

        let mixed = build_external_mcp_config(
            "mixed",
            parse_external_server(serde_json::json!({
                "command": "node",
                "url": "https://api.example.com/mcp"
            })),
        )
        .unwrap_err();
        assert!(mixed.contains("exactly one transport"));
    }

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
