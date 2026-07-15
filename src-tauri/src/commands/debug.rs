use crate::services::logger::{LogEntry, LogFilter};
use crate::AppState;
use a2c_smcp::smcp_computer::mcp_clients::model::{
    CallToolResult, Content, RawContent, Resource, Tool,
};
use serde::{Deserialize, Serialize};
use tauri::State;

const MAX_ERROR_SUMMARY_CHARS: usize = 500;

/// Tool with server attribution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    /// Stable SDK executable identity (`{bundle_id}__{tool}`).
    pub name: String,
    /// Human-friendly raw tool name for presentation only.
    #[serde(rename = "displayName")]
    pub display_name: String,
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
    pub computer_instance_id: String,
    pub server: String,
    pub tool: String,
    pub parameters: serde_json::Value,
    pub timeout: Option<f64>,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugResourceInfo {
    pub server: String,
    pub uri: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugResourcesResponse {
    pub resources: Vec<DebugResourceInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ToolCallHistoryDetails {
    pub req_id: String,
    pub computer_instance_id: String,
    pub server: String,
    pub tool: String,
    pub parameters: serde_json::Value,
    pub timeout: Option<f64>,
    pub success: bool,
    pub error: Option<String>,
}

/// Get all available tools from running MCP servers
#[tauri::command]
pub async fn get_available_tools(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Vec<ToolInfo>, String> {
    get_available_tools_core(&state, &instance_id).await
}

pub async fn get_available_tools_core(
    state: &AppState,
    instance_id: &str,
) -> Result<Vec<ToolInfo>, String> {
    let runtime = state
        .computer_registry
        .runtime(require_instance_id(instance_id)?)
        .await
        .ok_or_else(|| format!("Computer instance not found: {instance_id}"))?;
    let tools: Vec<Tool> = runtime.available_tools().await?;
    let running_servers = running_mcp_servers(runtime.mcp_server_statuses().await);

    let result = tools
        .into_iter()
        .map(|t| {
            // Extract server name from tool meta if available
            let server = tool_server(&t, &running_servers);

            let a2c_meta = t.meta.as_ref().and_then(|m| m.get("a2c_tool_meta"));

            let tags = a2c_meta
                .and_then(|m| m.get("tags"))
                .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok());

            let auto_apply = a2c_meta
                .and_then(|m| m.get("auto_apply"))
                .and_then(|v| v.as_bool());

            ToolInfo {
                display_name: display_tool_name(t.name.as_ref()),
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

fn display_tool_name(exposed_name: &str) -> String {
    exposed_name
        .split_once("__")
        .map(|(_, raw_name)| raw_name)
        .unwrap_or(exposed_name)
        .to_string()
}

#[tauri::command]
pub async fn get_debug_resources(
    state: State<'_, AppState>,
    instance_id: String,
    server_name: String,
    cursor: Option<String>,
) -> Result<DebugResourcesResponse, String> {
    let instance_id = require_instance_id(&instance_id)?.to_string();
    let server_name = require_server_name(&server_name)?.to_string();

    let runtime = state
        .computer_registry
        .runtime(&instance_id)
        .await
        .ok_or_else(|| format!("Computer instance not found: {instance_id}"))?;

    let (resources, next_cursor) = runtime.resources(&server_name, cursor).await?;

    Ok(DebugResourcesResponse {
        resources: resources
            .into_iter()
            .map(|resource| resource_to_debug_info(&server_name, resource))
            .collect(),
        next_cursor,
    })
}

fn resource_to_debug_info(server: &str, resource: Resource) -> DebugResourceInfo {
    DebugResourceInfo {
        server: server.to_string(),
        uri: resource.raw.uri,
        name: resource.raw.name,
        description: resource.raw.description,
        mime_type: resource.raw.mime_type,
    }
}

/// Execute a tool call for testing
#[tauri::command]
pub async fn execute_tool(
    state: State<'_, AppState>,
    instance_id: String,
    tool_name: String,
    params: serde_json::Value,
    timeout: Option<f64>,
) -> Result<ToolCallResponse, String> {
    execute_tool_core(&state, &instance_id, &tool_name, params, timeout).await
}

pub async fn execute_tool_core(
    state: &AppState,
    instance_id: &str,
    tool_name: &str,
    params: serde_json::Value,
    timeout: Option<f64>,
) -> Result<ToolCallResponse, String> {
    let instance_id = require_instance_id(instance_id)?.to_string();
    log::info!("Executing tool for instance {}: {}", instance_id, tool_name);

    let runtime = state
        .computer_registry
        .runtime(&instance_id)
        .await
        .ok_or_else(|| format!("Computer instance not found: {instance_id}"))?;

    let start = std::time::Instant::now();
    let req_id = uuid::Uuid::new_v4().to_string();
    let tools = runtime.available_tools().await.unwrap_or_default();
    let running_servers = running_mcp_servers(runtime.mcp_server_statuses().await);
    let fallback_server = resolve_tool_server(&tools, &running_servers, tool_name);
    let history_parameters = redact_sensitive_parameters(params.clone());

    let result = runtime
        .execute_tool_cancellable(&req_id, tool_name, params.clone(), timeout)
        .await;
    let duration_ms = start.elapsed().as_millis() as u64;
    let sdk_record = runtime
        .sdk_tool_history()
        .await
        .ok()
        .and_then(|history| history.into_iter().find(|record| record.req_id == req_id));
    let server = sdk_record
        .as_ref()
        .map(|record| record.server.clone())
        .unwrap_or(fallback_server);
    let history_tool = sdk_record
        .as_ref()
        .map(|record| record.tool.clone())
        .unwrap_or_else(|| tool_name.to_string());

    match result {
        Ok(call_result) => {
            let success = !call_result.is_error.unwrap_or(false);
            let error = if success {
                None
            } else {
                tool_result_error_summary(&call_result).map(|text| redact_sensitive_text(&text))
            };
            let details = ToolCallHistoryDetails {
                req_id,
                computer_instance_id: instance_id.clone(),
                server,
                tool: history_tool,
                parameters: history_parameters,
                timeout,
                success,
                error,
            };
            let _ = state.log_service.write_for_instance(
                if success { "info" } else { "error" },
                "tool",
                &format!("Tool {} executed ({}ms)", tool_name, duration_ms),
                serde_json::to_string(&details).ok().as_deref(),
                Some(&instance_id),
            );
            let response_result = redact_tool_call_result_for_display(&call_result);
            Ok(ToolCallResponse {
                success,
                result: Some(response_result),
                error: None,
                duration_ms,
            })
        }
        Err(e) => {
            let error = e.to_string();
            let redacted_error = redact_sensitive_text(&error);
            let details = ToolCallHistoryDetails {
                req_id,
                computer_instance_id: instance_id.clone(),
                server,
                tool: history_tool,
                parameters: history_parameters,
                timeout,
                success: false,
                error: Some(redacted_error.clone()),
            };
            let _ = state.log_service.write_for_instance(
                "error",
                "tool",
                &format!("Tool {} failed: {}", tool_name, redacted_error),
                serde_json::to_string(&details).ok().as_deref(),
                Some(&instance_id),
            );
            Ok(ToolCallResponse {
                success: false,
                result: None,
                error: Some(redacted_error),
                duration_ms,
            })
        }
    }
}

fn redact_tool_call_result_for_display(result: &CallToolResult) -> CallToolResult {
    if !result.is_error.unwrap_or(false) {
        return result.clone();
    }

    let mut redacted = result.clone();
    redacted.content = redacted
        .content
        .into_iter()
        .map(redact_tool_content_for_display)
        .collect();
    redacted.structured_content = redacted.structured_content.map(redact_sensitive_parameters);
    redacted
}

fn redact_tool_content_for_display(mut content: Content) -> Content {
    if let RawContent::Text(text) = &mut content.raw {
        text.text = redact_sensitive_text(&text.text);
    }
    content
}

fn redact_sensitive_parameters(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.into_iter()
                .map(|(key, value)| {
                    if is_sensitive_key(&key) {
                        (key, serde_json::Value::String("[REDACTED]".to_string()))
                    } else {
                        (key, redact_sensitive_parameters(value))
                    }
                })
                .collect(),
        ),
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(redact_sensitive_parameters).collect())
        }
        other => other,
    }
}

fn tool_result_error_summary(result: &CallToolResult) -> Option<String> {
    let text = result
        .content
        .iter()
        .filter_map(|item| item.as_text())
        .map(|text| text.text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    if !text.is_empty() {
        return Some(truncate_error_summary(&text));
    }

    result
        .structured_content
        .as_ref()
        .and_then(|value| serde_json::to_string(value).ok())
        .filter(|text| !text.is_empty())
        .map(|text| truncate_error_summary(&text))
}

fn truncate_error_summary(text: &str) -> String {
    let mut chars = text.chars();
    let summary = chars
        .by_ref()
        .take(MAX_ERROR_SUMMARY_CHARS)
        .collect::<String>();
    if chars.next().is_some() {
        format!("{summary}...")
    } else {
        summary
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|c| *c != '_' && *c != '-' && *c != ' ')
        .collect::<String>()
        .to_ascii_lowercase();
    [
        "token",
        "password",
        "passwd",
        "secret",
        "apikey",
        "authorization",
        "cookie",
        "credential",
    ]
    .iter()
    .any(|pattern| normalized.contains(pattern))
}

fn redact_sensitive_text(text: &str) -> String {
    text.lines()
        .map(redact_sensitive_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn redact_sensitive_line(line: &str) -> String {
    if !is_sensitive_key(line) {
        return line.to_string();
    }

    let lower = line.to_ascii_lowercase();
    let sensitive_pos = [
        "access_token",
        "access-token",
        "api_key",
        "api-key",
        "apikey",
        "authorization",
        "credential",
        "password",
        "passwd",
        "secret",
        "cookie",
        "token",
    ]
    .iter()
    .filter_map(|pattern| lower.find(pattern))
    .min();

    let Some(pos) = sensitive_pos else {
        return "[REDACTED]".to_string();
    };

    let delimiter_pos = line[pos..]
        .char_indices()
        .find_map(|(offset, ch)| (ch == ':' || ch == '=').then_some(pos + offset));

    match delimiter_pos {
        Some(idx) => format!("{} [REDACTED]", &line[..=idx]),
        None => "[REDACTED]".to_string(),
    }
}

fn running_mcp_servers(statuses: Vec<(String, bool, String)>) -> Vec<String> {
    statuses
        .into_iter()
        .filter_map(|(name, running, _)| running.then_some(name))
        .collect()
}

fn tool_server(tool: &Tool, running_servers: &[String]) -> String {
    tool.meta
        .as_ref()
        .and_then(|m| m.get("server_name"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .or_else(|| (running_servers.len() == 1).then(|| running_servers[0].clone()))
        .unwrap_or_else(|| "unknown".to_string())
}

fn resolve_tool_server(tools: &[Tool], running_servers: &[String], tool_name: &str) -> String {
    tools
        .iter()
        .find(|tool| tool.name.as_ref() == tool_name)
        .map(|tool| tool_server(tool, running_servers))
        .unwrap_or_else(|| "unknown".to_string())
}

fn require_instance_id(instance_id: &str) -> Result<&str, String> {
    let instance_id = instance_id.trim();
    if instance_id.is_empty() {
        return Err("instance_id is required".to_string());
    }
    Ok(instance_id)
}

fn require_server_name(server_name: &str) -> Result<&str, String> {
    let server_name = server_name.trim();
    if server_name.is_empty() {
        return Err("server_name is required".to_string());
    }
    Ok(server_name)
}

/// Get tool call history
#[tauri::command]
pub async fn get_tool_history(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Vec<ToolCallHistoryRecord>, String> {
    get_tool_history_core(&state, &instance_id)
}

pub fn get_tool_history_core(
    state: &AppState,
    instance_id: &str,
) -> Result<Vec<ToolCallHistoryRecord>, String> {
    let instance_id = require_instance_id(instance_id)?.to_string();
    let logs = state.log_service.query(&LogFilter {
        categories: Some(vec!["tool".to_string()]),
        computer_instance_id: Some(instance_id.clone()),
        limit: Some(100),
        ..Default::default()
    })?;

    Ok(tool_history_records_from_logs(logs, &instance_id))
}

fn tool_history_records_from_logs(
    logs: Vec<LogEntry>,
    instance_id: &str,
) -> Vec<ToolCallHistoryRecord> {
    logs.into_iter()
        .filter_map(|entry| {
            if entry.category != "tool" {
                return None;
            }
            let details = entry.details.as_deref()?;
            let details = serde_json::from_str::<ToolCallHistoryDetails>(details).ok()?;
            if details.computer_instance_id != instance_id {
                return None;
            }
            Some(ToolCallHistoryRecord {
                timestamp: entry.timestamp,
                req_id: details.req_id,
                computer_instance_id: details.computer_instance_id,
                server: details.server,
                tool: details.tool,
                parameters: details.parameters,
                timeout: details.timeout,
                success: details.success,
                error: details.error,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn display_tool_name_removes_only_the_bundle_prefix() {
        assert_eq!(display_tool_name("bundle__tool"), "tool");
        assert_eq!(display_tool_name("bundle__foo__bar"), "foo__bar");
        assert_eq!(display_tool_name("unprefixed"), "unprefixed");
    }

    #[test]
    fn redacts_sensitive_tool_parameters_recursively() {
        let redacted = redact_sensitive_parameters(json!({
            "url": "https://example.com",
            "api_key": "secret-key",
            "nested": {
                "accessToken": "secret-token",
                "safe": "value"
            },
            "items": [
                { "password": "secret-password" },
                { "name": "visible" }
            ]
        }));

        assert_eq!(redacted["url"], "https://example.com");
        assert_eq!(redacted["api_key"], "[REDACTED]");
        assert_eq!(redacted["nested"]["accessToken"], "[REDACTED]");
        assert_eq!(redacted["nested"]["safe"], "value");
        assert_eq!(redacted["items"][0]["password"], "[REDACTED]");
        assert_eq!(redacted["items"][1]["name"], "visible");
    }

    #[test]
    fn maps_mcp_resource_to_debug_resource_info() {
        let resource = a2c_smcp::smcp_computer::mcp_clients::model::make_resource(
            "file://readme",
            "README.md",
            Some("Project readme".to_string()),
            Some("text/markdown".to_string()),
        );

        let info = resource_to_debug_info("fs", resource);

        assert_eq!(info.server, "fs");
        assert_eq!(info.uri, "file://readme");
        assert_eq!(info.name, "README.md");
        assert_eq!(info.description.as_deref(), Some("Project readme"));
        assert_eq!(info.mime_type.as_deref(), Some("text/markdown"));
    }

    #[test]
    fn extracts_tool_error_summary_from_text_content() {
        let result = CallToolResult::error(vec![
            Content::text(" first failure "),
            Content::text("second failure"),
        ]);

        assert_eq!(
            tool_result_error_summary(&result).as_deref(),
            Some("first failure\nsecond failure")
        );
    }

    #[test]
    fn truncates_long_tool_error_summary_on_char_boundary() {
        let result = CallToolResult::error(vec![Content::text("错误".repeat(300))]);
        let summary = tool_result_error_summary(&result).unwrap();

        assert_eq!(summary.chars().count(), MAX_ERROR_SUMMARY_CHARS + 3);
        assert!(summary.ends_with("..."));
    }

    #[test]
    fn redacts_sensitive_values_from_error_text() {
        let redacted = redact_sensitive_text(
            "request failed: token=secret-token\nAuthorization: Bearer secret\nsafe line",
        );

        assert_eq!(
            redacted,
            "request failed: token= [REDACTED]\nAuthorization: [REDACTED]\nsafe line"
        );
        assert!(!redacted.contains("secret-token"));
        assert!(!redacted.contains("Bearer secret"));
    }

    #[test]
    fn redacts_sensitive_values_from_display_error_result() {
        let mut result = CallToolResult::error(vec![Content::text(
            "request failed: token=secret-token\nsafe line",
        )]);
        result.structured_content = Some(json!({
            "api_key": "secret-key",
            "nested": {
                "authorization": "Bearer secret",
                "safe": "value"
            }
        }));

        let redacted = redact_tool_call_result_for_display(&result);
        let text = redacted.content[0].as_text().unwrap().text.as_str();

        assert_eq!(text, "request failed: token= [REDACTED]\nsafe line");
        assert_eq!(
            redacted.structured_content.as_ref().unwrap()["api_key"],
            "[REDACTED]"
        );
        assert_eq!(
            redacted.structured_content.as_ref().unwrap()["nested"]["authorization"],
            "[REDACTED]"
        );
        assert_eq!(
            redacted.structured_content.as_ref().unwrap()["nested"]["safe"],
            "value"
        );
    }

    #[test]
    fn leaves_success_result_display_content_unchanged() {
        let result = CallToolResult::success(vec![Content::text("token=visible-for-debug")]);

        let display = redact_tool_call_result_for_display(&result);

        assert_eq!(display, result);
    }

    #[test]
    fn builds_tool_history_only_from_valid_matching_tool_logs() {
        let matching_details = serde_json::to_string(&ToolCallHistoryDetails {
            req_id: "req-a".to_string(),
            computer_instance_id: "computer-a".to_string(),
            server: "fs".to_string(),
            tool: "read_file".to_string(),
            parameters: json!({ "path": "/tmp/readme.md" }),
            timeout: Some(3.0),
            success: true,
            error: None,
        })
        .unwrap();
        let wrong_instance_details = serde_json::to_string(&ToolCallHistoryDetails {
            req_id: "req-b".to_string(),
            computer_instance_id: "computer-b".to_string(),
            server: "fs".to_string(),
            tool: "write_file".to_string(),
            parameters: json!({ "path": "/tmp/readme.md" }),
            timeout: None,
            success: false,
            error: Some("failed".to_string()),
        })
        .unwrap();

        let history = tool_history_records_from_logs(
            vec![
                test_log_entry(1, "tool", Some(&matching_details), Some("computer-a")),
                test_log_entry(2, "connection", Some(&matching_details), Some("computer-a")),
                test_log_entry(3, "tool", Some("not json"), Some("computer-a")),
                test_log_entry(4, "tool", Some(&wrong_instance_details), Some("computer-b")),
                test_log_entry(5, "tool", None, Some("computer-a")),
            ],
            "computer-a",
        );

        assert_eq!(history.len(), 1);
        assert_eq!(history[0].req_id, "req-a");
        assert_eq!(history[0].computer_instance_id, "computer-a");
        assert_eq!(history[0].server, "fs");
        assert_eq!(history[0].tool, "read_file");
        assert_eq!(history[0].parameters["path"], "/tmp/readme.md");
        assert!(history[0].success);
    }

    fn test_log_entry(
        id: i64,
        category: &str,
        details: Option<&str>,
        computer_instance_id: Option<&str>,
    ) -> LogEntry {
        LogEntry {
            id,
            timestamp: format!("2026-01-01T00:00:0{id}Z"),
            level: "info".to_string(),
            category: category.to_string(),
            message: "log message".to_string(),
            details: details.map(ToString::to_string),
            computer_instance_id: computer_instance_id.map(ToString::to_string),
        }
    }
}
