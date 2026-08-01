use a2c_smcp::smcp_computer::LifecycleState;
use serde::{Deserialize, Serialize};

/// Stable user-facing projection of the SDK lifecycle.
///
/// The raw SDK lifecycle remains available for advanced diagnostics, while normal clients should
/// render this projection so connection transitions are not confused with Runtime availability.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComputerRuntimeUserState {
    NotRunning,
    Starting,
    Running,
    Stopping,
    Degraded,
    Error,
}

impl From<LifecycleState> for ComputerRuntimeUserState {
    fn from(lifecycle: LifecycleState) -> Self {
        match lifecycle {
            LifecycleState::Created | LifecycleState::Stopped | LifecycleState::Shutdown => {
                Self::NotRunning
            }
            LifecycleState::Starting | LifecycleState::Syncing => Self::Starting,
            LifecycleState::Started
            | LifecycleState::Connecting
            | LifecycleState::Connected
            | LifecycleState::JoinedOffice
            | LifecycleState::Disconnecting => Self::Running,
            LifecycleState::Stopping => Self::Stopping,
            LifecycleState::Degraded => Self::Degraded,
            LifecycleState::Error => Self::Error,
        }
    }
}

/// Runtime actions exposed to clients. The backend remains authoritative: the same lifecycle
/// policy is serialized in snapshots and checked again at command boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputerRuntimeAction {
    Start,
    Stop,
    Restart,
    Connect,
    Disconnect,
    ManageMcp,
}

impl ComputerRuntimeAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::Connect => "connect",
            Self::Disconnect => "disconnect",
            Self::ManageMcp => "manage_mcp",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComputerRuntimeActionDisabledReason {
    AlreadyRunning,
    NotRunning,
    TransitionInProgress,
    Degraded,
    ConnectionUnavailable,
}

impl ComputerRuntimeActionDisabledReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AlreadyRunning => "already_running",
            Self::NotRunning => "not_running",
            Self::TransitionInProgress => "transition_in_progress",
            Self::Degraded => "degraded",
            Self::ConnectionUnavailable => "connection_unavailable",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputerRuntimeActionCapability {
    pub enabled: bool,
    pub disabled_reason: Option<ComputerRuntimeActionDisabledReason>,
}

impl ComputerRuntimeActionCapability {
    fn enabled() -> Self {
        Self {
            enabled: true,
            disabled_reason: None,
        }
    }

    fn disabled(reason: ComputerRuntimeActionDisabledReason) -> Self {
        Self {
            enabled: false,
            disabled_reason: Some(reason),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputerRuntimeActionCapabilities {
    pub start: ComputerRuntimeActionCapability,
    pub stop: ComputerRuntimeActionCapability,
    pub restart: ComputerRuntimeActionCapability,
    pub connect: ComputerRuntimeActionCapability,
    pub disconnect: ComputerRuntimeActionCapability,
    pub manage_mcp: ComputerRuntimeActionCapability,
}

impl ComputerRuntimeActionCapabilities {
    pub fn for_lifecycle(lifecycle: LifecycleState) -> Self {
        use ComputerRuntimeActionDisabledReason as Disabled;

        let inactive = matches!(
            lifecycle,
            LifecycleState::Created
                | LifecycleState::Stopped
                | LifecycleState::Shutdown
                | LifecycleState::Error
        );
        let transitioning = matches!(
            lifecycle,
            LifecycleState::Starting
                | LifecycleState::Connecting
                | LifecycleState::Syncing
                | LifecycleState::Disconnecting
                | LifecycleState::Stopping
        );
        let locally_operational = matches!(
            lifecycle,
            LifecycleState::Started
                | LifecycleState::Connected
                | LifecycleState::JoinedOffice
                | LifecycleState::Degraded
        );

        let inactive_or_transition_reason = || {
            if transitioning {
                Disabled::TransitionInProgress
            } else {
                Disabled::NotRunning
            }
        };

        Self {
            start: if inactive {
                ComputerRuntimeActionCapability::enabled()
            } else {
                ComputerRuntimeActionCapability::disabled(if transitioning {
                    Disabled::TransitionInProgress
                } else {
                    Disabled::AlreadyRunning
                })
            },
            stop: if locally_operational {
                ComputerRuntimeActionCapability::enabled()
            } else {
                ComputerRuntimeActionCapability::disabled(inactive_or_transition_reason())
            },
            restart: if locally_operational {
                ComputerRuntimeActionCapability::enabled()
            } else {
                ComputerRuntimeActionCapability::disabled(inactive_or_transition_reason())
            },
            connect: if lifecycle == LifecycleState::Started {
                ComputerRuntimeActionCapability::enabled()
            } else {
                ComputerRuntimeActionCapability::disabled(if transitioning {
                    Disabled::TransitionInProgress
                } else if inactive {
                    Disabled::NotRunning
                } else {
                    Disabled::ConnectionUnavailable
                })
            },
            disconnect: if matches!(
                lifecycle,
                LifecycleState::Connected | LifecycleState::JoinedOffice | LifecycleState::Degraded
            ) {
                ComputerRuntimeActionCapability::enabled()
            } else {
                ComputerRuntimeActionCapability::disabled(if transitioning {
                    Disabled::TransitionInProgress
                } else if inactive {
                    Disabled::NotRunning
                } else {
                    Disabled::ConnectionUnavailable
                })
            },
            manage_mcp: if matches!(
                lifecycle,
                LifecycleState::Started | LifecycleState::Connected | LifecycleState::JoinedOffice
            ) {
                ComputerRuntimeActionCapability::enabled()
            } else {
                ComputerRuntimeActionCapability::disabled(
                    if lifecycle == LifecycleState::Degraded {
                        Disabled::Degraded
                    } else {
                        inactive_or_transition_reason()
                    },
                )
            },
        }
    }

    pub fn capability(self, action: ComputerRuntimeAction) -> ComputerRuntimeActionCapability {
        match action {
            ComputerRuntimeAction::Start => self.start,
            ComputerRuntimeAction::Stop => self.stop,
            ComputerRuntimeAction::Restart => self.restart,
            ComputerRuntimeAction::Connect => self.connect,
            ComputerRuntimeAction::Disconnect => self.disconnect,
            ComputerRuntimeAction::ManageMcp => self.manage_mcp,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "runtime action '{action}' is unavailable while lifecycle is '{lifecycle}' ({disabled_reason})"
)]
pub struct ComputerRuntimeActionUnavailable {
    pub action: &'static str,
    pub lifecycle: LifecycleState,
    pub disabled_reason: &'static str,
}

pub fn ensure_runtime_action(
    lifecycle: LifecycleState,
    action: ComputerRuntimeAction,
) -> Result<(), ComputerRuntimeActionUnavailable> {
    let capability = ComputerRuntimeActionCapabilities::for_lifecycle(lifecycle).capability(action);
    if capability.enabled {
        Ok(())
    } else {
        Err(ComputerRuntimeActionUnavailable {
            action: action.as_str(),
            lifecycle,
            disabled_reason: capability
                .disabled_reason
                .expect("disabled runtime action must provide a reason")
                .as_str(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_state_mapping_covers_every_sdk_lifecycle() {
        let cases = [
            (
                LifecycleState::Created,
                ComputerRuntimeUserState::NotRunning,
            ),
            (LifecycleState::Starting, ComputerRuntimeUserState::Starting),
            (LifecycleState::Started, ComputerRuntimeUserState::Running),
            (
                LifecycleState::Connecting,
                ComputerRuntimeUserState::Running,
            ),
            (LifecycleState::Connected, ComputerRuntimeUserState::Running),
            (
                LifecycleState::JoinedOffice,
                ComputerRuntimeUserState::Running,
            ),
            (LifecycleState::Syncing, ComputerRuntimeUserState::Starting),
            (LifecycleState::Degraded, ComputerRuntimeUserState::Degraded),
            (
                LifecycleState::Disconnecting,
                ComputerRuntimeUserState::Running,
            ),
            (LifecycleState::Stopping, ComputerRuntimeUserState::Stopping),
            (
                LifecycleState::Stopped,
                ComputerRuntimeUserState::NotRunning,
            ),
            (
                LifecycleState::Shutdown,
                ComputerRuntimeUserState::NotRunning,
            ),
            (LifecycleState::Error, ComputerRuntimeUserState::Error),
        ];

        for (lifecycle, expected) in cases {
            assert_eq!(
                ComputerRuntimeUserState::from(lifecycle),
                expected,
                "unexpected user state for {lifecycle}"
            );
        }
    }

    #[test]
    fn action_matrix_is_explicit_for_every_sdk_lifecycle() {
        use ComputerRuntimeActionDisabledReason as Disabled;

        let enabled = ComputerRuntimeActionCapability::enabled;
        let disabled = ComputerRuntimeActionCapability::disabled;
        let cases = [
            (
                LifecycleState::Created,
                ComputerRuntimeActionCapabilities {
                    start: enabled(),
                    stop: disabled(Disabled::NotRunning),
                    restart: disabled(Disabled::NotRunning),
                    connect: disabled(Disabled::NotRunning),
                    disconnect: disabled(Disabled::NotRunning),
                    manage_mcp: disabled(Disabled::NotRunning),
                },
            ),
            (
                LifecycleState::Starting,
                ComputerRuntimeActionCapabilities {
                    start: disabled(Disabled::TransitionInProgress),
                    stop: disabled(Disabled::TransitionInProgress),
                    restart: disabled(Disabled::TransitionInProgress),
                    connect: disabled(Disabled::TransitionInProgress),
                    disconnect: disabled(Disabled::TransitionInProgress),
                    manage_mcp: disabled(Disabled::TransitionInProgress),
                },
            ),
            (
                LifecycleState::Started,
                ComputerRuntimeActionCapabilities {
                    start: disabled(Disabled::AlreadyRunning),
                    stop: enabled(),
                    restart: enabled(),
                    connect: enabled(),
                    disconnect: disabled(Disabled::ConnectionUnavailable),
                    manage_mcp: enabled(),
                },
            ),
            (
                LifecycleState::Connecting,
                ComputerRuntimeActionCapabilities {
                    start: disabled(Disabled::TransitionInProgress),
                    stop: disabled(Disabled::TransitionInProgress),
                    restart: disabled(Disabled::TransitionInProgress),
                    connect: disabled(Disabled::TransitionInProgress),
                    disconnect: disabled(Disabled::TransitionInProgress),
                    manage_mcp: disabled(Disabled::TransitionInProgress),
                },
            ),
            (
                LifecycleState::Connected,
                ComputerRuntimeActionCapabilities {
                    start: disabled(Disabled::AlreadyRunning),
                    stop: enabled(),
                    restart: enabled(),
                    connect: disabled(Disabled::ConnectionUnavailable),
                    disconnect: enabled(),
                    manage_mcp: enabled(),
                },
            ),
            (
                LifecycleState::JoinedOffice,
                ComputerRuntimeActionCapabilities {
                    start: disabled(Disabled::AlreadyRunning),
                    stop: enabled(),
                    restart: enabled(),
                    connect: disabled(Disabled::ConnectionUnavailable),
                    disconnect: enabled(),
                    manage_mcp: enabled(),
                },
            ),
            (
                LifecycleState::Syncing,
                ComputerRuntimeActionCapabilities {
                    start: disabled(Disabled::TransitionInProgress),
                    stop: disabled(Disabled::TransitionInProgress),
                    restart: disabled(Disabled::TransitionInProgress),
                    connect: disabled(Disabled::TransitionInProgress),
                    disconnect: disabled(Disabled::TransitionInProgress),
                    manage_mcp: disabled(Disabled::TransitionInProgress),
                },
            ),
            (
                LifecycleState::Degraded,
                ComputerRuntimeActionCapabilities {
                    start: disabled(Disabled::AlreadyRunning),
                    stop: enabled(),
                    restart: enabled(),
                    connect: disabled(Disabled::ConnectionUnavailable),
                    disconnect: enabled(),
                    manage_mcp: disabled(Disabled::Degraded),
                },
            ),
            (
                LifecycleState::Disconnecting,
                ComputerRuntimeActionCapabilities {
                    start: disabled(Disabled::TransitionInProgress),
                    stop: disabled(Disabled::TransitionInProgress),
                    restart: disabled(Disabled::TransitionInProgress),
                    connect: disabled(Disabled::TransitionInProgress),
                    disconnect: disabled(Disabled::TransitionInProgress),
                    manage_mcp: disabled(Disabled::TransitionInProgress),
                },
            ),
            (
                LifecycleState::Stopping,
                ComputerRuntimeActionCapabilities {
                    start: disabled(Disabled::TransitionInProgress),
                    stop: disabled(Disabled::TransitionInProgress),
                    restart: disabled(Disabled::TransitionInProgress),
                    connect: disabled(Disabled::TransitionInProgress),
                    disconnect: disabled(Disabled::TransitionInProgress),
                    manage_mcp: disabled(Disabled::TransitionInProgress),
                },
            ),
            (
                LifecycleState::Stopped,
                ComputerRuntimeActionCapabilities {
                    start: enabled(),
                    stop: disabled(Disabled::NotRunning),
                    restart: disabled(Disabled::NotRunning),
                    connect: disabled(Disabled::NotRunning),
                    disconnect: disabled(Disabled::NotRunning),
                    manage_mcp: disabled(Disabled::NotRunning),
                },
            ),
            (
                LifecycleState::Shutdown,
                ComputerRuntimeActionCapabilities {
                    start: enabled(),
                    stop: disabled(Disabled::NotRunning),
                    restart: disabled(Disabled::NotRunning),
                    connect: disabled(Disabled::NotRunning),
                    disconnect: disabled(Disabled::NotRunning),
                    manage_mcp: disabled(Disabled::NotRunning),
                },
            ),
            (
                LifecycleState::Error,
                ComputerRuntimeActionCapabilities {
                    start: enabled(),
                    stop: disabled(Disabled::NotRunning),
                    restart: disabled(Disabled::NotRunning),
                    connect: disabled(Disabled::NotRunning),
                    disconnect: disabled(Disabled::NotRunning),
                    manage_mcp: disabled(Disabled::NotRunning),
                },
            ),
        ];

        for (lifecycle, expected) in cases {
            let actions = ComputerRuntimeActionCapabilities::for_lifecycle(lifecycle);
            assert_eq!(
                actions, expected,
                "unexpected action capabilities for {lifecycle}"
            );
            for action in [
                ComputerRuntimeAction::Start,
                ComputerRuntimeAction::Stop,
                ComputerRuntimeAction::Restart,
                ComputerRuntimeAction::Connect,
                ComputerRuntimeAction::Disconnect,
                ComputerRuntimeAction::ManageMcp,
            ] {
                let capability = actions.capability(action);
                assert_eq!(
                    capability.enabled,
                    capability.disabled_reason.is_none(),
                    "capability and reason diverged for {} at {lifecycle}",
                    action.as_str()
                );
            }
        }
    }
}
