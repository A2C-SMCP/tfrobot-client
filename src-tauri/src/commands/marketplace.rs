use crate::commands::inputs::{InputDefinition, PickOption};
use crate::services::computer::McpServerManagedBy;
use crate::AppState;
use a2c_smcp::smcp_computer::inputs::load_plugin_inputs;
use a2c_smcp::smcp_computer::mcp_clients::model::MCPServerInput;
use a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig;
use a2c_smcp::smcp_computer::settings::{
    AddMarketplaceParams, DisableOptions, EnableOptions, EnvMap, InstallOptions, McpHookError,
    McpInstallHooks, RemoveMarketplaceParams, UninstallOptions,
};
use a2c_smcp::smcp_computer::skills::{MCP_INPUTS_FILENAME, MCP_SERVERS_SUBDIR};
use a2c_smcp::smcp_computer::{GovernanceDiagnostic, MarketplaceStatus, PluginStatus};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
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
    "Computer::governance_snapshot",
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
    pub installed: bool,
    pub enabled: bool,
    pub status: String,
    pub bundled_mcp_servers: Vec<String>,
    pub bundled_skills: Vec<String>,
    /// Catalog-declared capabilities. `None` means the SDK could not inspect the declaration;
    /// `Some` with empty lists means the plugin explicitly declares no such capability.
    pub declared: Option<DeclaredPluginCapabilities>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeclaredPluginCapabilities {
    pub version: Option<String>,
    pub description: Option<String>,
    pub mcp_servers: Vec<String>,
    pub skills: Vec<String>,
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
    let snapshot = marketplace_governance_snapshot(&runtime).await?;

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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
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
        .map_err(|error| error.to_string())?;
    runtime.mark_sdk_skills_dirty().await;
    Ok(())
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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let runtime = ensure_runtime(state, instance_id).await?;
    require_non_empty("marketplace", marketplace)?;
    let snapshot = marketplace_governance_snapshot(&runtime).await?;
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
        .map_err(|error| error.to_string())?;
    runtime.mark_sdk_skills_dirty().await;
    Ok(())
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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let runtime = ensure_runtime(state, instance_id).await?;
    let name = require_non_empty("marketplace name", &request.name)?;
    let git_url = require_non_empty("marketplace git_url", &request.git_url)?;
    let snapshot = marketplace_governance_snapshot(&runtime).await?;
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
        .map_err(|error| error.to_string())?;
    runtime.mark_sdk_skills_dirty().await;
    Ok(())
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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let runtime = ensure_runtime(state, instance_id).await?;
    validate_plugin_request(&request)?;
    let plugin_id = plugin_id(&request);
    let env = sdk_settings_env(state, instance_id);
    // Install is intentionally inactive, but the SDK still requires hooks to enforce its
    // zero-mutation bundled MCP name-conflict gate before recording installation intent.
    let hooks = MarketplaceMcpHooks::for_plugin(
        state,
        instance_id,
        &request.marketplace,
        &request.plugin,
        UserMcpConflictPolicy::Reject,
    )
    .await;
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

    runtime.mark_sdk_skills_dirty().await;
    Ok(())
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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
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
    )
    .await;
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

    runtime.mark_sdk_skills_dirty().await;
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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
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
    )
    .await;
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
        .map_err(|error| error.to_string())?;
    runtime.mark_sdk_skills_dirty().await;
    Ok(())
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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
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
    )
    .await;
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
        .map_err(|error| error.to_string())?;
    runtime.mark_sdk_skills_dirty().await;
    Ok(())
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
        runtime
            .start_mcp_server(&name)
            .await
            .map_err(|error| error.to_string())?;
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
    installed_marketplaces: HashSet<String>,
}

impl GovernanceSnapshot {
    fn has_installed_plugins_for_marketplace(&self, marketplace: &str) -> bool {
        self.installed_marketplaces.contains(marketplace)
    }
}

async fn marketplace_governance_snapshot(
    runtime: &crate::services::computer::ComputerInstanceRuntime,
) -> Result<GovernanceSnapshot, String> {
    let snapshot = runtime.sdk_governance_snapshot().await?;
    let installed_marketplaces = snapshot
        .plugins
        .iter()
        .filter(|plugin| plugin.installed)
        .map(|plugin| plugin.marketplace.clone())
        .collect();
    let marketplaces = snapshot
        .marketplaces
        .into_iter()
        .map(|marketplace| MarketplaceSummary {
            name: marketplace.name,
            git_url: marketplace.source_url,
            status: marketplace_status(marketplace.status).to_string(),
            message: diagnostic_message(
                &marketplace.diagnostics,
                marketplace
                    .last_updated
                    .map(|last_updated| format!("lastUpdated={last_updated}")),
            ),
        })
        .collect();
    let plugins = snapshot
        .plugins
        .into_iter()
        .map(|plugin| {
            let declared = plugin.declared.as_ref();
            let version = plugin
                .version
                .clone()
                .or_else(|| declared.and_then(|caps| caps.version.clone()));
            let bundled_mcp_servers = if plugin.installed {
                plugin.bundled_mcp_servers.clone()
            } else {
                declared
                    .map(|caps| caps.mcp_servers.clone())
                    .unwrap_or_default()
            };
            let bundled_skills = if plugin.installed {
                plugin.bundled_skills.clone()
            } else {
                declared.map(|caps| caps.skills.clone()).unwrap_or_default()
            };
            let fallback_message = plugin
                .install_path
                .clone()
                .or_else(|| declared.and_then(|caps| caps.description.clone()));
            let declared = declared.map(|caps| DeclaredPluginCapabilities {
                version: caps.version.clone(),
                description: caps.description.clone(),
                mcp_servers: caps.mcp_servers.clone(),
                skills: caps.skills.clone(),
            });

            PluginSummary {
                marketplace: plugin.marketplace,
                plugin: plugin.plugin,
                plugin_id: Some(plugin.id),
                version,
                installed: plugin.installed,
                enabled: plugin.enabled,
                status: plugin_status(plugin.status).to_string(),
                bundled_mcp_servers,
                bundled_skills,
                declared,
                message: diagnostic_message(&plugin.diagnostics, fallback_message),
            }
        })
        .collect();
    Ok(GovernanceSnapshot {
        marketplaces,
        plugins,
        installed_marketplaces,
    })
}

fn marketplace_status(status: MarketplaceStatus) -> &'static str {
    match status {
        MarketplaceStatus::Known => "known",
        MarketplaceStatus::Available => "available",
        MarketplaceStatus::Degraded => "degraded",
        _ => "unknown",
    }
}

fn plugin_status(status: PluginStatus) -> &'static str {
    match status {
        PluginStatus::Available => "available",
        PluginStatus::InstalledDisabled => "disabled",
        PluginStatus::InstalledEnabled => "enabled",
        PluginStatus::Degraded => "degraded",
        _ => "unknown",
    }
}

fn diagnostic_message(
    diagnostics: &[GovernanceDiagnostic],
    fallback: Option<String>,
) -> Option<String> {
    if diagnostics.is_empty() {
        fallback
    } else {
        Some(
            diagnostics
                .iter()
                .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.message))
                .collect::<Vec<_>>()
                .join("; "),
        )
    }
}

fn sdk_settings_env(state: &AppState, instance_id: &str) -> EnvMap {
    state.sdk_config.env(instance_id)
}

fn plugin_id(request: &PluginLifecycleRequest) -> String {
    format!("{}@{}", request.plugin.trim(), request.marketplace.trim())
}

struct MarketplaceMcpHooks {
    config: Arc<crate::services::config::ConfigService>,
    sdk_config: Arc<crate::services::sdk_config::SdkConfigService>,
    registry: Arc<crate::services::computer::ComputerRegistry>,
    secret_store: Arc<dyn crate::services::keychain::SecretStore>,
    input_mutation_lock: Arc<tokio::sync::Mutex<()>>,
    instance_id: String,
    marketplace: String,
    plugin: String,
    plugin_id: String,
    user_conflict_policy: UserMcpConflictPolicy,
    existing_server_names: HashSet<String>,
    registered_server_names: Arc<tokio::sync::Mutex<Vec<String>>>,
}

impl MarketplaceMcpHooks {
    async fn for_plugin(
        state: &AppState,
        instance_id: &str,
        marketplace: &str,
        plugin: &str,
        user_conflict_policy: UserMcpConflictPolicy,
    ) -> Self {
        debug_assert!(!marketplace.trim().is_empty());
        debug_assert!(!plugin.trim().is_empty());
        // Every server in the SDK snapshot is a real user declaration. `bundled` only reports
        // that an installed plugin contributes the same name; it is not ownership metadata.
        let mut existing_server_names =
            if matches!(user_conflict_policy, UserMcpConflictPolicy::Reject) {
                state
                    .sdk_config
                    .load(instance_id)
                    .mcp
                    .servers
                    .into_iter()
                    .map(|server| server.name)
                    .collect::<HashSet<_>>()
            } else {
                HashSet::new()
            };
        if let Some(runtime) = state.computer_registry.runtime(instance_id).await {
            for name in runtime.sdk_mcp_server_names().await {
                if runtime.plugin_mcp_server_owner(&name).await.is_some() {
                    existing_server_names.insert(name);
                }
            }
        }
        Self {
            config: state.config.clone(),
            sdk_config: state.sdk_config.clone(),
            registry: state.computer_registry.clone(),
            secret_store: state.secret_store.clone(),
            input_mutation_lock: state.input_mutation_lock.clone(),
            instance_id: instance_id.to_string(),
            marketplace: marketplace.to_string(),
            plugin: plugin.to_string(),
            plugin_id: format!("{plugin}@{marketplace}"),
            user_conflict_policy,
            existing_server_names,
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
        self.existing_server_names.clone()
    }

    async fn register_server(&self, cfg: MCPServerConfig) -> Result<(), McpHookError> {
        let name = cfg.name().to_string();
        if self
            .sdk_config
            .load(&self.instance_id)
            .mcp
            .servers
            .into_iter()
            .any(|server| server.name == name)
        {
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
                    if matches!(
                        self.user_conflict_policy,
                        UserMcpConflictPolicy::KeepUserServer
                    ) {
                        return Ok(());
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

        let _mutation_guard = self.input_mutation_lock.lock().await;
        let mut definitions = self
            .config
            .load_inputs_for_instance(&self.instance_id)
            .map_err(|error| McpHookError(error.to_string()))?;
        for input in &inputs {
            let id = input.id().to_string();
            definitions.retain(|definition| definition.id() != id);
            definitions.push(input_definition_from_mcp(input));
        }
        crate::commands::inputs::replace_global_input_definitions_with_parts_locked(
            self.config.as_ref(),
            self.registry.as_ref(),
            self.secret_store.as_ref(),
            &self.instance_id,
            &definitions,
        )
        .await
        .map_err(McpHookError)?;
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

    fn seed_sdk_user_server(state: &AppState, name: &str) {
        use a2c_smcp::smcp_computer::settings::config::{ConfigEdit, ConfigEntity, EditIntent};

        state
            .sdk_config
            .update(
                TEST_INSTANCE_ID,
                &[ConfigEdit::new(
                    ConfigEntity::McpServer(name.to_string()),
                    EditIntent::Upsert(serde_json::json!({
                        "type": "stdio",
                        "server_parameters": {
                            "command": "node",
                            "args": ["server.js"],
                            "env": {}
                        }
                    })),
                )],
            )
            .unwrap();
    }

    #[tokio::test]
    async fn hook_register_does_not_persist_plugin_server_to_config() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state_without_runtime(tmp.path());
        let instance = state
            .config
            .get_computer_instance(TEST_INSTANCE_ID)
            .unwrap();
        state.computer_registry.upsert_runtime(instance).await;
        let hooks = MarketplaceMcpHooks::for_plugin(
            &state,
            TEST_INSTANCE_ID,
            "acme",
            "audit",
            UserMcpConflictPolicy::Reject,
        )
        .await;

        hooks
            .register_server(server_config("audit-mcp"))
            .await
            .unwrap();

        assert!(state
            .sdk_config
            .load(TEST_INSTANCE_ID)
            .mcp
            .servers
            .is_empty());
        let runtime = state
            .computer_registry
            .runtime(TEST_INSTANCE_ID)
            .await
            .unwrap();
        assert!(runtime.plugin_mcp_server_owner("audit-mcp").await.is_some());

        hooks.remove_server("audit-mcp").await.unwrap();
        assert!(state
            .sdk_config
            .load(TEST_INSTANCE_ID)
            .mcp
            .servers
            .is_empty());
        assert!(runtime.plugin_mcp_server_owner("audit-mcp").await.is_none());
    }

    #[tokio::test]
    async fn hook_register_rejects_user_owned_server_name_conflict() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state_without_runtime(tmp.path());
        seed_sdk_user_server(&state, "audit-mcp");
        let hooks = MarketplaceMcpHooks::for_plugin(
            &state,
            TEST_INSTANCE_ID,
            "acme",
            "audit",
            UserMcpConflictPolicy::Reject,
        )
        .await;

        let error = hooks
            .register_server(server_config("audit-mcp"))
            .await
            .unwrap_err();

        assert!(error
            .0
            .contains("already exists as a user-managed MCP server"));
        assert_eq!(state.sdk_config.load(TEST_INSTANCE_ID).mcp.servers.len(), 1);
    }

    #[tokio::test]
    async fn hook_remove_without_runtime_does_not_touch_user_config() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state_without_runtime(tmp.path());
        seed_sdk_user_server(&state, "audit-mcp");
        let hooks = MarketplaceMcpHooks::for_plugin(
            &state,
            TEST_INSTANCE_ID,
            "acme",
            "audit",
            UserMcpConflictPolicy::Reject,
        )
        .await;

        let error = hooks.remove_server("audit-mcp").await.unwrap_err();

        assert!(error
            .0
            .contains("Computer instance not found while removing"));
        assert_eq!(state.sdk_config.load(TEST_INSTANCE_ID).mcp.servers.len(), 1);
    }

    #[tokio::test]
    async fn hook_register_keeps_user_server_when_policy_allows_conflict() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state_without_runtime(tmp.path());
        seed_sdk_user_server(&state, "audit-mcp");
        let hooks = MarketplaceMcpHooks::for_plugin(
            &state,
            TEST_INSTANCE_ID,
            "acme",
            "audit",
            UserMcpConflictPolicy::KeepUserServer,
        )
        .await;

        assert!(!hooks.existing_server_names().contains("audit-mcp"));
        hooks
            .register_server(server_config("audit-mcp"))
            .await
            .unwrap();

        assert_eq!(state.sdk_config.load(TEST_INSTANCE_ID).mcp.servers.len(), 1);
    }
}
