use crate::AppState;
use serde::{Deserialize, Serialize};
use smcp_computer::mcp_clients::MCPServerConfig;
use tauri::State;

/// Server status returned to frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerStatus {
    pub name: String,
    pub running: bool,
    pub status_message: String,
    pub disabled: bool,
}

#[tauri::command]
pub async fn get_mcp_servers(state: State<'_, AppState>) -> Result<Vec<McpServerStatus>, String> {
    let lock = state.manager.read().await;
    let mgr = lock.as_ref().ok_or("MCP manager not initialized".to_string())?;
    let statuses = mgr.get_server_status().await;

    Ok(statuses
        .into_iter()
        .map(|(name, running, status_message)| McpServerStatus {
            name,
            running,
            status_message,
            disabled: false,
        })
        .collect())
}

#[tauri::command]
pub async fn add_mcp_server(
    state: State<'_, AppState>,
    config: MCPServerConfig,
) -> Result<(), String> {
    let name = config.name().to_string();
    tracing::info!("Adding MCP server: {}", name);

    state
        .config
        .add_config(config.clone())
        .map_err(|e| e.to_string())?;

    let lock = state.manager.read().await;
    let mgr = lock.as_ref().ok_or("MCP manager not initialized".to_string())?;
    mgr.add_or_update_server(config)
        .await
        .map_err(|e| e.to_string())?;

    tracing::info!("MCP server added: {}", name);
    Ok(())
}

#[tauri::command]
pub async fn remove_mcp_server(state: State<'_, AppState>, name: String) -> Result<(), String> {
    tracing::info!("Removing MCP server: {}", name);

    let lock = state.manager.read().await;
    let mgr = lock.as_ref().ok_or("MCP manager not initialized".to_string())?;
    mgr.remove_server(&name)
        .await
        .map_err(|e| e.to_string())?;

    state
        .config
        .remove_config(&name)
        .map_err(|e| e.to_string())?;

    tracing::info!("MCP server removed: {}", name);
    Ok(())
}

#[tauri::command]
pub async fn update_mcp_server(
    state: State<'_, AppState>,
    config: MCPServerConfig,
) -> Result<(), String> {
    let name = config.name().to_string();
    tracing::info!("Updating MCP server: {}", name);

    state
        .config
        .add_config(config.clone())
        .map_err(|e| e.to_string())?;

    let lock = state.manager.read().await;
    let mgr = lock.as_ref().ok_or("MCP manager not initialized".to_string())?;
    mgr.add_or_update_server(config)
        .await
        .map_err(|e| e.to_string())?;

    tracing::info!("MCP server updated: {}", name);
    Ok(())
}

#[tauri::command]
pub async fn start_mcp_server(state: State<'_, AppState>, name: String) -> Result<(), String> {
    tracing::info!("Starting MCP server: {}", name);

    let lock = state.manager.read().await;
    let mgr = lock.as_ref().ok_or("MCP manager not initialized".to_string())?;
    mgr.start_client(&name)
        .await
        .map_err(|e| e.to_string())?;

    tracing::info!("MCP server started: {}", name);
    Ok(())
}

#[tauri::command]
pub async fn stop_mcp_server(state: State<'_, AppState>, name: String) -> Result<(), String> {
    tracing::info!("Stopping MCP server: {}", name);

    let lock = state.manager.read().await;
    let mgr = lock.as_ref().ok_or("MCP manager not initialized".to_string())?;
    mgr.stop_client(&name)
        .await
        .map_err(|e| e.to_string())?;

    tracing::info!("MCP server stopped: {}", name);
    Ok(())
}

#[tauri::command]
pub async fn start_all_servers(state: State<'_, AppState>) -> Result<(), String> {
    tracing::info!("Starting all MCP servers");

    let lock = state.manager.read().await;
    let mgr = lock.as_ref().ok_or("MCP manager not initialized".to_string())?;
    mgr.start_all().await.map_err(|e| e.to_string())?;

    tracing::info!("All MCP servers started");
    Ok(())
}

#[tauri::command]
pub async fn stop_all_servers(state: State<'_, AppState>) -> Result<(), String> {
    tracing::info!("Stopping all MCP servers");

    let lock = state.manager.read().await;
    let mgr = lock.as_ref().ok_or("MCP manager not initialized".to_string())?;
    mgr.stop_all().await.map_err(|e| e.to_string())?;

    tracing::info!("All MCP servers stopped");
    Ok(())
}
