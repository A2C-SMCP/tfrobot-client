use crate::commands::inputs::{InputDefinition, PickOption};
use crate::services::computer::McpServerManagedBy;
use crate::AppState;
use a2c_smcp::smcp_computer::inputs::load_plugin_inputs;
use a2c_smcp::smcp_computer::mcp_clients::model::MCPServerInput;
use a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig;
use a2c_smcp::smcp_computer::settings::{
    load_installed_plugins, load_known_marketplaces, resolve_policy_settings, resolve_settings,
    AddMarketplaceParams, DisableOptions, EnableOptions, EnvMap, InstallOptions, McpHookError,
    McpInstallHooks, RemoveMarketplaceParams, ResolveSettingsArgs, UninstallOptions,
    XDG_CONFIG_HOME_ENV,
};
use a2c_smcp::smcp_computer::skills::{MCP_INPUTS_FILENAME, MCP_SERVERS_SUBDIR};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use tauri::State;

const SUPPORTED_OPERATIONS: &[&str] = &[
    "add_marketplace",
    "refresh_marketplace",
    "remove_marketplace",
    "install_plugin",
    "enable_plugin",
    "disable_plugin",
    "uninstall_plugin",
];

const AVAILABLE_SDK_APIS: &[&str] = &[
    "Computer::add_marketplace",
    "Computer::refresh_marketplace",
    "Computer::remove_marketplace",
    "Computer::install_plugin",
    "Computer::enable_plugin",
    "Computer::disable_plugin",
    "Computer::uninstall_plugin",
];

// Intentionally do not expose Computer::reconcile_governance as a client
// operation yet. Current SDK recovery does not provide the full instance-scoped
// governance contract tfrobot-client needs for cold-start MCP remount/settings
// parity. The client should wait for that SDK capability instead of rebuilding
// SDK ledgers or recovery logic locally.

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceCapabilities {
    pub computer_lifecycle_api_available: bool,
    pub supported_operations: Vec<String>,
    pub required_sdk_apis: Vec<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceSummary {
    pub name: String,
    pub git_url: Option<String>,
    pub status: String,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginSummary {
    pub marketplace: String,
    pub plugin: String,
    pub plugin_id: Option<String>,
    pub version: Option<String>,
    pub enabled: bool,
    pub status: String,
    pub bundled_mcp_servers: Vec<String>,
    pub bundled_skills: Vec<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceGovernance {
    pub capabilities: MarketplaceCapabilities,
    pub marketplaces: Vec<MarketplaceSummary>,
    pub plugins: Vec<PluginSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AddMarketplaceRequest {
    pub name: String,
    pub git_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginLifecycleRequest {
    pub marketplace: String,
    pub plugin: String,
}

#[tauri::command]
pub async fn get_marketplace_capabilities(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<MarketplaceCapabilities, String> {
    get_marketplace_capabilities_core(&state, &instance_id).await
}

pub async fn get_marketplace_capabilities_core(
    state: &AppState,
    instance_id: &str,
) -> Result<MarketplaceCapabilities, String> {
    ensure_runtime(state, instance_id).await?;
    Ok(supported_capabilities())
}

#[tauri::command]
pub async fn get_marketplace_governance(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<MarketplaceGovernance, String> {
    get_marketplace_governance_core(&state, &instance_id).await
}

pub async fn get_marketplace_governance_core(
    state: &AppState,
    instance_id: &str,
) -> Result<MarketplaceGovernance, String> {
    let runtime = ensure_runtime(state, instance_id).await?;
    let env = sdk_settings_env(state, instance_id);
    let snapshot = marketplace_governance_snapshot(&runtime, &env).await;

    Ok(MarketplaceGovernance {
        capabilities: supported_capabilities(),
        marketplaces: snapshot.marketplaces,
        plugins: snapshot.plugins,
    })
}

#[tauri::command]
pub async fn add_marketplace(
    state: State<'_, AppState>,
    instance_id: String,
    request: AddMarketplaceRequest,
) -> Result<(), String> {
    add_marketplace_core(&state, &instance_id, request).await
}

pub async fn add_marketplace_core(
    state: &AppState,
    instance_id: &str,
    request: AddMarketplaceRequest,
) -> Result<(), String> {
    let runtime = ensure_runtime(state, instance_id).await?;
    let name = require_non_empty("marketplace name", &request.name)?;
    let git_url = require_non_empty("marketplace git_url", &request.git_url)?;
    runtime
        .sdk_add_marketplace(
            git_url,
            AddMarketplaceParams {
                name: Some(name),
                auto_update: false,
                no_clone: false,
            },
        )
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn refresh_marketplace(
    state: State<'_, AppState>,
    instance_id: String,
    marketplace: String,
) -> Result<(), String> {
    refresh_marketplace_core(&state, &instance_id, &marketplace).await
}

pub async fn refresh_marketplace_core(
    state: &AppState,
    instance_id: &str,
    marketplace: &str,
) -> Result<(), String> {
    let runtime = ensure_runtime(state, instance_id).await?;
    require_non_empty("marketplace", marketplace)?;
    let rows = runtime.sdk_refresh_marketplace(marketplace).await;
    if let Some(missing) = rows.iter().find(|row| row.status.as_str() == "missing") {
        return Err(format!("unknown marketplace: {:?}", missing.name));
    }
    Ok(())
}

#[tauri::command]
pub async fn remove_marketplace(
    state: State<'_, AppState>,
    instance_id: String,
    marketplace: String,
) -> Result<(), String> {
    remove_marketplace_core(&state, &instance_id, &marketplace).await
}

pub async fn remove_marketplace_core(
    state: &AppState,
    instance_id: &str,
    marketplace: &str,
) -> Result<(), String> {
    let runtime = ensure_runtime(state, instance_id).await?;
    require_non_empty("marketplace", marketplace)?;
    let env = sdk_settings_env(state, instance_id);
    let snapshot = marketplace_governance_snapshot(&runtime, &env).await;
    if snapshot.has_installed_plugins_for_marketplace(marketplace) {
        return Err(format!(
            "marketplace '{marketplace}' has installed plugins; uninstall plugins before removing the marketplace"
        ));
    }
    runtime
        .sdk_remove_marketplace(
            marketplace,
            RemoveMarketplaceParams {
                keep_plugins: true,
                hooks: None,
            },
        )
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn install_plugin(
    state: State<'_, AppState>,
    instance_id: String,
    request: PluginLifecycleRequest,
) -> Result<(), String> {
    install_plugin_core(&state, &instance_id, request).await
}

pub async fn install_plugin_core(
    state: &AppState,
    instance_id: &str,
    request: PluginLifecycleRequest,
) -> Result<(), String> {
    let runtime = ensure_runtime(state, instance_id).await?;
    validate_plugin_request(&request)?;
    let plugin_id = plugin_id(&request);
    let env = sdk_settings_env(state, instance_id);
    let hooks =
        MarketplaceMcpHooks::for_plugin(state, instance_id, &request.marketplace, &request.plugin);
    runtime
        .sdk_install_plugin(
            &plugin_id,
            InstallOptions {
                scope: Some("user"),
                env: Some(&env),
                ..Default::default()
            },
            Some(&hooks),
        )
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn enable_plugin(
    state: State<'_, AppState>,
    instance_id: String,
    request: PluginLifecycleRequest,
) -> Result<(), String> {
    enable_plugin_core(&state, &instance_id, request).await
}

pub async fn enable_plugin_core(
    state: &AppState,
    instance_id: &str,
    request: PluginLifecycleRequest,
) -> Result<(), String> {
    let runtime = ensure_runtime(state, instance_id).await?;
    validate_plugin_request(&request)?;
    let plugin_id = plugin_id(&request);
    let env = sdk_settings_env(state, instance_id);
    let hooks =
        MarketplaceMcpHooks::for_plugin(state, instance_id, &request.marketplace, &request.plugin);
    runtime
        .sdk_enable_plugin(
            &plugin_id,
            EnableOptions {
                scope: Some("user"),
                env: Some(&env),
                ..Default::default()
            },
            Some(&hooks),
        )
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn disable_plugin(
    state: State<'_, AppState>,
    instance_id: String,
    request: PluginLifecycleRequest,
) -> Result<(), String> {
    disable_plugin_core(&state, &instance_id, request).await
}

pub async fn disable_plugin_core(
    state: &AppState,
    instance_id: &str,
    request: PluginLifecycleRequest,
) -> Result<(), String> {
    let runtime = ensure_runtime(state, instance_id).await?;
    validate_plugin_request(&request)?;
    let plugin_id = plugin_id(&request);
    let env = sdk_settings_env(state, instance_id);
    let hooks =
        MarketplaceMcpHooks::for_plugin(state, instance_id, &request.marketplace, &request.plugin);
    runtime
        .sdk_disable_plugin(
            &plugin_id,
            DisableOptions {
                scope: Some("user"),
                env: Some(&env),
                ..Default::default()
            },
            Some(&hooks),
        )
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn uninstall_plugin(
    state: State<'_, AppState>,
    instance_id: String,
    request: PluginLifecycleRequest,
) -> Result<(), String> {
    uninstall_plugin_core(&state, &instance_id, request).await
}

pub async fn uninstall_plugin_core(
    state: &AppState,
    instance_id: &str,
    request: PluginLifecycleRequest,
) -> Result<(), String> {
    let runtime = ensure_runtime(state, instance_id).await?;
    validate_plugin_request(&request)?;
    let plugin_id = plugin_id(&request);
    let env = sdk_settings_env(state, instance_id);
    let hooks =
        MarketplaceMcpHooks::for_plugin(state, instance_id, &request.marketplace, &request.plugin);
    runtime
        .sdk_uninstall_plugin(
            &plugin_id,
            UninstallOptions {
                scope: Some("user"),
                keep_servers: false,
                env: Some(&env),
            },
            Some(&hooks),
        )
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

async fn ensure_runtime(
    state: &AppState,
    instance_id: &str,
) -> Result<crate::services::computer::ComputerInstanceRuntime, String> {
    let instance_id = require_non_empty("instance_id", instance_id)?;
    state
        .computer_registry
        .runtime(instance_id)
        .await
        .ok_or_else(|| format!("Computer instance not found: {instance_id}"))
}

fn validate_plugin_request(request: &PluginLifecycleRequest) -> Result<(), String> {
    require_non_empty("marketplace", &request.marketplace)?;
    require_non_empty("plugin", &request.plugin)?;
    Ok(())
}

fn require_non_empty<'a>(field: &str, value: &'a str) -> Result<&'a str, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("{field} is required"));
    }
    Ok(value)
}

fn supported_capabilities() -> MarketplaceCapabilities {
    MarketplaceCapabilities {
        computer_lifecycle_api_available: true,
        supported_operations: SUPPORTED_OPERATIONS
            .iter()
            .map(|operation| (*operation).to_string())
            .collect(),
        required_sdk_apis: AVAILABLE_SDK_APIS
            .iter()
            .map(|api| (*api).to_string())
            .collect(),
        reason: "smcp-computer Computer-level marketplace/plugin lifecycle APIs are available"
            .to_string(),
    }
}

struct GovernanceSnapshot {
    marketplaces: Vec<MarketplaceSummary>,
    plugins: Vec<PluginSummary>,
}

impl GovernanceSnapshot {
    fn has_installed_plugins_for_marketplace(&self, marketplace: &str) -> bool {
        self.plugins
            .iter()
            .any(|plugin| plugin.marketplace == marketplace)
    }
}

async fn marketplace_governance_snapshot(
    runtime: &crate::services::computer::ComputerInstanceRuntime,
    env: &EnvMap,
) -> GovernanceSnapshot {
    let skill_home = runtime.sdk_skill_home().await;
    // SDK does not yet expose a Computer-level read-only governance summary. Keep
    // direct ledger reads isolated here so the call site can be replaced by that
    // API without spreading SDK store schema knowledge through command handlers.
    let known = load_known_marketplaces(Some(&skill_home), Some(env));
    let installed = load_installed_plugins(Some(&skill_home), Some(env));
    let registered_workdirs = runtime.sdk_registered_workdirs().await;
    let active_workdir = runtime.sdk_active_workdir().await;
    // Keep the read model constrained to a snapshot. Cold-start recovery,
    // env-aware settings merge, and bundled MCP remount remain SDK-owned
    // lifecycle responsibilities; tfrobot-client must not compensate by
    // mutating SDK governance state from this view.
    let policy = resolve_policy_settings(Some(env), None, None);
    let declared = resolve_settings(ResolveSettingsArgs {
        registered_workdirs: &registered_workdirs,
        active_workdir: active_workdir.as_deref(),
        env: Some(env),
        flag_settings_path: None,
        policy_settings: Some(&policy),
    })
    .settings;
    let active_skills = runtime.sdk_skills().await;

    let marketplaces = known
        .account
        .marketplaces
        .into_iter()
        .map(|(name, entry)| MarketplaceSummary {
            name,
            git_url: entry
                .source
                .get("url")
                .and_then(|value| value.as_str())
                .map(ToString::to_string),
            status: "known".to_string(),
            message: entry
                .extra
                .get("lastUpdated")
                .and_then(|value| value.as_str())
                .map(|last_updated| format!("lastUpdated={last_updated}")),
        })
        .collect();

    let plugins = installed
        .account
        .plugins
        .into_iter()
        .flat_map(|(plugin_id, records)| {
            let declared = declared.clone();
            let bundled_skills = active_skills
                .iter()
                .filter(|skill| {
                    skill.source
                        == format!("marketplace:{}", marketplace_from_plugin_id(&plugin_id))
                })
                .filter(|skill| {
                    skill
                        .name
                        .starts_with(&format!("{}:", plugin_from_plugin_id(&plugin_id)))
                })
                .map(|skill| skill.name.clone())
                .collect::<Vec<_>>();
            records.into_iter().map(move |record| {
                let (plugin, marketplace) = split_plugin_id(&plugin_id);
                let enabled = plugin_enabled(&declared, &plugin_id);
                PluginSummary {
                    marketplace,
                    plugin,
                    plugin_id: Some(plugin_id.clone()),
                    version: record
                        .extra
                        .get("version")
                        .and_then(|value| value.as_str())
                        .map(ToString::to_string),
                    enabled,
                    status: if enabled { "enabled" } else { "disabled" }.to_string(),
                    bundled_mcp_servers: record.bundled_mcp_servers,
                    bundled_skills: bundled_skills.clone(),
                    message: record.install_path,
                }
            })
        })
        .collect();

    GovernanceSnapshot {
        marketplaces,
        plugins,
    }
}

fn sdk_settings_env(state: &AppState, instance_id: &str) -> EnvMap {
    let mut env = HashMap::new();
    env.insert(
        XDG_CONFIG_HOME_ENV.to_string(),
        state
            .config
            .computer_instance_storage_root(instance_id)
            .join("sdk_config")
            .to_string_lossy()
            .to_string(),
    );
    env
}

fn plugin_id(request: &PluginLifecycleRequest) -> String {
    format!("{}@{}", request.plugin.trim(), request.marketplace.trim())
}

fn split_plugin_id(plugin_id: &str) -> (String, String) {
    plugin_id
        .split_once('@')
        .map(|(plugin, marketplace)| (plugin.to_string(), marketplace.to_string()))
        .unwrap_or_else(|| (plugin_id.to_string(), String::new()))
}

fn plugin_from_plugin_id(plugin_id: &str) -> String {
    split_plugin_id(plugin_id).0
}

fn marketplace_from_plugin_id(plugin_id: &str) -> String {
    split_plugin_id(plugin_id).1
}

fn plugin_enabled(declared: &Map<String, Value>, plugin_id: &str) -> bool {
    declared
        .get("enabledPlugins")
        .and_then(Value::as_object)
        .and_then(|plugins| plugins.get(plugin_id))
        .and_then(Value::as_bool)
        != Some(false)
}

struct MarketplaceMcpHooks {
    config: Arc<crate::services::config::ConfigService>,
    registry: Arc<crate::services::computer::ComputerRegistry>,
    instance_id: String,
    marketplace: String,
    plugin: String,
    plugin_id: String,
}

impl MarketplaceMcpHooks {
    fn for_plugin(state: &AppState, instance_id: &str, marketplace: &str, plugin: &str) -> Self {
        debug_assert!(!marketplace.trim().is_empty());
        debug_assert!(!plugin.trim().is_empty());
        Self {
            config: state.config.clone(),
            registry: state.computer_registry.clone(),
            instance_id: instance_id.to_string(),
            marketplace: marketplace.to_string(),
            plugin: plugin.to_string(),
            plugin_id: format!("{plugin}@{marketplace}"),
        }
    }
}

#[async_trait]
impl McpInstallHooks for MarketplaceMcpHooks {
    fn existing_server_names(&self) -> HashSet<String> {
        self.config
            .load_managed_configs_for_instance(&self.instance_id)
            .map(|servers| {
                servers
                    .into_iter()
                    .map(|server| server.name().to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    async fn register_server(&self, cfg: MCPServerConfig) -> Result<(), McpHookError> {
        if let Ok(existing) = self
            .config
            .get_managed_config_for_instance(&self.instance_id, cfg.name())
        {
            if !existing.is_plugin_owned() {
                return Ok(());
            }
        }
        let managed_by = McpServerManagedBy::Plugin {
            marketplace: self.marketplace.clone(),
            plugin: self.plugin.clone(),
            plugin_id: (!self.plugin_id.is_empty()).then(|| self.plugin_id.clone()),
        };
        match self.registry.runtime(&self.instance_id).await {
            Some(runtime) => {
                if let Some(owner) = runtime.plugin_mcp_server_owner(cfg.name()).await {
                    if !same_plugin_owner(&owner, &managed_by) {
                        return Err(McpHookError(format!(
                            "MCP server '{}' is already managed by another Marketplace plugin",
                            cfg.name()
                        )));
                    }
                }
                runtime
                    .add_or_update_plugin_server(cfg, managed_by)
                    .await
                    .map_err(McpHookError)
            }
            None => Err(McpHookError(format!(
                "Computer instance not found while registering Marketplace MCP server: {}",
                self.instance_id
            ))),
        }
    }

    async fn remove_server(&self, name: &str) -> Result<(), McpHookError> {
        match self.registry.runtime(&self.instance_id).await {
            Some(runtime) => runtime
                .remove_plugin_server(name)
                .await
                .map_err(McpHookError),
            None => Err(McpHookError(format!(
                "Computer instance not found while removing Marketplace MCP server: {}",
                self.instance_id
            ))),
        }
    }

    async fn inject_inputs(&self, plugin_root: &Path) -> Result<(), McpHookError> {
        if self.plugin.is_empty() || self.marketplace.is_empty() {
            return Ok(());
        }
        let inputs_json = plugin_root
            .join(MCP_SERVERS_SUBDIR)
            .join(MCP_INPUTS_FILENAME);
        let inputs = load_plugin_inputs(&inputs_json, &self.plugin, &self.marketplace);
        if inputs.is_empty() {
            return Ok(());
        }

        let mut definitions = self
            .config
            .load_inputs_for_instance(&self.instance_id)
            .map_err(|error| McpHookError(error.to_string()))?;
        for input in &inputs {
            let id = input.id().to_string();
            definitions.retain(|definition| definition.id() != id);
            definitions.push(input_definition_from_mcp(input));
        }
        self.config
            .save_inputs_for_instance(&self.instance_id, &definitions)
            .map_err(|error| McpHookError(error.to_string()))?;

        if let Some(runtime) = self.registry.runtime(&self.instance_id).await {
            for input in inputs {
                runtime
                    .add_or_update_input(input)
                    .await
                    .map_err(McpHookError)?;
            }
        }
        Ok(())
    }
}

fn same_plugin_owner(left: &McpServerManagedBy, right: &McpServerManagedBy) -> bool {
    match (left, right) {
        (
            McpServerManagedBy::Plugin {
                plugin_id: left_id, ..
            },
            McpServerManagedBy::Plugin {
                plugin_id: right_id,
                ..
            },
        ) => left_id == right_id,
        _ => false,
    }
}

fn input_definition_from_mcp(input: &MCPServerInput) -> InputDefinition {
    match input {
        MCPServerInput::PromptString(input) => InputDefinition::PromptString {
            id: input.id.clone(),
            label: input.description.clone(),
            description: Some(input.description.clone()),
            default: input.default.clone(),
            password: input.password,
        },
        MCPServerInput::PickString(input) => InputDefinition::PickString {
            id: input.id.clone(),
            label: input.description.clone(),
            description: Some(input.description.clone()),
            options: input
                .options
                .iter()
                .map(|value| PickOption {
                    label: value.clone(),
                    value: value.clone(),
                })
                .collect(),
            default: input.default.clone(),
        },
        MCPServerInput::Command(input) => InputDefinition::Command {
            id: input.id.clone(),
            label: input.description.clone(),
            command: input.command.clone(),
            args: input.args.as_ref().map(|args| {
                let mut pairs: Vec<_> = args.iter().collect();
                pairs.sort_by_key(|(index, _)| *index);
                pairs.into_iter().map(|(_, value)| value.clone()).collect()
            }),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::computer::ComputerInstance;
    use crate::services::config::ConfigService;
    use crate::services::keychain::InMemorySecretStore;
    use crate::services::logger::LogService;
    use crate::services::settings::SettingsService;

    const TEST_INSTANCE_ID: &str = "computer-a";

    fn test_state_without_runtime(path: &std::path::Path) -> AppState {
        let config = ConfigService::new(path.to_path_buf()).unwrap();
        let log_service = LogService::new(path).unwrap();
        let settings_service = SettingsService::new(path.to_path_buf());
        let state = AppState::new_with_secret_store(
            config,
            log_service,
            settings_service,
            InMemorySecretStore::shared(),
        );
        state
            .config
            .add_computer_instance(ComputerInstance::new(TEST_INSTANCE_ID, "Computer A"))
            .unwrap();
        state
    }

    fn server_config(name: &str) -> MCPServerConfig {
        serde_json::from_value(serde_json::json!({
            "type": "Stdio",
            "name": name,
            "server_parameters": {
                "command": "node",
                "args": ["server.js"],
                "env": {}
            }
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn hook_register_does_not_persist_plugin_server_to_config() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state_without_runtime(tmp.path());
        let hooks = MarketplaceMcpHooks::for_plugin(&state, TEST_INSTANCE_ID, "acme", "audit");

        let error = hooks
            .register_server(server_config("audit-mcp"))
            .await
            .unwrap_err();

        assert!(error
            .0
            .contains("Computer instance not found while registering"));
        let managed = state
            .config
            .load_managed_configs_for_instance(TEST_INSTANCE_ID)
            .unwrap();
        assert!(managed.is_empty());
    }

    #[tokio::test]
    async fn hook_remove_without_runtime_does_not_touch_user_config() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state_without_runtime(tmp.path());
        state
            .config
            .add_config_for_instance(TEST_INSTANCE_ID, server_config("audit-mcp"))
            .unwrap();
        let hooks = MarketplaceMcpHooks::for_plugin(&state, TEST_INSTANCE_ID, "acme", "audit");

        let error = hooks.remove_server("audit-mcp").await.unwrap_err();

        assert!(error
            .0
            .contains("Computer instance not found while removing"));
        let managed = state
            .config
            .load_managed_configs_for_instance(TEST_INSTANCE_ID)
            .unwrap();
        assert_eq!(managed.len(), 1);
        assert_eq!(managed[0].name(), "audit-mcp");
        assert!(!managed[0].is_plugin_owned());
    }
}
