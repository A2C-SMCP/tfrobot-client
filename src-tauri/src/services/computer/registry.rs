use super::*;

pub struct ComputerRegistry {
    runtimes: RwLock<HashMap<ComputerInstanceId, ComputerInstanceRuntime>>,
    runtime_mutations: std::sync::Mutex<HashMap<ComputerInstanceId, Weak<Mutex<()>>>>,
    skill_home_base: PathBuf,
    secret_store: Arc<dyn SecretStore>,
    runtime_event_sink: SharedRuntimeEventSink,
}

/// A two-phase runtime removal. Creating the transaction closes activity admission, drains
/// in-flight work, and holds the instance lifecycle lock without shutting down the SDK. Dropping
/// an uncommitted transaction reopens the same authoritative runtime and preserves its connection.
pub(crate) struct PreparedRuntimeRemoval {
    runtime: ComputerInstanceRuntime,
    _mutation_guard: OwnedMutexGuard<()>,
    _lifecycle_guard: OwnedMutexGuard<()>,
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

        for instance in config.instances {
            let instance_id = instance.id.clone();
            let runtime = ComputerInstanceRuntime::new_with_secret_store_and_event_sink(
                instance,
                skill_home_base.clone(),
                secret_store.clone(),
                runtime_event_sink.clone(),
            );
            runtimes.insert(instance_id, runtime);
        }

        let initial_runtime = runtimes.values().next().cloned();
        let registry = Self {
            runtimes: RwLock::new(runtimes),
            runtime_mutations: std::sync::Mutex::new(HashMap::new()),
            skill_home_base,
            secret_store,
            runtime_event_sink,
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
        let runtime = existing.with_instance(instance);
        if let Err(error) = runtime.sync_runtime().await {
            let restore_runtime = existing.with_instance(existing.instance.clone());
            if let Err(restore_error) = restore_runtime.sync_runtime().await {
                return Err(error.append_context(format!(
                    "additionally failed to restore previous runtime: {restore_error}"
                )));
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
        while runtime.active_operations.load(Ordering::SeqCst) != 0 {
            runtime.activity_changed.notified().await;
        }
        let lifecycle_guard = runtime.lifecycle_lock.clone().lock_owned().await;
        if let Err(error) = runtime.preflight_sdk_shutdown_inner().await {
            runtime.retired.store(false, Ordering::SeqCst);
            return Err(error);
        }
        Ok(Some(PreparedRuntimeRemoval {
            runtime,
            _mutation_guard: mutation_guard,
            _lifecycle_guard: lifecycle_guard,
            committed: false,
        }))
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
