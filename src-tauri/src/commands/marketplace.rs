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
use a2c_smcp::smcp_computer::skills::{
    iter_plugin_entries, load_bundled_servers, marketplace_skill_dir, read_marketplace_manifest,
    MCP_INPUTS_FILENAME, MCP_SERVERS_SUBDIR,
};
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
    "update_marketplace",
    "install_plugin",
    "enable_plugin",
    "disable_plugin",
    "uninstall_plugin",
    "reconcile_governance",
];

const AVAILABLE_SDK_APIS: &[&str] = &[
    "Computer::add_marketplace",
    "Computer::refresh_marketplace",
    "Computer::remove_marketplace",
    "Computer::install_plugin",
    "Computer::enable_plugin",
    "Computer::disable_plugin",
    "Computer::uninstall_plugin",
    "Computer::reconcile_governance",
    "Computer::list_mcp_servers_with_metadata",
];

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
pub struct UpdateMarketplaceRequest {
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
pub async fn update_marketplace(
    state: State<'_, AppState>,
    instance_id: String,
    request: UpdateMarketplaceRequest,
) -> Result<(), String> {
    update_marketplace_core(&state, &instance_id, request).await
}

pub async fn update_marketplace_core(
    state: &AppState,
    instance_id: &str,
    request: UpdateMarketplaceRequest,
) -> Result<(), String> {
    let runtime = ensure_runtime(state, instance_id).await?;
    let name = require_non_empty("marketplace name", &request.name)?;
    let git_url = require_non_empty("marketplace git_url", &request.git_url)?;
    let env = sdk_settings_env(state, instance_id);
    let snapshot = marketplace_governance_snapshot(&runtime, &env).await;
    if snapshot.has_installed_plugins_for_marketplace(name) {
        return Err(format!(
            "marketplace '{name}' has installed plugins; uninstall plugins before updating the marketplace URL"
        ));
    }
    runtime
        .sdk_remove_marketplace(
            name,
            RemoveMarketplaceParams {
                keep_plugins: true,
                hooks: None,
            },
        )
        .await
        .map_err(|error| error.to_string())?;
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
    let hooks = MarketplaceMcpHooks::for_plugin(
        state,
        instance_id,
        &request.marketplace,
        &request.plugin,
        UserMcpConflictPolicy::Reject,
    );
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
        .map_err(|error| error.to_string())?;

    start_registered_plugin_servers_if_running(&runtime, &hooks).await
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
    let hooks = MarketplaceMcpHooks::for_plugin(
        state,
        instance_id,
        &request.marketplace,
        &request.plugin,
        UserMcpConflictPolicy::KeepUserServer,
    );
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
        .map_err(|error| error.to_string())?;

    start_registered_plugin_servers_if_running(&runtime, &hooks).await
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
    let hooks = MarketplaceMcpHooks::for_plugin(
        state,
        instance_id,
        &request.marketplace,
        &request.plugin,
        UserMcpConflictPolicy::Reject,
    );
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
    let hooks = MarketplaceMcpHooks::for_plugin(
        state,
        instance_id,
        &request.marketplace,
        &request.plugin,
        UserMcpConflictPolicy::Reject,
    );
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

async fn start_registered_plugin_servers_if_running(
    runtime: &crate::services::computer::ComputerInstanceRuntime,
    hooks: &MarketplaceMcpHooks,
) -> Result<(), String> {
    if !runtime.is_running().await {
        return Ok(());
    }

    for name in hooks.registered_server_names().await {
        runtime.start_mcp_server(&name).await?;
    }

    Ok(())
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
            .any(|plugin| plugin.marketplace == marketplace && plugin.status != "available")
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
    // Keep the read model constrained to a snapshot. Cold-start recovery,
    // env-aware settings merge, and bundled MCP remount remain SDK-owned
    // lifecycle responsibilities; tfrobot-client must not compensate by
    // mutating SDK governance state from this view.
    let policy = resolve_policy_settings(Some(env), None, None);
    let cwd = state_like_instance_cwd_from_skill_home(&skill_home);
    let declared = resolve_settings(ResolveSettingsArgs {
        cwd: cwd.as_deref(),
        env: Some(env),
        flag_settings_path: None,
        policy_settings: Some(&policy),
    })
    .settings;
    let active_skills = runtime.sdk_skills().await;

    let known_marketplaces = known.account.marketplaces;
    let installed_plugins = installed.account.plugins;
    let installed_plugin_ids: HashSet<String> = installed_plugins.keys().cloned().collect();

    let marketplaces = known_marketplaces
        .iter()
        .map(|(name, entry)| MarketplaceSummary {
            name: name.clone(),
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

    let mut plugins: Vec<PluginSummary> = installed_plugins
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

    for (marketplace, _) in &known_marketplaces {
        plugins.extend(available_plugin_summaries(
            &skill_home,
            marketplace,
            &installed_plugin_ids,
        ));
    }

    plugins.sort_by(|left, right| {
        left.marketplace
            .cmp(&right.marketplace)
            .then_with(|| left.plugin.cmp(&right.plugin))
            .then_with(|| left.status.cmp(&right.status))
    });

    GovernanceSnapshot {
        marketplaces,
        plugins,
    }
}

fn state_like_instance_cwd_from_skill_home(skill_home: &Path) -> Option<std::path::PathBuf> {
    skill_home.parent().map(Path::to_path_buf)
}

fn available_plugin_summaries(
    skill_home: &Path,
    marketplace: &str,
    installed_plugin_ids: &HashSet<String>,
) -> Vec<PluginSummary> {
    let catalog_dir = marketplace_skill_dir(skill_home, marketplace, &[]);
    let manifest = match read_marketplace_manifest(&catalog_dir) {
        Ok(manifest) => manifest,
        Err(error) => {
            log::warn!(
                "Failed to read marketplace manifest for '{}': {}",
                marketplace,
                error
            );
            return Vec::new();
        }
    };

    iter_plugin_entries(&manifest)
        .into_iter()
        .filter_map(|entry| {
            let plugin = entry.get("name").and_then(Value::as_str)?.trim();
            if plugin.is_empty() {
                return None;
            }
            let plugin_id = format!("{plugin}@{marketplace}");
            if installed_plugin_ids.contains(&plugin_id) {
                return None;
            }
            let plugin_root = catalog_dir.join(
                entry
                    .get("source")
                    .and_then(Value::as_str)
                    .unwrap_or(plugin),
            );
            let bundled_mcp_servers = load_bundled_servers(&plugin_root)
                .map(|servers| {
                    servers
                        .into_iter()
                        .map(|server| server.name().to_string())
                        .collect()
                })
                .unwrap_or_default();
            let bundled_skills = std::fs::read_dir(plugin_root.join("skills"))
                .map(|entries| {
                    entries
                        .filter_map(Result::ok)
                        .filter(|entry| entry.path().join("SKILL.md").is_file())
                        .filter_map(|entry| entry.file_name().into_string().ok())
                        .map(|skill| format!("{plugin}:{skill}"))
                        .collect()
                })
                .unwrap_or_default();

            Some(PluginSummary {
                marketplace: marketplace.to_string(),
                plugin: plugin.to_string(),
                plugin_id: Some(plugin_id),
                version: entry
                    .get("version")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                enabled: false,
                status: "available".to_string(),
                bundled_mcp_servers,
                bundled_skills,
                message: entry
                    .get("description")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
            })
        })
        .collect()
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
    user_conflict_policy: UserMcpConflictPolicy,
    registered_server_names: Arc<tokio::sync::Mutex<Vec<String>>>,
}

impl MarketplaceMcpHooks {
    fn for_plugin(
        state: &AppState,
        instance_id: &str,
        marketplace: &str,
        plugin: &str,
        user_conflict_policy: UserMcpConflictPolicy,
    ) -> Self {
        debug_assert!(!marketplace.trim().is_empty());
        debug_assert!(!plugin.trim().is_empty());
        Self {
            config: state.config.clone(),
            registry: state.computer_registry.clone(),
            instance_id: instance_id.to_string(),
            marketplace: marketplace.to_string(),
            plugin: plugin.to_string(),
            plugin_id: format!("{plugin}@{marketplace}"),
            user_conflict_policy,
            registered_server_names: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        }
    }

    async fn registered_server_names(&self) -> Vec<String> {
        self.registered_server_names.lock().await.clone()
    }
}

#[derive(Clone, Copy)]
enum UserMcpConflictPolicy {
    Reject,
    KeepUserServer,
}

#[async_trait]
impl McpInstallHooks for MarketplaceMcpHooks {
    fn existing_server_names(&self) -> HashSet<String> {
        self.config
            .load_managed_configs_for_instance(&self.instance_id)
            .map(|servers| {
                servers
                    .into_iter()
                    .filter(|server| {
                        server.is_plugin_owned()
                            || matches!(self.user_conflict_policy, UserMcpConflictPolicy::Reject)
                    })
                    .map(|server| server.name().to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    async fn register_server(&self, cfg: MCPServerConfig) -> Result<(), McpHookError> {
        let name = cfg.name().to_string();
        if let Ok(existing) = self
            .config
            .get_managed_config_for_instance(&self.instance_id, &name)
        {
            if !existing.is_plugin_owned() {
                if matches!(
                    self.user_conflict_policy,
                    UserMcpConflictPolicy::KeepUserServer
                ) {
                    return Ok(());
                }
                return Err(McpHookError(format!(
                    "MCP server '{}' already exists as a user-managed MCP server; rename or remove it before installing plugin '{}@{}'",
                    name,
                    self.plugin,
                    self.marketplace,
                )));
            }
        }
        let managed_by = McpServerManagedBy::Plugin {
            marketplace: self.marketplace.clone(),
            plugin: self.plugin.clone(),
            plugin_id: (!self.plugin_id.is_empty()).then(|| self.plugin_id.clone()),
        };
        match self.registry.runtime(&self.instance_id).await {
            Some(runtime) => {
                if runtime.sdk_mcp_server_names().await.contains(&name) {
                    if let Some(owner) = runtime.plugin_mcp_server_owner(&name).await {
                        if same_plugin_owner(&owner, &managed_by) {
                            self.registered_server_names.lock().await.push(name);
                            return Ok(());
                        }
                        return Err(McpHookError(format!(
                            "MCP server '{}' is already managed by another Marketplace plugin",
                            name
                        )));
                    }
                    return Err(McpHookError(format!(
                        "MCP server '{}' already exists in the active SDK Computer; rename or remove it before installing plugin '{}@{}'",
                        name,
                        self.plugin,
                        self.marketplace,
                    )));
                }
                if let Some(owner) = runtime.plugin_mcp_server_owner(&name).await {
                    if !same_plugin_owner(&owner, &managed_by) {
                        return Err(McpHookError(format!(
                            "MCP server '{}' is already managed by another Marketplace plugin",
                            name
                        )));
                    }
                }
                runtime
                    .add_or_update_plugin_server(cfg, managed_by)
                    .await
                    .map_err(McpHookError)?;
                self.registered_server_names.lock().await.push(name);
                Ok(())
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
        let hooks = MarketplaceMcpHooks::for_plugin(
            &state,
            TEST_INSTANCE_ID,
            "acme",
            "audit",
            UserMcpConflictPolicy::Reject,
        );

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
    async fn hook_register_rejects_user_owned_server_name_conflict() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state_without_runtime(tmp.path());
        state
            .config
            .add_config_for_instance(TEST_INSTANCE_ID, server_config("audit-mcp"))
            .unwrap();
        let hooks = MarketplaceMcpHooks::for_plugin(
            &state,
            TEST_INSTANCE_ID,
            "acme",
            "audit",
            UserMcpConflictPolicy::Reject,
        );

        let error = hooks
            .register_server(server_config("audit-mcp"))
            .await
            .unwrap_err();

        assert!(error
            .0
            .contains("already exists as a user-managed MCP server"));
        let managed = state
            .config
            .load_managed_configs_for_instance(TEST_INSTANCE_ID)
            .unwrap();
        assert_eq!(managed.len(), 1);
        assert_eq!(managed[0].name(), "audit-mcp");
        assert!(!managed[0].is_plugin_owned());
    }

    #[tokio::test]
    async fn hook_remove_without_runtime_does_not_touch_user_config() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state_without_runtime(tmp.path());
        state
            .config
            .add_config_for_instance(TEST_INSTANCE_ID, server_config("audit-mcp"))
            .unwrap();
        let hooks = MarketplaceMcpHooks::for_plugin(
            &state,
            TEST_INSTANCE_ID,
            "acme",
            "audit",
            UserMcpConflictPolicy::Reject,
        );

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

    #[tokio::test]
    async fn hook_register_keeps_user_server_when_policy_allows_conflict() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state_without_runtime(tmp.path());
        state
            .config
            .add_config_for_instance(TEST_INSTANCE_ID, server_config("audit-mcp"))
            .unwrap();
        let hooks = MarketplaceMcpHooks::for_plugin(
            &state,
            TEST_INSTANCE_ID,
            "acme",
            "audit",
            UserMcpConflictPolicy::KeepUserServer,
        );

        assert!(!hooks.existing_server_names().contains("audit-mcp"));
        hooks
            .register_server(server_config("audit-mcp"))
            .await
            .unwrap();

        let managed = state
            .config
            .load_managed_configs_for_instance(TEST_INSTANCE_ID)
            .unwrap();
        assert_eq!(managed.len(), 1);
        assert_eq!(managed[0].name(), "audit-mcp");
        assert!(!managed[0].is_plugin_owned());
    }
}
