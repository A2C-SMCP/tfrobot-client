use crate::services::computer::{ComputerRuntimeActionUnavailable, ComputerRuntimeStartError};
use a2c_smcp::smcp_computer::errors::ComputerError;
use a2c_smcp::smcp_computer::inputs::{InputKind, InputResolutionError};
use serde::Serialize;

use crate::services::input_resolver::{REDACTED_SECRET_SELECTION, RUNTIME_INPUT_CANCELLED};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RequestingMcp {
    pub bundle_id: String,
    pub name: String,
}

/// Stable Tauri error contract for runtime actions that may resolve client-owned inputs.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, thiserror::Error)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum RuntimeActionError {
    #[error("{message}")]
    MissingInputDefinition {
        input_id: String,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        requesting_mcp: Option<RequestingMcp>,
    },
    #[error("{message}")]
    MissingInput {
        input_id: String,
        env_hint: String,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        requesting_mcp: Option<RequestingMcp>,
    },
    #[error("{message}")]
    MissingSecret {
        input_id: String,
        env_hint: String,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        requesting_mcp: Option<RequestingMcp>,
    },
    #[error("{message}")]
    ResolverFailed { input_id: String, message: String },
    #[error("{message}")]
    RuntimeInputCancelled { input_id: String, message: String },
    #[error("{message}")]
    InvalidSelection {
        input_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<String>,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        requesting_mcp: Option<RequestingMcp>,
    },
    #[error("{message}")]
    ActionUnavailable {
        action: String,
        lifecycle: String,
        disabled_reason: String,
        message: String,
    },
    #[error("{message}")]
    RuntimeError { message: String },
}

impl From<ComputerRuntimeActionUnavailable> for RuntimeActionError {
    fn from(error: ComputerRuntimeActionUnavailable) -> Self {
        let message = error.to_string();
        Self::ActionUnavailable {
            action: error.action.to_string(),
            lifecycle: error.lifecycle.to_string(),
            disabled_reason: error.disabled_reason.to_string(),
            message,
        }
    }
}

impl RuntimeActionError {
    pub fn runtime(message: impl Into<String>) -> Self {
        Self::RuntimeError {
            message: message.into(),
        }
    }

    pub fn append_context(mut self, context: impl std::fmt::Display) -> Self {
        let suffix = context.to_string();
        match &mut self {
            Self::MissingInputDefinition { message, .. }
            | Self::MissingInput { message, .. }
            | Self::MissingSecret { message, .. }
            | Self::ResolverFailed { message, .. }
            | Self::RuntimeInputCancelled { message, .. }
            | Self::InvalidSelection { message, .. }
            | Self::RuntimeError { message } => {
                *message = format!("{message}; {suffix}");
            }
            Self::ActionUnavailable { .. } => {
                return Self::runtime(format!("{self}; {suffix}"));
            }
        }
        self
    }

    pub fn with_requesting_mcp(
        mut self,
        bundle_id: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        let requesting_mcp = Some(RequestingMcp {
            bundle_id: bundle_id.into(),
            name: name.into(),
        });
        match &mut self {
            Self::MissingInputDefinition {
                requesting_mcp: target,
                ..
            }
            | Self::MissingInput {
                requesting_mcp: target,
                ..
            }
            | Self::MissingSecret {
                requesting_mcp: target,
                ..
            }
            | Self::InvalidSelection {
                requesting_mcp: target,
                ..
            } => *target = requesting_mcp,
            _ => {}
        }
        self
    }
}

impl From<ComputerError> for RuntimeActionError {
    fn from(error: ComputerError) -> Self {
        match error {
            ComputerError::InputResolution(InputResolutionError::Missing {
                id,
                kind,
                env_hint,
            }) => match kind {
                InputKind::Value => Self::MissingInput {
                    message: format!("Required value input '{id}' is unresolved"),
                    input_id: id,
                    env_hint,
                    requesting_mcp: None,
                },
                InputKind::Secret => Self::MissingSecret {
                    message: format!("Required secret input '{id}' is unresolved"),
                    input_id: id,
                    env_hint,
                    requesting_mcp: None,
                },
            },
            ComputerError::InputResolution(InputResolutionError::ResolverFailed { id, reason }) => {
                if reason == RUNTIME_INPUT_CANCELLED {
                    return Self::RuntimeInputCancelled {
                        input_id: id,
                        message: reason,
                    };
                }
                Self::ResolverFailed {
                    input_id: id,
                    message: reason,
                }
            }
            ComputerError::InputResolution(InputResolutionError::InvalidSelection {
                id,
                value,
            }) => Self::InvalidSelection {
                message: if value == REDACTED_SECRET_SELECTION {
                    format!("Stored secret for PickString input '{id}' is not one of its current options")
                } else {
                    format!("Stored value for PickString input '{id}' is not one of its current options")
                },
                input_id: id,
                value: (value != REDACTED_SECRET_SELECTION).then_some(value),
                requesting_mcp: None,
            },
            other => Self::runtime(other.to_string()),
        }
    }
}

impl From<ComputerRuntimeStartError> for RuntimeActionError {
    fn from(error: ComputerRuntimeStartError) -> Self {
        match error {
            ComputerRuntimeStartError::Sdk(error) => error.into(),
            ComputerRuntimeStartError::SdkWithContext { source, context } => {
                Self::from(source).append_context(context)
            }
            ComputerRuntimeStartError::Client(message) => Self::runtime(message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_missing_secret_fields_without_secret_value() {
        let error = RuntimeActionError::from(ComputerError::InputResolution(
            InputResolutionError::Missing {
                id: "api-key".to_string(),
                kind: InputKind::Secret,
                env_hint: "A2C_SMCP_api_key".to_string(),
            },
        ));

        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({
                "code": "missing_secret",
                "input_id": "api-key",
                "env_hint": "A2C_SMCP_api_key",
                "message": "Required secret input 'api-key' is unresolved"
            })
        );
    }

    #[test]
    fn preserves_resolver_failure_as_structured_error() {
        let error = RuntimeActionError::from(ComputerError::InputResolution(
            InputResolutionError::ResolverFailed {
                id: "region".to_string(),
                reason: "secret store unavailable".to_string(),
            },
        ));

        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({
                "code": "resolver_failed",
                "input_id": "region",
                "message": "secret store unavailable"
            })
        );
    }

    #[test]
    fn preserves_runtime_input_cancellation_as_a_distinct_error() {
        let error = RuntimeActionError::from(ComputerError::InputResolution(
            InputResolutionError::ResolverFailed {
                id: "region".to_string(),
                reason: RUNTIME_INPUT_CANCELLED.to_string(),
            },
        ));

        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({
                "code": "runtime_input_cancelled",
                "input_id": "region",
                "message": RUNTIME_INPUT_CANCELLED
            })
        );
    }

    #[test]
    fn preserves_invalid_pick_selection_for_reselection_ui() {
        let error = RuntimeActionError::from(ComputerError::InputResolution(
            InputResolutionError::InvalidSelection {
                id: "region".to_string(),
                value: "retired".to_string(),
            },
        ));

        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({
                "code": "invalid_selection",
                "input_id": "region",
                "value": "retired",
                "message": "Stored value for PickString input 'region' is not one of its current options"
            })
        );
    }

    #[test]
    fn redacts_secret_pick_selection_from_the_tauri_error_contract() {
        let error = RuntimeActionError::from(ComputerError::InputResolution(
            InputResolutionError::InvalidSelection {
                id: "region".to_string(),
                value: REDACTED_SECRET_SELECTION.to_string(),
            },
        ));
        let serialized = serde_json::to_value(error).unwrap();

        assert_eq!(
            serialized,
            serde_json::json!({
                "code": "invalid_selection",
                "input_id": "region",
                "message": "Stored secret for PickString input 'region' is not one of its current options"
            })
        );
        assert!(!serialized.to_string().contains(REDACTED_SECRET_SELECTION));
    }

    #[test]
    fn preserves_missing_input_fields_when_runtime_restore_adds_context() {
        let error = RuntimeActionError::from(ComputerRuntimeStartError::SdkWithContext {
            source: ComputerError::InputResolution(InputResolutionError::Missing {
                id: "audit@acme/api-key".to_string(),
                kind: InputKind::Secret,
                env_hint: "A2C_SMCP_audit_acme_api_key".to_string(),
            }),
            context: "previous runtime restore also failed".to_string(),
        });

        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({
                "code": "missing_secret",
                "input_id": "audit@acme/api-key",
                "env_hint": "A2C_SMCP_audit_acme_api_key",
                "message": "Required secret input 'audit@acme/api-key' is unresolved; previous runtime restore also failed"
            })
        );
    }

    #[test]
    fn serializes_unavailable_actions_with_the_current_lifecycle() {
        let error = RuntimeActionError::from(ComputerRuntimeActionUnavailable {
            action: "connect",
            lifecycle: a2c_smcp::smcp_computer::LifecycleState::Connecting,
            disabled_reason: "transition_in_progress",
        });

        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({
                "code": "action_unavailable",
                "action": "connect",
                "lifecycle": "connecting",
                "disabled_reason": "transition_in_progress",
                "message": "runtime action 'connect' is unavailable while lifecycle is 'connecting' (transition_in_progress)"
            })
        );
    }
}
