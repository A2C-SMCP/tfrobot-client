use super::*;

pub struct ComputerRegistry {
    runtimes: RwLock<HashMap<ComputerInstanceId, ComputerInstanceRuntime>>,
    runtime_membership: Arc<Mutex<()>>,
    departing_manager_contexts: std::sync::Mutex<HashSet<ManagerContextKey>>,
    runtime_mutations: std::sync::Mutex<HashMap<ComputerInstanceId, Weak<Mutex<()>>>>,
    runtime_operations: std::sync::Mutex<HashMap<ComputerInstanceId, Weak<RwLock<()>>>>,
    skill_home_base: PathBuf,
    secret_store: Arc<dyn SecretStore>,
    runtime_event_sink: SharedRuntimeEventSink,
    runtime_input_bridge: Arc<crate::services::runtime_input_bridge::RuntimeInputBridge>,
    client_control_binding: ClientControlBinding,
}

/// A two-phase runtime removal. Creating the transaction closes activity admission, drains
/// in-flight work, and holds the instance lifecycle lock without shutting down the SDK. Dropping
/// an uncommitted transaction reopens the same authoritative runtime and preserves its connection.
pub(crate) struct PreparedRuntimeRemoval {
    runtime: ComputerInstanceRuntime,
    _mutation_guard: OwnedMutexGuard<()>,
    _lifecycle_guard: Option<OwnedMutexGuard<()>>,
    committed: bool,
}

impl Drop for PreparedRuntimeRemoval {
    fn drop(&mut self) {
        if !self.committed {
            self.runtime.retired.store(false, Ordering::SeqCst);
        }
    }
}

impl ComputerRegistry {
    pub fn from_config(config: ComputerInstancesConfig) -> Self {
        Self::from_config_with_skill_home_base(config, default_skill_home_base())
    }

    pub fn from_config_with_skill_home_base(
        config: ComputerInstancesConfig,
        skill_home_base: PathBuf,
    ) -> Self {
        let (registry, _) =
            Self::from_config_with_initial_runtime_and_skill_home_base_and_secret_store(
                config,
                skill_home_base,
                Arc::new(InMemorySecretStore::default()),
            );
        registry
    }

    pub fn from_config_with_skill_home_base_and_secret_store(
        config: ComputerInstancesConfig,
        skill_home_base: PathBuf,
        secret_store: Arc<dyn SecretStore>,
    ) -> Self {
        let (registry, _) =
            Self::from_config_with_initial_runtime_and_skill_home_base_and_secret_store(
                config,
                skill_home_base,
                secret_store,
            );
        registry
    }

    pub fn from_config_with_initial_runtime(
        config: ComputerInstancesConfig,
    ) -> (Self, Option<ComputerInstanceRuntime>) {
        Self::from_config_with_initial_runtime_and_skill_home_base(
            config,
            default_skill_home_base(),
        )
    }

    pub fn from_config_with_initial_runtime_and_skill_home_base(
        config: ComputerInstancesConfig,
        skill_home_base: PathBuf,
    ) -> (Self, Option<ComputerInstanceRuntime>) {
        Self::from_config_with_initial_runtime_and_skill_home_base_and_secret_store(
            config,
            skill_home_base,
            Arc::new(InMemorySecretStore::default()),
        )
    }

    fn from_config_with_initial_runtime_and_skill_home_base_and_secret_store(
        mut config: ComputerInstancesConfig,
        skill_home_base: PathBuf,
        secret_store: Arc<dyn SecretStore>,
    ) -> (Self, Option<ComputerInstanceRuntime>) {
        config.normalize();
        let mut runtimes = HashMap::new();
        let runtime_event_sink: SharedRuntimeEventSink = Arc::new(RwLock::new(None));
        let runtime_input_bridge =
            Arc::new(crate::services::runtime_input_bridge::RuntimeInputBridge::new());
        let client_control_binding = ClientControlBinding::default();

        for instance in config.instances {
            let instance_id = instance.id.clone();
            let runtime = ComputerInstanceRuntime::new_with_secret_store_and_event_sink(
                instance,
                skill_home_base.clone(),
                secret_store.clone(),
                runtime_event_sink.clone(),
                runtime_input_bridge.clone(),
                client_control_binding.clone(),
            );
            runtimes.insert(instance_id, runtime);
        }

        let initial_runtime = runtimes.values().next().cloned();
        let registry = Self {
            runtimes: RwLock::new(runtimes),
            runtime_membership: Arc::new(Mutex::new(())),
            departing_manager_contexts: std::sync::Mutex::new(HashSet::new()),
            runtime_mutations: std::sync::Mutex::new(HashMap::new()),
            runtime_operations: std::sync::Mutex::new(HashMap::new()),
            skill_home_base,
            secret_store,
            runtime_event_sink,
            runtime_input_bridge,
            client_control_binding,
        };

        (registry, initial_runtime)
    }

    pub async fn runtime(&self, id: &str) -> Option<ComputerInstanceRuntime> {
        let runtimes = self.runtimes.read().await;
        runtimes.get(id).cloned()
    }

    pub async fn list_runtimes(&self) -> Vec<ComputerInstanceRuntime> {
        let runtimes = self.runtimes.read().await;
        let mut values: Vec<_> = runtimes.values().cloned().collect();
        values.sort_by(|a, b| a.instance.name.cmp(&b.instance.name));
        values
    }

    pub async fn set_runtime_event_sink(&self, sink: Arc<dyn ComputerRuntimeEventSink>) {
        *self.runtime_event_sink.write().await = Some(sink);
        for runtime in self.list_runtimes().await {
            runtime.start_runtime_event_relay().await;
        }
    }

    pub fn runtime_input_bridge(
        &self,
    ) -> Arc<crate::services::runtime_input_bridge::RuntimeInputBridge> {
        self.runtime_input_bridge.clone()
    }

    pub fn bind_client_control(
        &self,
        plane: &Arc<crate::services::client_control::ClientControlPlane>,
    ) {
        self.client_control_binding.bind(plane);
    }

    pub async fn runtime_observations(
        &self,
    ) -> Vec<(
        ComputerInstanceId,
        ComputerRuntimeSnapshot,
        ClientConnectionStateSnapshot,
    )> {
        let runtimes = self.list_runtimes().await;
        let mut snapshots = Vec::with_capacity(runtimes.len());
        for runtime in runtimes {
            snapshots.push((
                runtime.instance.id.clone(),
                runtime.runtime_snapshot().await,
                runtime.connection_snapshot().await,
            ));
        }
        snapshots
    }

    pub(super) fn runtime_mutation_coordinator(&self, id: &str) -> Arc<Mutex<()>> {
        let mut coordinators = self
            .runtime_mutations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(coordinator) = coordinators.get(id).and_then(Weak::upgrade) {
            return coordinator;
        }
        coordinators.retain(|_, coordinator| coordinator.strong_count() > 0);
        let coordinator = Arc::new(Mutex::new(()));
        coordinators.insert(id.to_string(), Arc::downgrade(&coordinator));
        coordinator
    }

    fn operation_coordinator(&self, id: &str) -> Arc<RwLock<()>> {
        let coordinator = {
            let mut coordinators = self
                .runtime_operations
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(coordinator) = coordinators.get(id).and_then(Weak::upgrade) {
                coordinator
            } else {
                coordinators.retain(|_, coordinator| coordinator.strong_count() > 0);
                let coordinator = Arc::new(RwLock::new(()));
                coordinators.insert(id.to_string(), Arc::downgrade(&coordinator));
                coordinator
            }
        };
        coordinator
    }

    /// Exclusively serializes one Computer's configuration and lifecycle transactions. The gate
    /// is per Computer; unrelated Computers never wait behind it.
    pub async fn operation_lease(&self, id: &str) -> OwnedRwLockWriteGuard<()> {
        self.operation_coordinator(id).write_owned().await
    }

    /// Admits a runtime operation that may run alongside other non-mutating work for the same
    /// Computer while excluding configuration replacement, deletion, and governance mutations.
    pub async fn shared_operation_lease(&self, id: &str) -> OwnedRwLockReadGuard<()> {
        self.operation_coordinator(id).read_owned().await
    }

    /// Linearizes runtime membership changes that can carry Manager authority with Context
    /// cleanup. The guard covers only short publication phases; waiting for one Computer's
    /// operation gate cannot convoy unrelated lifecycle commands.
    pub(crate) async fn membership_lease(&self) -> OwnedMutexGuard<()> {
        self.runtime_membership.clone().lock_owned().await
    }

    /// Marks a Manager Context transition while holding [`Self::membership_lease`]. Duplicate
    /// publication consults this tombstone so the long per-Computer cleanup can run after the
    /// membership guard is released without allowing inherited authority to escape its snapshot.
    pub(crate) fn mark_manager_context_departing(&self, context: ManagerContextKey) {
        self.departing_manager_contexts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(context);
    }

    pub(crate) fn clear_departing_manager_context(&self, context: &ManagerContextKey) {
        self.departing_manager_contexts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(context);
    }

    pub(crate) fn is_manager_context_departing(&self, context: &ManagerContextKey) -> bool {
        self.departing_manager_contexts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(context)
    }

    fn runtime_entry_matches(
        current: Option<&ComputerInstanceRuntime>,
        expected: Option<&ComputerInstanceRuntime>,
    ) -> bool {
        match (current, expected) {
            (None, None) => true,
            (Some(current), Some(expected)) => {
                current.runtime_incarnation == expected.runtime_incarnation
                    && Arc::ptr_eq(&current.computer, &expected.computer)
            }
            _ => false,
        }
    }

    pub(crate) async fn ensure_current_runtime(
        &self,
        expected: &ComputerInstanceRuntime,
    ) -> Result<(), String> {
        let runtimes = self.runtimes.read().await;
        if Self::runtime_entry_matches(runtimes.get(&expected.instance.id), Some(expected)) {
            Ok(())
        } else {
            Err(format!(
                "Computer runtime changed while committing instance {}",
                expected.instance.id
            ))
        }
    }

    pub async fn upsert_runtime(
        &self,
        instance: ComputerInstance,
    ) -> Result<ComputerInstanceRuntime, String> {
        let instance_id = instance.id.clone();
        let coordinator = self.runtime_mutation_coordinator(&instance_id);
        let _mutation_guard = coordinator.lock().await;
        let existing = {
            let runtimes = self.runtimes.read().await;
            runtimes.get(&instance_id).cloned()
        };
        if let Some(existing) = existing.as_ref() {
            existing.retire().await?;
        }
        let runtime = ComputerInstanceRuntime::new_with_secret_store_and_event_sink(
            instance,
            self.skill_home_base.clone(),
            self.secret_store.clone(),
            self.runtime_event_sink.clone(),
            self.runtime_input_bridge.clone(),
            self.client_control_binding.clone(),
        );
        {
            let mut runtimes = self.runtimes.write().await;
            if !Self::runtime_entry_matches(runtimes.get(&instance_id), existing.as_ref()) {
                return Err(format!(
                    "Computer runtime changed while replacing instance {instance_id}"
                ));
            }
            runtimes.insert(instance_id, runtime.clone());
        }
        runtime.start_runtime_event_relay().await;
        Ok(runtime)
    }

    pub async fn update_runtime_instance(
        &self,
        instance: ComputerInstance,
    ) -> Result<ComputerInstanceRuntime, String> {
        self.update_runtime_instance_typed(instance)
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn update_runtime_instance_typed(
        &self,
        instance: ComputerInstance,
    ) -> Result<ComputerInstanceRuntime, ComputerRuntimeStartError> {
        let instance_id = instance.id.clone();
        let coordinator = self.runtime_mutation_coordinator(&instance_id);
        let _mutation_guard = coordinator.lock().await;
        let previous = {
            let runtimes = self.runtimes.read().await;
            runtimes.get(&instance_id).cloned()
        };
        let existing = previous.as_ref().ok_or_else(|| {
            ComputerRuntimeStartError::Client(format!(
                "Computer runtime does not exist for instance {instance_id}"
            ))
        })?;
        let remote_control_policy_changed =
            existing.instance.remote_control != instance.remote_control;
        let command_line_policy_changed = existing.instance.command_line != instance.command_line;
        let was_running = existing.is_running().await;
        let runtime = existing.with_instance(instance);
        if let Err(error) = runtime
            .sync_runtime_for_policy_change(
                remote_control_policy_changed,
                command_line_policy_changed,
            )
            .await
        {
            let restore_runtime = existing.with_instance(existing.instance.clone());
            if let Err(restore_error) = restore_runtime
                .sync_runtime_for_policy_change(
                    remote_control_policy_changed,
                    command_line_policy_changed,
                )
                .await
            {
                return Err(error.append_context(format!(
                    "additionally failed to restore previous runtime: {restore_error}"
                )));
            }
            if was_running && !restore_runtime.is_running().await {
                if let Err(restore_error) = restore_runtime.start().await {
                    return Err(error.append_context(format!(
                        "previous Computer configuration was restored, but its running state could not be restored: {restore_error}"
                    )));
                }
            }
            return Err(error);
        }
        {
            let mut runtimes = self.runtimes.write().await;
            if !Self::runtime_entry_matches(runtimes.get(&instance_id), previous.as_ref()) {
                return Err(ComputerRuntimeStartError::Client(format!(
                    "Computer runtime changed while updating instance {instance_id}"
                )));
            }
            runtimes.insert(instance_id, runtime.clone());
        }
        Ok(runtime)
    }

    pub async fn remove_runtime(
        &self,
        id: &str,
    ) -> Result<Option<ComputerInstanceRuntime>, String> {
        let prepared = self.prepare_runtime_removal(id).await?;
        match prepared {
            Some(prepared) => self.commit_runtime_removal(prepared).await.map(Some),
            None => Ok(None),
        }
    }

    pub(crate) async fn prepare_runtime_removal(
        &self,
        id: &str,
    ) -> Result<Option<PreparedRuntimeRemoval>, String> {
        let coordinator = self.runtime_mutation_coordinator(id);
        let mutation_guard = coordinator.lock_owned().await;
        let runtime = {
            let runtimes = self.runtimes.read().await;
            runtimes.get(id).cloned()
        };
        let Some(runtime) = runtime else {
            return Ok(None);
        };
        runtime.retired.store(true, Ordering::SeqCst);
        // Construct the rollback owner before the first await after closing admission. Dropping
        // this future at any later suspension point therefore reopens the authoritative runtime.
        let mut prepared = PreparedRuntimeRemoval {
            runtime,
            _mutation_guard: mutation_guard,
            _lifecycle_guard: None,
            committed: false,
        };
        while prepared.runtime.active_operations.load(Ordering::SeqCst) != 0 {
            prepared.runtime.activity_changed.notified().await;
        }
        prepared._lifecycle_guard =
            Some(prepared.runtime.lifecycle_lock.clone().lock_owned().await);
        prepared.runtime.preflight_sdk_shutdown_inner().await?;
        Ok(Some(prepared))
    }

    pub(crate) async fn commit_runtime_removal(
        &self,
        mut prepared: PreparedRuntimeRemoval,
    ) -> Result<ComputerInstanceRuntime, String> {
        let runtime = prepared.runtime.clone();
        let id = runtime.instance.id.clone();
        {
            let runtimes = self.runtimes.read().await;
            if !Self::runtime_entry_matches(runtimes.get(&id), Some(&runtime)) {
                return Err(format!(
                    "Computer runtime changed while removing instance {id}"
                ));
            }
        }
        // Preflight is the last rollback boundary. From here, cleanup errors cannot restore a
        // partially stopped SDK, so removal finishes consistently and reports cleanup diagnostics.
        let cleanup_errors = runtime.shutdown_after_preflight_inner().await;
        runtime.log_committed_shutdown_diagnostics(&cleanup_errors);
        runtime.stop_runtime_event_relay().await;
        let mut runtimes = self.runtimes.write().await;
        if !Self::runtime_entry_matches(runtimes.get(&id), Some(&runtime)) {
            return Err(format!(
                "Computer runtime changed while removing instance {id}"
            ));
        }
        let removed = runtimes
            .remove(&id)
            .expect("matching runtime must exist while committing removal");
        prepared.committed = true;
        Ok(removed)
    }

    pub async fn start_runtime(&self, id: &str) -> Result<(), String> {
        let runtime = self
            .runtime(id)
            .await
            .ok_or_else(|| format!("Computer instance not found: {id}"))?;
        runtime.start().await.map_err(|error| error.to_string())
    }

    pub async fn stop_runtime(&self, id: &str) -> Result<(), String> {
        let runtime = self
            .runtime(id)
            .await
            .ok_or_else(|| format!("Computer instance not found: {id}"))?;
        runtime.try_shutdown().await
    }

    pub async fn shutdown_all(&self) {
        let runtimes: Vec<_> = {
            let runtimes = self.runtimes.read().await;
            runtimes.values().cloned().collect()
        };

        for runtime in runtimes {
            runtime.shutdown().await;
        }
    }
}
