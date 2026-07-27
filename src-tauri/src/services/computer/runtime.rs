use super::*;

impl ComputerInstanceRuntime {
    pub async fn start(&self) -> Result<(), ComputerRuntimeStartError> {
        let _guard = self.lifecycle_lock.lock().await;
        self.ensure_active()
            .map_err(ComputerRuntimeStartError::Client)?;
        let lifecycle = self.runtime_state().await;
        if self.sdk_config_reload_required.load(Ordering::Acquire)
            || matches!(
                lifecycle,
                LifecycleState::Stopped | LifecycleState::Shutdown | LifecycleState::Error
            )
        {
            self.replace_sdk_computer(false, "start_after_terminal")
                .await?;
        } else if matches!(
            lifecycle,
            LifecycleState::Starting
                | LifecycleState::Started
                | LifecycleState::Connecting
                | LifecycleState::Connected
                | LifecycleState::JoinedOffice
                | LifecycleState::Syncing
                | LifecycleState::Degraded
                | LifecycleState::Disconnecting
                | LifecycleState::Stopping
        ) {
            return Ok(());
        }
        self.start_runtime_event_relay().await;

        if let Err(error) = self.computer.read().await.boot_up().await {
            return Err(ComputerRuntimeStartError::Sdk(error));
        }
        self.reconcile_sdk_governance_inner()
            .await
            .map_err(ComputerRuntimeStartError::Client)?;
        let failures = self.start_desired_mcp_servers_inner().await;
        self.log_mcp_start_failures(&failures, "Computer startup");

        Ok(())
    }

    pub async fn is_running(&self) -> bool {
        matches!(
            self.runtime_state().await,
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

    pub async fn runtime_state(&self) -> ComputerRuntimeState {
        self.computer.read().await.lifecycle_state()
    }

    pub async fn ensure_runtime_action(
        &self,
        action: ComputerRuntimeAction,
    ) -> Result<(), ComputerRuntimeActionUnavailable> {
        runtime_lifecycle::ensure_runtime_action(self.runtime_state().await, action)
    }

    pub fn runtime_generation(&self) -> u64 {
        self.runtime_generation.load(Ordering::Acquire)
    }

    pub async fn runtime_snapshot(&self) -> ComputerRuntimeSnapshot {
        loop {
            let _snapshot_guard = self.runtime_snapshot_lock.lock().await;
            let generation = self.runtime_generation();
            let snapshot = self.computer.read().await.status().await;
            let client_last_error = self.client_runtime_diagnostic.read().await.clone();
            if generation == self.runtime_generation() {
                let snapshot_revision = self
                    .runtime_snapshot_revision
                    .fetch_add(1, Ordering::AcqRel)
                    + 1;
                return ComputerRuntimeSnapshot::from_sdk(
                    self.runtime_incarnation,
                    generation,
                    snapshot_revision,
                    snapshot,
                    client_last_error,
                );
            }
        }
    }

    pub(super) async fn publish_runtime_status(&self, cause: ComputerRuntimeEventCause) {
        let Some(sink) = self.runtime_event_sink.read().await.clone() else {
            return;
        };
        let event = ComputerRuntimeStatusEvent::from_observation(
            self.instance.id.clone(),
            cause,
            self.runtime_snapshot().await,
            self.connection_authority_snapshot().await,
        );
        if let Err(error) = sink.emit(&event) {
            log::warn!(
                "Failed to publish runtime status event for instance {}: {}",
                self.instance.id,
                error
            );
        }
    }

    pub async fn start_runtime_event_relay(&self) {
        if self.is_retired() || self.runtime_event_sink.read().await.is_none() {
            return;
        }

        let (generation, mut receiver) = {
            let _snapshot_guard = self.runtime_snapshot_lock.lock().await;
            (
                self.runtime_generation(),
                self.computer.read().await.subscribe_events(),
            )
        };
        let computer = self.computer.clone();
        let current_generation = self.runtime_generation.clone();
        let snapshot_revision = self.runtime_snapshot_revision.clone();
        let snapshot_lock = self.runtime_snapshot_lock.clone();
        let sink = self.runtime_event_sink.clone();
        let instance_id = self.instance.id.clone();
        let incarnation = self.runtime_incarnation;
        let client_runtime_diagnostic = self.client_runtime_diagnostic.clone();
        let connection = self.connection.clone();
        let connection_authority_revision = self.connection_authority_revision.clone();
        let mut relay = self.runtime_event_task.lock().await;
        if self.is_retired()
            || generation != self.runtime_generation()
            || relay
                .as_ref()
                .is_some_and(|current| current.generation > generation)
        {
            return;
        }
        if relay
            .as_ref()
            .is_some_and(|current| current.generation == generation && !current.task.is_finished())
        {
            return;
        }
        if let Some(stale) = relay.take() {
            stale.task.abort();
        }
        let relay_task = tokio::spawn(async move {
            loop {
                let (cause, terminal) = match receiver.recv().await {
                    Ok(event) => {
                        let terminal = matches!(
                            event,
                            a2c_smcp::smcp_computer::ComputerEvent::LifecycleChanged {
                                state: LifecycleState::Shutdown
                            }
                        );
                        (ComputerRuntimeEventCause::from(event), terminal)
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped_events)) => {
                        (ComputerRuntimeEventCause::Resync { skipped_events }, false)
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                };

                if current_generation.load(Ordering::Acquire) != generation {
                    break;
                }
                let runtime_snapshot = {
                    let _snapshot_guard = snapshot_lock.lock().await;
                    if current_generation.load(Ordering::Acquire) != generation {
                        break;
                    }
                    let snapshot = computer.read().await.status().await;
                    let client_last_error = client_runtime_diagnostic.read().await.clone();
                    if current_generation.load(Ordering::Acquire) != generation {
                        break;
                    }
                    ComputerRuntimeSnapshot::from_sdk(
                        incarnation,
                        generation,
                        snapshot_revision.fetch_add(1, Ordering::AcqRel) + 1,
                        snapshot,
                        client_last_error,
                    )
                };
                let Some(sink) = sink.read().await.clone() else {
                    break;
                };
                let event = ComputerRuntimeStatusEvent::from_observation(
                    instance_id.clone(),
                    cause,
                    runtime_snapshot,
                    {
                        let connection = connection.read().await;
                        ClientConnectionAuthoritySnapshot::from_connection(
                            connection_authority_revision.load(Ordering::Acquire),
                            connection.as_ref(),
                        )
                    },
                );
                if let Err(error) = sink.emit(&event) {
                    log::warn!(
                        "Failed to relay SDK runtime event for instance {}: {}",
                        instance_id,
                        error
                    );
                }
                if terminal {
                    break;
                }
            }
        });
        *relay = Some(RuntimeEventRelay {
            generation,
            task: relay_task,
        });
    }

    pub(super) async fn stop_runtime_event_relay(&self) {
        let relay = self.runtime_event_task.lock().await.take();
        if let Some(relay) = relay {
            relay.task.abort();
            let _ = relay.task.await;
        }
    }

    pub async fn restart(&self) -> Result<(), ComputerRuntimeStartError> {
        let _guard = self.lifecycle_lock.lock().await;
        self.ensure_active()
            .map_err(ComputerRuntimeStartError::Client)?;
        self.replace_sdk_computer(true, "restart").await
    }

    pub async fn reload(&self) -> Result<(), ComputerRuntimeStartError> {
        let _guard = self.lifecycle_lock.lock().await;
        self.ensure_active()
            .map_err(ComputerRuntimeStartError::Client)?;
        let was_running = self.is_running().await;
        self.replace_sdk_computer(was_running, "reload").await
    }

    pub async fn try_shutdown(&self) -> Result<(), String> {
        let _guard = self.lifecycle_lock.lock().await;
        self.try_shutdown_inner().await
    }

    pub(super) async fn try_shutdown_inner(&self) -> Result<(), String> {
        self.preflight_sdk_shutdown_inner().await?;
        let cleanup_errors = self.shutdown_after_preflight_inner().await;
        self.log_committed_shutdown_diagnostics(&cleanup_errors);
        Ok(())
    }

    /// Validates every condition that can reject shutdown without mutating SDK lifecycle state.
    /// Removal runs this while the runtime is retired and lifecycle-locked, before storage or
    /// profile state is quarantined, so a failure can reopen the exact same runtime safely.
    pub(super) async fn preflight_sdk_shutdown_inner(&self) -> Result<(), String> {
        if self.shutdown_completed.load(Ordering::Acquire) {
            return Ok(());
        }
        let computer = self.computer.read().await;
        if computer.lifecycle_state() == LifecycleState::Shutdown {
            return Err(format!(
                "SDK Computer for instance {} reached Shutdown without client teardown confirmation",
                self.instance.id
            ));
        }
        Ok(())
    }

    /// Crosses the shutdown commit point. Callers must run the non-mutating preflight first.
    pub(super) async fn shutdown_after_preflight_inner(&self) -> Vec<String> {
        let mut cleanup_errors = Vec::new();
        if let Err(error) = self.prepare_sdk_shutdown_inner().await {
            cleanup_errors.push(error);
        }

        if self.has_smcp_transport().await {
            if let Err(error) = self.disconnect_smcp_socketio_inner().await {
                cleanup_errors.push(format!(
                    "Failed to disconnect SMCP socket during shutdown for instance {}: {}",
                    self.instance.id, error
                ));
            }
        }
        // Teardown is deliberately exhaustive after the commit point: no cleanup failure may
        // leave refresh work or logical connection state alive for an instance being removed.
        self.abort_refresh_task().await;
        self.take_connection_state().await;
        self.clear_client_runtime_diagnostic_silent().await;
        if let Err(error) = self.shutdown_sdk_computer_inner().await {
            cleanup_errors.push(error);
        }
        cleanup_errors
    }

    pub(super) fn log_committed_shutdown_diagnostics(&self, cleanup_errors: &[String]) {
        if !cleanup_errors.is_empty() {
            log::warn!(
                "Computer runtime {} completed committed shutdown with cleanup diagnostics: {}",
                self.instance.id,
                cleanup_errors.join("; ")
            );
        }
    }

    /// Stops SDK MCP clients after shutdown has crossed its explicit commit point.
    pub(super) async fn prepare_sdk_shutdown_inner(&self) -> Result<(), String> {
        if self.shutdown_completed.load(Ordering::Acquire) {
            return Ok(());
        }
        #[cfg(debug_assertions)]
        if self
            .fail_prepare_shutdown_once
            .swap(false, Ordering::SeqCst)
        {
            return Err(format!(
                "Injected SDK MCP stop failure for instance {}",
                self.instance.id
            ));
        }
        let computer = self.computer.read().await;
        if computer.is_mcp_manager_initialized().await {
            computer.stop_all_mcp_clients().await.map_err(|error| {
                format!(
                    "Failed to stop SDK MCP clients before shutdown for instance {}: {}",
                    self.instance.id, error
                )
            })?;
        }
        Ok(())
    }

    /// Tears down the authoritative SDK handle exactly once.
    ///
    /// The pinned SDK enters `Shutdown` before all fallible cleanup completes, so that lifecycle
    /// value cannot prove teardown success. Stopping MCP clients first keeps failures retryable;
    /// only a successful SDK shutdown records completion for this handle generation.
    pub(super) async fn shutdown_sdk_computer_inner(&self) -> Result<(), String> {
        if self.shutdown_completed.load(Ordering::Acquire) {
            return Ok(());
        }

        let computer = self.computer.read().await;
        if let Err(error) = computer.shutdown().await {
            if computer.lifecycle_state() != LifecycleState::Shutdown {
                return Err(format!(
                    "Failed to shutdown SDK Computer for instance {}: {}",
                    self.instance.id, error
                ));
            }
            // The pinned SDK enters Shutdown and removes its manager before fallible final cleanup.
            // At that point reopening admission would create a false rollback, so record terminal
            // teardown and let the deletion transaction finish consistently.
            log::warn!(
                "SDK Computer for instance {} reached terminal Shutdown with cleanup error: {}",
                self.instance.id,
                error
            );
        }
        self.shutdown_completed.store(true, Ordering::Release);
        Ok(())
    }

    pub(super) async fn retire(&self) -> Result<(), String> {
        self.retired.store(true, Ordering::SeqCst);
        while self.active_operations.load(Ordering::SeqCst) != 0 {
            self.activity_changed.notified().await;
        }
        let _guard = self.lifecycle_lock.lock().await;
        match self.try_shutdown_inner().await {
            Ok(()) => {
                self.stop_runtime_event_relay().await;
                Ok(())
            }
            Err(error) => {
                // No replacement/removal is published on teardown failure. Restore admission so
                // the still-authoritative runtime can be repaired and retirement retried.
                self.retired.store(false, Ordering::SeqCst);
                Err(error)
            }
        }
    }

    pub(super) fn is_retired(&self) -> bool {
        self.retired.load(Ordering::SeqCst)
    }

    pub(super) fn retired_error(&self) -> String {
        format!(
            "Computer runtime incarnation {} for instance {} has been retired",
            self.runtime_incarnation, self.instance.id
        )
    }

    pub(super) fn ensure_active(&self) -> Result<(), String> {
        if self.is_retired() {
            Err(self.retired_error())
        } else {
            Ok(())
        }
    }

    pub(super) fn ensure_active_computer(&self) -> ComputerResult<()> {
        self.ensure_active().map_err(ComputerError::InvalidState)
    }

    pub(super) fn begin_activity(&self) -> Result<RuntimeActivityGuard, String> {
        let guard = RuntimeActivityGuard {
            active_operations: self.active_operations.clone(),
            activity_changed: self.activity_changed.clone(),
        };
        self.active_operations.fetch_add(1, Ordering::SeqCst);
        if self.is_retired() {
            drop(guard);
            Err(self.retired_error())
        } else {
            Ok(guard)
        }
    }
}
