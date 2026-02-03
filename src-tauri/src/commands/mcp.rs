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

/// Get all MCP servers with their status
#[tauri::command]
pub async fn get_mcp_servers(state: State<'_, AppState>) -> Result<Vec<McpServerStatus>, String> {
    let manager = state.manager.read().await;
    let statuses = manager.get_server_status().await;

    Ok(statuses
        .into_iter()
        .map(|(name, running, status_message)| McpServerStatus {
            name,
            running,
            status_message,
            disabled: false, // TODO: get from config
        })
        .collect())
}

/// Add a new MCP server
#[tauri::command]
pub async fn add_mcp_server(
    state: State<'_, AppState>,
    config: MCPServerConfig,
) -> Result<(), String> {
    let name = config.name().to_string();
    tracing::info!("Adding MCP server: {}", name);

    // Save to persistent storage first
    state
        .config
        .add_config(config.clone())
        .map_err(|e| e.to_string())?;

    // Then add to manager
    let manager = state.manager.read().await;
    manager
        .add_or_update_server(config)
        .await
        .map_err(|e| e.to_string())?;

    tracing::info!("MCP server added: {}", name);
    Ok(())
}

/// Remove an MCP server
#[tauri::command]
pub async fn remove_mcp_server(state: State<'_, AppState>, name: String) -> Result<(), String> {
    tracing::info!("Removing MCP server: {}", name);

    // Stop and remove from manager
    let manager = state.manager.read().await;
    manager
        .remove_server(&name)
        .await
        .map_err(|e| e.to_string())?;

    // Remove from persistent storage
    state
        .config
        .remove_config(&name)
        .map_err(|e| e.to_string())?;

    tracing::info!("MCP server removed: {}", name);
    Ok(())
}

/// Update an existing MCP server configuration
#[tauri::command]
pub async fn update_mcp_server(
    state: State<'_, AppState>,
    config: MCPServerConfig,
) -> Result<(), String> {
    let name = config.name().to_string();
    tracing::info!("Updating MCP server: {}", name);

    // Update in persistent storage
    state
        .config
        .add_config(config.clone())
        .map_err(|e| e.to_string())?;

    // Update in manager (this will stop and restart if needed)
    let manager = state.manager.read().await;
    manager
        .add_or_update_server(config)
        .await
        .map_err(|e| e.to_string())?;

    tracing::info!("MCP server updated: {}", name);
    Ok(())
}

/// Start a single MCP server
#[tauri::command]
pub async fn start_mcp_server(state: State<'_, AppState>, name: String) -> Result<(), String> {
    tracing::info!("Starting MCP server: {}", name);

    let manager = state.manager.read().await;
    manager
        .start_client(&name)
        .await
        .map_err(|e| e.to_string())?;

    tracing::info!("MCP server started: {}", name);
    Ok(())
}

/// Stop a single MCP server
#[tauri::command]
pub async fn stop_mcp_server(state: State<'_, AppState>, name: String) -> Result<(), String> {
    tracing::info!("Stopping MCP server: {}", name);

    let manager = state.manager.read().await;
    manager
        .stop_client(&name)
        .await
        .map_err(|e| e.to_string())?;

    tracing::info!("MCP server stopped: {}", name);
    Ok(())
}

/// Start all MCP servers
#[tauri::command]
pub async fn start_all_servers(state: State<'_, AppState>) -> Result<(), String> {
    tracing::info!("Starting all MCP servers");

    let manager = state.manager.read().await;
    manager.start_all().await.map_err(|e| e.to_string())?;

    tracing::info!("All MCP servers started");
    Ok(())
}

/// Stop all MCP servers
#[tauri::command]
pub async fn stop_all_servers(state: State<'_, AppState>) -> Result<(), String> {
    tracing::info!("Stopping all MCP servers");

    let manager = state.manager.read().await;
    manager.stop_all().await.map_err(|e| e.to_string())?;

    tracing::info!("All MCP servers stopped");
    Ok(())
}
