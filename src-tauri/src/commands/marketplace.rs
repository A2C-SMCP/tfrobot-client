use crate::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;

const SDK_GOVERNANCE_UNSUPPORTED: &str = "smcp-computer does not expose Computer-level marketplace/plugin lifecycle APIs in the current SDK version; tfrobot-client will not emulate SDK governance ledgers";

const REQUIRED_SDK_APIS: &[&str] = &[
    "Computer::add_marketplace",
    "Computer::refresh_marketplace",
    "Computer::remove_marketplace",
    "Computer::install_plugin",
    "Computer::enable_plugin",
    "Computer::disable_plugin",
    "Computer::uninstall_plugin",
    "Computer::reconcile_governance",
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
    Ok(unsupported_capabilities())
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
    ensure_runtime(state, instance_id).await?;
    Ok(unsupported_governance())
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
    ensure_runtime(state, instance_id).await?;
    require_non_empty("marketplace name", &request.name)?;
    require_non_empty("marketplace git_url", &request.git_url)?;
    unsupported_lifecycle()
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
    ensure_runtime(state, instance_id).await?;
    require_non_empty("marketplace", marketplace)?;
    unsupported_lifecycle()
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
    ensure_runtime(state, instance_id).await?;
    require_non_empty("marketplace", marketplace)?;
    unsupported_lifecycle()
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
    ensure_runtime(state, instance_id).await?;
    validate_plugin_request(&request)?;
    unsupported_lifecycle()
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
    ensure_runtime(state, instance_id).await?;
    validate_plugin_request(&request)?;
    unsupported_lifecycle()
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
    ensure_runtime(state, instance_id).await?;
    validate_plugin_request(&request)?;
    unsupported_lifecycle()
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
    ensure_runtime(state, instance_id).await?;
    validate_plugin_request(&request)?;
    unsupported_lifecycle()
}

#[tauri::command]
pub async fn reconcile_governance(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<(), String> {
    reconcile_governance_core(&state, &instance_id).await
}

pub async fn reconcile_governance_core(state: &AppState, instance_id: &str) -> Result<(), String> {
    ensure_runtime(state, instance_id).await?;
    unsupported_lifecycle()
}

async fn ensure_runtime(state: &AppState, instance_id: &str) -> Result<(), String> {
    let instance_id = require_non_empty("instance_id", instance_id)?;
    state
        .computer_registry
        .runtime(instance_id)
        .await
        .map(|_| ())
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

fn unsupported_capabilities() -> MarketplaceCapabilities {
    MarketplaceCapabilities {
        computer_lifecycle_api_available: false,
        supported_operations: Vec::new(),
        required_sdk_apis: REQUIRED_SDK_APIS
            .iter()
            .map(|api| (*api).to_string())
            .collect(),
        reason: SDK_GOVERNANCE_UNSUPPORTED.to_string(),
    }
}

fn unsupported_governance() -> MarketplaceGovernance {
    MarketplaceGovernance {
        capabilities: unsupported_capabilities(),
        marketplaces: Vec::new(),
        plugins: Vec::new(),
    }
}

fn unsupported_lifecycle<T>() -> Result<T, String> {
    Err(SDK_GOVERNANCE_UNSUPPORTED.to_string())
}
