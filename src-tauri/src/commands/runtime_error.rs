use crate::services::computer::{ComputerRuntimeActionUnavailable, ComputerRuntimeStartError};
use a2c_smcp::smcp_computer::errors::ComputerError;
use a2c_smcp::smcp_computer::inputs::{InputKind, InputResolutionError};
use serde::Serialize;

/// Stable Tauri error contract for runtime actions that may resolve client-owned inputs.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, thiserror::Error)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum RuntimeActionError {
    #[error("{message}")]
    MissingInput {
        input_id: String,
        env_hint: String,
        message: String,
    },
    #[error("{message}")]
    MissingSecret {
        input_id: String,
        env_hint: String,
        message: String,
    },
    #[error("{message}")]
    ResolverFailed { input_id: String, message: String },
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
            Self::MissingInput { message, .. }
            | Self::MissingSecret { message, .. }
            | Self::ResolverFailed { message, .. }
            | Self::RuntimeError { message } => {
                *message = format!("{message}; {suffix}");
            }
            Self::ActionUnavailable { .. } => {
                return Self::runtime(format!("{self}; {suffix}"));
            }
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
                },
                InputKind::Secret => Self::MissingSecret {
                    message: format!("Required secret input '{id}' is unresolved"),
                    input_id: id,
                    env_hint,
                },
            },
            ComputerError::InputResolution(InputResolutionError::ResolverFailed { id, reason }) => {
                Self::ResolverFailed {
                    input_id: id,
                    message: reason,
                }
            }
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
