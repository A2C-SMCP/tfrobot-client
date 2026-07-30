use crate::commands::runtime_error::RuntimeActionError;
use crate::services::computer::force_mcp_server_enabled;
use crate::AppState;
use a2c_smcp::smcp_computer::errors::ComputerError;
use a2c_smcp::smcp_computer::inputs::load_plugin_inputs;
use a2c_smcp::smcp_computer::mcp_clients::bundle_id::resolve_bundle_id;
use a2c_smcp::smcp_computer::mcp_clients::model::{BundleId, ServerName};
use a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig;
use a2c_smcp::smcp_computer::settings::{
    AddMarketplaceParams, DisableOptions, EnableOptions, EnvMap, InstallOptions, McpHookError,
    McpInstallHooks, RemoveMarketplaceParams, UninstallOptions,
};
use a2c_smcp::smcp_computer::skills::{MCP_INPUTS_FILENAME, MCP_SERVERS_SUBDIR};
use a2c_smcp::smcp_computer::{GovernanceDiagnostic, MarketplaceStatus, PluginStatus};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
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
    /// Sanitized URL returned by the SDK for presentation only.
    ///
    /// Credentials, query, and fragment may be absent. Callers must never submit this value as
    /// the source for an update.
    pub display_git_url: Option<String>,
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
    // Install is intentionally inactive, but the SDK still requires hooks to resolve existing
    // bundle dependencies before recording installation intent.
    let hooks = MarketplaceMcpHooks::for_plugin(
        state,
        instance_id,
        &request.marketplace,
        &request.plugin,
        UserMcpConflictPolicy::Reject,
    )
    .await?;
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
) -> Result<(), RuntimeActionError> {
    enable_plugin_core(&state, &instance_id, request).await
}

pub async fn enable_plugin_core(
    state: &AppState,
    instance_id: &str,
    request: PluginLifecycleRequest,
) -> Result<(), RuntimeActionError> {
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let runtime = ensure_runtime(state, instance_id)
        .await
        .map_err(RuntimeActionError::runtime)?;
    validate_plugin_request(&request).map_err(RuntimeActionError::runtime)?;
    let plugin_id = plugin_id(&request);
    let env = sdk_settings_env(state, instance_id);
    let hooks = MarketplaceMcpHooks::for_plugin(
        state,
        instance_id,
        &request.marketplace,
        &request.plugin,
        UserMcpConflictPolicy::KeepUserServer,
    )
    .await
    .map_err(RuntimeActionError::runtime)?;
    hooks
        .inject_installed_plugin_inputs(&runtime)
        .await
        .map_err(RuntimeActionError::runtime)?;
    if let Err(error) = runtime
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
    {
        let primary = hooks
            .take_runtime_action_error()
            .await
            .unwrap_or_else(|| RuntimeActionError::runtime(error.to_string()));
        let cleanup = async {
            hooks
                .restore_deferred_servers_after_plugin_release(&runtime)
                .await?;
            hooks.reclaim_unowned_plugin_servers(&runtime).await
        }
        .await;
        return match cleanup {
            Ok(()) => Err(primary),
            Err(cleanup_error) => Err(primary.append_context(format!(
                "Marketplace MCP rollback cleanup failed: {cleanup_error}"
            ))),
        };
    }

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
    .await?;
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
    hooks
        .restore_deferred_servers_after_plugin_release(&runtime)
        .await?;
    hooks.reclaim_unowned_plugin_servers(&runtime).await?;
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
    .await?;
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
    hooks
        .restore_deferred_servers_after_plugin_release(&runtime)
        .await?;
    hooks.reclaim_unowned_plugin_servers(&runtime).await?;
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
) -> Result<(), RuntimeActionError> {
    if runtime
        .ensure_runtime_action(crate::services::computer::ComputerRuntimeAction::ManageMcp)
        .await
        .is_err()
    {
        return Ok(());
    }

    let running: HashSet<BundleId> = runtime
        .mcp_server_statuses()
        .await
        .into_iter()
        .filter_map(|(bundle_id, _, is_running, _)| is_running.then_some(bundle_id))
        .collect();
    let mut seen = HashSet::new();
    let bundle_ids = hooks
        .registered_server_ids()
        .await
        .into_iter()
        .filter(|bundle_id| seen.insert(bundle_id.clone()) && !running.contains(bundle_id))
        .collect();
    let failures = runtime.start_mcp_servers_best_effort(bundle_ids).await;
    let mut input_resolution_error = None;
    for (bundle_id, error) in failures {
        if input_resolution_error.is_none() && matches!(&error, ComputerError::InputResolution(_)) {
            input_resolution_error = Some(RuntimeActionError::from(error));
            continue;
        }
        log::warn!(
            "Plugin enabled, but bundled MCP server failed to start for instance {}: {} ({})",
            runtime.instance.id,
            bundle_id,
            error
        );
    }

    match input_resolution_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
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
            display_git_url: marketplace.source_url,
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
    sdk_config: Arc<crate::services::sdk_config::SdkConfigService>,
    registry: Arc<crate::services::computer::ComputerRegistry>,
    instance_id: String,
    marketplace: String,
    plugin: String,
    user_conflict_policy: UserMcpConflictPolicy,
    existing_servers: HashMap<BundleId, ServerName>,
    independent_server_ids: HashSet<BundleId>,
    disabled_independent_server_ids: HashSet<BundleId>,
    registered_server_ids: Arc<tokio::sync::Mutex<Vec<BundleId>>>,
    mounted_server_ids: Arc<tokio::sync::Mutex<HashSet<BundleId>>>,
    preserved_registration_counts: Arc<tokio::sync::Mutex<HashMap<BundleId, usize>>>,
    deferred_restore_server_ids: Arc<tokio::sync::Mutex<HashSet<BundleId>>>,
    first_runtime_action_error: Arc<tokio::sync::Mutex<Option<RuntimeActionError>>>,
}

impl MarketplaceMcpHooks {
    async fn for_plugin(
        state: &AppState,
        instance_id: &str,
        marketplace: &str,
        plugin: &str,
        user_conflict_policy: UserMcpConflictPolicy,
    ) -> Result<Self, String> {
        debug_assert!(!marketplace.trim().is_empty());
        debug_assert!(!plugin.trim().is_empty());
        // The SDK snapshot now projects enabled plugin servers too. Only non-bundled entries are
        // independent declarations that can satisfy a plugin's bundle dependency.
        let independent_servers = state
            .sdk_config
            .load(instance_id)
            .mcp
            .servers
            .into_iter()
            .filter(|server| !server.bundled)
            .collect::<Vec<_>>();
        let disabled_independent_server_ids = independent_servers
            .iter()
            .filter(|server| server.config.disabled())
            .map(|server| resolve_bundle_id(&server.config))
            .collect();
        let mut existing_servers = independent_servers
            .into_iter()
            .map(|server| (resolve_bundle_id(&server.config), server.name))
            .collect::<HashMap<_, _>>();
        let mut independent_server_ids = existing_servers.keys().cloned().collect::<HashSet<_>>();
        independent_server_ids.extend(state
            .sdk_config
            .project_mcp_bundle_ids(instance_id)
            .map_err(|error| {
                format!(
                    "Failed to inspect independent MCP declarations for Computer '{instance_id}': {error}"
                )
            })?);
        for bundle_id in &independent_server_ids {
            existing_servers
                .entry(bundle_id.clone())
                .or_insert_with(|| bundle_id.as_str().to_string());
        }
        if let Some(runtime) = state.computer_registry.runtime(instance_id).await {
            for (bundle_id, name) in runtime.synced_sdk_servers().await {
                if runtime.plugin_mcp_server_owner(&bundle_id).await.is_some() {
                    existing_servers.insert(bundle_id, name);
                }
            }
        }
        Ok(Self {
            sdk_config: state.sdk_config.clone(),
            registry: state.computer_registry.clone(),
            instance_id: instance_id.to_string(),
            marketplace: marketplace.to_string(),
            plugin: plugin.to_string(),
            user_conflict_policy,
            existing_servers,
            independent_server_ids,
            disabled_independent_server_ids,
            registered_server_ids: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            mounted_server_ids: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
            preserved_registration_counts: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            deferred_restore_server_ids: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
            first_runtime_action_error: Arc::new(tokio::sync::Mutex::new(None)),
        })
    }

    async fn registered_server_ids(&self) -> Vec<BundleId> {
        self.registered_server_ids.lock().await.clone()
    }

    async fn inject_installed_plugin_inputs(
        &self,
        runtime: &crate::services::computer::ComputerInstanceRuntime,
    ) -> Result<(), String> {
        let snapshot = runtime.sdk_governance_snapshot().await?;
        let Some(install_path) = snapshot
            .plugins
            .into_iter()
            .find(|plugin| {
                plugin.installed
                    && plugin.marketplace == self.marketplace
                    && plugin.plugin == self.plugin
            })
            .and_then(|plugin| plugin.install_path)
        else {
            // Let the SDK lifecycle method return its canonical not-installed/precondition error.
            return Ok(());
        };
        self.inject_inputs(Path::new(&install_path))
            .await
            .map_err(|error| error.to_string())
    }

    async fn mount_plugin_server(
        &self,
        runtime: &crate::services::computer::ComputerInstanceRuntime,
        config: MCPServerConfig,
    ) -> Result<(), McpHookError> {
        match runtime.add_or_update_plugin_server(config).await {
            Ok(()) => Ok(()),
            Err(error) => {
                let message = error.to_string();
                let mut first_error = self.first_runtime_action_error.lock().await;
                if first_error.is_none() {
                    *first_error = Some(RuntimeActionError::from(error));
                }
                Err(McpHookError(message))
            }
        }
    }

    async fn take_runtime_action_error(&self) -> Option<RuntimeActionError> {
        self.first_runtime_action_error.lock().await.take()
    }

    async fn preserve_existing_registration(&self, bundle_id: BundleId) {
        self.registered_server_ids
            .lock()
            .await
            .push(bundle_id.clone());
        *self
            .preserved_registration_counts
            .lock()
            .await
            .entry(bundle_id)
            .or_default() += 1;
    }

    async fn reclaim_unowned_plugin_servers(
        &self,
        runtime: &crate::services::computer::ComputerInstanceRuntime,
    ) -> Result<(), String> {
        for bundle_id in runtime.tracked_plugin_mcp_server_ids().await {
            if !self.independent_server_ids.contains(&bundle_id)
                && runtime.plugin_mcp_server_owner(&bundle_id).await.is_none()
            {
                runtime.remove_plugin_server(&bundle_id).await?;
            }
        }
        Ok(())
    }

    async fn restore_deferred_servers_after_plugin_release(
        &self,
        runtime: &crate::services::computer::ComputerInstanceRuntime,
    ) -> Result<(), String> {
        let mut deferred = self.deferred_restore_server_ids.lock().await.clone();
        deferred.extend(
            runtime
                .tracked_plugin_mcp_server_ids()
                .await
                .into_iter()
                .filter(|bundle_id| self.independent_server_ids.contains(bundle_id)),
        );
        let user_servers = self
            .sdk_config
            .load(&self.instance_id)
            .mcp
            .servers
            .into_iter()
            .filter(|server| !server.bundled)
            .map(|server| (resolve_bundle_id(&server.config), server.config))
            .collect::<HashMap<_, _>>();
        for bundle_id in deferred {
            if runtime.plugin_mcp_server_owner(&bundle_id).await.is_some() {
                continue;
            }
            if let Some(server) = user_servers.get(&bundle_id) {
                runtime
                    .restore_user_mcp_server_config_after_plugin_release(server.clone())
                    .await?;
            } else if runtime.has_tracked_plugin_mcp_server(&bundle_id).await {
                runtime.remove_plugin_server(&bundle_id).await?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum UserMcpConflictPolicy {
    Reject,
    KeepUserServer,
}

#[async_trait]
impl McpInstallHooks for MarketplaceMcpHooks {
    fn existing_servers(&self) -> HashMap<BundleId, ServerName> {
        self.existing_servers.clone()
    }

    async fn register_server(&self, cfg: MCPServerConfig) -> Result<(), McpHookError> {
        let name = cfg.name().to_string();
        let bundle_id = resolve_bundle_id(&cfg);
        if self.independent_server_ids.contains(&bundle_id)
            && matches!(
                self.user_conflict_policy,
                UserMcpConflictPolicy::KeepUserServer
            )
        {
            if self.disabled_independent_server_ids.contains(&bundle_id) {
                let runtime = self
                    .registry
                    .runtime(&self.instance_id)
                    .await
                    .ok_or_else(|| {
                        McpHookError(format!(
                        "Computer instance not found while activating Marketplace MCP server: {}",
                        self.instance_id
                    ))
                    })?;
                self.mount_plugin_server(&runtime, force_mcp_server_enabled(cfg))
                    .await?;
                self.mounted_server_ids
                    .lock()
                    .await
                    .insert(bundle_id.clone());
                self.registered_server_ids.lock().await.push(bundle_id);
                return Ok(());
            }
            // The pre-enable snapshot is authoritative here: after the SDK writes
            // enabledPlugins=true, its merged read projection may show the plugin origin even
            // though this independent declaration already satisfied the dependency.
            self.preserve_existing_registration(bundle_id).await;
            return Ok(());
        }
        if self
            .sdk_config
            .load(&self.instance_id)
            .mcp
            .servers
            .into_iter()
            .filter(|server| !server.bundled)
            .any(|server| resolve_bundle_id(&server.config) == bundle_id)
        {
            if matches!(
                self.user_conflict_policy,
                UserMcpConflictPolicy::KeepUserServer
            ) {
                self.preserve_existing_registration(bundle_id).await;
                return Ok(());
            }
            return Err(McpHookError(format!(
                "MCP server '{}' already exists as a user-managed MCP server; rename or remove it before installing plugin '{}@{}'",
                name,
                self.plugin,
                self.marketplace,
            )));
        }
        match self.registry.runtime(&self.instance_id).await {
            Some(runtime) => {
                if runtime.sdk_mcp_server_ids().await.contains(&bundle_id) {
                    if runtime.plugin_mcp_server_owner(&bundle_id).await.is_some()
                        || matches!(
                            self.user_conflict_policy,
                            UserMcpConflictPolicy::KeepUserServer
                        )
                    {
                        self.preserve_existing_registration(bundle_id).await;
                        return Ok(());
                    }
                    return Err(McpHookError(format!(
                        "MCP server '{}' already exists in the active SDK Computer; rename or remove it before installing plugin '{}@{}'",
                        name,
                        self.plugin,
                        self.marketplace,
                    )));
                }
                self.mount_plugin_server(&runtime, cfg).await?;
                self.mounted_server_ids
                    .lock()
                    .await
                    .insert(bundle_id.clone());
                self.registered_server_ids.lock().await.push(bundle_id);
                Ok(())
            }
            None => Err(McpHookError(format!(
                "Computer instance not found while registering Marketplace MCP server: {}",
                self.instance_id
            ))),
        }
    }

    async fn remove_server(&self, bundle_id: &BundleId) -> Result<(), McpHookError> {
        match self.registry.runtime(&self.instance_id).await {
            Some(runtime) => {
                let mut preserved = self.preserved_registration_counts.lock().await;
                let mut released_preserved_registration = false;
                if let Some(count) = preserved.get_mut(bundle_id) {
                    *count -= 1;
                    if *count == 0 {
                        preserved.remove(bundle_id);
                        released_preserved_registration = true;
                    } else {
                        return Ok(());
                    }
                }
                drop(preserved);
                if released_preserved_registration {
                    self.deferred_restore_server_ids
                        .lock()
                        .await
                        .insert(bundle_id.clone());
                    return Ok(());
                }
                if self.independent_server_ids.contains(bundle_id) {
                    // The SDK finalizes ownership after hooks return. Keep the transient plugin
                    // config mounted during the callback; the caller restores the authoritative
                    // user declaration once the SDK lifecycle transition has completed.
                    self.deferred_restore_server_ids
                        .lock()
                        .await
                        .insert(bundle_id.clone());
                    return Ok(());
                }
                if matches!(
                    self.user_conflict_policy,
                    UserMcpConflictPolicy::KeepUserServer
                ) && !self.mounted_server_ids.lock().await.contains(bundle_id)
                {
                    return Ok(());
                }
                if !runtime.has_tracked_plugin_mcp_server(bundle_id).await {
                    // This plugin only depended on an independently declared bundle. Disabling
                    // the plugin must not unmount that declaration from the Computer runtime.
                    return Ok(());
                }
                runtime
                    .remove_plugin_server(bundle_id)
                    .await
                    .map_err(McpHookError)
            }
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

        let runtime = self
            .registry
            .runtime(&self.instance_id)
            .await
            .ok_or_else(|| {
                McpHookError(format!(
                    "Computer instance not found while injecting Marketplace inputs: {}",
                    self.instance_id
                ))
            })?;
        for input in inputs {
            runtime
                .add_or_update_input(input)
                .await
                .map_err(McpHookError)?;
        }
        Ok(())
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

    fn audit_bundle_id() -> BundleId {
        BundleId::try_from("audit-mcp").unwrap()
    }

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
        state
            .computer_registry
            .upsert_runtime(instance)
            .await
            .unwrap();
        let hooks = MarketplaceMcpHooks::for_plugin(
            &state,
            TEST_INSTANCE_ID,
            "acme",
            "audit",
            UserMcpConflictPolicy::Reject,
        )
        .await
        .unwrap();

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
        assert!(
            runtime
                .has_tracked_plugin_mcp_server(&audit_bundle_id())
                .await
        );

        hooks.remove_server(&audit_bundle_id()).await.unwrap();
        assert!(state
            .sdk_config
            .load(TEST_INSTANCE_ID)
            .mcp
            .servers
            .is_empty());
        assert!(
            !runtime
                .has_tracked_plugin_mcp_server(&audit_bundle_id())
                .await
        );
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
        .await
        .unwrap();

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
        .await
        .unwrap();

        let error = hooks.remove_server(&audit_bundle_id()).await.unwrap_err();

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
        .await
        .unwrap();

        assert!(hooks.existing_servers().contains_key(&audit_bundle_id()));
        hooks
            .register_server(server_config("audit-mcp"))
            .await
            .unwrap();

        assert_eq!(state.sdk_config.load(TEST_INSTANCE_ID).mcp.servers.len(), 1);
    }
}
