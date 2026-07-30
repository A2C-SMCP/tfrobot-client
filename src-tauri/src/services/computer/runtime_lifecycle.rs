use a2c_smcp::smcp_computer::LifecycleState;
use serde::{Deserialize, Serialize};

/// Runtime actions exposed to clients. The backend remains authoritative: the same
/// lifecycle policy is serialized in snapshots and checked again at command boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputerRuntimeAction {
    Start,
    Stop,
    Restart,
    Reload,
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
            Self::Reload => "reload",
            Self::Connect => "connect",
            Self::Disconnect => "disconnect",
            Self::ManageMcp => "manage_mcp",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputerRuntimeActionCapabilities {
    pub can_start: bool,
    pub can_stop: bool,
    pub can_restart: bool,
    pub can_reload: bool,
    pub can_connect: bool,
    pub can_disconnect: bool,
    pub can_manage_mcp: bool,
}

impl ComputerRuntimeActionCapabilities {
    pub fn for_lifecycle(lifecycle: LifecycleState) -> Self {
        let inactive = matches!(
            lifecycle,
            LifecycleState::Created
                | LifecycleState::Stopped
                | LifecycleState::Shutdown
                | LifecycleState::Error
        );
        let locally_operational = matches!(
            lifecycle,
            LifecycleState::Started
                | LifecycleState::Connected
                | LifecycleState::JoinedOffice
                | LifecycleState::Degraded
        );
        let mcp_operational = matches!(
            lifecycle,
            LifecycleState::Started | LifecycleState::Connected | LifecycleState::JoinedOffice
        );

        Self {
            can_start: inactive,
            can_stop: locally_operational,
            can_restart: locally_operational,
            can_reload: inactive || locally_operational,
            can_connect: lifecycle == LifecycleState::Started,
            can_disconnect: matches!(
                lifecycle,
                LifecycleState::Connected | LifecycleState::JoinedOffice | LifecycleState::Degraded
            ),
            can_manage_mcp: mcp_operational,
        }
    }

    pub fn allows(self, action: ComputerRuntimeAction) -> bool {
        match action {
            ComputerRuntimeAction::Start => self.can_start,
            ComputerRuntimeAction::Stop => self.can_stop,
            ComputerRuntimeAction::Restart => self.can_restart,
            ComputerRuntimeAction::Reload => self.can_reload,
            ComputerRuntimeAction::Connect => self.can_connect,
            ComputerRuntimeAction::Disconnect => self.can_disconnect,
            ComputerRuntimeAction::ManageMcp => self.can_manage_mcp,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("runtime action '{action}' is unavailable while lifecycle is '{lifecycle}'")]
pub struct ComputerRuntimeActionUnavailable {
    pub action: &'static str,
    pub lifecycle: LifecycleState,
}

pub fn ensure_runtime_action(
    lifecycle: LifecycleState,
    action: ComputerRuntimeAction,
) -> Result<(), ComputerRuntimeActionUnavailable> {
    if ComputerRuntimeActionCapabilities::for_lifecycle(lifecycle).allows(action) {
        Ok(())
    } else {
        Err(ComputerRuntimeActionUnavailable {
            action: action.as_str(),
            lifecycle,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_matrix_is_explicit_for_every_sdk_lifecycle() {
        let cases = [
            (
                LifecycleState::Created,
                (true, false, false, true, false, false, false),
            ),
            (
                LifecycleState::Starting,
                (false, false, false, false, false, false, false),
            ),
            (
                LifecycleState::Started,
                (false, true, true, true, true, false, true),
            ),
            (
                LifecycleState::Connecting,
                (false, false, false, false, false, false, false),
            ),
            (
                LifecycleState::Connected,
                (false, true, true, true, false, true, true),
            ),
            (
                LifecycleState::JoinedOffice,
                (false, true, true, true, false, true, true),
            ),
            (
                LifecycleState::Syncing,
                (false, false, false, false, false, false, false),
            ),
            (
                LifecycleState::Degraded,
                (false, true, true, true, false, true, false),
            ),
            (
                LifecycleState::Disconnecting,
                (false, false, false, false, false, false, false),
            ),
            (
                LifecycleState::Stopping,
                (false, false, false, false, false, false, false),
            ),
            (
                LifecycleState::Stopped,
                (true, false, false, true, false, false, false),
            ),
            (
                LifecycleState::Shutdown,
                (true, false, false, true, false, false, false),
            ),
            (
                LifecycleState::Error,
                (true, false, false, true, false, false, false),
            ),
        ];

        for (lifecycle, expected) in cases {
            let actions = ComputerRuntimeActionCapabilities::for_lifecycle(lifecycle);
            assert_eq!(
                (
                    actions.can_start,
                    actions.can_stop,
                    actions.can_restart,
                    actions.can_reload,
                    actions.can_connect,
                    actions.can_disconnect,
                    actions.can_manage_mcp,
                ),
                expected,
                "unexpected action capabilities for {lifecycle}"
            );
        }
    }
}
