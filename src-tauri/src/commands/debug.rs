use crate::AppState;
use serde::{Deserialize, Serialize};
use smcp_computer::mcp_clients::model::{CallToolResult, Tool};
use tauri::State;

/// Tool with server attribution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
    pub server: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

/// Tool call response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<CallToolResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub duration_ms: u64,
}

/// Tool call history record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallHistoryRecord {
    pub timestamp: String,
    pub req_id: String,
    pub server: String,
    pub tool: String,
    pub parameters: serde_json::Value,
    pub timeout: Option<f64>,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Get all available tools from running MCP servers
#[tauri::command]
pub async fn get_available_tools(
    state: State<'_, AppState>,
) -> Result<Vec<ToolInfo>, String> {
    let lock = state.manager.read().await;
    let mgr = lock.as_ref().ok_or("MCP manager not initialized".to_string())?;
    let tools: Vec<Tool> = mgr.list_available_tools().await;

    let result = tools
        .into_iter()
        .map(|t| {
            // Extract server name from tool meta if available
            let server = t
                .meta
                .as_ref()
                .and_then(|m| m.get("server_name"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            let tags = t
                .meta
                .as_ref()
                .and_then(|m| m.get("tags"))
                .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok());

            ToolInfo {
                name: t.name,
                description: t.description,
                input_schema: t.input_schema,
                server,
                tags,
            }
        })
        .collect();

    Ok(result)
}

/// Execute a tool call for testing
#[tauri::command]
pub async fn execute_tool(
    state: State<'_, AppState>,
    tool_name: String,
    params: serde_json::Value,
    timeout: Option<f64>,
) -> Result<ToolCallResponse, String> {
    tracing::info!("Executing tool: {}", tool_name);

    let lock = state.manager.read().await;
    let mgr = lock.as_ref().ok_or("MCP manager not initialized".to_string())?;

    let start = std::time::Instant::now();
    let duration_timeout = timeout.map(|s| std::time::Duration::from_secs_f64(s));

    let result = mgr.execute_tool(&tool_name, params, duration_timeout).await;
    let duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(call_result) => {
            let _ = state.log_service.write(
                if call_result.is_error { "error" } else { "info" },
                "tool",
                &format!("Tool {} executed ({}ms)", tool_name, duration_ms),
                None,
            );
            Ok(ToolCallResponse {
                success: !call_result.is_error,
                result: Some(call_result),
                error: None,
                duration_ms,
            })
        }
        Err(e) => {
            let _ = state.log_service.write(
                "error",
                "tool",
                &format!("Tool {} failed: {}", tool_name, e),
                None,
            );
            Ok(ToolCallResponse {
                success: false,
                result: None,
                error: Some(e.to_string()),
                duration_ms,
            })
        }
    }
}

/// Get tool call history
#[tauri::command]
pub async fn get_tool_history(
    state: State<'_, AppState>,
) -> Result<Vec<ToolCallHistoryRecord>, String> {
    // Tool history is tracked in Computer struct, but we're using MCPServerManager directly.
    // For now, return empty - will be populated when we track calls locally.
    // TODO: Integrate with Computer's tool history tracking
    let _ = state;
    Ok(vec![])
}
