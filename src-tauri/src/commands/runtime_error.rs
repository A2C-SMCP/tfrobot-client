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
    #[error("runtime action '{action}' is unavailable while lifecycle is '{lifecycle}'")]
    ActionUnavailable { action: String, lifecycle: String },
    #[error("{message}")]
    RuntimeError { message: String },
}

impl From<ComputerRuntimeActionUnavailable> for RuntimeActionError {
    fn from(error: ComputerRuntimeActionUnavailable) -> Self {
        Self::ActionUnavailable {
            action: error.action.to_string(),
            lifecycle: error.lifecycle.to_string(),
        }
    }
}

impl RuntimeActionError {
    pub fn runtime(message: impl Into<String>) -> Self {
        Self::RuntimeError {
            message: message.into(),
        }
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
                env_hint: "A2C_INPUT_API_KEY".to_string(),
            },
        ));

        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({
                "code": "missing_secret",
                "input_id": "api-key",
                "env_hint": "A2C_INPUT_API_KEY",
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
    fn serializes_unavailable_actions_with_the_current_lifecycle() {
        let error = RuntimeActionError::from(ComputerRuntimeActionUnavailable {
            action: "connect",
            lifecycle: a2c_smcp::smcp_computer::LifecycleState::Connecting,
        });

        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({
                "code": "action_unavailable",
                "action": "connect",
                "lifecycle": "connecting"
            })
        );
    }
}
