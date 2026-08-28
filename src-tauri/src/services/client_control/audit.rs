use super::ToolId;
use crate::services::observability::{
    redact_json, redact_text, ActivityEventDraft, ActivityLevel, ActivityOutcome,
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
        let activity = ActivityEventDraft::computer(
            &record.source_computer_id,
            if success {
                ActivityLevel::Info
            } else {
                ActivityLevel::Warn
            },
            "client_control",
            "tool_call",
            record.tool.as_str(),
            outcome,
            format!("Client Control {} target={target}", record.tool),
        );
        self.observability.record_tool_call(
            &activity,
            &ToolCallHistoryDraft {
                req_id: record.request_id,
                computer_instance_id: record.source_computer_id,
                server: "client_control".to_string(),
                tool: record.tool.to_string(),
                parameters: redact_json(record.parameters),
                timeout: None,
                success,
                error,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
