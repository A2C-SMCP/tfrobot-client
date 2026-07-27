use super::*;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientConnectionAuthoritySnapshot {
    pub present: bool,
    pub revision: u64,
    pub context: Option<ConnectionStateSummary>,
}

impl ClientConnectionAuthoritySnapshot {
    pub(super) fn from_connection(revision: u64, connection: Option<&ConnectionState>) -> Self {
        Self {
            present: connection.is_some(),
            revision,
            context: connection.map(ConnectionStateSummary::from),
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
        let result = self.disconnect_smcp_socketio_inner().await;
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

        if let Err(error) = self.disconnect_smcp_socketio_inner().await {
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
        self.set_client_runtime_diagnostic("reconnect", None).await;

        if self
            .refresh_connection_timestamp_for_generation(generation)
            .await
        {
            return Ok(SmcpReconnectOutcome::Reconnected { expires_in });
        }
        if let Err(error) = self.disconnect_smcp_socketio_inner().await {
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
        self.connection_authority_snapshot().await.context
    }

    /// Reads the client-owned connection authority and its independent monotonic revision while
    /// holding the same lock writers use to publish connection changes.
    pub async fn connection_authority_snapshot(&self) -> ClientConnectionAuthoritySnapshot {
        let connection = self.connection.read().await;
        ClientConnectionAuthoritySnapshot::from_connection(
            self.connection_authority_revision.load(Ordering::Acquire),
            connection.as_ref(),
        )
    }

    pub async fn connection_state_snapshot(&self) -> Option<ConnectionState> {
        self.connection.read().await.clone()
    }

    pub async fn has_connection_state(&self) -> bool {
        self.connection.read().await.is_some()
    }

    pub async fn refresh_connection_timestamp_for_generation(&self, generation: u64) -> bool {
        let mut connection = self.connection.write().await;
        let refreshed = match connection.as_mut() {
            Some(connection) if connection.generation == generation => {
                connection.connected_at = chrono::Utc::now();
                self.advance_connection_authority_revision();
                true
            }
            _ => false,
        };
        drop(connection);
        if refreshed {
            let revision = self.connection_authority_revision.load(Ordering::Acquire);
            self.publish_runtime_status(
                ComputerRuntimeEventCause::ClientConnectionAuthorityChanged {
                    revision,
                    present: true,
                },
            )
            .await;
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
        let revision = self
            .connection_authority_revision
            .fetch_add(1, Ordering::AcqRel)
            + 1;
        drop(connection);
        self.publish_runtime_status(
            ComputerRuntimeEventCause::ClientConnectionAuthorityChanged {
                revision,
                present: true,
            },
        )
        .await;
        Ok(())
    }

    pub async fn take_connection_state(&self) -> Option<ConnectionState> {
        let mut connection = self.connection.write().await;
        let previous = connection.take();
        if previous.is_some() {
            let revision = self
                .connection_authority_revision
                .fetch_add(1, Ordering::AcqRel)
                + 1;
            drop(connection);
            self.publish_runtime_status(
                ComputerRuntimeEventCause::ClientConnectionAuthorityChanged {
                    revision,
                    present: false,
                },
            )
            .await;
        }
        previous
    }

    pub(super) fn advance_connection_authority_revision(&self) {
        self.connection_authority_revision
            .fetch_add(1, Ordering::AcqRel);
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
            if let Err(disconnect_error) = self.disconnect_smcp_socketio_inner().await {
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
        self.computer
            .read()
            .await
            .disconnect_socketio()
            .await
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub(super) async fn has_smcp_transport(&self) -> bool {
        if self.connection.read().await.is_some() {
            return true;
        }
        let socketio_ref = self.computer.read().await.get_socketio_client();
        let has_socket = socketio_ref.read().await.is_some();
        has_socket
    }

    pub(super) async fn clear_smcp_connection_inner(&self) -> Result<(), String> {
        match self.disconnect_smcp_socketio_inner().await {
            Ok(()) => {
                self.abort_refresh_task().await;
                self.take_connection_state().await;
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
            if *current == diagnostic {
                false
            } else {
                *current = diagnostic;
                true
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
