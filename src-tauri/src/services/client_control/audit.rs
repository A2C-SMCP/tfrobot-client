use super::ToolId;
use crate::services::observability::{
    redact_json, redact_text, ActivityEventDraft, ActivityLevel, ActivityManagedBy,
    ActivityOutcome, ActivityProvider, ActivityTrigger, ComputerActivityCategory,
    ObservabilityService, ToolCallHistoryDraft,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditOutcome {
    Succeeded,
    Denied,
    Failed,
}

#[derive(Debug, Clone)]
pub struct ControlAuditRecord {
    pub request_id: String,
    pub source_computer_id: String,
    pub target_computer_id: Option<String>,
    pub tool: ToolId,
    pub parameters: serde_json::Value,
    pub outcome: AuditOutcome,
    pub error: Option<String>,
    pub duration_ms: u128,
}

pub trait ControlAuditSink: Send + Sync {
    fn record(&self, record: ControlAuditRecord) -> Result<(), String>;
}

#[derive(Debug, Default)]
pub struct NoopControlAuditSink;

impl ControlAuditSink for NoopControlAuditSink {
    fn record(&self, _record: ControlAuditRecord) -> Result<(), String> {
        Ok(())
    }
}

pub struct ObservabilityControlAuditSink {
    observability: Arc<ObservabilityService>,
}

impl ObservabilityControlAuditSink {
    pub fn new(observability: Arc<ObservabilityService>) -> Self {
        Self { observability }
    }
}

impl ControlAuditSink for ObservabilityControlAuditSink {
    fn record(&self, record: ControlAuditRecord) -> Result<(), String> {
        let success = record.outcome == AuditOutcome::Succeeded;
        let outcome = match record.outcome {
            AuditOutcome::Succeeded => ActivityOutcome::Succeeded,
            AuditOutcome::Denied | AuditOutcome::Failed => ActivityOutcome::Failed,
        };
        let target = record.target_computer_id.as_deref().unwrap_or("none");
        let error = record.error.as_deref().map(redact_text);
        let mut activity = ActivityEventDraft::computer(
            &record.source_computer_id,
            if success {
                ActivityLevel::Info
            } else {
                ActivityLevel::Warn
            },
            ComputerActivityCategory::Tool,
            "tool_call",
            record.tool.as_str(),
            outcome,
            format!("Client Control {} target={target}", record.tool),
        )
        .with_standard_fields(
            ActivityTrigger::ClientControl,
            Some(ActivityManagedBy::BuiltIn),
            Some(ActivityProvider::BuiltInMcp),
        );
        activity.correlation_id = Some(record.request_id.clone());
        activity.merge_fields(serde_json::json!({
            "bundle_id": super::CLIENT_CONTROL_BUNDLE_ID,
            "source_computer_id": record.source_computer_id,
            "target_computer_id": record.target_computer_id,
            "error": error,
            "duration_ms": record.duration_ms,
        }));
        self.observability.record_tool_call(
            &activity,
            &ToolCallHistoryDraft {
                req_id: record.request_id,
                computer_instance_id: record.source_computer_id,
                server: super::CLIENT_CONTROL_BUNDLE_ID.to_string(),
                tool: record.tool.to_string(),
                parameters: redact_json(record.parameters),
                timeout: None,
                success,
                error: activity
                    .fields
                    .as_ref()
                    .and_then(|fields| fields.get("error"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::observability::{ActivityQuery, ActivityScopeFilter};
    use tempfile::tempdir;

    #[test]
    fn audit_input_is_redacted_before_persistence() {
        let redacted = redact_json(serde_json::json!({
            "password": "secret",
            "nested": {"accessToken": "token", "safe": "visible"}
        }));
        assert_eq!(redacted["password"], "[REDACTED]");
        assert_eq!(redacted["nested"]["accessToken"], "[REDACTED]");
        assert_eq!(redacted["nested"]["safe"], "visible");
    }

    #[test]
    fn client_control_is_a_builtin_mcp_tool_activity_not_a_separate_category() {
        let dir = tempdir().unwrap();
        let observability = Arc::new(ObservabilityService::new(dir.path()).unwrap());
        let sink = ObservabilityControlAuditSink::new(observability.clone());
        sink.record(ControlAuditRecord {
            request_id: "request-1".to_string(),
            source_computer_id: "source".to_string(),
            target_computer_id: Some("target".to_string()),
            tool: ToolId::ComputerGetStatus,
            parameters: serde_json::json!({"computer_id": "target"}),
            outcome: AuditOutcome::Succeeded,
            error: None,
            duration_ms: 12,
        })
        .unwrap();

        let page = observability
            .query_activity(&ActivityQuery {
                scope: ActivityScopeFilter::Computer {
                    computer_id: "source".to_string(),
                },
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page.items.len(), 1);
        let event = &page.items[0];
        assert_eq!(event.category, ComputerActivityCategory::Tool.as_str());
        assert_eq!(event.correlation_id.as_deref(), Some("request-1"));
        let fields = event.fields.as_ref().unwrap();
        assert_eq!(fields["trigger"], ActivityTrigger::ClientControl.as_str());
        assert_eq!(fields["provider"], ActivityProvider::BuiltInMcp.as_str());
        assert_eq!(fields["managed_by"], ActivityManagedBy::BuiltIn.as_str());
        assert_eq!(fields["source_computer_id"], "source");
        assert_eq!(fields["target_computer_id"], "target");
        assert_eq!(fields["duration_ms"], 12);
    }
}
