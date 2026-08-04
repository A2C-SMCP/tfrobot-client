use super::ToolId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientControlErrorCode {
    SourceNotFound,
    TargetNotFound,
    RemoteControlDisabled,
    ToolNotAllowed,
    TargetNotAllowed,
    SourceSelfProtection,
    InvalidArguments,
    Conflict,
    SkillAlreadyExists,
    SkillNotFound,
    RevisionConflict,
    RestartRequired,
    InvalidSkillPackage,
    OperationFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{code:?}: {message}")]
pub struct ClientControlError {
    pub code: ClientControlErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<ToolId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_computer_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_computer_id: Option<String>,
}

impl ClientControlError {
    pub fn new(code: ClientControlErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            tool: None,
            source_computer_id: None,
            target_computer_id: None,
        }
    }

    pub fn invocation(
        code: ClientControlErrorCode,
        message: impl Into<String>,
        source: &str,
        tool: ToolId,
        target: Option<&str>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            tool: Some(tool),
            source_computer_id: Some(source.to_string()),
            target_computer_id: target.map(str::to_string),
        }
    }
}
