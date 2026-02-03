use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum McpServerConfig {
    Stdio(StdioServerConfig),
    Http(HttpServerConfig),
    Sse(SseServerConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StdioServerConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    pub cwd: Option<String>,
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpServerConfig {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SseServerConfig {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerStatus {
    pub name: String,
    pub config: McpServerConfig,
    pub status: ServerStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerStatus {
    Running,
    Stopped,
    Error { message: String },
}

#[tauri::command]
pub async fn get_mcp_servers() -> Result<Vec<McpServerStatus>, String> {
    // TODO: Integrate with smcp-computer MCPServerManager
    Ok(vec![])
}

#[tauri::command]
pub async fn add_mcp_server(config: McpServerConfig) -> Result<(), String> {
    tracing::info!("Adding MCP server: {:?}", config);
    // TODO: Integrate with smcp-computer MCPServerManager
    Ok(())
}

#[tauri::command]
pub async fn remove_mcp_server(name: String) -> Result<(), String> {
    tracing::info!("Removing MCP server: {}", name);
    // TODO: Integrate with smcp-computer MCPServerManager
    Ok(())
}

#[tauri::command]
pub async fn start_mcp_server(name: String) -> Result<(), String> {
    tracing::info!("Starting MCP server: {}", name);
    // TODO: Integrate with smcp-computer MCPServerManager
    Ok(())
}

#[tauri::command]
pub async fn stop_mcp_server(name: String) -> Result<(), String> {
    tracing::info!("Stopping MCP server: {}", name);
    // TODO: Integrate with smcp-computer MCPServerManager
    Ok(())
}
