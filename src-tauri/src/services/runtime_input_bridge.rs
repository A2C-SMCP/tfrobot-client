use crate::services::observability::{
    redact_text, ActivityEventDraft, ActivityLevel, ActivityManagedBy, ActivityOutcome,
    ActivityProvider, ActivityTrigger, ComputerActivityCategory, ObservabilityService,
};
use a2c_smcp::smcp_computer::mcp_clients::model::MCPServerInput;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::oneshot;
use uuid::Uuid;

pub const RUNTIME_INPUT_REQUEST_EVENT: &str = "runtime-input:request";

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeInputRequestReason {
    Missing,
    InvalidSelection,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInputRequest {
    pub request_id: String,
    pub instance_id: String,
    pub definition: MCPServerInput,
    pub reason: RuntimeInputRequestReason,
    pub secret: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RuntimeInputCompletion {
    Confirmed { value: String },
    Cancelled,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RuntimeInputCompletionError {
    #[error("Runtime Input request is no longer active")]
    Inactive,
    #[error("Runtime Input resolver did not acknowledge the request")]
    NotAcknowledged,
    #[error("{0}")]
    Rejected(String),
}

impl RuntimeInputCompletionError {
    /// Native completion errors are terminal because ownership is removed from `pending` before
    /// delivery. Only a frontend transport failure (where this command never ran) is retryable.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Inactive => "inactive",
            Self::NotAcknowledged => "not_acknowledged",
            Self::Rejected(_) => "resolver_rejected",
        }
    }
}

pub trait RuntimeInputRequestSink: Send + Sync {
    fn emit(&self, request: &RuntimeInputRequest) -> Result<(), String>;
}

struct BridgeState {
    ready_lease: Option<String>,
    pending: HashMap<String, oneshot::Sender<RuntimeInputResponse>>,
}

impl BridgeState {
    fn new() -> Self {
        Self {
            ready_lease: None,
            pending: HashMap::new(),
        }
    }
}

/// Event-driven rendezvous between a native resolver invocation and the single global UI prompt.
///
/// Requests are serialized before emission so concurrent Computers and start-all operations cannot
/// compete for the modal. The bridge owns no input values; confirmed values are validated and
/// persisted by `RuntimeInputResolver`, which already owns the authoritative InputEntry store.
pub struct RuntimeInputBridge {
    sink: RwLock<Option<Arc<dyn RuntimeInputRequestSink>>>,
    state: Mutex<BridgeState>,
    request_queue: tokio::sync::Mutex<()>,
    observability: RwLock<Option<Arc<ObservabilityService>>>,
}

#[derive(Debug)]
pub struct RuntimeInputResponse {
    pub completion: RuntimeInputCompletion,
    acknowledgement: Option<oneshot::Sender<Result<(), String>>>,
}

impl RuntimeInputResponse {
    pub fn acknowledge(mut self, result: Result<(), String>) {
        if let Some(sender) = self.acknowledgement.take() {
            let _ = sender.send(result);
        }
    }
}

impl Drop for RuntimeInputResponse {
    fn drop(&mut self) {
        if let Some(sender) = self.acknowledgement.take() {
            let _ = sender.send(Err(
                "Runtime Input resolver did not complete the request".to_string()
            ));
        }
    }
}

impl RuntimeInputBridge {
    pub fn new() -> Self {
        Self {
            sink: RwLock::new(None),
            state: Mutex::new(BridgeState::new()),
            request_queue: tokio::sync::Mutex::new(()),
            observability: RwLock::new(None),
        }
    }

    pub fn configure_observability(&self, observability: Arc<ObservabilityService>) {
        *self
            .observability
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(observability);
    }

    pub fn set_sink(&self, sink: Arc<dyn RuntimeInputRequestSink>) {
        *self
            .sink
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(sink);
    }

    pub fn set_ready(&self, lease_id: &str, ready: bool) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if ready {
            if state.ready_lease.as_deref() != Some(lease_id) {
                state.pending.clear();
                state.ready_lease = Some(lease_id.to_string());
            }
        } else if state.ready_lease.as_deref() == Some(lease_id) {
            state.ready_lease = None;
            state.pending.clear();
        }
    }

    pub async fn request(
        self: &Arc<Self>,
        instance_id: &str,
        definition: &MCPServerInput,
        reason: RuntimeInputRequestReason,
        secret: bool,
    ) -> Result<RuntimeInputResponse, String> {
        let requested_at = std::time::Instant::now();
        let _queue_guard = self.request_queue.lock().await;
        let sink = self
            .sink
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .ok_or_else(|| "Runtime Input prompt bridge is unavailable".to_string())?;
        let request_id = Uuid::new_v4().to_string();
        let request = RuntimeInputRequest {
            request_id: request_id.clone(),
            instance_id: instance_id.to_string(),
            definition: definition.clone(),
            reason,
            secret,
        };
        let (sender, receiver) = oneshot::channel();
        {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if state.ready_lease.is_none() {
                return Err("Runtime Input prompt bridge is not ready".to_string());
            }
            state.pending.insert(request_id.clone(), sender);
        }
        let mut pending_guard = PendingRequestGuard::new(self.clone(), request_id.clone());
        if let Err(error) = sink.emit(&request) {
            self.record_request_activity(
                &request,
                "request",
                ActivityOutcome::Failed,
                Some(&error),
                requested_at.elapsed().as_millis(),
            )
            .await;
            return Err(format!(
                "Runtime Input prompt request could not be delivered: {error}"
            ));
        }
        self.record_request_activity(
            &request,
            "request",
            ActivityOutcome::Unknown,
            None,
            requested_at.elapsed().as_millis(),
        )
        .await;
        let completion = match receiver.await {
            Ok(completion) => completion,
            Err(_) => {
                let error = "Runtime Input prompt request was interrupted";
                self.record_request_activity(
                    &request,
                    "complete",
                    ActivityOutcome::Failed,
                    Some(error),
                    requested_at.elapsed().as_millis(),
                )
                .await;
                return Err(error.to_string());
            }
        };
        pending_guard.disarm();
        let (operation, outcome) = match &completion.completion {
            RuntimeInputCompletion::Confirmed { .. } => ("confirmed", ActivityOutcome::Succeeded),
            RuntimeInputCompletion::Cancelled => ("cancelled", ActivityOutcome::Unknown),
        };
        self.record_request_activity(
            &request,
            operation,
            outcome,
            None,
            requested_at.elapsed().as_millis(),
        )
        .await;
        Ok(completion)
    }

    async fn record_request_activity(
        &self,
        request: &RuntimeInputRequest,
        operation: &str,
        outcome: ActivityOutcome,
        error: Option<&str>,
        duration_ms: u128,
    ) {
        let observability = self
            .observability
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let Some(observability) = observability else {
            return;
        };
        let mut activity = ActivityEventDraft::computer(
            &request.instance_id,
            if error.is_some() {
                ActivityLevel::Warn
            } else {
                ActivityLevel::Info
            },
            ComputerActivityCategory::Input,
            "runtime_input_request",
            operation,
            outcome,
            format!("Runtime Input request {operation}"),
        )
        .with_standard_fields(
            ActivityTrigger::Runtime,
            Some(ActivityManagedBy::System),
            Some(ActivityProvider::Client),
        );
        activity.correlation_id = Some(request.request_id.clone());
        activity.merge_fields(serde_json::json!({
            "request_id": request.request_id,
            "input_id": request.definition.id(),
            "input_kind": runtime_input_kind(&request.definition),
            "reason": request.reason,
            "secret": request.secret,
            "value_recorded": false,
            "duration_ms": duration_ms,
            "error": error.map(redact_text),
        }));
        if let Err(error) = observability.record_activity_async(activity).await {
            log::error!("failed to persist Runtime Input activity: {error}");
        }
    }

    pub async fn complete(
        &self,
        request_id: &str,
        completion: RuntimeInputCompletion,
    ) -> Result<(), RuntimeInputCompletionError> {
        let sender = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pending
            .remove(request_id)
            .ok_or(RuntimeInputCompletionError::Inactive)?;
        let (acknowledgement, acknowledged) = oneshot::channel();
        sender
            .send(RuntimeInputResponse {
                completion,
                acknowledgement: Some(acknowledgement),
            })
            .map_err(|_| RuntimeInputCompletionError::Inactive)?;
        acknowledged
            .await
            .map_err(|_| RuntimeInputCompletionError::NotAcknowledged)?
            .map_err(RuntimeInputCompletionError::Rejected)
    }

    fn discard(&self, request_id: &str) {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pending
            .remove(request_id);
    }
}

fn runtime_input_kind(input: &MCPServerInput) -> &'static str {
    match input {
        MCPServerInput::PromptString(_) => "prompt_string",
        MCPServerInput::PickString(_) => "pick_string",
        MCPServerInput::Command(_) => "command",
    }
}

impl Default for RuntimeInputBridge {
    fn default() -> Self {
        Self::new()
    }
}

struct PendingRequestGuard {
    bridge: Arc<RuntimeInputBridge>,
    request_id: String,
    armed: bool,
}

impl PendingRequestGuard {
    fn new(bridge: Arc<RuntimeInputBridge>, request_id: String) -> Self {
        Self {
            bridge,
            request_id,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for PendingRequestGuard {
    fn drop(&mut self) {
        if self.armed {
            self.bridge.discard(&self.request_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a2c_smcp::smcp_computer::mcp_clients::model::PromptStringInput;
    use tokio::sync::mpsc;

    struct RecordingSink {
        sender: mpsc::UnboundedSender<RuntimeInputRequest>,
    }

    impl RuntimeInputRequestSink for RecordingSink {
        fn emit(&self, request: &RuntimeInputRequest) -> Result<(), String> {
            self.sender
                .send(request.clone())
                .map_err(|error| error.to_string())
        }
    }

    struct FailingSink;

    impl RuntimeInputRequestSink for FailingSink {
        fn emit(&self, _request: &RuntimeInputRequest) -> Result<(), String> {
            Err("event channel closed".to_string())
        }
    }

    fn definition(id: &str) -> MCPServerInput {
        MCPServerInput::PromptString(PromptStringInput {
            id: id.to_string(),
            description: "User name".to_string(),
            default: Some("Ada".to_string()),
            password: Some(false),
        })
    }

    #[tokio::test]
    async fn emits_and_completes_one_request() {
        let bridge = Arc::new(RuntimeInputBridge::new());
        let (sender, mut receiver) = mpsc::unbounded_channel();
        bridge.set_sink(Arc::new(RecordingSink { sender }));
        bridge.set_ready("test", true);
        let request_bridge = bridge.clone();
        let task = tokio::spawn(async move {
            request_bridge
                .request(
                    "computer-a",
                    &definition("name"),
                    RuntimeInputRequestReason::Missing,
                    false,
                )
                .await
        });
        let request = receiver.recv().await.unwrap();
        assert_eq!(request.instance_id, "computer-a");
        assert_eq!(request.definition, definition("name"));
        let completion_bridge = bridge.clone();
        let request_id = request.request_id.clone();
        let completion = tokio::spawn(async move {
            completion_bridge
                .complete(
                    &request_id,
                    RuntimeInputCompletion::Confirmed {
                        value: "Grace".to_string(),
                    },
                )
                .await
        });
        let response = task.await.unwrap().unwrap();
        assert_eq!(
            response.completion,
            RuntimeInputCompletion::Confirmed {
                value: "Grace".to_string()
            }
        );
        response.acknowledge(Ok(()));
        completion.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn changing_the_ready_lease_interrupts_pending_requests() {
        let bridge = Arc::new(RuntimeInputBridge::new());
        let (sender, mut receiver) = mpsc::unbounded_channel();
        bridge.set_sink(Arc::new(RecordingSink { sender }));
        bridge.set_ready("first", true);
        let request_bridge = bridge.clone();
        let task = tokio::spawn(async move {
            request_bridge
                .request(
                    "computer-a",
                    &definition("name"),
                    RuntimeInputRequestReason::Missing,
                    false,
                )
                .await
        });
        let request = receiver.recv().await.unwrap();
        bridge.set_ready("second", true);
        assert!(task.await.unwrap().unwrap_err().contains("interrupted"));
        assert!(bridge
            .complete(&request.request_id, RuntimeInputCompletion::Cancelled)
            .await
            .is_err_and(|error| error == RuntimeInputCompletionError::Inactive));
    }

    #[tokio::test]
    async fn serializes_requests_until_the_prior_resolver_acknowledges() {
        let bridge = Arc::new(RuntimeInputBridge::new());
        let (sender, mut receiver) = mpsc::unbounded_channel();
        bridge.set_sink(Arc::new(RecordingSink { sender }));
        bridge.set_ready("test", true);

        let first_bridge = bridge.clone();
        let first = tokio::spawn(async move {
            first_bridge
                .request(
                    "computer-a",
                    &definition("first"),
                    RuntimeInputRequestReason::Missing,
                    false,
                )
                .await
        });
        let first_request = receiver.recv().await.unwrap();
        let second_bridge = bridge.clone();
        let second = tokio::spawn(async move {
            second_bridge
                .request(
                    "computer-b",
                    &definition("second"),
                    RuntimeInputRequestReason::Missing,
                    false,
                )
                .await
        });
        assert!(receiver.try_recv().is_err());

        let completion_bridge = bridge.clone();
        let first_request_id = first_request.request_id;
        let first_completion = tokio::spawn(async move {
            completion_bridge
                .complete(&first_request_id, RuntimeInputCompletion::Cancelled)
                .await
        });
        first.await.unwrap().unwrap().acknowledge(Ok(()));
        first_completion.await.unwrap().unwrap();

        let second_request = receiver.recv().await.unwrap();
        assert_eq!(second_request.instance_id, "computer-b");
        let completion_bridge = bridge.clone();
        let second_request_id = second_request.request_id;
        let second_completion = tokio::spawn(async move {
            completion_bridge
                .complete(&second_request_id, RuntimeInputCompletion::Cancelled)
                .await
        });
        second.await.unwrap().unwrap().acknowledge(Ok(()));
        second_completion.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn discards_pending_state_when_event_delivery_fails() {
        let bridge = Arc::new(RuntimeInputBridge::new());
        bridge.set_sink(Arc::new(FailingSink));
        bridge.set_ready("test", true);

        let error = bridge
            .request(
                "computer-a",
                &definition("name"),
                RuntimeInputRequestReason::Missing,
                false,
            )
            .await
            .unwrap_err();

        assert!(error.contains("could not be delivered"));
        assert!(bridge
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pending
            .is_empty());
    }

    #[tokio::test]
    async fn reports_terminal_acknowledgement_failures() {
        let bridge = Arc::new(RuntimeInputBridge::new());
        let (sender, mut receiver) = mpsc::unbounded_channel();
        bridge.set_sink(Arc::new(RecordingSink { sender }));
        bridge.set_ready("test", true);
        let request_bridge = bridge.clone();
        let request = tokio::spawn(async move {
            request_bridge
                .request(
                    "computer-a",
                    &definition("name"),
                    RuntimeInputRequestReason::Missing,
                    false,
                )
                .await
        });
        let emitted = receiver.recv().await.unwrap();
        let completion_bridge = bridge.clone();
        let completion = tokio::spawn(async move {
            completion_bridge
                .complete(&emitted.request_id, RuntimeInputCompletion::Cancelled)
                .await
        });
        drop(request.await.unwrap().unwrap());

        assert_eq!(
            completion.await.unwrap().unwrap_err(),
            RuntimeInputCompletionError::Rejected(
                "Runtime Input resolver did not complete the request".to_string()
            )
        );
    }
}
