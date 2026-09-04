use crate::commands::activity_support::{
    mcp_activity_ownership, record_computer_activity, ComputerActivitySpec,
};
pub use crate::services::observability::ToolCallHistoryRecord;
use crate::services::observability::{
    redact_json as redact_sensitive_parameters, redact_text as redact_sensitive_text,
    ActivityEventDraft, ActivityLevel, ActivityManagedBy, ActivityOutcome, ActivityProvider,
    ActivityTrigger, ComputerActivityCategory, ToolCallHistoryDraft,
};
use crate::AppState;
use a2c_smcp::smcp_computer::mcp_clients::model::{
    BundleId, CallToolResult, Content, MCPServerRuntimeStatus, Resource, ServerName, Tool,
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
    #[serde(rename = "bundleId", skip_serializing_if = "Option::is_none")]
    pub bundle_id: Option<BundleId>,
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
    let running_servers = connected_mcp_servers(runtime.mcp_server_runtime_statuses().await);

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
            let bundle_id = tool_bundle_id(&t, &running_servers);

            ToolInfo {
                display_name: display_tool_name(t.name.as_ref()),
                name: t.name.to_string(),
                description: t.description.map(|d| d.to_string()).unwrap_or_default(),
                input_schema: serde_json::Value::Object((*t.input_schema).clone()),
                server,
                bundle_id,
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
    bundle_id: BundleId,
    cursor: Option<String>,
) -> Result<DebugResourcesResponse, String> {
    get_debug_resources_core(&state, &instance_id, &bundle_id, cursor).await
}

pub async fn get_debug_resources_core(
    state: &AppState,
    instance_id: &str,
    bundle_id: &BundleId,
    cursor: Option<String>,
) -> Result<DebugResourcesResponse, String> {
    let started = std::time::Instant::now();
    let result = async {
        let instance_id = require_instance_id(instance_id)?.to_string();

        let runtime = state
            .computer_registry
            .runtime(&instance_id)
            .await
            .ok_or_else(|| format!("Computer instance not found: {instance_id}"))?;
        let server_name = runtime
            .mcp_server_display_name(bundle_id)
            .await
            .ok_or_else(|| format!("MCP server not found: {bundle_id}"))?;

        let (resources, next_cursor) = runtime.resources(bundle_id, cursor).await?;

        Ok(DebugResourcesResponse {
            resources: resources
                .into_iter()
                .map(|resource| resource_to_debug_info(&server_name, resource))
                .collect(),
            next_cursor,
        })
    }
    .await;
    let (managed_by, provider) = mcp_activity_ownership(state, instance_id, bundle_id.as_str());
    let resource_count = result.as_ref().ok().map(|value| value.resources.len());
    let has_more = result
        .as_ref()
        .ok()
        .map(|value| value.next_cursor.is_some());
    record_computer_activity(
        state,
        ComputerActivitySpec {
            computer_id: instance_id,
            category: ComputerActivityCategory::Resource,
            event_type: "mcp_resource",
            operation: "list",
            trigger: ActivityTrigger::User,
            managed_by: Some(managed_by),
            provider: Some(provider),
            message_subject: "MCP resource list",
            fields: serde_json::json!({
                "bundle_id": bundle_id,
                "resource_count": resource_count,
                "has_more": has_more,
                "content_recorded": false,
            }),
        },
        started,
        &result,
    )
    .await;
    result
}

fn resource_to_debug_info(server: &str, resource: Resource) -> DebugResourceInfo {
    DebugResourceInfo {
        server: server.to_string(),
        uri: resource.uri,
        name: resource.name,
        description: resource.description,
        mime_type: resource.mime_type,
    }
}

/// Default deadline (seconds) applied when a tool caller omits `timeout`.
///
/// Safety net against MCP transports that strand a response when their
/// server→client channel dies mid-stream — notably Streamable-HTTP/SSE servers
/// (e.g. Atlassian) whose edge resets the long-lived GET connection. The
/// pending tool response is lost on the flap, so without a deadline the call
/// hangs forever. See `experiments/codex-sse-flap-hang-repro`.
const DEFAULT_TOOL_TIMEOUT_SECS: f64 = 120.0;

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

    // An explicit caller timeout always wins; only a missing timeout falls back
    // to the default deadline so a dead/flapping transport fails fast instead of
    // hanging the invoke forever.
    let timeout = timeout.or(Some(DEFAULT_TOOL_TIMEOUT_SECS));

    let runtime = state
        .computer_registry
        .runtime(&instance_id)
        .await
        .ok_or_else(|| format!("Computer instance not found: {instance_id}"))?;

    let start = std::time::Instant::now();
    let req_id = uuid::Uuid::new_v4().to_string();
    let tools = runtime.available_tools().await.unwrap_or_default();
    let tool_bundle_id = tools
        .iter()
        .find(|tool| tool.name == tool_name)
        .and_then(|tool| tool.name.split_once("__"))
        .map(|(bundle_id, _)| bundle_id.to_string());
    let (managed_by, provider) = tool_bundle_id
        .as_deref()
        .map(|bundle_id| mcp_activity_ownership(state, &instance_id, bundle_id))
        .unwrap_or((ActivityManagedBy::User, ActivityProvider::UserMcp));
    let running_servers = connected_mcp_servers(runtime.mcp_server_runtime_statuses().await);
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
            let history_error = error.clone();
            let mut activity = ActivityEventDraft::computer(
                &instance_id,
                if success {
                    ActivityLevel::Info
                } else {
                    ActivityLevel::Error
                },
                ComputerActivityCategory::Tool,
                "tool_call",
                history_tool.clone(),
                if success {
                    ActivityOutcome::Succeeded
                } else {
                    ActivityOutcome::Failed
                },
                format!("Tool {tool_name} executed ({duration_ms}ms)"),
            )
            .with_standard_fields(
                ActivityTrigger::User,
                Some(managed_by),
                Some(provider),
            );
            activity.correlation_id = Some(req_id.clone());
            activity.merge_fields(serde_json::json!({
                "bundle_id": tool_bundle_id,
                "server_name": server,
                "duration_ms": duration_ms,
                "parameters_recorded": true,
                "result_recorded": false,
                "error": error,
            }));
            if let Err(persist_error) = state
                .observability
                .record_tool_call_async(
                    activity,
                    ToolCallHistoryDraft {
                        req_id,
                        computer_instance_id: instance_id.clone(),
                        server,
                        tool: history_tool,
                        parameters: history_parameters,
                        timeout,
                        success,
                        error: history_error,
                    },
                )
                .await
            {
                log::error!("failed to persist tool call history: {persist_error}");
            }
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
            let mut activity = ActivityEventDraft::computer(
                &instance_id,
                ActivityLevel::Error,
                ComputerActivityCategory::Tool,
                "tool_call",
                history_tool.clone(),
                ActivityOutcome::Failed,
                format!("Tool {tool_name} failed: {redacted_error}"),
            )
            .with_standard_fields(
                ActivityTrigger::User,
                Some(managed_by),
                Some(provider),
            );
            activity.correlation_id = Some(req_id.clone());
            activity.merge_fields(serde_json::json!({
                "bundle_id": tool_bundle_id,
                "server_name": server,
                "duration_ms": duration_ms,
                "parameters_recorded": true,
                "result_recorded": false,
                "error": redacted_error,
            }));
            if let Err(persist_error) = state
                .observability
                .record_tool_call_async(
                    activity,
                    ToolCallHistoryDraft {
                        req_id,
                        computer_instance_id: instance_id.clone(),
                        server,
                        tool: history_tool,
                        parameters: history_parameters,
                        timeout,
                        success: false,
                        error: Some(redacted_error.clone()),
                    },
                )
                .await
            {
                log::error!("failed to persist failed tool call history: {persist_error}");
            }
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
    if let Content::Text(text) = &mut content {
        text.text = redact_sensitive_text(&text.text);
    }
    content
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

fn connected_mcp_servers(statuses: Vec<MCPServerRuntimeStatus>) -> Vec<(BundleId, ServerName)> {
    statuses
        .into_iter()
        .filter_map(|status| {
            status
                .is_connected()
                .then_some((status.bundle_id, status.name))
        })
        .collect()
}

fn tool_server(tool: &Tool, running_servers: &[(BundleId, ServerName)]) -> String {
    tool.meta
        .as_ref()
        .and_then(|m| m.get("server_name"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .or_else(|| (running_servers.len() == 1).then(|| running_servers[0].1.clone()))
        .unwrap_or_else(|| "unknown".to_string())
}

fn tool_bundle_id(tool: &Tool, running_servers: &[(BundleId, ServerName)]) -> Option<BundleId> {
    tool.name
        .split_once("__")
        .and_then(|(raw, _)| BundleId::try_from(raw).ok())
        .or_else(|| (running_servers.len() == 1).then(|| running_servers[0].0.clone()))
}

fn resolve_tool_server(
    tools: &[Tool],
    running_servers: &[(BundleId, ServerName)],
    tool_name: &str,
) -> String {
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

/// Get tool call history
#[tauri::command]
pub async fn get_tool_history(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Vec<ToolCallHistoryRecord>, String> {
    let instance_id = require_instance_id(&instance_id)?.to_string();
    let service = state.observability.as_ref().clone();
    tauri::async_runtime::spawn_blocking(move || service.tool_history(&instance_id, 100))
        .await
        .map_err(|error| error.to_string())?
}

pub fn get_tool_history_core(
    state: &AppState,
    instance_id: &str,
) -> Result<Vec<ToolCallHistoryRecord>, String> {
    let instance_id = require_instance_id(instance_id)?;
    state.observability.tool_history(instance_id, 100)
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
}
