use super::*;
use std::time::Duration;
use tokio::time::timeout;

const SMCP_TRANSPORT_TEARDOWN_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionStateSummary {
    pub url: String,
    pub office_id: String,
    pub computer_name: String,
    pub connected_at: String,
    pub profile_name: String,
    pub source_type: String,
    pub target_id: Option<String>,
    pub target_name: Option<String>,
    pub employee_id: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClientConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Disconnecting,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClientConnectionOperation {
    Connect,
    Disconnect,
    Reconnect,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientConnectionOperationTarget {
    pub source_type: String,
    pub target_id: Option<String>,
    pub employee_id: Option<u64>,
}

impl From<&ConnectionState> for ClientConnectionOperationTarget {
    fn from(connection: &ConnectionState) -> Self {
        Self {
            source_type: connection.source_type.clone(),
            target_id: connection.target_id.clone(),
            employee_id: connection.employee_id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientConnectionOperationToken {
    operation: ClientConnectionOperation,
    epoch: u64,
    runtime_generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientConnectionOperationError {
    pub operation: ClientConnectionOperation,
    pub message: String,
    pub retryable: bool,
    pub occurred_at: String,
}

impl ClientConnectionOperationError {
    fn new(operation: ClientConnectionOperation, message: String, retryable: bool) -> Self {
        Self {
            operation,
            message,
            retryable,
            occurred_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    fn replace_current(
        current: &mut Option<Self>,
        operation: ClientConnectionOperation,
        message: String,
        retryable: bool,
    ) {
        match current.as_mut() {
            Some(error) if error.operation == operation => {
                error.message = message;
                error.retryable = retryable;
            }
            _ => {
                *current = Some(Self::new(operation, message, retryable));
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClientConnectionActionDisabledReason {
    AlreadyConnected,
    NotConnected,
    TransitionInProgress,
    NotRunning,
    ConnectionUnavailable,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientConnectionActionCapability {
    pub enabled: bool,
    pub disabled_reason: Option<ClientConnectionActionDisabledReason>,
}

impl ClientConnectionActionCapability {
    fn enabled() -> Self {
        Self {
            enabled: true,
            disabled_reason: None,
        }
    }

    fn disabled(reason: ClientConnectionActionDisabledReason) -> Self {
        Self {
            enabled: false,
            disabled_reason: Some(reason),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientConnectionActionCapabilities {
    pub connect: ClientConnectionActionCapability,
    pub disconnect: ClientConnectionActionCapability,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientConnectionStateSnapshot {
    pub status: ClientConnectionStatus,
    pub present: bool,
    pub revision: u64,
    pub context: Option<ConnectionStateSummary>,
    pub operation: Option<ClientConnectionOperation>,
    pub operation_target: Option<ClientConnectionOperationTarget>,
    pub last_error: Option<ClientConnectionOperationError>,
    pub actions: ClientConnectionActionCapabilities,
}

#[derive(Debug, Clone, Default)]
pub(super) struct ClientConnectionOperationState {
    operation: Option<ClientConnectionOperation>,
    operation_target: Option<ClientConnectionOperationTarget>,
    generation: Option<u64>,
    last_error: Option<ClientConnectionOperationError>,
    epoch: u64,
}

impl ClientConnectionOperationState {
    pub(super) fn last_error(&self) -> Option<ClientConnectionOperationError> {
        self.last_error.clone()
    }
}

impl ClientConnectionStateSnapshot {
    pub(super) fn from_parts(
        revision: u64,
        connection: Option<&ConnectionState>,
        operation: &ClientConnectionOperationState,
        runtime_state: ComputerRuntimeState,
    ) -> Self {
        let status = match operation.operation {
            Some(ClientConnectionOperation::Connect | ClientConnectionOperation::Reconnect) => {
                ClientConnectionStatus::Connecting
            }
            Some(ClientConnectionOperation::Disconnect) => ClientConnectionStatus::Disconnecting,
            None if connection.is_some() => ClientConnectionStatus::Connected,
            None => ClientConnectionStatus::Disconnected,
        };
        let transition_disabled = ClientConnectionActionCapability::disabled(
            ClientConnectionActionDisabledReason::TransitionInProgress,
        );
        let actions = match status {
            ClientConnectionStatus::Connecting | ClientConnectionStatus::Disconnecting => {
                ClientConnectionActionCapabilities {
                    connect: transition_disabled,
                    disconnect: transition_disabled,
                }
            }
            ClientConnectionStatus::Connected => ClientConnectionActionCapabilities {
                connect: ClientConnectionActionCapability::disabled(
                    ClientConnectionActionDisabledReason::AlreadyConnected,
                ),
                disconnect: ClientConnectionActionCapability::enabled(),
            },
            ClientConnectionStatus::Disconnected => {
                let runtime_actions =
                    ComputerRuntimeActionCapabilities::for_lifecycle(runtime_state);
                let runtime_connect = runtime_actions.connect;
                let connect = if runtime_connect.enabled {
                    ClientConnectionActionCapability::enabled()
                } else {
                    let reason = match runtime_connect.disabled_reason {
                        Some(ComputerRuntimeActionDisabledReason::NotRunning) => {
                            ClientConnectionActionDisabledReason::NotRunning
                        }
                        Some(ComputerRuntimeActionDisabledReason::TransitionInProgress) => {
                            ClientConnectionActionDisabledReason::TransitionInProgress
                        }
                        _ => ClientConnectionActionDisabledReason::ConnectionUnavailable,
                    };
                    ClientConnectionActionCapability::disabled(reason)
                };
                // A failed disconnect may leave an SDK transport without client authority.
                // Keep disconnect actionable until that orphan transport is actually gone.
                let disconnect = if runtime_actions.disconnect.enabled {
                    ClientConnectionActionCapability::enabled()
                } else {
                    ClientConnectionActionCapability::disabled(
                        ClientConnectionActionDisabledReason::NotConnected,
                    )
                };
                ClientConnectionActionCapabilities {
                    connect,
                    disconnect,
                }
            }
        };
        Self {
            status,
            present: connection.is_some(),
            revision,
            context: connection.map(ConnectionStateSummary::from),
            operation: operation.operation,
            operation_target: operation.operation_target.clone(),
            last_error: operation.last_error.clone(),
            actions,
        }
    }
}

impl From<&ConnectionState> for ConnectionStateSummary {
    fn from(connection: &ConnectionState) -> Self {
        Self {
            url: connection.url.clone(),
            office_id: connection.office_id.clone(),
            computer_name: connection.computer_name.clone(),
            connected_at: connection.connected_at.to_rfc3339(),
            profile_name: connection.profile_name.clone(),
            source_type: connection.source_type.clone(),
            target_id: connection.target_id.clone(),
            target_name: connection.target_name.clone(),
            employee_id: connection.employee_id,
        }
    }
}

impl ComputerInstanceRuntime {
    #[allow(clippy::too_many_arguments)]
    pub async fn connect_and_install_smcp_socketio(
        &self,
        token: ClientConnectionOperationToken,
        url: &str,
        auth_payload: Option<serde_json::Value>,
        headers: HashMap<String, String>,
        namespace: Option<String>,
        office_id: &str,
        computer_name: &str,
        connection_state: ConnectionState,
    ) -> Result<(), String> {
        let _guard = self.lifecycle_lock.lock().await;
        self.ensure_connection_operation(token).await?;
        if let Err(error) = self
            .connect_smcp_socketio_inner(
                url,
                auth_payload,
                headers,
                namespace,
                office_id,
                computer_name,
            )
            .await
        {
            self.set_client_runtime_diagnostic(
                "connect",
                Some("SMCP connection failed; see logs for details".to_string()),
            )
            .await;
            return Err(error);
        }
        if let Err(error) = self.ensure_connection_operation(token).await {
            let _ = self.disconnect_smcp_socketio_bounded_inner().await;
            return Err(error);
        }
        if let Err(error) = self.install_connection_state(connection_state).await {
            let _ = self.disconnect_smcp_socketio_bounded_inner().await;
            return Err(error);
        }
        self.set_client_runtime_diagnostic("connect", None).await;
        Ok(())
    }

    pub async fn connect_smcp_socketio(
        &self,
        url: &str,
        auth_payload: Option<serde_json::Value>,
        headers: HashMap<String, String>,
        namespace: Option<String>,
        office_id: &str,
        computer_name: &str,
    ) -> Result<(), String> {
        let _guard = self.lifecycle_lock.lock().await;
        self.ensure_active()?;
        let result = self
            .connect_smcp_socketio_inner(
                url,
                auth_payload,
                headers,
                namespace,
                office_id,
                computer_name,
            )
            .await;
        match result {
            Ok(()) => {
                self.set_client_runtime_diagnostic("connect", None).await;
                Ok(())
            }
            Err(error) => {
                self.set_client_runtime_diagnostic(
                    "connect",
                    Some("SMCP connection failed; see logs for details".to_string()),
                )
                .await;
                Err(error)
            }
        }
    }

    pub async fn disconnect_smcp_socketio(&self) -> Result<(), String> {
        let _guard = self.lifecycle_lock.lock().await;
        self.ensure_active()?;
        let result = self.disconnect_smcp_socketio_bounded_inner().await;
        if result.is_ok() {
            self.set_client_runtime_diagnostic("disconnect", None).await;
        }
        result
    }

    pub async fn clear_smcp_connection(&self) -> Result<(), String> {
        let _guard = self.lifecycle_lock.lock().await;
        self.ensure_active()?;
        let result = self.clear_smcp_connection_inner().await;
        if result.is_ok() {
            self.set_client_runtime_diagnostic("disconnect", None).await;
        }
        result
    }

    pub async fn clear_smcp_connection_for_operation(
        &self,
        token: ClientConnectionOperationToken,
    ) -> Result<bool, String> {
        let _guard = self.lifecycle_lock.lock().await;
        if self.ensure_connection_operation(token).await.is_err() {
            return Ok(false);
        }
        if self.has_smcp_transport().await {
            self.clear_smcp_connection_inner().await?;
        }
        Ok(true)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn reconnect_smcp_socketio_for_generation(
        &self,
        generation: u64,
        url: &str,
        auth_payload: Option<serde_json::Value>,
        headers: HashMap<String, String>,
        namespace: Option<String>,
        office_id: &str,
        computer_name: &str,
        expires_in: i64,
    ) -> Result<SmcpReconnectOutcome, String> {
        let _guard = self.lifecycle_lock.lock().await;
        self.ensure_active()?;
        {
            let guard = self.connection.read().await;
            match guard.as_ref() {
                Some(connection) if connection.generation == generation => {}
                _ => return Ok(SmcpReconnectOutcome::Stale),
            }
        }

        if let Err(error) = self.disconnect_smcp_socketio_bounded_inner().await {
            self.set_client_runtime_diagnostic(
                "reconnect",
                Some("SMCP reconnect failed; retrying".to_string()),
            )
            .await;
            return Err(error);
        }

        if let Err(error) = self
            .connect_smcp_socketio_inner(
                url,
                auth_payload,
                headers,
                namespace,
                office_id,
                computer_name,
            )
            .await
        {
            self.set_client_runtime_diagnostic(
                "reconnect",
                Some("SMCP reconnect failed; retrying".to_string()),
            )
            .await;
            return Err(error);
        }
        if self
            .refresh_connection_timestamp_for_generation(generation)
            .await
        {
            return Ok(SmcpReconnectOutcome::Reconnected { expires_in });
        }
        if let Err(error) = self.disconnect_smcp_socketio_bounded_inner().await {
            self.set_client_runtime_diagnostic(
                "reconnect",
                Some("SMCP reconnect failed; retrying".to_string()),
            )
            .await;
            return Err(error);
        }
        Ok(SmcpReconnectOutcome::Stale)
    }

    pub async fn set_refresh_task(&self, task: tokio::task::JoinHandle<()>) {
        if self.is_retired() {
            task.abort();
            return;
        }
        let mut lock = self.refresh_task.lock().await;
        if self.is_retired() {
            task.abort();
            return;
        }
        if let Some(previous) = lock.replace(task) {
            previous.abort();
        }
    }

    pub async fn set_refresh_task_for_generation(
        &self,
        generation: u64,
        task: tokio::task::JoinHandle<()>,
    ) -> Result<(), String> {
        let _guard = self.lifecycle_lock.lock().await;
        if self
            .connection
            .read()
            .await
            .as_ref()
            .is_none_or(|connection| connection.generation != generation)
        {
            task.abort();
            return Err("Connection was replaced before refresh ownership was installed".into());
        }
        self.set_refresh_task(task).await;
        Ok(())
    }

    pub async fn abort_refresh_task(&self) {
        if let Some(task) = self.refresh_task.lock().await.take() {
            task.abort();
        }
    }

    pub async fn is_connected(&self) -> bool {
        self.runtime_state().await == ComputerRuntimeState::JoinedOffice
            && self.connection.read().await.is_some()
    }

    pub async fn connection_status(&self) -> Option<ConnectionStateSummary> {
        if self.runtime_state().await != ComputerRuntimeState::JoinedOffice {
            return None;
        }
        self.connection_context().await
    }

    /// Returns the client-owned logical connection context independently of SDK transport state.
    /// During token refresh the SDK temporarily leaves JoinedOffice while this context remains the
    /// authority that allows a later JoinedOffice event to restore business connectivity.
    pub async fn connection_context(&self) -> Option<ConnectionStateSummary> {
        self.connection_snapshot().await.context
    }

    /// Returns the single client-owned, versioned connection projection consumed by commands,
    /// runtime events, resync and UI stores.
    pub async fn connection_snapshot(&self) -> ClientConnectionStateSnapshot {
        let runtime_state = self.runtime_state().await;
        let connection = self.connection.read().await;
        let operation = self.connection_operation.read().await;
        ClientConnectionStateSnapshot::from_parts(
            self.connection_authority_revision.load(Ordering::Acquire),
            connection.as_ref(),
            &operation,
            runtime_state,
        )
    }

    pub async fn connection_state_snapshot(&self) -> Option<ConnectionState> {
        self.connection.read().await.clone()
    }

    pub async fn has_connection_state(&self) -> bool {
        self.connection.read().await.is_some()
    }

    async fn publish_connection_state(&self) {
        let snapshot = self.connection_snapshot().await;
        self.publish_runtime_status(ComputerRuntimeEventCause::ClientConnectionStateChanged {
            revision: snapshot.revision,
            status: snapshot.status,
        })
        .await;
    }

    fn advance_connection_revision(&self) -> u64 {
        self.connection_authority_revision
            .fetch_add(1, Ordering::AcqRel)
            + 1
    }

    pub async fn begin_connection_operation(
        &self,
        operation: ClientConnectionOperation,
        operation_target: Option<ClientConnectionOperationTarget>,
    ) -> Result<ClientConnectionOperationToken, String> {
        let mut state = self.connection_operation.write().await;
        if state.operation.is_some() {
            return Err("A connection operation is already in progress".to_string());
        }
        state.epoch = state.epoch.wrapping_add(1);
        state.operation = Some(operation);
        state.operation_target = operation_target;
        state.generation = None;
        let token = ClientConnectionOperationToken {
            operation,
            epoch: state.epoch,
            runtime_generation: self.runtime_generation(),
        };
        self.advance_connection_revision();
        drop(state);
        self.publish_connection_state().await;
        Ok(token)
    }

    pub async fn ensure_connection_operation(
        &self,
        token: ClientConnectionOperationToken,
    ) -> Result<(), String> {
        let state = self.connection_operation.read().await;
        if self.runtime_generation() != token.runtime_generation
            || state.operation != Some(token.operation)
            || state.epoch != token.epoch
        {
            return Err("Connection operation was superseded by a runtime lifecycle change".into());
        }
        self.ensure_active()
    }

    pub async fn begin_reconnect_for_generation(&self, generation: u64) -> bool {
        let connection = self.connection.read().await;
        if connection
            .as_ref()
            .is_none_or(|connection| connection.generation != generation)
        {
            return false;
        }
        let mut state = self.connection_operation.write().await;
        match (state.operation, state.generation) {
            (None, _) => {
                state.operation = Some(ClientConnectionOperation::Reconnect);
                state.operation_target = connection
                    .as_ref()
                    .map(ClientConnectionOperationTarget::from);
                state.generation = Some(generation);
                state.last_error = None;
            }
            (Some(ClientConnectionOperation::Reconnect), Some(active)) if active == generation => {
                return true;
            }
            _ => return false,
        }
        self.advance_connection_revision();
        drop(state);
        drop(connection);
        self.publish_connection_state().await;
        true
    }

    pub async fn complete_connection_operation(&self) {
        let mut state = self.connection_operation.write().await;
        if state.operation.is_none() && state.last_error.is_none() {
            return;
        }
        state.operation = None;
        state.operation_target = None;
        state.generation = None;
        state.last_error = None;
        self.advance_connection_revision();
        drop(state);
        self.publish_connection_state().await;
    }

    pub async fn complete_connection_operation_for_token(
        &self,
        token: ClientConnectionOperationToken,
    ) -> bool {
        let mut state = self.connection_operation.write().await;
        if self.runtime_generation() != token.runtime_generation
            || state.operation != Some(token.operation)
            || state.epoch != token.epoch
        {
            return false;
        }
        state.operation = None;
        state.operation_target = None;
        state.generation = None;
        state.last_error = None;
        self.advance_connection_revision();
        drop(state);
        self.publish_connection_state().await;
        true
    }

    pub async fn fail_connection_operation(
        &self,
        operation: ClientConnectionOperation,
        message: String,
        retryable: bool,
    ) {
        let mut connection = self.connection.write().await;
        connection.take();
        let mut state = self.connection_operation.write().await;
        state.operation = None;
        state.operation_target = None;
        state.generation = None;
        ClientConnectionOperationError::replace_current(
            &mut state.last_error,
            operation,
            message,
            retryable,
        );
        self.advance_connection_revision();
        drop(state);
        drop(connection);
        self.publish_connection_state().await;
    }

    pub async fn fail_connection_operation_for_token(
        &self,
        token: ClientConnectionOperationToken,
        message: String,
        retryable: bool,
    ) -> bool {
        let mut connection = self.connection.write().await;
        let mut state = self.connection_operation.write().await;
        if self.runtime_generation() != token.runtime_generation
            || state.operation != Some(token.operation)
            || state.epoch != token.epoch
        {
            return false;
        }
        connection.take();
        state.operation = None;
        state.operation_target = None;
        state.generation = None;
        ClientConnectionOperationError::replace_current(
            &mut state.last_error,
            token.operation,
            message,
            retryable,
        );
        self.advance_connection_revision();
        drop(state);
        drop(connection);
        self.publish_connection_state().await;
        true
    }

    pub async fn cancel_connection_operation_for_handle_replacement(&self) {
        let mut state = self.connection_operation.write().await;
        if state.operation.is_none() {
            return;
        }
        state.operation = None;
        state.operation_target = None;
        state.generation = None;
        self.advance_connection_revision();
        drop(state);
        self.publish_connection_state().await;
    }

    pub(super) async fn clear_connection_diagnostic_for_handle_replacement_silent(&self) {
        let mut state = self.connection_operation.write().await;
        if state.last_error.take().is_some() {
            self.advance_connection_revision();
        }
    }

    pub async fn reconcile_disconnect_failure(&self, message: String) {
        let mut connection = self.connection.write().await;
        // The SDK does not expose a transport-liveness signal. A retained client slot and stale
        // lifecycle therefore cannot prove that the remote connection is usable after teardown
        // failed. Fail closed and leave any retained slot discoverable through orphan cleanup.
        connection.take();
        let mut state = self.connection_operation.write().await;
        state.operation = None;
        state.operation_target = None;
        state.generation = None;
        ClientConnectionOperationError::replace_current(
            &mut state.last_error,
            ClientConnectionOperation::Disconnect,
            message,
            true,
        );
        self.advance_connection_revision();
        drop(state);
        drop(connection);
        self.publish_connection_state().await;
    }

    pub async fn reconcile_disconnect_failure_for_token(
        &self,
        token: ClientConnectionOperationToken,
        message: String,
    ) -> bool {
        let mut connection = self.connection.write().await;
        let mut state = self.connection_operation.write().await;
        if self.runtime_generation() != token.runtime_generation
            || token.operation != ClientConnectionOperation::Disconnect
            || state.operation != Some(token.operation)
            || state.epoch != token.epoch
        {
            return false;
        }
        // See `reconcile_disconnect_failure`: without positive liveness evidence, preserving
        // authority would project a false connected state after a remote transport failure.
        connection.take();
        state.operation = None;
        state.operation_target = None;
        state.generation = None;
        ClientConnectionOperationError::replace_current(
            &mut state.last_error,
            ClientConnectionOperation::Disconnect,
            message,
            true,
        );
        self.advance_connection_revision();
        drop(state);
        drop(connection);
        self.publish_connection_state().await;
        true
    }

    pub async fn record_reconnect_retry(&self, generation: u64, message: String) -> bool {
        let connection = self.connection.read().await;
        if connection
            .as_ref()
            .is_none_or(|connection| connection.generation != generation)
        {
            return false;
        }
        let mut state = self.connection_operation.write().await;
        if state.operation != Some(ClientConnectionOperation::Reconnect)
            || state.generation != Some(generation)
        {
            return false;
        }
        ClientConnectionOperationError::replace_current(
            &mut state.last_error,
            ClientConnectionOperation::Reconnect,
            message,
            true,
        );
        self.advance_connection_revision();
        drop(state);
        drop(connection);
        self.publish_connection_state().await;
        true
    }

    pub async fn complete_reconnect_for_generation(&self, generation: u64) -> bool {
        let connection = self.connection.read().await;
        if connection
            .as_ref()
            .is_none_or(|connection| connection.generation != generation)
        {
            return false;
        }
        // Keep the low-level diagnostic and the connection-state error coherent for snapshot
        // readers. The product projection acquires these locks in the same order.
        let mut diagnostic = self.client_runtime_diagnostic.write().await;
        let mut state = self.connection_operation.write().await;
        if state.operation != Some(ClientConnectionOperation::Reconnect)
            || state.generation != Some(generation)
        {
            return false;
        }
        let diagnostic_cleared = diagnostic
            .as_ref()
            .is_some_and(|value| value.operation == "reconnect");
        if diagnostic_cleared {
            *diagnostic = None;
        }
        state.operation = None;
        state.operation_target = None;
        state.generation = None;
        state.last_error = None;
        self.advance_connection_revision();
        drop(state);
        drop(diagnostic);
        drop(connection);
        self.publish_connection_state().await;
        if diagnostic_cleared {
            self.publish_runtime_status(ComputerRuntimeEventCause::ClientDiagnosticChanged {
                operation: "reconnect".to_string(),
                has_error: false,
            })
            .await;
        }
        true
    }

    pub async fn fail_reconnect_for_generation(
        &self,
        generation: u64,
        message: String,
        retryable: bool,
    ) -> bool {
        let mut connection = self.connection.write().await;
        if connection
            .as_ref()
            .is_none_or(|connection| connection.generation != generation)
        {
            return false;
        }
        let mut state = self.connection_operation.write().await;
        if state.operation != Some(ClientConnectionOperation::Reconnect)
            || state.generation != Some(generation)
        {
            return false;
        }
        connection.take();
        state.operation = None;
        state.operation_target = None;
        state.generation = None;
        ClientConnectionOperationError::replace_current(
            &mut state.last_error,
            ClientConnectionOperation::Reconnect,
            message,
            retryable,
        );
        self.advance_connection_revision();
        drop(state);
        drop(connection);
        self.publish_connection_state().await;
        true
    }

    /// Closes the transport owned by a failed refresh generation and then settles its logical
    /// authority. This intentionally does not abort `refresh_task`: it is called from that task's
    /// own terminal branch.
    pub async fn terminate_reconnect_for_generation(
        &self,
        generation: u64,
        message: String,
        retryable: bool,
    ) -> bool {
        let _guard = self.lifecycle_lock.lock().await;
        {
            let connection = self.connection.read().await;
            let operation = self.connection_operation.read().await;
            if connection
                .as_ref()
                .is_none_or(|connection| connection.generation != generation)
                || operation.operation != Some(ClientConnectionOperation::Reconnect)
                || operation.generation != Some(generation)
            {
                return false;
            }
        }

        let cleanup_error = self.disconnect_smcp_socketio_bounded_inner().await.err();
        let mut connection = self.connection.write().await;
        let mut operation = self.connection_operation.write().await;
        if connection
            .as_ref()
            .is_none_or(|connection| connection.generation != generation)
            || operation.operation != Some(ClientConnectionOperation::Reconnect)
            || operation.generation != Some(generation)
        {
            return false;
        }
        connection.take();
        operation.operation = None;
        operation.operation_target = None;
        operation.generation = None;
        ClientConnectionOperationError::replace_current(
            &mut operation.last_error,
            ClientConnectionOperation::Reconnect,
            match cleanup_error {
                Some(cleanup_error) => {
                    format!("{message}; failed to close stale SMCP transport: {cleanup_error}")
                }
                None => message,
            },
            retryable,
        );
        self.advance_connection_revision();
        drop(operation);
        drop(connection);
        self.publish_connection_state().await;
        true
    }

    /// Settles only the reconnect operation owned by `generation`.
    ///
    /// This is used by every non-success terminal path (stale generation, runtime handle
    /// replacement and task teardown) so an aborted refresh cannot leave the client projection
    /// permanently in `connecting`. A newer connection generation is never touched.
    pub async fn abort_reconnect_for_generation(&self, generation: u64) -> bool {
        let mut state = self.connection_operation.write().await;
        if state.operation != Some(ClientConnectionOperation::Reconnect)
            || state.generation != Some(generation)
        {
            return false;
        }
        state.operation = None;
        state.operation_target = None;
        state.generation = None;
        self.advance_connection_revision();
        drop(state);
        self.publish_connection_state().await;
        true
    }

    pub async fn refresh_connection_timestamp_for_generation(&self, generation: u64) -> bool {
        let mut connection = self.connection.write().await;
        let refreshed = match connection.as_mut() {
            Some(connection) if connection.generation == generation => {
                connection.connected_at = chrono::Utc::now();
                self.advance_connection_revision();
                true
            }
            _ => false,
        };
        drop(connection);
        if refreshed {
            self.publish_connection_state().await;
        }
        refreshed
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn connection_handle_for_test(&self) -> Arc<RwLock<Option<ConnectionState>>> {
        self.connection.clone()
    }

    pub async fn install_connection_state(&self, state: ConnectionState) -> Result<(), String> {
        let mut connection = self.connection.write().await;
        if connection.is_some() {
            return Err(
                "Computer instance already has a connection snapshot; disconnect before reconnecting"
                    .to_string(),
            );
        }
        *connection = Some(state);
        // Installing new connection authority is the product-level recovery point for any
        // previous connect/reconnect transport diagnostic, even when the recovery operation has a
        // different name (for example a manual Connect after automatic reconnect exhaustion).
        let mut diagnostic = self.client_runtime_diagnostic.write().await;
        let mut operation = self.connection_operation.write().await;
        let cleared_diagnostic_operation = diagnostic.take().map(|diagnostic| diagnostic.operation);
        operation.last_error = None;
        self.advance_connection_revision();
        drop(operation);
        drop(diagnostic);
        drop(connection);
        self.publish_connection_state().await;
        if let Some(operation) = cleared_diagnostic_operation {
            self.publish_runtime_status(ComputerRuntimeEventCause::ClientDiagnosticChanged {
                operation,
                has_error: false,
            })
            .await;
        }
        Ok(())
    }

    pub async fn take_connection_state(&self) -> Option<ConnectionState> {
        let mut connection = self.connection.write().await;
        let previous = connection.take();
        if previous.is_some() {
            self.advance_connection_revision();
            drop(connection);
            self.publish_connection_state().await;
        }
        previous
    }

    pub(super) async fn connect_smcp_socketio_inner(
        &self,
        url: &str,
        auth_payload: Option<serde_json::Value>,
        headers: HashMap<String, String>,
        namespace: Option<String>,
        office_id: &str,
        computer_name: &str,
    ) -> Result<(), String> {
        let options = ConnectOptions {
            auth_payload,
            headers: headers_to_connect_options(headers),
            namespace: namespace.unwrap_or_default(),
        };
        let options = if options.namespace.trim().is_empty() {
            ConnectOptions {
                namespace: ConnectOptions::default().namespace,
                ..options
            }
        } else {
            options
        };

        if let Err(error) = self
            .computer
            .read()
            .await
            .connect_socketio(url, options)
            .await
        {
            return Err(error.to_string());
        }
        if let Err(error) = self
            .computer
            .read()
            .await
            .join_office(office_id, computer_name)
            .await
        {
            if let Err(disconnect_error) = self.disconnect_smcp_socketio_bounded_inner().await {
                log::warn!(
                    "Failed to disconnect Socket.IO after join_office failure for instance {}: {}",
                    self.instance.id,
                    disconnect_error
                );
            }
            return Err(error.to_string());
        }
        Ok(())
    }

    pub(super) async fn disconnect_smcp_socketio_inner(&self) -> Result<(), String> {
        if let Err(error) = self.computer.read().await.leave_office().await {
            log::warn!(
                "Failed to leave SMCP office for instance {}: {}",
                self.instance.id,
                error
            );
        }
        #[cfg(debug_assertions)]
        if self.hang_smcp_disconnect_once.swap(false, Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        #[cfg(debug_assertions)]
        if self.fail_smcp_disconnect_once.swap(false, Ordering::SeqCst) {
            return Err("Injected Socket.IO disconnect failure".to_string());
        }
        self.computer
            .read()
            .await
            .disconnect_socketio()
            .await
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub(super) async fn disconnect_smcp_socketio_bounded_inner(&self) -> Result<(), String> {
        match timeout(
            SMCP_TRANSPORT_TEARDOWN_TIMEOUT,
            self.disconnect_smcp_socketio_inner(),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err(format!(
                "Timed out disconnecting SMCP socket after {:?}",
                SMCP_TRANSPORT_TEARDOWN_TIMEOUT
            )),
        }
    }

    pub(crate) async fn has_smcp_transport(&self) -> bool {
        if self.connection.read().await.is_some() {
            return true;
        }
        let socketio_ref = self.computer.read().await.get_socketio_client();
        let has_socket = socketio_ref.read().await.is_some();
        has_socket
    }

    pub(super) async fn clear_smcp_connection_inner(&self) -> Result<(), String> {
        match self.disconnect_smcp_socketio_bounded_inner().await {
            Ok(()) => {
                self.abort_refresh_task().await;
                let mut connection = self.connection.write().await;
                let generation = connection.as_ref().map(|state| state.generation);
                let connection_changed = connection.take().is_some();
                let mut operation = self.connection_operation.write().await;
                let reconnect_changed = generation.is_some_and(|generation| {
                    operation.operation == Some(ClientConnectionOperation::Reconnect)
                        && operation.generation == Some(generation)
                });
                if reconnect_changed {
                    operation.operation = None;
                    operation.operation_target = None;
                    operation.generation = None;
                }
                if connection_changed || reconnect_changed {
                    self.advance_connection_revision();
                }
                drop(operation);
                drop(connection);
                if connection_changed || reconnect_changed {
                    self.publish_connection_state().await;
                }
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    pub(super) async fn set_client_runtime_diagnostic(
        &self,
        operation: &str,
        diagnostic: Option<String>,
    ) {
        let has_error = diagnostic.is_some();
        let changed = {
            let mut current = self.client_runtime_diagnostic.write().await;
            match diagnostic {
                Some(message)
                    if current.as_ref().is_some_and(|value| {
                        value.operation == operation && value.message == message
                    }) =>
                {
                    false
                }
                Some(message) => {
                    *current = Some(
                        RuntimeDiagnosticRecord::new(operation, message)
                            .preserve_occurrence_from(current.as_ref()),
                    );
                    true
                }
                None if current
                    .as_ref()
                    .is_none_or(|value| value.operation != operation) =>
                {
                    false
                }
                None => {
                    *current = None;
                    true
                }
            }
        };
        if changed {
            self.publish_runtime_status(ComputerRuntimeEventCause::ClientDiagnosticChanged {
                operation: operation.to_string(),
                has_error,
            })
            .await;
        }
    }

    pub(super) async fn clear_client_runtime_diagnostic_silent(&self) {
        self.client_runtime_diagnostic.write().await.take();
    }
}
