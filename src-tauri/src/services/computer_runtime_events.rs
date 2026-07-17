use crate::services::computer::{
    ClientConnectionAuthoritySnapshot, ComputerRuntimeActionCapabilities,
};
use a2c_smcp::smcp_computer::{ComputerEvent, ComputerStatusSnapshot, LifecycleState};
use serde::{Deserialize, Serialize};

pub const COMPUTER_RUNTIME_STATUS_EVENT: &str = "computer-runtime-status";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputerRuntimeSnapshot {
    pub incarnation: u64,
    pub generation: u64,
    pub snapshot_revision: u64,
    pub lifecycle: LifecycleState,
    pub actions: ComputerRuntimeActionCapabilities,
    pub config_revision: u64,
    pub capability_revision: u64,
    pub mcp_servers: usize,
    pub active_mcp_servers: usize,
    pub tools: usize,
    pub skills: usize,
    pub last_error: Option<String>,
    pub degraded_reason: Option<String>,
}

impl ComputerRuntimeSnapshot {
    pub fn from_sdk(
        incarnation: u64,
        generation: u64,
        snapshot_revision: u64,
        snapshot: ComputerStatusSnapshot,
        client_last_error: Option<String>,
    ) -> Self {
        let last_error = snapshot.last_error.or(client_last_error);
        let lifecycle = snapshot.lifecycle;
        Self {
            incarnation,
            generation,
            snapshot_revision,
            lifecycle,
            actions: ComputerRuntimeActionCapabilities::for_lifecycle(lifecycle),
            config_revision: snapshot.config_revision,
            capability_revision: snapshot.capability_revision,
            mcp_servers: snapshot.mcp_servers,
            active_mcp_servers: snapshot.active_mcp_servers,
            tools: snapshot.tools,
            skills: snapshot.skills,
            last_error,
            degraded_reason: snapshot.degraded_reason,
        }
    }

    pub fn is_running(&self) -> bool {
        matches!(
            self.lifecycle,
            LifecycleState::Starting
                | LifecycleState::Started
                | LifecycleState::Connecting
                | LifecycleState::Connected
                | LifecycleState::JoinedOffice
                | LifecycleState::Syncing
                | LifecycleState::Degraded
                | LifecycleState::Disconnecting
                | LifecycleState::Stopping
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ComputerRuntimeEventCause {
    LifecycleChanged { state: LifecycleState },
    ConfigRevisionBumped { revision: u64 },
    CapabilityRevisionBumped { revision: u64 },
    ClientConnectionAuthorityChanged { revision: u64, present: bool },
    ClientDiagnosticChanged { operation: String, has_error: bool },
    HandleReplaced { reason: String },
    ObservationAdvanced,
    Resync { skipped_events: u64 },
}

impl From<ComputerEvent> for ComputerRuntimeEventCause {
    fn from(event: ComputerEvent) -> Self {
        match event {
            ComputerEvent::LifecycleChanged { state } => Self::LifecycleChanged { state },
            ComputerEvent::ConfigRevisionBumped { revision } => {
                Self::ConfigRevisionBumped { revision }
            }
            ComputerEvent::CapabilityRevisionBumped { revision } => {
                Self::CapabilityRevisionBumped { revision }
            }
        }
    }
}

impl ComputerRuntimeEventCause {
    fn matches_observation(
        &self,
        snapshot: &ComputerRuntimeSnapshot,
        connection: &ClientConnectionAuthoritySnapshot,
    ) -> bool {
        match self {
            Self::LifecycleChanged { state } => snapshot.lifecycle == *state,
            Self::ConfigRevisionBumped { revision } => snapshot.config_revision == *revision,
            Self::CapabilityRevisionBumped { revision } => {
                snapshot.capability_revision == *revision
            }
            Self::ClientConnectionAuthorityChanged { revision, present } => {
                connection.revision == *revision && connection.present == *present
            }
            Self::ClientDiagnosticChanged { has_error, .. } => {
                snapshot.last_error.is_some() == *has_error
            }
            Self::HandleReplaced { .. } | Self::ObservationAdvanced | Self::Resync { .. } => true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputerRuntimeStatusEvent {
    pub instance_id: String,
    pub cause: ComputerRuntimeEventCause,
    pub snapshot: ComputerRuntimeSnapshot,
    pub connection: ClientConnectionAuthoritySnapshot,
}

impl ComputerRuntimeStatusEvent {
    /// Couples an event cause with the snapshot actually observed by the relay. SDK broadcasts
    /// carry causes but no snapshots, so a busy receiver may observe a later state. In that case
    /// expose an explicit advanced observation instead of attaching stale cause details to a
    /// newer snapshot. `Resync.skipped_events` remains reserved for the broadcast receiver's
    /// exact lag count.
    pub fn from_observation(
        instance_id: String,
        cause: ComputerRuntimeEventCause,
        snapshot: ComputerRuntimeSnapshot,
        connection: ClientConnectionAuthoritySnapshot,
    ) -> Self {
        let cause = if cause.matches_observation(&snapshot, &connection) {
            cause
        } else {
            ComputerRuntimeEventCause::ObservationAdvanced
        };
        Self {
            instance_id,
            cause,
            snapshot,
            connection,
        }
    }
}

pub trait ComputerRuntimeEventSink: Send + Sync {
    fn emit(&self, event: &ComputerRuntimeStatusEvent) -> Result<(), String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sdk_snapshot(lifecycle: LifecycleState) -> ComputerStatusSnapshot {
        ComputerStatusSnapshot {
            lifecycle,
            config_revision: 2,
            capability_revision: 3,
            mcp_servers: 4,
            active_mcp_servers: 1,
            tools: 5,
            skills: 6,
            last_error: Some("runtime_error".to_string()),
            degraded_reason: None,
        }
    }

    fn connection_authority(revision: u64, present: bool) -> ClientConnectionAuthoritySnapshot {
        ClientConnectionAuthoritySnapshot {
            present,
            revision,
            context: None,
        }
    }

    #[test]
    fn snapshot_preserves_sdk_runtime_fields_and_generation() {
        let snapshot = ComputerRuntimeSnapshot::from_sdk(
            5,
            7,
            11,
            sdk_snapshot(LifecycleState::Degraded),
            None,
        );

        assert_eq!(snapshot.incarnation, 5);
        assert_eq!(snapshot.generation, 7);
        assert_eq!(snapshot.snapshot_revision, 11);
        assert_eq!(snapshot.lifecycle, LifecycleState::Degraded);
        assert_eq!(snapshot.config_revision, 2);
        assert_eq!(snapshot.capability_revision, 3);
        assert!(snapshot.is_running());
    }

    #[test]
    fn terminal_and_unstarted_snapshots_are_not_running() {
        for lifecycle in [
            LifecycleState::Created,
            LifecycleState::Stopped,
            LifecycleState::Shutdown,
            LifecycleState::Error,
        ] {
            assert!(
                !ComputerRuntimeSnapshot::from_sdk(1, 1, 1, sdk_snapshot(lifecycle), None)
                    .is_running()
            );
        }
    }

    #[test]
    fn sdk_error_takes_priority_over_client_diagnostic() {
        let snapshot = ComputerRuntimeSnapshot::from_sdk(
            1,
            1,
            1,
            sdk_snapshot(LifecycleState::Started),
            Some("client_error".to_string()),
        );

        assert_eq!(snapshot.last_error.as_deref(), Some("runtime_error"));
    }

    #[test]
    fn client_diagnostic_fills_missing_sdk_error() {
        let mut sdk_snapshot = sdk_snapshot(LifecycleState::Started);
        sdk_snapshot.last_error = None;
        let snapshot = ComputerRuntimeSnapshot::from_sdk(
            1,
            1,
            1,
            sdk_snapshot,
            Some("client_error".to_string()),
        );

        assert_eq!(snapshot.last_error.as_deref(), Some("client_error"));
    }

    #[test]
    fn event_marks_a_cause_as_advanced_when_the_observed_snapshot_has_advanced() {
        let event = ComputerRuntimeStatusEvent::from_observation(
            "computer-a".to_string(),
            ComputerRuntimeEventCause::LifecycleChanged {
                state: LifecycleState::Connecting,
            },
            ComputerRuntimeSnapshot::from_sdk(
                1,
                1,
                1,
                sdk_snapshot(LifecycleState::JoinedOffice),
                None,
            ),
            connection_authority(0, false),
        );

        assert_eq!(event.cause, ComputerRuntimeEventCause::ObservationAdvanced);
    }

    #[test]
    fn event_preserves_matching_connection_authority_cause() {
        let event = ComputerRuntimeStatusEvent::from_observation(
            "computer-a".to_string(),
            ComputerRuntimeEventCause::ClientConnectionAuthorityChanged {
                revision: 4,
                present: true,
            },
            ComputerRuntimeSnapshot::from_sdk(
                1,
                1,
                1,
                sdk_snapshot(LifecycleState::JoinedOffice),
                None,
            ),
            connection_authority(4, true),
        );

        assert_eq!(
            event.cause,
            ComputerRuntimeEventCause::ClientConnectionAuthorityChanged {
                revision: 4,
                present: true,
            }
        );
    }
}
