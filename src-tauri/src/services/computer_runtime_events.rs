use crate::services::computer::{
    ClientConnectionOperation, ClientConnectionOperationError, ClientConnectionStateSnapshot,
    ClientConnectionStatus, ComputerRuntimeActionCapabilities, ComputerRuntimeUserState,
};
use crate::services::observability::redact_text;
use a2c_smcp::smcp_computer::oauth::OAuthStatus;
use a2c_smcp::smcp_computer::{ComputerEvent, ComputerStatusSnapshot, LifecycleState};
use serde::{Deserialize, Serialize};

pub const COMPUTER_RUNTIME_STATUS_EVENT: &str = "computer-runtime-status";

/// OAuth state safe to expose through client IPC. SDK diagnostic messages stay in backend logs
/// and must never be retained in frontend runtime-event history.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum PublicOAuthStatus {
    NotApplicable,
    Unauthorized,
    AuthorizationPending,
    Authorized { scopes: Vec<String> },
    ReauthorizationRequired { required_scope: String },
    Error,
}

impl From<OAuthStatus> for PublicOAuthStatus {
    fn from(status: OAuthStatus) -> Self {
        match status {
            OAuthStatus::Unauthorized => Self::Unauthorized,
            OAuthStatus::AuthorizationPending => Self::AuthorizationPending,
            OAuthStatus::Authorized { scopes } => Self::Authorized { scopes },
            OAuthStatus::ReauthorizationRequired { required_scope } => {
                Self::ReauthorizationRequired { required_scope }
            }
            OAuthStatus::Error { .. } => Self::Error,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComputerRuntimeProblemSource {
    Sdk,
    ClientConnection,
    Mcp,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComputerRuntimeProblemSeverity {
    Error,
    Degraded,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComputerRuntimeProblemMessage {
    SdkRuntimeError,
    SdkRuntimeDegraded,
    ConnectionFailed,
    DisconnectionFailed,
    ReconnectionFailed,
    McpStartFailed,
    McpConfigurationApplyFailed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComputerRuntimeProblemAction {
    StartRuntime,
    RestartRuntime,
    RetryConnection,
    RetryDisconnection,
    ViewLogs,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ComputerRuntimeAffectedCapability {
    Runtime,
    Connection,
    McpServer {
        bundle_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputerRuntimeProblem {
    pub id: String,
    pub source: ComputerRuntimeProblemSource,
    pub operation: String,
    pub severity: ComputerRuntimeProblemSeverity,
    pub affected_capabilities: Vec<ComputerRuntimeAffectedCapability>,
    pub occurred_at: String,
    pub current: bool,
    pub message: ComputerRuntimeProblemMessage,
    pub recommended_actions: Vec<ComputerRuntimeProblemAction>,
    /// Redacted MCP startup detail intended for the ordinary problem alert.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation_detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub technical_detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeDiagnosticRecord {
    pub operation: String,
    pub message: String,
    pub occurred_at: String,
    pub mcp_server_name: Option<String>,
}

impl RuntimeDiagnosticRecord {
    pub(crate) fn new(operation: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            operation: operation.into(),
            message: message.into(),
            occurred_at: chrono::Utc::now().to_rfc3339(),
            mcp_server_name: None,
        }
    }

    pub(crate) fn for_mcp(
        operation: impl Into<String>,
        message: impl Into<String>,
        server_name: impl Into<String>,
    ) -> Self {
        Self {
            operation: operation.into(),
            message: message.into(),
            occurred_at: chrono::Utc::now().to_rfc3339(),
            mcp_server_name: Some(server_name.into()),
        }
    }

    pub(crate) fn preserve_occurrence_from(mut self, current: Option<&Self>) -> Self {
        if let Some(current) = current.filter(|current| current.operation == self.operation) {
            self.occurred_at.clone_from(&current.occurred_at);
        }
        self
    }
}

#[derive(Debug, Default)]
pub(crate) struct SdkProblemObservations {
    generation: u64,
    last_error: Option<SdkProblemObservation>,
    degraded_reason: Option<SdkProblemObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SdkProblemObservation {
    pub occurred_at: String,
    pub technical_detail: Option<String>,
}

impl SdkProblemObservations {
    pub(crate) fn observe(
        &mut self,
        generation: u64,
        lifecycle: LifecycleState,
        last_error: Option<&str>,
        degraded_reason: Option<&str>,
    ) -> (Option<SdkProblemObservation>, Option<SdkProblemObservation>) {
        if self.generation != generation {
            self.generation = generation;
            self.last_error = None;
            self.degraded_reason = None;
        }
        observe_sdk_problem(
            &mut self.last_error,
            lifecycle == LifecycleState::Error,
            last_error,
        );
        observe_sdk_problem(
            &mut self.degraded_reason,
            lifecycle == LifecycleState::Degraded,
            degraded_reason,
        );
        (self.last_error.clone(), self.degraded_reason.clone())
    }
}

fn observe_sdk_problem(
    current: &mut Option<SdkProblemObservation>,
    active: bool,
    technical_detail: Option<&str>,
) {
    if !active {
        *current = None;
        return;
    }
    let technical_detail = technical_detail.map(ToString::to_string);
    match current {
        Some(observation) => {
            observation.technical_detail = technical_detail;
        }
        None => {
            *current = Some(SdkProblemObservation {
                occurred_at: chrono::Utc::now().to_rfc3339(),
                technical_detail,
            });
        }
    }
}

impl ComputerRuntimeProblem {
    pub(crate) fn sdk_error(generation: u64, observation: SdkProblemObservation) -> Self {
        Self {
            id: format!("sdk:{generation}:runtime_error"),
            source: ComputerRuntimeProblemSource::Sdk,
            operation: "runtime".to_string(),
            severity: ComputerRuntimeProblemSeverity::Error,
            affected_capabilities: vec![ComputerRuntimeAffectedCapability::Runtime],
            occurred_at: observation.occurred_at,
            current: true,
            message: ComputerRuntimeProblemMessage::SdkRuntimeError,
            recommended_actions: vec![
                ComputerRuntimeProblemAction::StartRuntime,
                ComputerRuntimeProblemAction::ViewLogs,
            ],
            presentation_detail: None,
            technical_detail: observation.technical_detail,
        }
    }

    pub(crate) fn sdk_degraded(generation: u64, observation: SdkProblemObservation) -> Self {
        Self {
            id: format!("sdk:{generation}:runtime_degraded"),
            source: ComputerRuntimeProblemSource::Sdk,
            operation: "runtime_degraded".to_string(),
            severity: ComputerRuntimeProblemSeverity::Degraded,
            affected_capabilities: vec![ComputerRuntimeAffectedCapability::Runtime],
            occurred_at: observation.occurred_at,
            current: true,
            message: ComputerRuntimeProblemMessage::SdkRuntimeDegraded,
            recommended_actions: vec![
                ComputerRuntimeProblemAction::RestartRuntime,
                ComputerRuntimeProblemAction::ViewLogs,
            ],
            presentation_detail: None,
            technical_detail: observation.technical_detail,
        }
    }

    pub(crate) fn connection(generation: u64, error: &ClientConnectionOperationError) -> Self {
        let (message, retry_action) = match error.operation {
            ClientConnectionOperation::Connect => (
                ComputerRuntimeProblemMessage::ConnectionFailed,
                ComputerRuntimeProblemAction::RetryConnection,
            ),
            ClientConnectionOperation::Disconnect => (
                ComputerRuntimeProblemMessage::DisconnectionFailed,
                ComputerRuntimeProblemAction::RetryDisconnection,
            ),
            ClientConnectionOperation::Reconnect => (
                ComputerRuntimeProblemMessage::ReconnectionFailed,
                ComputerRuntimeProblemAction::RetryConnection,
            ),
        };
        let mut recommended_actions = Vec::new();
        if error.retryable {
            recommended_actions.push(retry_action);
        }
        recommended_actions.push(ComputerRuntimeProblemAction::ViewLogs);
        Self {
            id: format!(
                "client_connection:{generation}:{}",
                client_connection_operation_name(error.operation)
            ),
            source: ComputerRuntimeProblemSource::ClientConnection,
            operation: client_connection_operation_name(error.operation).to_string(),
            severity: ComputerRuntimeProblemSeverity::Degraded,
            affected_capabilities: vec![ComputerRuntimeAffectedCapability::Connection],
            occurred_at: error.occurred_at.clone(),
            current: true,
            message,
            recommended_actions,
            presentation_detail: None,
            technical_detail: Some(error.message.clone()),
        }
    }

    pub(crate) fn client_diagnostic(generation: u64, diagnostic: RuntimeDiagnosticRecord) -> Self {
        let operation = diagnostic.operation.as_str();
        let (message, retry_action) = match operation {
            "disconnect" => (
                ComputerRuntimeProblemMessage::DisconnectionFailed,
                ComputerRuntimeProblemAction::RetryDisconnection,
            ),
            "reconnect" => (
                ComputerRuntimeProblemMessage::ReconnectionFailed,
                ComputerRuntimeProblemAction::RetryConnection,
            ),
            _ => (
                ComputerRuntimeProblemMessage::ConnectionFailed,
                ComputerRuntimeProblemAction::RetryConnection,
            ),
        };
        Self {
            id: format!("client_connection:{generation}:{operation}"),
            source: ComputerRuntimeProblemSource::ClientConnection,
            operation: diagnostic.operation,
            severity: ComputerRuntimeProblemSeverity::Degraded,
            affected_capabilities: vec![ComputerRuntimeAffectedCapability::Connection],
            occurred_at: diagnostic.occurred_at,
            current: true,
            message,
            recommended_actions: vec![retry_action, ComputerRuntimeProblemAction::ViewLogs],
            presentation_detail: None,
            technical_detail: Some(diagnostic.message),
        }
    }

    pub(crate) fn mcp(
        generation: u64,
        bundle_id: &str,
        name: Option<String>,
        diagnostic: RuntimeDiagnosticRecord,
    ) -> Self {
        let presentation_detail = if diagnostic.operation == "apply_configuration" {
            None
        } else {
            mcp_presentation_detail(&diagnostic.message)
        };
        let message = if diagnostic.operation == "apply_configuration" {
            ComputerRuntimeProblemMessage::McpConfigurationApplyFailed
        } else {
            ComputerRuntimeProblemMessage::McpStartFailed
        };
        Self {
            id: format!("mcp:{generation}:{bundle_id}:{}", diagnostic.operation),
            source: ComputerRuntimeProblemSource::Mcp,
            operation: diagnostic.operation,
            severity: ComputerRuntimeProblemSeverity::Degraded,
            affected_capabilities: vec![ComputerRuntimeAffectedCapability::McpServer {
                bundle_id: bundle_id.to_string(),
                name,
            }],
            occurred_at: diagnostic.occurred_at,
            current: true,
            message,
            recommended_actions: vec![
                ComputerRuntimeProblemAction::RestartRuntime,
                ComputerRuntimeProblemAction::ViewLogs,
            ],
            presentation_detail,
            technical_detail: Some(diagnostic.message),
        }
    }
}

fn mcp_presentation_detail(detail: &str) -> Option<String> {
    let detail = redact_text(detail);
    let detail = detail.trim();
    (!detail.is_empty()).then(|| detail.to_string())
}

fn client_connection_operation_name(operation: ClientConnectionOperation) -> &'static str {
    match operation {
        ClientConnectionOperation::Connect => "connect",
        ClientConnectionOperation::Disconnect => "disconnect",
        ClientConnectionOperation::Reconnect => "reconnect",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputerRuntimeSnapshot {
    pub incarnation: u64,
    pub generation: u64,
    pub snapshot_revision: u64,
    pub lifecycle: LifecycleState,
    pub user_state: ComputerRuntimeUserState,
    pub actions: ComputerRuntimeActionCapabilities,
    pub config_revision: u64,
    pub capability_revision: u64,
    pub diagnostics_revision: u64,
    pub mcp_servers: usize,
    pub active_mcp_servers: usize,
    pub tools: usize,
    pub skills: usize,
    pub problems: Vec<ComputerRuntimeProblem>,
    pub last_error: Option<String>,
    pub degraded_reason: Option<String>,
}

impl ComputerRuntimeSnapshot {
    pub fn from_sdk(
        incarnation: u64,
        generation: u64,
        snapshot_revision: u64,
        snapshot: ComputerStatusSnapshot,
    ) -> Self {
        let lifecycle = snapshot.lifecycle;
        Self {
            incarnation,
            generation,
            snapshot_revision,
            lifecycle,
            user_state: lifecycle.into(),
            actions: ComputerRuntimeActionCapabilities::for_lifecycle(lifecycle),
            config_revision: snapshot.config_revision,
            capability_revision: snapshot.capability_revision,
            diagnostics_revision: snapshot.diagnostics_revision,
            mcp_servers: snapshot.mcp_servers,
            active_mcp_servers: snapshot.active_mcp_servers,
            tools: snapshot.tools,
            skills: snapshot.skills,
            problems: Vec::new(),
            last_error: snapshot.last_error,
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
    LifecycleChanged {
        state: LifecycleState,
    },
    ConfigRevisionBumped {
        revision: u64,
    },
    CapabilityRevisionBumped {
        revision: u64,
    },
    DiagnosticsChanged {
        revision: u64,
    },
    #[serde(rename = "oauth_status_changed")]
    OAuthStatusChanged {
        bundle_id: String,
        status: PublicOAuthStatus,
    },
    ClientConnectionStateChanged {
        revision: u64,
        status: ClientConnectionStatus,
    },
    ClientDiagnosticChanged {
        operation: String,
        has_error: bool,
    },
    McpDiagnosticChanged {
        bundle_id: String,
        operation: String,
        has_error: bool,
    },
    HandleReplaced {
        reason: String,
    },
    ObservationAdvanced,
    Resync {
        skipped_events: u64,
    },
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
            // Per-server status is not part of the public runtime event contract yet. Still emit
            // an observation so consumers refresh from the authoritative aggregate snapshot.
            ComputerEvent::MCPServerStatusChanged { .. } => Self::ObservationAdvanced,
            ComputerEvent::DiagnosticsChanged { revision } => Self::DiagnosticsChanged { revision },
            ComputerEvent::OAuthStatusChanged { bundle_id, status } => Self::OAuthStatusChanged {
                bundle_id: bundle_id.into_string(),
                status: status.into(),
            },
        }
    }
}

impl ComputerRuntimeEventCause {
    fn matches_observation(
        &self,
        snapshot: &ComputerRuntimeSnapshot,
        connection: &ClientConnectionStateSnapshot,
    ) -> bool {
        match self {
            Self::LifecycleChanged { state } => snapshot.lifecycle == *state,
            Self::ConfigRevisionBumped { revision } => snapshot.config_revision == *revision,
            Self::CapabilityRevisionBumped { revision } => {
                snapshot.capability_revision == *revision
            }
            Self::DiagnosticsChanged { revision } => snapshot.diagnostics_revision == *revision,
            Self::OAuthStatusChanged { .. } => true,
            Self::ClientConnectionStateChanged { revision, status } => {
                connection.revision == *revision && connection.status == *status
            }
            Self::ClientDiagnosticChanged {
                operation,
                has_error,
            } => {
                snapshot.problems.iter().any(|problem| {
                    problem.source == ComputerRuntimeProblemSource::ClientConnection
                        && problem.operation == *operation
                }) == *has_error
            }
            Self::McpDiagnosticChanged {
                bundle_id,
                operation,
                has_error,
            } => {
                snapshot.problems.iter().any(|problem| {
                    problem.source == ComputerRuntimeProblemSource::Mcp
                        && problem.operation == *operation
                        && problem.affected_capabilities.iter().any(|capability| {
                            matches!(
                                capability,
                                ComputerRuntimeAffectedCapability::McpServer {
                                    bundle_id: problem_bundle_id,
                                    ..
                                } if problem_bundle_id == bundle_id
                            )
                        })
                }) == *has_error
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
    pub connection: ClientConnectionStateSnapshot,
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
        connection: ClientConnectionStateSnapshot,
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
    use crate::services::computer::{
        ClientConnectionActionCapabilities, ClientConnectionActionCapability,
        ClientConnectionActionDisabledReason,
    };
    use a2c_smcp::smcp_computer::mcp_clients::model::{
        BundleId, MCPServerActivationState, MCPServerConnectionState, MCPServerRuntimeStatus,
        ServerName,
    };

    fn sdk_snapshot(lifecycle: LifecycleState) -> ComputerStatusSnapshot {
        ComputerStatusSnapshot {
            lifecycle,
            config_revision: 2,
            capability_revision: 3,
            server_status_revision: 0,
            mcp_servers: 4,
            active_mcp_servers: 1,
            tools: 5,
            skills: 6,
            last_error: Some("runtime_error".to_string()),
            degraded_reason: None,
            diagnostics_revision: 0,
            diagnostics: Vec::new(),
        }
    }

    fn connection_state(revision: u64, present: bool) -> ClientConnectionStateSnapshot {
        let status = if present {
            ClientConnectionStatus::Connected
        } else {
            ClientConnectionStatus::Disconnected
        };
        ClientConnectionStateSnapshot {
            status,
            present,
            revision,
            context: None,
            operation: None,
            operation_target: None,
            last_error: None,
            actions: ClientConnectionActionCapabilities {
                connect: ClientConnectionActionCapability {
                    enabled: !present,
                    disabled_reason: present
                        .then_some(ClientConnectionActionDisabledReason::AlreadyConnected),
                },
                disconnect: ClientConnectionActionCapability {
                    enabled: present,
                    disabled_reason: (!present)
                        .then_some(ClientConnectionActionDisabledReason::NotConnected),
                },
            },
        }
    }

    #[test]
    fn snapshot_preserves_sdk_runtime_fields_and_generation() {
        let snapshot =
            ComputerRuntimeSnapshot::from_sdk(5, 7, 11, sdk_snapshot(LifecycleState::Degraded));

        assert_eq!(snapshot.incarnation, 5);
        assert_eq!(snapshot.generation, 7);
        assert_eq!(snapshot.snapshot_revision, 11);
        assert_eq!(snapshot.lifecycle, LifecycleState::Degraded);
        assert_eq!(snapshot.user_state, ComputerRuntimeUserState::Degraded);
        assert_eq!(snapshot.config_revision, 2);
        assert_eq!(snapshot.capability_revision, 3);
        assert_eq!(snapshot.diagnostics_revision, 0);
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
                !ComputerRuntimeSnapshot::from_sdk(1, 1, 1, sdk_snapshot(lifecycle)).is_running()
            );
        }
    }

    #[test]
    fn snapshot_keeps_raw_sdk_diagnostics_out_of_the_problem_projection() {
        let snapshot =
            ComputerRuntimeSnapshot::from_sdk(1, 1, 1, sdk_snapshot(LifecycleState::Started));

        assert_eq!(snapshot.last_error.as_deref(), Some("runtime_error"));
        assert!(snapshot.problems.is_empty());
    }

    #[test]
    fn oauth_error_event_projection_drops_sdk_diagnostic_message() {
        let cause = ComputerRuntimeEventCause::from(ComputerEvent::OAuthStatusChanged {
            bundle_id: a2c_smcp::smcp_computer::mcp_clients::model::BundleId::try_from("protected")
                .unwrap(),
            status: OAuthStatus::Error {
                message: "sensitive-provider-diagnostic".to_string(),
            },
        });
        let value = serde_json::to_value(cause).unwrap();

        assert_eq!(
            value,
            serde_json::json!({
                "kind": "oauth_status_changed",
                "bundle_id": "protected",
                "status": { "state": "error" }
            })
        );
        assert!(!value.to_string().contains("sensitive-provider-diagnostic"));
    }

    #[test]
    fn diagnostics_changed_event_is_forwarded_as_a_secret_free_resync_hint() {
        let cause =
            ComputerRuntimeEventCause::from(ComputerEvent::DiagnosticsChanged { revision: 17 });

        assert_eq!(
            serde_json::to_value(cause).unwrap(),
            serde_json::json!({ "kind": "diagnostics_changed", "revision": 17 })
        );
    }

    #[test]
    fn mcp_server_status_event_requests_an_aggregate_snapshot_refresh() {
        let bundle_id = BundleId::try_from("server-a").unwrap();
        let cause = ComputerRuntimeEventCause::from(ComputerEvent::MCPServerStatusChanged {
            bundle_id: bundle_id.clone(),
            status: MCPServerRuntimeStatus {
                bundle_id,
                name: ServerName::from("Server A"),
                activation: MCPServerActivationState::Started,
                connection: MCPServerConnectionState::Connected,
            },
            revision: 9,
        });

        assert_eq!(cause, ComputerRuntimeEventCause::ObservationAdvanced);
    }

    #[test]
    fn stale_diagnostics_cause_is_not_attached_to_a_newer_snapshot() {
        let mut snapshot = sdk_snapshot(LifecycleState::Started);
        snapshot.diagnostics_revision = 18;
        let event = ComputerRuntimeStatusEvent::from_observation(
            "computer-a".to_string(),
            ComputerRuntimeEventCause::DiagnosticsChanged { revision: 17 },
            ComputerRuntimeSnapshot::from_sdk(1, 1, 1, snapshot),
            connection_state(0, false),
        );

        assert_eq!(event.cause, ComputerRuntimeEventCause::ObservationAdvanced);
    }

    #[test]
    fn sdk_problem_observation_time_is_stable_until_recovery_or_replacement() {
        let mut observations = SdkProblemObservations::default();
        observations.observe(3, LifecycleState::Error, Some("failed"), None);
        observations.last_error.as_mut().unwrap().occurred_at = "first-occurrence".to_string();
        let (same, _) = observations.observe(
            3,
            LifecycleState::Error,
            Some("updated failure detail"),
            None,
        );
        observations.observe(3, LifecycleState::Started, None, None);
        let (recovered, _) = observations.observe(3, LifecycleState::Error, Some("failed"), None);
        observations.last_error.as_mut().unwrap().occurred_at = "second-occurrence".to_string();
        let (replacement, _) = observations.observe(4, LifecycleState::Error, Some("failed"), None);

        let same = same.unwrap();
        assert_eq!(same.occurred_at, "first-occurrence");
        assert_eq!(
            same.technical_detail.as_deref(),
            Some("updated failure detail")
        );
        assert_ne!(recovered.unwrap().occurred_at, "first-occurrence");
        assert_ne!(replacement.unwrap().occurred_at, "second-occurrence");
    }

    #[test]
    fn sdk_lifecycle_projects_a_safe_problem_without_optional_diagnostic_text() {
        let mut observations = SdkProblemObservations::default();
        let (error, degraded) = observations.observe(2, LifecycleState::Error, None, None);
        let problem = ComputerRuntimeProblem::sdk_error(2, error.unwrap());

        assert!(degraded.is_none());
        assert_eq!(
            problem.message,
            ComputerRuntimeProblemMessage::SdkRuntimeError
        );
        assert_eq!(
            problem.affected_capabilities,
            vec![ComputerRuntimeAffectedCapability::Runtime]
        );
        assert!(problem.technical_detail.is_none());

        let (recovered, _) = observations.observe(2, LifecycleState::Started, None, None);
        assert!(recovered.is_none());
    }

    #[test]
    fn runtime_problem_serialization_preserves_the_tauri_contract() {
        let problem = ComputerRuntimeProblem::sdk_error(
            4,
            SdkProblemObservation {
                occurred_at: "2026-07-29T10:00:00Z".to_string(),
                technical_detail: Some("safe public SDK detail".to_string()),
            },
        );

        assert_eq!(
            serde_json::to_value(problem).unwrap(),
            serde_json::json!({
                "id": "sdk:4:runtime_error",
                "source": "sdk",
                "operation": "runtime",
                "severity": "error",
                "affected_capabilities": [{ "kind": "runtime" }],
                "occurred_at": "2026-07-29T10:00:00Z",
                "current": true,
                "message": "sdk_runtime_error",
                "recommended_actions": ["start_runtime", "view_logs"],
                "technical_detail": "safe public SDK detail"
            })
        );
    }

    #[test]
    fn connection_problem_preserves_ownership_impact_and_recovery_action() {
        let problem = ComputerRuntimeProblem::connection(
            7,
            &ClientConnectionOperationError {
                operation: ClientConnectionOperation::Reconnect,
                message: "token refresh failed".to_string(),
                retryable: true,
                occurred_at: "2026-07-29T10:00:00Z".to_string(),
            },
        );

        assert_eq!(
            problem.source,
            ComputerRuntimeProblemSource::ClientConnection
        );
        assert_eq!(problem.severity, ComputerRuntimeProblemSeverity::Degraded);
        assert_eq!(
            problem.affected_capabilities,
            vec![ComputerRuntimeAffectedCapability::Connection]
        );
        assert!(problem
            .recommended_actions
            .contains(&ComputerRuntimeProblemAction::RetryConnection));
    }

    #[test]
    fn mcp_start_problem_exposes_a_redacted_presentation_detail() {
        let problem = ComputerRuntimeProblem::mcp(
            3,
            "browser",
            Some("Browser MCP".to_string()),
            RuntimeDiagnosticRecord {
                operation: "start".to_string(),
                message: "Start failed: Authorization: Bearer private-token".to_string(),
                occurred_at: "2026-08-19T09:00:00Z".to_string(),
                mcp_server_name: None,
            },
        );

        assert_eq!(
            problem.presentation_detail.as_deref(),
            Some("Start failed: Authorization: [REDACTED]")
        );
        assert_eq!(
            problem.technical_detail.as_deref(),
            Some("Start failed: Authorization: Bearer private-token")
        );
        assert_eq!(
            serde_json::to_value(&problem).unwrap()["presentation_detail"],
            "Start failed: Authorization: [REDACTED]"
        );
    }

    #[test]
    fn event_marks_a_cause_as_advanced_when_the_observed_snapshot_has_advanced() {
        let event = ComputerRuntimeStatusEvent::from_observation(
            "computer-a".to_string(),
            ComputerRuntimeEventCause::LifecycleChanged {
                state: LifecycleState::Connecting,
            },
            ComputerRuntimeSnapshot::from_sdk(1, 1, 1, sdk_snapshot(LifecycleState::JoinedOffice)),
            connection_state(0, false),
        );

        assert_eq!(event.cause, ComputerRuntimeEventCause::ObservationAdvanced);
    }

    #[test]
    fn event_preserves_matching_connection_state_cause() {
        let event = ComputerRuntimeStatusEvent::from_observation(
            "computer-a".to_string(),
            ComputerRuntimeEventCause::ClientConnectionStateChanged {
                revision: 4,
                status: ClientConnectionStatus::Connected,
            },
            ComputerRuntimeSnapshot::from_sdk(1, 1, 1, sdk_snapshot(LifecycleState::JoinedOffice)),
            connection_state(4, true),
        );

        assert_eq!(
            event.cause,
            ComputerRuntimeEventCause::ClientConnectionStateChanged {
                revision: 4,
                status: ClientConnectionStatus::Connected,
            }
        );
    }

    #[test]
    fn client_diagnostic_cause_matches_its_operation_instead_of_any_connection_problem() {
        let mut snapshot =
            ComputerRuntimeSnapshot::from_sdk(1, 1, 1, sdk_snapshot(LifecycleState::Started));
        snapshot
            .problems
            .push(ComputerRuntimeProblem::client_diagnostic(
                1,
                RuntimeDiagnosticRecord {
                    operation: "disconnect".to_string(),
                    message: "disconnect cleanup failed".to_string(),
                    occurred_at: "2026-07-29T10:00:00Z".to_string(),
                    mcp_server_name: None,
                },
            ));
        let event = ComputerRuntimeStatusEvent::from_observation(
            "computer-a".to_string(),
            ComputerRuntimeEventCause::ClientDiagnosticChanged {
                operation: "connect".to_string(),
                has_error: false,
            },
            snapshot,
            connection_state(0, false),
        );

        assert_eq!(
            event.cause,
            ComputerRuntimeEventCause::ClientDiagnosticChanged {
                operation: "connect".to_string(),
                has_error: false,
            }
        );
    }
}
