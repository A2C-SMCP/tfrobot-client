use crate::AppState;
use serde::{Deserialize, Serialize};
use smcp_computer::computer::ToolCallRecord;
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_apply: Option<bool>,
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
pub async fn get_available_tools(state: State<'_, AppState>) -> Result<Vec<ToolInfo>, String> {
    let tools: Vec<Tool> = state
        .runtime
        .computer()
        .get_available_tools()
        .await
        .map_err(|e| e.to_string())?;

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

            let a2c_meta = t.meta.as_ref().and_then(|m| m.get("a2c_tool_meta"));

            let tags = a2c_meta
                .and_then(|m| m.get("tags"))
                .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok());

            let auto_apply = a2c_meta
                .and_then(|m| m.get("auto_apply"))
                .and_then(|v| v.as_bool());

            ToolInfo {
                name: t.name.to_string(),
                description: t.description.map(|d| d.to_string()).unwrap_or_default(),
                input_schema: serde_json::Value::Object((*t.input_schema).clone()),
                server,
                tags,
                auto_apply,
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
    log::info!("Executing tool: {}", tool_name);

    let start = std::time::Instant::now();

    let result = state
        .runtime
        .computer()
        .execute_tool_cancellable("debug-tool-call", &tool_name, params, timeout)
        .await;
    let duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(call_result) => {
            let _ = state.log_service.write(
                if call_result.is_error.unwrap_or(false) {
                    "error"
                } else {
                    "info"
                },
                "tool",
                &format!("Tool {} executed ({}ms)", tool_name, duration_ms),
                None,
            );
            Ok(ToolCallResponse {
                success: !call_result.is_error.unwrap_or(false),
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
    let history = state
        .runtime
        .computer()
        .get_tool_history()
        .await
        .map_err(|e| e.to_string())?;
    Ok(history.into_iter().map(tool_call_history_record).collect())
}

fn tool_call_history_record(record: ToolCallRecord) -> ToolCallHistoryRecord {
    ToolCallHistoryRecord {
        timestamp: record.timestamp.to_rfc3339(),
        req_id: record.req_id,
        server: record.server,
        tool: record.tool,
        parameters: record.parameters,
        timeout: record.timeout,
        success: record.success,
        error: record.error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn tool_history_record_maps_computer_history_contract() {
        let record = ToolCallRecord {
            timestamp: chrono::Utc.with_ymd_and_hms(2026, 6, 16, 12, 0, 0).unwrap(),
            req_id: "req-1".to_string(),
            server: "server-a".to_string(),
            tool: "echo".to_string(),
            parameters: serde_json::json!({"message": "hello"}),
            timeout: Some(1.5),
            success: true,
            error: None,
        };

        let mapped = tool_call_history_record(record);

        assert_eq!(mapped.timestamp, "2026-06-16T12:00:00+00:00");
        assert_eq!(mapped.req_id, "req-1");
        assert_eq!(mapped.server, "server-a");
        assert_eq!(mapped.tool, "echo");
        assert_eq!(mapped.parameters, serde_json::json!({"message": "hello"}));
        assert_eq!(mapped.timeout, Some(1.5));
        assert!(mapped.success);
        assert!(mapped.error.is_none());
    }
}
