//! Client-owned observations, never a second durable configuration or a health projection.
use super::*;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::{LazyLock, Mutex as StdMutex};

#[derive(Default)]
pub(super) struct DeclarationEpoch(StdMutex<(u64, usize)>);
pub(super) struct DeclarationMutation(Arc<DeclarationEpoch>);
impl DeclarationEpoch {
    pub(super) fn begin(self: &Arc<Self>) -> DeclarationMutation {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        state.0 += 1;
        state.1 += 1;
        DeclarationMutation(self.clone())
    }
    fn stable(&self) -> Option<u64> {
        let state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        (state.1 == 0).then_some(state.0)
    }
}
impl Drop for DeclarationMutation {
    fn drop(&mut self) {
        let mut state = self.0 .0.lock().unwrap_or_else(|e| e.into_inner());
        state.0 += 1;
        state.1 -= 1;
    }
}

// Scoped to one SDK call. The public client factory proves an actual Stdio/SSE construction;
// input definitions come from the resolver invocation, never from a pre-start prediction.
#[derive(Default)]
pub(super) struct StartObservation {
    created_bundle: Option<BundleId>,
    definitions: HashMap<String, Value>,
}
tokio::task_local! {
    static START_OBSERVATION: Arc<StdMutex<StartObservation>>;
}
pub(super) fn observe_materialization(config: &MCPServerConfig) {
    let _ = START_OBSERVATION.try_with(|observation| {
        observation
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .created_bundle = Some(resolve_bundle_id(config));
    });
}
pub(crate) fn observe_resolved_input(definition: &MCPServerInput) {
    let _ = START_OBSERVATION.try_with(|observation| {
        let mut observation = observation.lock().unwrap_or_else(|e| e.into_inner());
        observation.definitions.insert(
            definition.id().into(),
            serde_json::to_value(definition).expect("input serialization"),
        );
    });
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AdoptionAction {
    StartMcp,
    RestartMcp,
    StopMcp,
    RestartComputer,
    ManagePlugin,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdoptionChange {
    pub owner: Option<McpServerManagedBy>,
    pub resource_id: String,
    pub section: String,
    pub reason: String,
    pub action: AdoptionAction,
    pub current_value: Option<String>,
    pub saved_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigAdoptionStatus {
    pub incarnation: u64,
    pub generation: u64,
    pub state: String,
    pub changes: Vec<AdoptionChange>,
    pub unknown_resources: Vec<String>,
    pub plugin_owners: BTreeMap<String, McpServerManagedBy>,
}

// Only in memory. Random salt and digests never cross IPC, storage, Debug or logs.
// Store writes and resolver reads share this observer; status reads never access Keychain.
pub(crate) struct InputObservations {
    salt: [u8; 16],
    state: StdMutex<InputObservationsState>,
}
#[derive(Default)]
struct InputObservationsState {
    current: BTreeMap<String, [u8; 32]>,
    consumers: BTreeMap<String, BTreeMap<String, [u8; 32]>>,
    backups: BTreeMap<String, Option<BTreeMap<String, [u8; 32]>>>,
}
static INPUT_OBSERVATIONS: LazyLock<StdMutex<BTreeMap<PathBuf, Weak<InputObservations>>>> =
    LazyLock::new(|| StdMutex::new(BTreeMap::new()));

impl InputObservations {
    pub(crate) fn for_root(root: &Path) -> Arc<Self> {
        let mut roots = INPUT_OBSERVATIONS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = roots.get(root).and_then(Weak::upgrade) {
            return existing;
        }
        roots.retain(|_, value| value.strong_count() > 0);
        let observer = Arc::new(Self {
            salt: *uuid::Uuid::new_v4().as_bytes(),
            state: StdMutex::new(InputObservationsState::default()),
        });
        roots.insert(root.to_path_buf(), Arc::downgrade(&observer));
        observer
    }
    fn fingerprint(&self, value: Option<&Value>) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(self.salt);
        digest.update(serde_json::to_vec(&value).expect("JSON value serialization"));
        digest.finalize().into()
    }
    pub(crate) fn written(&self, key: &str, value: Option<&Value>) {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .current
            .insert(key.to_string(), self.fingerprint(value));
    }
    pub(crate) fn invalidate_value(&self, key: &str) {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .current
            .remove(key);
    }
    pub(crate) fn consumed(&self, consumers: &[String], key: &str, value: &Value) {
        let fingerprint = self.fingerprint(Some(value));
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        // A concurrent write may already hold a newer current fingerprint; retain it.
        state.current.entry(key.to_string()).or_insert(fingerprint);
        for consumer in consumers {
            state
                .consumers
                .entry(consumer.clone())
                .or_default()
                .insert(key.to_string(), fingerprint);
        }
    }
    pub(crate) fn clear(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.consumers.clear();
        state.backups.clear();
    }
    pub(crate) fn begin_consumer(&self, consumer: &str) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let previous = state.consumers.remove(consumer);
        state.backups.insert(consumer.to_string(), previous);
    }
    pub(crate) fn finish_consumer(&self, consumer: &str, preserve_previous: bool) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(previous) = state.backups.remove(consumer) {
            if preserve_previous {
                state.consumers.remove(consumer);
                if let Some(previous) = previous {
                    state.consumers.insert(consumer.to_string(), previous);
                }
            }
        }
    }
    pub(crate) fn consumer_status(
        &self,
        consumer: &str,
        referenced: &std::collections::BTreeSet<String>,
    ) -> Option<bool> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let consumed = state.consumers.get(consumer)?;
        if consumed
            .iter()
            .any(|(key, actual)| state.current.get(key).is_some_and(|saved| saved != actual))
        {
            return Some(true);
        }
        if referenced
            .iter()
            .all(|key| consumed.contains_key(key) && state.current.contains_key(key))
        {
            Some(false)
        } else {
            None
        }
    }
    pub(crate) fn invalidate_consumer(&self, consumer: &str) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.consumers.remove(consumer);
        state.backups.remove(consumer);
    }
    pub(crate) fn changed(&self, consumer: &str, key: &str) -> Option<bool> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let actual = state.consumers.get(consumer)?.get(key)?;
        Some(actual != state.current.get(key)?)
    }
}

fn canonical(config: &MCPServerConfig) -> Value {
    serde_json::to_value(normalize_mcp_server_tool_meta(config.clone()))
        .expect("MCP config serialization")
}

pub(super) struct ConsumedDeclaration {
    config: Value,
    inputs: HashMap<String, Value>,
    observed_inputs: HashMap<String, Value>,
}

fn plugin_definition_changed(
    owner: &McpServerManagedBy,
    referenced: &std::collections::BTreeSet<String>,
    process: Option<&ConsumedDeclaration>,
    desired: &HashMap<&str, &MCPServerInput>,
    plugin_inputs: &HashMap<String, MCPServerInput>,
) -> Option<bool> {
    let process = process?;
    let McpServerManagedBy::Plugin {
        marketplace,
        plugin,
        ..
    } = owner
    else {
        return None;
    };
    let mut unknown = false;
    for key in referenced {
        let scoped = a2c_smcp::smcp_computer::inputs::prefix_input_id(plugin, marketplace, key);
        let actual_key = if !key.contains('/') && process.observed_inputs.contains_key(&scoped) {
            &scoped
        } else {
            key
        };
        let Some(actual) = process.observed_inputs.get(actual_key) else {
            unknown = true;
            continue;
        };
        let latest = if actual_key.contains('/') {
            plugin_inputs
                .get(actual_key)
                .or_else(|| desired.get(actual_key.as_str()).copied())
        } else {
            desired.get(actual_key.as_str()).copied()
        };
        match latest {
            Some(latest) if serde_json::to_value(latest).ok().as_ref() != Some(actual) => {
                return Some(true)
            }
            None if !actual_key.contains('/') => return Some(true),
            None => unknown = true,
            _ => {}
        }
        // A bare reference that used the global fallback can switch to a scoped value next
        // time. Do not guess that choice by reading Keychain from this status projection.
        if actual_key == key
            && !key.contains('/')
            && (plugin_inputs.contains_key(&scoped) || desired.contains_key(scoped.as_str()))
        {
            unknown = true;
        }
    }
    if unknown {
        None
    } else {
        Some(false)
    }
}
pub(super) struct PendingDeclaration {
    declaration: ConsumedDeclaration,
    observation: Arc<StdMutex<StartObservation>>,
    epoch: Option<u64>,
    restart: bool,
    pub(super) fresh_handle: bool,
}

impl ComputerInstanceRuntime {
    pub(super) async fn record_consumed_declaration(&self, id: &BundleId, restart: bool) {
        let epoch = self.declaration_epoch.stable();
        self.input_resolver.begin_consumer(id.as_str());
        if let Some(config) = self.sdk_mcp_server_config_map().await.get(id) {
            let inputs = self
                .inputs
                .read()
                .await
                .iter()
                .filter(|(key, _)| referenced_input_ids(&canonical(config)).contains(*key))
                .map(|(key, definition)| {
                    (
                        key.clone(),
                        serde_json::to_value(definition).expect("input serialization"),
                    )
                })
                .collect();
            self.pending_declarations.write().await.insert(
                id.clone(),
                PendingDeclaration {
                    declaration: ConsumedDeclaration {
                        config: canonical(config),
                        inputs,
                        observed_inputs: HashMap::new(),
                    },
                    observation: Arc::new(StdMutex::new(StartObservation::default())),
                    epoch,
                    restart,
                    fresh_handle: false,
                },
            );
        }
    }

    pub(super) async fn with_start_observation<F: std::future::Future>(
        &self,
        id: &BundleId,
        future: F,
    ) -> F::Output {
        let observation = self
            .pending_declarations
            .read()
            .await
            .get(id)
            .map(|pending| pending.observation.clone());
        match observation {
            Some(observation) => {
                START_OBSERVATION
                    .scope(
                        observation,
                        self.input_resolver
                            .with_consumers(vec![id.to_string()], future),
                    )
                    .await
            }
            None => future.await,
        }
    }

    pub(super) async fn finish_start_observation(
        &self,
        id: &BundleId,
        result: &ComputerResult<()>,
    ) {
        // Commit receipts atomically with handle generation replacement and its receipt reset.
        let _snapshot = self.runtime_snapshot_lock.lock().await;
        let Some(pending) = self.pending_declarations.write().await.remove(id) else {
            return;
        };
        let (created_bundle, definitions) = {
            let mut observation = pending
                .observation
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            (
                observation.created_bundle.take(),
                std::mem::take(&mut observation.definitions),
            )
        };
        // HTTP clients are constructed internally by the SDK and bypass its public factory.
        // Without an actual materialization receipt we cannot infer adoption from Ok(()) or
        // pre-call connection state. Existing Stdio/SSE clients with no factory call are no-ops.
        let http = pending
            .declaration
            .config
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind.eq_ignore_ascii_case("http"));
        if created_bundle.is_none() && !http {
            self.input_resolver.finish_consumer(id.as_str(), true);
            return;
        }
        self.input_resolver.finish_consumer(id.as_str(), false);
        let mut declarations = self.consumed_declarations.write().await;
        // Restart always materializes under the SDK bundle lock. A freshly built handle has
        // no previous HTTP process that could satisfy an idempotent Start. Both also require
        // an unchanged declaration epoch; ordinary HTTP Start has no public receipt.
        let proven_http_start = http
            && (pending.restart || pending.fresh_handle)
            && (result.is_ok() || Self::is_expected_oauth_required(result));
        if (created_bundle.as_ref() == Some(id) || proven_http_start)
            && pending.epoch.is_some()
            && pending.epoch == self.declaration_epoch.stable()
        {
            let mut declaration = pending.declaration;
            declaration.inputs.extend(definitions.clone());
            declaration.observed_inputs = definitions;
            declarations.insert(id.clone(), declaration);
        } else {
            declarations.remove(id);
        }
    }

    pub(crate) async fn config_adoption_status(&self) -> ConfigAdoptionStatus {
        let _lifecycle = self.lifecycle_lock.lock().await;
        let mut status = ConfigAdoptionStatus {
            incarnation: self.runtime_incarnation,
            generation: self.runtime_generation(),
            state: "current".into(),
            changes: vec![],
            unknown_resources: vec![],
            plugin_owners: BTreeMap::new(),
        };
        if !self.is_running().await {
            status.state = "waiting_start".into();
            return status;
        }
        let context = instance_config_context(&self.instance, &self.skill_home_base);
        if !context.validate().is_valid() {
            status.state = "unknown".into();
            return status;
        }
        let saved = context.load();
        let current_home = self.sdk_skill_home().await;
        let desired_home = self.configured_skill_home();
        if current_home != desired_home {
            status.changes.push(AdoptionChange {
                owner: None,
                resource_id: "skill_home".into(),
                section: "skills".into(),
                reason: "skill_home".into(),
                action: AdoptionAction::RestartComputer,
                current_value: Some(current_home.display().to_string()),
                saved_value: Some(desired_home.display().to_string()),
            });
        }
        let concurrency = self.adopted_start_concurrency.load(Ordering::Acquire);
        if concurrency != self.instance.mcp_start_concurrency {
            status.changes.push(AdoptionChange {
                owner: None,
                resource_id: "mcp_start_concurrency".into(),
                section: "general".into(),
                reason: "startup".into(),
                action: AdoptionAction::RestartComputer,
                current_value: Some(concurrency.to_string()),
                saved_value: Some(self.instance.mcp_start_concurrency.to_string()),
            });
        }
        let configs = self.sdk_mcp_server_config_map().await;
        let active: HashSet<_> = self
            .computer
            .read()
            .await
            .get_server_runtime_statuses()
            .await
            .into_iter()
            .filter(|s| s.is_started())
            .map(|s| s.bundle_id)
            .collect();
        let declarations = self.consumed_declarations.read().await;
        let mut desired_ids = HashSet::new();
        let desired_inputs: HashMap<_, _> =
            saved.inputs.inputs.iter().map(|d| (d.id(), d)).collect();
        for server in saved.mcp.servers {
            let id = resolve_bundle_id(&server.config);
            if server.origin == ProvenanceScope::Plugin
                || is_reserved_built_in_bundle_id(id.as_str())
                || self.plugin_mcp_server_owner_inner(&id).await.is_some()
            {
                continue;
            }
            desired_ids.insert(id.clone());
            let running = active.contains(&id);
            let process = declarations.get(&id);
            if running && process.is_none() && !server.config.disabled() {
                status.unknown_resources.push(id.to_string());
            }
            let action = if server.config.disabled() {
                AdoptionAction::StopMcp
            } else if running {
                AdoptionAction::RestartMcp
            } else {
                AdoptionAction::StartMcp
            };
            let mut reason = None;
            if server.config.disabled() {
                if running {
                    reason = Some("disabled_running");
                }
            } else if running {
                if process.is_some_and(|actual| actual.config != canonical(&server.config)) {
                    reason = Some("mcp_config");
                }
            } else if let Some(current) = configs.get(&id) {
                let actual = canonical(current);
                if actual != canonical(&server.config) {
                    reason = Some("mcp_config");
                }
            } else {
                reason = Some("mcp_config");
            }
            if !server.config.disabled() && running {
                for key in referenced_input_ids(&canonical(&server.config)) {
                    if process.is_some_and(|process| {
                        process.inputs.get(&key).cloned()
                            != desired_inputs
                                .get(key.as_str())
                                .and_then(|d| serde_json::to_value(d).ok())
                    }) {
                        reason = Some("input_definition");
                    }
                    match self
                        .input_resolver
                        .input_adoption_changed(id.as_str(), &key)
                    {
                        Some(true) => reason = Some("input_value"),
                        None => status.unknown_resources.push(format!("{}:{}", id, key)),
                        _ => {}
                    }
                }
            }
            if let Some(reason) = reason {
                status.changes.push(AdoptionChange {
                    owner: None,
                    resource_id: id.to_string(),
                    section: "mcp".into(),
                    reason: reason.into(),
                    action,
                    current_value: None,
                    saved_value: None,
                });
            }
        }
        for (id, config) in &configs {
            if is_reserved_built_in_bundle_id(id.as_str()) {
                continue;
            }
            if let Some(owner) = self.plugin_mcp_server_owner_inner(id).await {
                if active.contains(id) {
                    let referenced = referenced_input_ids(&canonical(config));
                    if !referenced.is_empty() {
                        status.plugin_owners.insert(id.to_string(), owner.clone());
                        let definitions = plugin_definition_changed(
                            &owner,
                            &referenced,
                            declarations.get(id),
                            &desired_inputs,
                            &*self.plugin_runtime_inputs.read().await,
                        );
                        let values = self
                            .input_resolver
                            .consumer_adoption_changed(id.as_str(), &referenced);
                        if definitions.is_none() || values.is_none() {
                            status.unknown_resources.push(id.to_string());
                        }
                        let reason = if values == Some(true) {
                            Some("input_value")
                        } else if definitions == Some(true) {
                            Some("input_definition")
                        } else {
                            None
                        };
                        if let Some(reason) = reason {
                            status.changes.push(AdoptionChange {
                                owner: Some(owner),
                                resource_id: id.to_string(),
                                section: "plugins".into(),
                                reason: reason.into(),
                                action: AdoptionAction::ManagePlugin,
                                current_value: None,
                                saved_value: None,
                            });
                        }
                    }
                }
                continue;
            }
            if active.contains(id) && !desired_ids.contains(id) {
                status.changes.push(AdoptionChange {
                    owner: None,
                    resource_id: id.to_string(),
                    section: "mcp".into(),
                    reason: "mcp_config".into(),
                    action: AdoptionAction::StopMcp,
                    current_value: None,
                    saved_value: None,
                });
            }
        }
        status.state = if !status.changes.is_empty() {
            "pending"
        } else if !status.unknown_resources.is_empty() {
            "unknown"
        } else {
            "current"
        }
        .into();
        status
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ControlledClient {
        connected: AtomicBool,
        slow: Option<Arc<(Notify, Notify)>>,
    }
    #[async_trait::async_trait]
    impl a2c_smcp::smcp_computer::mcp_clients::model::MCPClientProtocol for ControlledClient {
        fn state(&self) -> a2c_smcp::smcp_computer::mcp_clients::model::ClientState {
            use a2c_smcp::smcp_computer::mcp_clients::model::ClientState;
            if self.connected.load(Ordering::SeqCst) {
                ClientState::Connected
            } else {
                ClientState::Disconnected
            }
        }
        async fn connect(
            &self,
        ) -> Result<(), a2c_smcp::smcp_computer::mcp_clients::model::MCPClientError> {
            if let Some(slow) = &self.slow {
                slow.0.notify_one();
                slow.1.notified().await;
            }
            self.connected.store(true, Ordering::SeqCst);
            Ok(())
        }
        async fn disconnect(
            &self,
        ) -> Result<(), a2c_smcp::smcp_computer::mcp_clients::model::MCPClientError> {
            self.connected.store(false, Ordering::SeqCst);
            Ok(())
        }
        async fn list_tools(
            &self,
        ) -> Result<Vec<Tool>, a2c_smcp::smcp_computer::mcp_clients::model::MCPClientError>
        {
            Ok(vec![])
        }
        async fn call_tool(
            &self,
            _: &str,
            _: Value,
        ) -> Result<CallToolResult, a2c_smcp::smcp_computer::mcp_clients::model::MCPClientError>
        {
            Err(
                a2c_smcp::smcp_computer::mcp_clients::model::MCPClientError::ProtocolError(
                    "unused".into(),
                ),
            )
        }
        async fn list_windows(
            &self,
        ) -> Result<Vec<Resource>, a2c_smcp::smcp_computer::mcp_clients::model::MCPClientError>
        {
            Ok(vec![])
        }
        async fn list_resources_page(
            &self,
            _: Option<String>,
        ) -> Result<
            (Vec<Resource>, Option<String>),
            a2c_smcp::smcp_computer::mcp_clients::model::MCPClientError,
        > {
            Ok((vec![], None))
        }
        async fn get_window_detail(
            &self,
            _: Resource,
        ) -> Result<ReadResourceResult, a2c_smcp::smcp_computer::mcp_clients::model::MCPClientError>
        {
            Err(
                a2c_smcp::smcp_computer::mcp_clients::model::MCPClientError::ProtocolError(
                    "unused".into(),
                ),
            )
        }
        async fn subscribe_window(
            &self,
            _: Resource,
        ) -> Result<(), a2c_smcp::smcp_computer::mcp_clients::model::MCPClientError> {
            Ok(())
        }
        async fn unsubscribe_window(
            &self,
            _: Resource,
        ) -> Result<(), a2c_smcp::smcp_computer::mcp_clients::model::MCPClientError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn sdk_queue_uses_actual_factory_receipt_across_recovery_disconnect_and_redeclaration() {
        use a2c_smcp::smcp_computer::mcp_clients::model::MCPClientProtocol;
        use futures_util::poll;
        for scenario in ["recover", "disconnect", "redeclare"] {
            let temp = tempfile::tempdir().unwrap();
            let runtime = Arc::new(ComputerInstanceRuntime::new(
                ComputerInstance::new("queued", "Queued"),
                temp.path().into(),
            ));
            let clients: Arc<StdMutex<Vec<Arc<ControlledClient>>>> = Arc::default();
            let slow = Arc::new((Notify::new(), Notify::new()));
            let factory_clients = clients.clone();
            let factory_slow = slow.clone();
            let factory: ClientFactory = Arc::new(move |config, _| {
                observe_materialization(&config);
                let is_slow = config.name() == "slow";
                let client = Arc::new(ControlledClient {
                    connected: AtomicBool::new(false),
                    slow: is_slow.then(|| factory_slow.clone()),
                });
                if !is_slow {
                    factory_clients.lock().unwrap().push(client.clone());
                }
                client
            });
            {
                let mut handle = runtime.computer.write().await;
                let placeholder = Computer::new(
                    "placeholder",
                    InstanceSession::new("placeholder"),
                    None,
                    None,
                    false,
                    true,
                );
                let computer = std::mem::replace(&mut *handle, placeholder);
                *handle = computer
                    .with_client_factory(factory)
                    .with_mcp_start_concurrency(1);
            }
            let config = |name: &str, command: &str| {
                serde_json::from_value::<MCPServerConfig>(serde_json::json!({
                "type":"Stdio", "name":name, "bundle_id":name, "server_parameters":{"command":command,"args":[],"env":{}}
            })).unwrap()
            };
            let old = config("target", "old");
            let next = config("target", "next");
            let target = resolve_bundle_id(&old);
            let slow_id = BundleId::try_from("slow").unwrap();
            runtime
                .computer
                .read()
                .await
                .mount_server(old.clone())
                .await
                .unwrap();
            runtime
                .computer
                .read()
                .await
                .mount_server(config("slow", "slow"))
                .await
                .unwrap();
            runtime.computer.read().await.boot_up().await.unwrap();
            runtime.record_consumed_declaration(&target, false).await;
            {
                let computer = runtime.computer.read().await;
                runtime
                    .with_start_observation(&target, Box::pin(computer.start_mcp_client(&target)))
                    .await
                    .unwrap();
            }
            runtime.finish_start_observation(&target, &Ok(())).await;
            let old_client = clients.lock().unwrap()[0].clone();
            old_client
                .connected
                .store(scenario == "disconnect", Ordering::SeqCst);
            {
                let _mutation = runtime.declaration_epoch.begin();
                runtime
                    .computer
                    .read()
                    .await
                    .mount_server(next.clone())
                    .await
                    .unwrap();
            }
            let slow_runtime = runtime.clone();
            let slow_task = tokio::spawn(async move {
                slow_runtime
                    .computer
                    .read()
                    .await
                    .start_mcp_client(&slow_id)
                    .await
            });
            slow.0.notified().await;
            runtime.record_consumed_declaration(&target, false).await;
            let computer = runtime.computer.read().await;
            let mut queued = Box::pin(
                runtime
                    .with_start_observation(&target, Box::pin(computer.start_mcp_client(&target))),
            );
            assert!(
                poll!(queued.as_mut()).is_pending(),
                "the real SDK concurrency gate must hold the call"
            );
            old_client
                .connected
                .store(scenario == "recover", Ordering::SeqCst);
            if scenario == "redeclare" {
                let _mutation = runtime.declaration_epoch.begin();
                computer
                    .mount_server(config("target", "replacement"))
                    .await
                    .unwrap();
            }
            slow.1.notify_one();
            queued.await.unwrap();
            slow_task.await.unwrap().unwrap();
            drop(computer);
            runtime.finish_start_observation(&target, &Ok(())).await;
            let receipts = runtime.consumed_declarations.read().await;
            match scenario {
                "recover" => {
                    assert_eq!(
                        clients.lock().unwrap().len(),
                        1,
                        "SDK recovered old client and did not materialize"
                    );
                    assert_eq!(receipts[&target].config, canonical(&old));
                }
                "disconnect" => {
                    assert_eq!(
                        clients.lock().unwrap().len(),
                        2,
                        "SDK actually started after the live connection dropped"
                    );
                    assert_eq!(receipts[&target].config, canonical(&next));
                }
                _ => assert!(
                    !receipts.contains_key(&target),
                    "a changed declaration cannot receive the queued candidate's receipt"
                ),
            }
            drop(receipts);
            assert!(
                clients.lock().unwrap().last().unwrap().state()
                    == a2c_smcp::smcp_computer::mcp_clients::model::ClientState::Connected
            );
            // An older batch entry point must wait until a prepared call finishes its handoff.
            let lease = runtime.mcp_start_observation_lease(&target).await;
            runtime.record_consumed_declaration(&target, false).await;
            let original_observation = runtime.pending_declarations.read().await[&target]
                .observation
                .clone();
            let mut legacy =
                Box::pin(runtime.start_mcp_servers_best_effort_inner(vec![target.clone()], false));
            assert!(poll!(legacy.as_mut()).is_pending());
            assert!(Arc::ptr_eq(
                &runtime.pending_declarations.read().await[&target].observation,
                &original_observation
            ));
            runtime.finish_start_observation(&target, &Ok(())).await;
            drop(lease);
            assert!(legacy.await.is_empty());
            runtime.try_shutdown().await.unwrap();
        }
    }

    #[test]
    fn uncertain_value_invalidates_user_and_plugin_receipts_until_reobserved() {
        let root = tempfile::tempdir().unwrap();
        let observations = InputObservations::for_root(root.path());
        let old = serde_json::json!("old-private-value");
        let next = serde_json::json!("next-private-value");
        let consumers = vec!["user-mcp".to_string(), "plugin-mcp".to_string()];
        let referenced = ["token".to_string()].into_iter().collect();
        observations.consumed(&consumers, "token", &old);
        observations.consumed(&["unrelated".into()], "other", &old);
        observations.invalidate_value("token");
        for consumer in &consumers {
            assert_eq!(observations.changed(consumer, "token"), None);
            assert_eq!(observations.consumer_status(consumer, &referenced), None);
        }
        assert_eq!(observations.changed("unrelated", "other"), Some(false));
        // A reliable read establishes the current value without acknowledging old processes.
        observations.consumed(&["new-mcp".into()], "token", &next);
        assert_eq!(observations.changed("user-mcp", "token"), Some(true));
        assert_eq!(
            observations.consumer_status("plugin-mcp", &referenced),
            Some(true)
        );
        assert_eq!(observations.changed("new-mcp", "token"), Some(false));
        observations.written("token", Some(&old));
        assert_eq!(
            observations.consumer_status("plugin-mcp", &referenced),
            Some(false)
        );
    }
}
