use super::*;
use crate::services::observability::CONNECTION_LOG_TARGET;
#[cfg(test)]
use a2c_smcp::smcp_computer::oauth::OAuthStatus;
use a2c_smcp::smcp_computer::ComputerEvent;
use std::collections::VecDeque;
use std::time::Duration;
use tokio::time::timeout;

const SDK_COMPUTER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

fn coalesce_oauth_events(events: Vec<ComputerEvent>) -> Vec<ComputerEvent> {
    let mut seen_oauth_bundles = HashSet::new();
    let mut retained = Vec::with_capacity(events.len());
    for event in events.into_iter().rev() {
        if let ComputerEvent::OAuthStatusChanged { bundle_id, .. } = &event {
            if !seen_oauth_bundles.insert(bundle_id.clone()) {
                continue;
            }
        }
        retained.push(event);
    }
    retained.reverse();
    retained
}

struct PendingRuntimeCause {
    cause: ComputerRuntimeEventCause,
    terminal: bool,
}

async fn project_oauth_required_scope(
    required_scopes: &RwLock<oauth::OAuthRequiredScopeCache>,
    cause: &ComputerRuntimeEventCause,
) {
    let mut required_scopes = required_scopes.write().await;
    match cause {
        ComputerRuntimeEventCause::OAuthStatusChanged { bundle_id, status } => {
            if let Ok(bundle_id) = BundleId::try_from(bundle_id.as_str()) {
                required_scopes.apply_public_status(&bundle_id, status);
            }
        }
        ComputerRuntimeEventCause::Resync { .. } => required_scopes.clear(),
        _ => {}
    }
}

impl ComputerInstanceRuntime {
    pub async fn start(&self) -> Result<(), ComputerRuntimeStartError> {
        self.lifecycle_lease()
            .await
            .start_with_failure_policy(RuntimeMcpStartFailurePolicy::BestEffort)
            .await
    }

    pub(crate) async fn lifecycle_lease(&self) -> ComputerRuntimeLifecycleLease<'_> {
        ComputerRuntimeLifecycleLease {
            runtime: self,
            _lifecycle: self.lifecycle_lock.lock().await,
        }
    }

    async fn start_with_failure_policy_inner(
        &self,
        failure_policy: RuntimeMcpStartFailurePolicy,
    ) -> Result<(), ComputerRuntimeStartError> {
        self.ensure_active()
            .map_err(ComputerRuntimeStartError::Client)?;
        let lifecycle = self.runtime_state().await;
        if matches!(
            lifecycle,
            LifecycleState::Created
                | LifecycleState::Stopped
                | LifecycleState::Shutdown
                | LifecycleState::Error
        ) {
            self.replace_sdk_computer(
                false,
                "start_with_persisted_configuration",
                HandleReplacementConfig::ReloadPersisted,
                failure_policy,
            )
            .await?;
        } else if matches!(
            lifecycle,
            LifecycleState::Started | LifecycleState::Degraded
        ) {
            self.reconcile_governance_for_computer_start(
                "idempotent Computer startup",
                failure_policy,
            )
            .await
            .map_err(ComputerRuntimeStartError::Sdk)?;
            let failures = self.start_desired_mcp_servers_inner(failure_policy).await;
            self.handle_desired_mcp_start_failures(
                failures,
                "idempotent Computer startup",
                failure_policy,
            )
            .map_err(ComputerRuntimeStartError::Sdk)?;
            return Ok(());
        } else if matches!(
            lifecycle,
            LifecycleState::Starting
                | LifecycleState::Connecting
                | LifecycleState::Connected
                | LifecycleState::JoinedOffice
                | LifecycleState::Syncing
                | LifecycleState::Disconnecting
                | LifecycleState::Stopping
        ) {
            return Ok(());
        }
        self.start_runtime_event_relay().await;

        if let Err(error) = self.computer.read().await.boot_up().await {
            return Err(ComputerRuntimeStartError::Sdk(error));
        }
        if let Err(error) = self
            .reconcile_governance_for_computer_start("Computer startup", failure_policy)
            .await
        {
            // boot_up has already moved the SDK lifecycle to Started. A governance failure is
            // still a failed Computer start transaction, so roll the partially started handle
            // back to Shutdown; otherwise the public Start action becomes unavailable and the
            // user cannot save the missing runtime input and retry.
            let mut start_error = ComputerRuntimeStartError::Sdk(error);
            if let Err(cleanup_error) = self.try_shutdown_inner().await {
                start_error = start_error.append_context(format!(
                    "failed to roll back the partially started Computer: {cleanup_error}"
                ));
            }
            return Err(start_error);
        }
        let failures = self.start_desired_mcp_servers_inner(failure_policy).await;
        if let Err(error) =
            self.handle_desired_mcp_start_failures(failures, "Computer startup", failure_policy)
        {
            if Self::is_command_line_start_error(&error) {
                return Err(ComputerRuntimeStartError::Sdk(error));
            }
            let mut start_error = ComputerRuntimeStartError::Sdk(error);
            if let Err(cleanup_error) = self.try_shutdown_inner().await {
                start_error = start_error.append_context(format!(
                    "failed to roll back the partially started Computer: {cleanup_error}"
                ));
            }
            return Err(start_error);
        }
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
            let snapshot = user_visible_sdk_status_snapshot(&self.computer).await;
            if generation == self.runtime_generation() {
                let snapshot_revision = self
                    .runtime_snapshot_revision
                    .fetch_add(1, Ordering::AcqRel)
                    + 1;
                let mut runtime_snapshot = ComputerRuntimeSnapshot::from_sdk(
                    self.runtime_incarnation,
                    generation,
                    snapshot_revision,
                    snapshot,
                );
                runtime_snapshot.problems = collect_runtime_problems(
                    &runtime_snapshot,
                    &self.sdk_problem_observations,
                    &self.client_runtime_diagnostic,
                    &self.connection_operation,
                    &self.mcp_start_diagnostics,
                    &self.mcp_config_apply_diagnostics,
                    &self.sdk_servers,
                )
                .await;
                return runtime_snapshot;
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
            self.connection_snapshot().await,
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
        let sdk_problem_observations = self.sdk_problem_observations.clone();
        let client_runtime_diagnostic = self.client_runtime_diagnostic.clone();
        let mcp_start_diagnostics = self.mcp_start_diagnostics.clone();
        let mcp_config_apply_diagnostics = self.mcp_config_apply_diagnostics.clone();
        let sdk_servers = self.sdk_servers.clone();
        let oauth_required_scopes = self.oauth_required_scopes.clone();
        let connection = self.connection.clone();
        let connection_operation = self.connection_operation.clone();
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
            let mut pending = VecDeque::<PendingRuntimeCause>::new();
            loop {
                if pending.is_empty() {
                    let first = match receiver.recv().await {
                        Ok(event) => event,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped_events)) => {
                            pending.push_back(PendingRuntimeCause {
                                cause: ComputerRuntimeEventCause::Resync { skipped_events },
                                terminal: false,
                            });
                            continue;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    };
                    let mut events = vec![first];
                    let mut lagged = None;
                    loop {
                        match receiver.try_recv() {
                            Ok(event) => events.push(event),
                            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
                            Err(tokio::sync::broadcast::error::TryRecvError::Closed) => break,
                            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(skipped)) => {
                                lagged = Some(skipped);
                                break;
                            }
                        }
                    }
                    if let Some(skipped_events) = lagged {
                        pending.push_back(PendingRuntimeCause {
                            cause: ComputerRuntimeEventCause::Resync { skipped_events },
                            terminal: false,
                        });
                    } else {
                        pending.extend(coalesce_oauth_events(events).into_iter().map(|event| {
                            let terminal = matches!(
                                &event,
                                ComputerEvent::LifecycleChanged {
                                    state: LifecycleState::Shutdown
                                }
                            );
                            PendingRuntimeCause {
                                cause: ComputerRuntimeEventCause::from(event),
                                terminal,
                            }
                        }));
                    }
                }

                let PendingRuntimeCause { cause, terminal } = pending
                    .pop_front()
                    .expect("a relay batch always yields one pending cause");

                if current_generation.load(Ordering::Acquire) != generation {
                    break;
                }
                project_oauth_required_scope(&oauth_required_scopes, &cause).await;
                let runtime_snapshot = {
                    let _snapshot_guard = snapshot_lock.lock().await;
                    if current_generation.load(Ordering::Acquire) != generation {
                        break;
                    }
                    let snapshot = user_visible_sdk_status_snapshot(&computer).await;
                    if current_generation.load(Ordering::Acquire) != generation {
                        break;
                    }
                    let mut runtime_snapshot = ComputerRuntimeSnapshot::from_sdk(
                        incarnation,
                        generation,
                        snapshot_revision.fetch_add(1, Ordering::AcqRel) + 1,
                        snapshot,
                    );
                    runtime_snapshot.problems = collect_runtime_problems(
                        &runtime_snapshot,
                        &sdk_problem_observations,
                        &client_runtime_diagnostic,
                        &connection_operation,
                        &mcp_start_diagnostics,
                        &mcp_config_apply_diagnostics,
                        &sdk_servers,
                    )
                    .await;
                    runtime_snapshot
                };
                let Some(sink) = sink.read().await.clone() else {
                    break;
                };
                let (connection_snapshot, connection_generation) = {
                    let connection = connection.read().await;
                    let operation = connection_operation.read().await;
                    (
                        ClientConnectionStateSnapshot::from_parts(
                            connection_authority_revision.load(Ordering::Acquire),
                            connection.as_ref(),
                            &operation,
                            runtime_snapshot.lifecycle,
                        ),
                        connection.as_ref().map(|connection| connection.generation),
                    )
                };
                match &cause {
                    ComputerRuntimeEventCause::LifecycleChanged { state }
                        if connection_snapshot.present =>
                    {
                        tracing::debug!(
                            target: CONNECTION_LOG_TARGET,
                            event = "connection.sdk_lifecycle_changed",
                            instance_id = %instance_id,
                            runtime_incarnation = incarnation,
                            runtime_generation = generation,
                            snapshot_revision = runtime_snapshot.snapshot_revision,
                            connection_generation,
                            sdk_lifecycle = ?state,
                            projected_status = ?connection_snapshot.status,
                            connection_revision = connection_snapshot.revision,
                            "observed SDK lifecycle change for an SMCP connection"
                        );
                    }
                    ComputerRuntimeEventCause::Resync { skipped_events } => {
                        tracing::warn!(
                            target: CONNECTION_LOG_TARGET,
                            event = "connection.runtime_event_resync",
                            instance_id = %instance_id,
                            runtime_incarnation = incarnation,
                            runtime_generation = generation,
                            skipped_events,
                            connection_present = connection_snapshot.present,
                            "runtime event relay lagged and rebuilt connection projection"
                        );
                    }
                    _ => {}
                }
                let event = ComputerRuntimeStatusEvent::from_observation(
                    instance_id.clone(),
                    cause,
                    runtime_snapshot.clone(),
                    connection_snapshot,
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
        self.lifecycle_lease()
            .await
            .restart_with_failure_policy(RuntimeMcpStartFailurePolicy::BestEffort)
            .await
    }

    async fn restart_with_failure_policy_inner(
        &self,
        failure_policy: RuntimeMcpStartFailurePolicy,
    ) -> Result<(), ComputerRuntimeStartError> {
        self.ensure_active()
            .map_err(ComputerRuntimeStartError::Client)?;
        self.replace_sdk_computer(
            true,
            "restart",
            HandleReplacementConfig::ReloadPersisted,
            failure_policy,
        )
        .await
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
        // Shutdown is a terminal OAuth admission boundary. Unlike handle replacement, it remains
        // closed after cleanup; a later runtime start installs a fresh SDK handle first.
        self.close_oauth_admission().await;
        self.cancel_all_oauth_authorizations().await;
        if let Err(error) = self.prepare_sdk_shutdown_inner().await {
            cleanup_errors.push(error);
        }

        if self.has_smcp_transport().await {
            if let Err(error) = self.disconnect_smcp_socketio_bounded_inner().await {
                cleanup_errors.push(format!(
                    "Failed to disconnect SMCP socket during shutdown for instance {}: {}",
                    self.instance.id, error
                ));
            }
        }
        // Teardown is deliberately exhaustive after the commit point: no cleanup failure may
        // leave logical connection state alive for an instance being removed.
        self.take_connection_state().await;
        self.complete_connection_operation().await;
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
        #[cfg(debug_assertions)]
        if self.fail_sdk_shutdown_once.swap(false, Ordering::SeqCst) {
            return Err(format!(
                "Injected SDK shutdown failure for instance {}",
                self.instance.id
            ));
        }

        let shutdown_result = timeout(SDK_COMPUTER_SHUTDOWN_TIMEOUT, async {
            self.computer.read().await.shutdown().await
        })
        .await;
        let shutdown_result = match shutdown_result {
            Ok(result) => result,
            Err(_) => {
                return Err(format!(
                    "Timed out shutting down SDK Computer for instance {} after {:?}",
                    self.instance.id, SDK_COMPUTER_SHUTDOWN_TIMEOUT
                ))
            }
        };
        if let Err(error) = shutdown_result {
            let computer = self.computer.read().await;
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

impl ComputerRuntimeLifecycleLease<'_> {
    pub(crate) async fn start(&self) -> Result<(), ComputerRuntimeStartError> {
        self.start_with_failure_policy(RuntimeMcpStartFailurePolicy::BestEffort)
            .await
    }

    pub(crate) async fn start_interactive(&self) -> Result<(), ComputerRuntimeStartError> {
        self.start_with_failure_policy(RuntimeMcpStartFailurePolicy::PropagateRuntimeInputFailures)
            .await
    }

    async fn start_with_failure_policy(
        &self,
        failure_policy: RuntimeMcpStartFailurePolicy,
    ) -> Result<(), ComputerRuntimeStartError> {
        self.runtime
            .start_with_failure_policy_inner(failure_policy)
            .await
    }

    pub(crate) async fn restart(&self) -> Result<(), ComputerRuntimeStartError> {
        self.restart_with_failure_policy(RuntimeMcpStartFailurePolicy::BestEffort)
            .await
    }

    pub(crate) async fn restart_interactive(&self) -> Result<(), ComputerRuntimeStartError> {
        self.restart_with_failure_policy(
            RuntimeMcpStartFailurePolicy::PropagateRuntimeInputFailures,
        )
        .await
    }

    async fn restart_with_failure_policy(
        &self,
        failure_policy: RuntimeMcpStartFailurePolicy,
    ) -> Result<(), ComputerRuntimeStartError> {
        self.runtime
            .restart_with_failure_policy_inner(failure_policy)
            .await
    }
}

/// Projects the SDK's complete runtime inventory into the client-facing capability view.
/// Built-in providers are part of the observable MCP runtime even though their lifecycle remains
/// owned by the feature that mounted them.
async fn user_visible_sdk_status_snapshot(
    computer: &RwLock<Computer<InstanceSession>>,
) -> ComputerStatusSnapshot {
    let computer = computer.read().await;
    computer.status().await
}

async fn collect_runtime_problems(
    snapshot: &ComputerRuntimeSnapshot,
    sdk_problem_observations: &Mutex<SdkProblemObservations>,
    client_runtime_diagnostic: &RwLock<Option<RuntimeDiagnosticRecord>>,
    connection_operation: &RwLock<ClientConnectionOperationState>,
    mcp_start_diagnostics: &RwLock<HashMap<BundleId, RuntimeDiagnosticRecord>>,
    mcp_config_apply_diagnostics: &RwLock<HashMap<BundleId, RuntimeDiagnosticRecord>>,
    sdk_servers: &RwLock<HashMap<BundleId, ServerName>>,
) -> Vec<ComputerRuntimeProblem> {
    let (sdk_error, sdk_degraded) = sdk_problem_observations.lock().await.observe(
        snapshot.generation,
        snapshot.lifecycle,
        snapshot.last_error.as_deref(),
        snapshot.degraded_reason.as_deref(),
    );
    let mut problems = Vec::new();
    if let Some(diagnostic) = sdk_error {
        problems.push(ComputerRuntimeProblem::sdk_error(
            snapshot.generation,
            diagnostic,
        ));
    }
    if let Some(diagnostic) = sdk_degraded {
        problems.push(ComputerRuntimeProblem::sdk_degraded(
            snapshot.generation,
            diagnostic,
        ));
    }

    // Read both client-owned records coherently. Reconnect completion acquires the corresponding
    // write locks in this order so a product snapshot cannot observe a half-cleared problem.
    let client_diagnostic_guard = client_runtime_diagnostic.read().await;
    let connection_operation_guard = connection_operation.read().await;
    let client_diagnostic = client_diagnostic_guard.clone();
    let mut connection_error = connection_operation_guard.last_error();
    drop(connection_operation_guard);
    drop(client_diagnostic_guard);
    if let (Some(error), Some(diagnostic)) = (connection_error.as_mut(), client_diagnostic.as_ref())
    {
        let operation = match error.operation {
            ClientConnectionOperation::Connect => "connect",
            ClientConnectionOperation::Disconnect => "disconnect",
            ClientConnectionOperation::Reconnect => "reconnect",
        };
        if diagnostic.operation == operation {
            // Both records describe one product problem. Preserve the earliest owner observation
            // when the richer connection-state projection supersedes the low-level diagnostic.
            error.occurred_at = earliest_runtime_occurrence(
                error.occurred_at.as_str(),
                diagnostic.occurred_at.as_str(),
            );
        }
    }
    if let Some(error) = connection_error.as_ref() {
        problems.push(ComputerRuntimeProblem::connection(
            snapshot.generation,
            error,
        ));
    }
    if let Some(diagnostic) = client_diagnostic {
        let already_projected = connection_error.as_ref().is_some_and(|error| {
            diagnostic.operation
                == match error.operation {
                    ClientConnectionOperation::Connect => "connect",
                    ClientConnectionOperation::Disconnect => "disconnect",
                    ClientConnectionOperation::Reconnect => "reconnect",
                }
        });
        if !already_projected {
            problems.push(ComputerRuntimeProblem::client_diagnostic(
                snapshot.generation,
                diagnostic,
            ));
        }
    }

    if snapshot.is_running() {
        let server_names = sdk_servers.read().await.clone();
        let start_diagnostics = mcp_start_diagnostics.read().await.clone();
        let apply_diagnostics = mcp_config_apply_diagnostics.read().await.clone();
        let mut mcp_diagnostics: Vec<_> = start_diagnostics
            .into_iter()
            .chain(apply_diagnostics)
            .filter(|(bundle_id, diagnostic)| {
                server_names.contains_key(bundle_id) || diagnostic.mcp_server_name.is_some()
            })
            .collect();
        mcp_diagnostics.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.1.operation.cmp(&right.1.operation))
        });
        for (bundle_id, diagnostic) in mcp_diagnostics {
            let server_name = server_names
                .get(&bundle_id)
                .map(ToString::to_string)
                .or_else(|| diagnostic.mcp_server_name.clone());
            problems.push(ComputerRuntimeProblem::mcp(
                snapshot.generation,
                bundle_id.as_str(),
                server_name,
                diagnostic,
            ));
        }
    }
    problems
}

fn earliest_runtime_occurrence(left: &str, right: &str) -> String {
    match (
        chrono::DateTime::parse_from_rfc3339(left),
        chrono::DateTime::parse_from_rfc3339(right),
    ) {
        (Ok(left_time), Ok(right_time)) if right_time < left_time => right.to_string(),
        (Ok(_), Ok(_)) => left.to_string(),
        _ if right < left => right.to_string(),
        _ => left.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sdk_event_projection_tracks_and_invalidates_required_scope() {
        let scopes = RwLock::new(oauth::OAuthRequiredScopeCache::default());
        let bundle_id = BundleId::try_from("protected").unwrap();

        project_oauth_required_scope(
            &scopes,
            &ComputerRuntimeEventCause::OAuthStatusChanged {
                bundle_id: bundle_id.to_string(),
                status: PublicOAuthStatus::ReauthorizationRequired {
                    required_scope: "tools.write".to_string(),
                },
            },
        )
        .await;
        assert_eq!(
            scopes.read().await.required_scope(&bundle_id).as_deref(),
            Some("tools.write")
        );

        project_oauth_required_scope(
            &scopes,
            &ComputerRuntimeEventCause::OAuthStatusChanged {
                bundle_id: bundle_id.to_string(),
                status: PublicOAuthStatus::Authorized {
                    scopes: vec!["tools.write".to_string()],
                },
            },
        )
        .await;
        assert!(scopes.read().await.required_scope(&bundle_id).is_none());

        scopes.write().await.apply_public_status(
            &bundle_id,
            &PublicOAuthStatus::ReauthorizationRequired {
                required_scope: "tools.admin".to_string(),
            },
        );
        project_oauth_required_scope(
            &scopes,
            &ComputerRuntimeEventCause::Resync { skipped_events: 1 },
        )
        .await;
        assert!(scopes.read().await.is_empty());
    }

    #[tokio::test]
    async fn queued_oauth_events_keep_latest_status_per_bundle_in_batch() {
        let bundle_id = BundleId::try_from("protected").unwrap();
        let events = coalesce_oauth_events(vec![
            ComputerEvent::OAuthStatusChanged {
                bundle_id: bundle_id.clone(),
                status: OAuthStatus::Unauthorized,
            },
            ComputerEvent::CapabilityRevisionBumped { revision: 7 },
            ComputerEvent::OAuthStatusChanged {
                bundle_id: bundle_id.clone(),
                status: OAuthStatus::ReauthorizationRequired {
                    required_scope: "tools.write".to_string(),
                },
            },
        ]);

        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0],
            ComputerEvent::CapabilityRevisionBumped { revision: 7 }
        ));
        assert!(matches!(
            &events[1],
            ComputerEvent::OAuthStatusChanged {
                status: OAuthStatus::ReauthorizationRequired { required_scope },
                ..
            } if required_scope == "tools.write"
        ));
    }
}
