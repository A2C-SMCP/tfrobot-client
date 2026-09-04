use super::{
    redact_text, ActivityEventDraft, ActivityLevel, ActivityManagedBy, ActivityOutcome,
    ActivityProvider, ActivityTrigger, ComputerActivityCategory, ObservabilityService,
};
use crate::services::computer::ClientConnectionOperation;
use crate::services::computer_runtime_events::{
    ComputerRuntimeEventSink, ComputerRuntimeStatusEvent,
};
use a2c_smcp::smcp_computer::LifecycleState;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

struct Outage {
    correlation_id: String,
    started: Instant,
}

struct ObservedConnection {
    incarnation: u64,
    generation: u64,
    lifecycle: LifecycleState,
    outage: Option<Outage>,
}

struct ConnectionActivity<'a> {
    event: &'a ComputerRuntimeStatusEvent,
    trigger: ActivityTrigger,
    operation: &'a str,
    outcome: ActivityOutcome,
    message: &'a str,
    correlation_id: String,
    initiator: &'a str,
    reason_code: &'a str,
    will_reconnect: bool,
    outage_duration_ms: Option<u128>,
    error: Option<String>,
}

/// Converts the coarse SDK lifecycle stream into explicitly inferred Activity records. AS-47
/// will replace these projections with typed SDK reason/initiator events.
pub(crate) struct ConnectionActivitySink {
    observability: Arc<ObservabilityService>,
    observed: Mutex<HashMap<String, ObservedConnection>>,
}

impl ConnectionActivitySink {
    pub(crate) fn new(observability: Arc<ObservabilityService>) -> Self {
        Self {
            observability,
            observed: Mutex::new(HashMap::new()),
        }
    }

    fn record(&self, spec: ConnectionActivity<'_>) -> Result<(), String> {
        let event = spec.event;
        let mut activity = ActivityEventDraft::computer(
            &event.instance_id,
            if spec.outcome == ActivityOutcome::Succeeded {
                ActivityLevel::Info
            } else {
                ActivityLevel::Warn
            },
            ComputerActivityCategory::Connection,
            "smcp_connection",
            spec.operation,
            spec.outcome,
            spec.message,
        )
        .with_standard_fields(
            spec.trigger,
            Some(ActivityManagedBy::System),
            Some(ActivityProvider::Smcp),
        );
        activity.correlation_id = Some(spec.correlation_id);
        activity.merge_fields(serde_json::json!({
            "inferred": true,
            "inference_source": "sdk_lifecycle_projection",
            "sdk_contract_dependency": "AS-47",
            "runtime_incarnation": event.snapshot.incarnation,
            "runtime_generation": event.snapshot.generation,
            "connection_revision": event.connection.revision,
            "source_type": event.connection.context.as_ref().map(|context| context.source_type.as_str()),
            "target_id": event.connection.context.as_ref().and_then(|context| context.target_id.as_deref()),
            "initiator": spec.initiator,
            "reason_code": spec.reason_code,
            "will_reconnect": spec.will_reconnect,
            "outage_duration_ms": spec.outage_duration_ms,
            "duration_ms": spec.outage_duration_ms,
            "attempt_count_available": false,
            "error": spec.error.map(|error| redact_text(&error)),
        }));
        self.observability.record_activity(&activity).map(|_| ())
    }
}

impl ComputerRuntimeEventSink for ConnectionActivitySink {
    fn emit(&self, event: &ComputerRuntimeStatusEvent) -> Result<(), String> {
        let lifecycle = event.snapshot.lifecycle;
        let mut observed = self
            .observed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(previous) = observed.get_mut(&event.instance_id) else {
            let outage = (event.connection.operation == Some(ClientConnectionOperation::Reconnect))
                .then(|| Outage {
                    correlation_id: uuid::Uuid::new_v4().to_string(),
                    started: Instant::now(),
                });
            let correlation_id = outage.as_ref().map(|outage| outage.correlation_id.clone());
            observed.insert(
                event.instance_id.clone(),
                ObservedConnection {
                    incarnation: event.snapshot.incarnation,
                    generation: event.snapshot.generation,
                    lifecycle,
                    outage,
                },
            );
            if let Some(correlation_id) = correlation_id {
                drop(observed);
                return self.record(ConnectionActivity {
                    event,
                    trigger: ActivityTrigger::Runtime,
                    operation: "transport_disconnected",
                    outcome: ActivityOutcome::Unknown,
                    message: "SMCP transport disconnected; automatic reconnect started",
                    correlation_id,
                    initiator: "runtime",
                    reason_code: "client_reconnect_operation_started",
                    will_reconnect: true,
                    outage_duration_ms: None,
                    error: None,
                });
            }
            return Ok(());
        };
        if previous.incarnation != event.snapshot.incarnation
            || previous.generation != event.snapshot.generation
        {
            *previous = ObservedConnection {
                incarnation: event.snapshot.incarnation,
                generation: event.snapshot.generation,
                lifecycle,
                outage: None,
            };
            return Ok(());
        }
        let prior_lifecycle = previous.lifecycle;
        previous.lifecycle = lifecycle;
        // Explicit client operations already write authoritative command Activity. Suppressing
        // their lifecycle projection prevents stop/restart/delete/manual disconnect duplicates.
        if matches!(
            event.connection.operation,
            Some(ClientConnectionOperation::Connect | ClientConnectionOperation::Disconnect)
        ) {
            previous.outage = None;
            return Ok(());
        }

        if event.connection.operation == Some(ClientConnectionOperation::Reconnect) {
            if previous.outage.is_none() {
                let outage = Outage {
                    correlation_id: uuid::Uuid::new_v4().to_string(),
                    started: Instant::now(),
                };
                let correlation_id = outage.correlation_id.clone();
                previous.outage = Some(outage);
                drop(observed);
                return self.record(ConnectionActivity {
                    event,
                    trigger: ActivityTrigger::Runtime,
                    operation: "transport_disconnected",
                    outcome: ActivityOutcome::Unknown,
                    message: "SMCP transport disconnected; automatic reconnect started",
                    correlation_id,
                    initiator: "runtime",
                    reason_code: "client_reconnect_operation_started",
                    will_reconnect: true,
                    outage_duration_ms: None,
                    error: None,
                });
            }
            return Ok(());
        }

        let online = matches!(
            lifecycle,
            LifecycleState::Connected | LifecycleState::JoinedOffice
        );
        if previous.outage.is_some() && online {
            let outage = previous.outage.take().expect("outage checked above");
            let duration_ms = outage.started.elapsed().as_millis();
            let correlation_id = outage.correlation_id;
            drop(observed);
            return self.record(ConnectionActivity {
                event,
                trigger: ActivityTrigger::Runtime,
                operation: "reconnected",
                outcome: ActivityOutcome::Succeeded,
                message: "SMCP connection recovered",
                correlation_id,
                initiator: "runtime",
                reason_code: "sdk_lifecycle_recovered",
                will_reconnect: false,
                outage_duration_ms: Some(duration_ms),
                error: None,
            });
        }

        let reconnect_error = event
            .connection
            .last_error
            .as_ref()
            .filter(|error| error.operation == ClientConnectionOperation::Reconnect);
        let reconnect_terminal = reconnect_error.is_some()
            || !event.connection.present
            || matches!(
                lifecycle,
                LifecycleState::Created
                    | LifecycleState::Started
                    | LifecycleState::Degraded
                    | LifecycleState::Stopping
                    | LifecycleState::Stopped
                    | LifecycleState::Shutdown
                    | LifecycleState::Error
            );
        if previous.outage.is_some() && reconnect_terminal {
            let outage = previous.outage.take().expect("outage checked above");
            let duration_ms = outage.started.elapsed().as_millis();
            let correlation_id = outage.correlation_id;
            let error = reconnect_error.map(|error| error.message.clone());
            let reason_code = if error.is_some() {
                "client_reconnect_error"
            } else {
                "sdk_lifecycle_reconnect_terminal"
            };
            drop(observed);
            return self.record(ConnectionActivity {
                event,
                trigger: ActivityTrigger::Runtime,
                operation: "reconnect_failed",
                outcome: ActivityOutcome::Failed,
                message: "SMCP automatic reconnect failed",
                correlation_id,
                initiator: "runtime",
                reason_code,
                will_reconnect: false,
                outage_duration_ms: Some(duration_ms),
                error,
            });
        }

        if prior_lifecycle == lifecycle || !event.connection.present {
            return Ok(());
        }

        let was_online = matches!(
            prior_lifecycle,
            LifecycleState::Connected | LifecycleState::JoinedOffice
        );
        if was_online && lifecycle == LifecycleState::Connecting {
            let outage = Outage {
                correlation_id: uuid::Uuid::new_v4().to_string(),
                started: Instant::now(),
            };
            let correlation_id = outage.correlation_id.clone();
            previous.outage = Some(outage);
            drop(observed);
            return self.record(ConnectionActivity {
                event,
                trigger: ActivityTrigger::Runtime,
                operation: "transport_disconnected",
                outcome: ActivityOutcome::Unknown,
                message: "SMCP transport disconnected; automatic reconnect started",
                correlation_id,
                initiator: "transport",
                reason_code: "sdk_lifecycle_connecting",
                will_reconnect: true,
                outage_duration_ms: None,
                error: None,
            });
        }
        if was_online && lifecycle == LifecycleState::Started {
            previous.outage = None;
            let correlation_id = uuid::Uuid::new_v4().to_string();
            drop(observed);
            return self.record(ConnectionActivity {
                event,
                trigger: ActivityTrigger::Server,
                operation: "server_disconnected",
                outcome: ActivityOutcome::Unknown,
                message: "SMCP server disconnected the Computer",
                correlation_id,
                initiator: "server_inferred",
                reason_code: "sdk_lifecycle_started",
                will_reconnect: false,
                outage_duration_ms: None,
                error: None,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::connection::ConnectionState;
    use crate::services::computer::{
        ClientConnectionActionCapabilities, ClientConnectionActionCapability,
        ClientConnectionOperation, ClientConnectionOperationError, ClientConnectionStateSnapshot,
        ClientConnectionStatus, ConnectionStateSummary,
    };
    use crate::services::computer_runtime_events::{
        ComputerRuntimeEventCause, ComputerRuntimeSnapshot,
    };
    use crate::services::observability::{ActivityQuery, ActivityScopeFilter};
    use a2c_smcp::smcp_computer::ComputerStatusSnapshot;

    fn event(
        lifecycle: LifecycleState,
        operation: Option<ClientConnectionOperation>,
    ) -> ComputerRuntimeStatusEvent {
        let snapshot = ComputerRuntimeSnapshot::from_sdk(
            7,
            3,
            1,
            ComputerStatusSnapshot {
                lifecycle,
                config_revision: 0,
                capability_revision: 0,
                server_status_revision: 0,
                mcp_servers: 0,
                active_mcp_servers: 0,
                tools: 0,
                skills: 0,
                last_error: None,
                degraded_reason: None,
                diagnostics_revision: 0,
                diagnostics: Vec::new(),
            },
        );
        ComputerRuntimeStatusEvent {
            instance_id: "computer-a".to_string(),
            cause: ComputerRuntimeEventCause::LifecycleChanged { state: lifecycle },
            snapshot,
            connection: ClientConnectionStateSnapshot {
                status: if matches!(
                    lifecycle,
                    LifecycleState::Connected | LifecycleState::JoinedOffice
                ) {
                    ClientConnectionStatus::Connected
                } else {
                    ClientConnectionStatus::Disconnected
                },
                present: true,
                revision: 9,
                context: Some(ConnectionStateSummary::from(&ConnectionState {
                    profile_name: "manual".to_string(),
                    url: "https://smcp.example".to_string(),
                    office_id: "office".to_string(),
                    computer_name: "computer".to_string(),
                    connected_at: chrono::Utc::now(),
                    source_type: "manual".to_string(),
                    target_id: Some("target-a".to_string()),
                    target_name: Some("Target".to_string()),
                    employee_id: None,
                    generation: 4,
                })),
                operation,
                operation_target: None,
                last_error: None,
                actions: ClientConnectionActionCapabilities {
                    connect: ClientConnectionActionCapability {
                        enabled: true,
                        disabled_reason: None,
                    },
                    disconnect: ClientConnectionActionCapability {
                        enabled: true,
                        disabled_reason: None,
                    },
                },
            },
        }
    }

    #[test]
    fn transport_outage_and_recovery_share_one_correlation() {
        let dir = tempfile::tempdir().unwrap();
        let observability = Arc::new(ObservabilityService::new(dir.path()).unwrap());
        let sink = ConnectionActivitySink::new(observability.clone());
        sink.emit(&event(LifecycleState::JoinedOffice, None))
            .unwrap();
        sink.emit(&event(LifecycleState::Connecting, None)).unwrap();
        sink.emit(&event(LifecycleState::JoinedOffice, None))
            .unwrap();

        let page = observability
            .query_activity(&ActivityQuery {
                scope: ActivityScopeFilter::Computer {
                    computer_id: "computer-a".to_string(),
                },
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page.total, 2);
        assert_eq!(page.items[0].operation, "reconnected");
        assert_eq!(page.items[1].operation, "transport_disconnected");
        assert_eq!(page.items[0].correlation_id, page.items[1].correlation_id);
        assert!(page.items[0].fields.as_ref().unwrap()["outage_duration_ms"].is_number());
        assert_eq!(page.items[0].fields.as_ref().unwrap()["inferred"], true);
    }

    #[test]
    fn server_close_is_recorded_but_explicit_disconnect_is_suppressed() {
        let dir = tempfile::tempdir().unwrap();
        let observability = Arc::new(ObservabilityService::new(dir.path()).unwrap());
        let sink = ConnectionActivitySink::new(observability.clone());
        sink.emit(&event(LifecycleState::JoinedOffice, None))
            .unwrap();
        sink.emit(&event(LifecycleState::Started, None)).unwrap();
        sink.emit(&event(LifecycleState::JoinedOffice, None))
            .unwrap();
        sink.emit(&event(
            LifecycleState::Started,
            Some(ClientConnectionOperation::Disconnect),
        ))
        .unwrap();

        let page = observability
            .query_activity(&ActivityQuery::default())
            .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].operation, "server_disconnected");
        assert_eq!(page.items[0].fields.as_ref().unwrap()["trigger"], "server");
    }

    #[test]
    fn reconnect_operation_and_same_lifecycle_completion_form_a_success_pair() {
        let dir = tempfile::tempdir().unwrap();
        let observability = Arc::new(ObservabilityService::new(dir.path()).unwrap());
        let sink = ConnectionActivitySink::new(observability.clone());
        sink.emit(&event(LifecycleState::JoinedOffice, None))
            .unwrap();
        sink.emit(&event(
            LifecycleState::Connecting,
            Some(ClientConnectionOperation::Reconnect),
        ))
        .unwrap();
        sink.emit(&event(
            LifecycleState::JoinedOffice,
            Some(ClientConnectionOperation::Reconnect),
        ))
        .unwrap();
        sink.emit(&event(LifecycleState::JoinedOffice, None))
            .unwrap();

        let page = observability
            .query_activity(&ActivityQuery::default())
            .unwrap();
        assert_eq!(page.total, 2);
        assert_eq!(page.items[0].operation, "reconnected");
        assert_eq!(page.items[1].operation, "transport_disconnected");
        assert_eq!(page.items[0].correlation_id, page.items[1].correlation_id);
    }

    #[test]
    fn reconnect_terminal_error_closes_the_outage_as_failed() {
        let dir = tempfile::tempdir().unwrap();
        let observability = Arc::new(ObservabilityService::new(dir.path()).unwrap());
        let sink = ConnectionActivitySink::new(observability.clone());
        sink.emit(&event(LifecycleState::JoinedOffice, None))
            .unwrap();
        sink.emit(&event(
            LifecycleState::Connecting,
            Some(ClientConnectionOperation::Reconnect),
        ))
        .unwrap();
        sink.emit(&event(
            LifecycleState::Error,
            Some(ClientConnectionOperation::Reconnect),
        ))
        .unwrap();
        let mut failed = event(LifecycleState::Error, None);
        failed.connection.present = false;
        failed.connection.last_error = Some(ClientConnectionOperationError {
            operation: ClientConnectionOperation::Reconnect,
            message: "token=must-not-persist".to_string(),
            retryable: false,
            occurred_at: chrono::Utc::now().to_rfc3339(),
        });
        sink.emit(&failed).unwrap();

        let page = observability
            .query_activity(&ActivityQuery::default())
            .unwrap();
        assert_eq!(page.total, 2);
        assert_eq!(page.items[0].operation, "reconnect_failed");
        assert_eq!(page.items[0].outcome, ActivityOutcome::Failed);
        assert_eq!(page.items[1].operation, "transport_disconnected");
        assert_eq!(page.items[0].correlation_id, page.items[1].correlation_id);
        let fields = page.items[0].fields.as_ref().unwrap();
        assert!(fields["outage_duration_ms"].is_number());
        assert!(!fields.to_string().contains("must-not-persist"));
    }
}
