use crate::commands::connection::{
    connect_connection_target_for_policy_inner, connect_manager_robot_target_for_policy,
    disconnect_smcp_core,
};
use crate::commands::runtime_error::RuntimeActionError;
use crate::commands::runtime_sync::apply_updated_computer_instance;
use crate::services::built_in_tools::{
    command_line_server_config, CommandLineToolPolicy, COMMAND_LINE_BUNDLE_ID,
};
use crate::services::client_control::RemoteControlPolicy;
use crate::services::computer::{
    ClientConnectionStateSnapshot, ClientConnectionStatus, ComputerConnectionPolicy,
    ComputerConnectionTarget, ComputerInstance, ComputerInstanceId, ComputerRuntimeAction,
    ConnectionStateSummary, ManagerRobotBindingState, RobotBindingMetadata,
};
use crate::services::computer_runtime_events::{
    ComputerRuntimeAffectedCapability, ComputerRuntimeProblemSource, ComputerRuntimeSnapshot,
};
use crate::services::input_entry_store::{InputEntryStorageKind, InputEntryStore};
use crate::services::input_resolver::RuntimeInputInteractionMode;
use crate::services::input_value_index;
use crate::services::input_value_store::InputValueStore;
use crate::services::keychain;
use crate::services::manager_client::ManagerError;
use crate::services::observability::{
    redact_text, ActivityEventDraft, ActivityLevel, ActivityOutcome,
};
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputerInstanceStatus {
    pub id: ComputerInstanceId,
    pub name: String,
    pub description: Option<String>,
    pub local_skills_root: Option<PathBuf>,
    pub default_skill_home: PathBuf,
    pub configured_skill_home: PathBuf,
    pub effective_skill_home: PathBuf,
    pub running: bool,
    pub runtime: ComputerRuntimeSnapshot,
    pub connection_state: ClientConnectionStateSnapshot,
    // Compatibility projections for existing non-runtime consumers. `connection_state` is the
    // only versioned source of truth and every alias below is derived from the same snapshot.
    pub connected: bool,
    pub client_connection_present: bool,
    pub connection_revision: u64,
    pub connection_context: Option<ConnectionStateSummary>,
    pub mcp_server_count: usize,
    pub robot_binding: Option<RobotBindingMetadata>,
    pub connection_policy: ComputerConnectionPolicy,
    pub remote_control: RemoteControlPolicy,
    pub command_line: CommandLineToolPolicy,
    pub mcp_start_concurrency: usize,
    pub connection: Option<ConnectionStateSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateComputerInstanceRequest {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameComputerInstanceRequest {
    pub id: ComputerInstanceId,
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub mcp_start_concurrency: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateComputerInstanceRequest {
    pub source_id: ComputerInstanceId,
    pub name: String,
    pub description: Option<String>,
    pub copy_robot_binding: bool,
    pub connection_target_id: Option<String>,
    #[serde(default)]
    pub skill_home_mode: DuplicateSkillHomeMode,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DuplicateSkillHomeMode {
    #[default]
    Empty,
    Copy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateComputerConnectionPolicyRequest {
    pub id: ComputerInstanceId,
    pub target: Option<ComputerConnectionTarget>,
    pub auto_connect: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateComputerSkillHomeRequest {
    pub id: ComputerInstanceId,
    pub local_skills_root: Option<String>,
}

#[tauri::command]
pub async fn list_computer_instances(
    state: State<'_, AppState>,
) -> Result<Vec<ComputerInstanceStatus>, RuntimeActionError> {
    list_computer_instances_core(&state).await
}

pub async fn list_computer_instances_core(
    state: &AppState,
) -> Result<Vec<ComputerInstanceStatus>, RuntimeActionError> {
    // Runtime membership is the publication boundary. Reads never repair persisted state or wait
    // for a Computer mutation: create/update/delete publish one authoritative runtime only after
    // their durable work is ready, and startup recovery constructs the initial registry.
    let runtimes = state.computer_registry.list_runtimes().await;
    let mut statuses = Vec::with_capacity(runtimes.len());
    for runtime in runtimes {
        let instance = runtime.instance.clone();
        statuses.push(status_from_instance(&instance, &runtime).await);
    }

    Ok(statuses)
}

#[tauri::command]
pub async fn get_computer_instance_status(
    state: State<'_, AppState>,
    id: ComputerInstanceId,
) -> Result<ComputerInstanceStatus, RuntimeActionError> {
    get_computer_instance_status_core(&state, id).await
}

pub async fn get_computer_instance_status_core(
    state: &AppState,
    id: ComputerInstanceId,
) -> Result<ComputerInstanceStatus, RuntimeActionError> {
    let runtime =
        state.computer_registry.runtime(&id).await.ok_or_else(|| {
            RuntimeActionError::runtime(format!("Computer instance not found: {id}"))
        })?;
    let instance = runtime.instance.clone();

    Ok(status_from_instance(&instance, &runtime).await)
}

#[tauri::command]
pub async fn create_computer_instance(
    state: State<'_, AppState>,
    request: CreateComputerInstanceRequest,
) -> Result<ComputerInstanceStatus, String> {
    create_computer_instance_core(&state, request).await
}

pub async fn create_computer_instance_core(
    state: &AppState,
    request: CreateComputerInstanceRequest,
) -> Result<ComputerInstanceStatus, String> {
    create_computer_instance_with_trigger(state, request, "user").await
}

pub(crate) async fn create_computer_instance_with_trigger(
    state: &AppState,
    request: CreateComputerInstanceRequest,
    trigger: &str,
) -> Result<ComputerInstanceStatus, String> {
    let started = std::time::Instant::now();
    let name = match normalize_name(&request.name) {
        Ok(name) => name,
        Err(error) => {
            let result = Err(error);
            record_computer_profile_activity(state, None, "create", trigger, started, &result)
                .await;
            return result;
        }
    };
    let instance = ComputerInstance {
        id: generate_instance_id(),
        name,
        description: normalize_optional_text(request.description),
        mcp_servers: Vec::new(),
        inputs: Vec::new(),
        input_values: Default::default(),
        local_skills_root: None,
        connection_policy: ComputerConnectionPolicy::default(),
        remote_control: RemoteControlPolicy::default(),
        command_line: CommandLineToolPolicy::default(),
        mcp_start_concurrency: crate::services::computer::DEFAULT_MCP_START_CONCURRENCY,
        robot_binding: None,
    };

    let computer_id = instance.id.clone();
    let result = async {
        state
            .config
            .add_computer_instance(instance.clone())
            .map_err(|error| error.to_string())?;
        let instance_storage_root = state.config.computer_instance_storage_root(&instance.id);
        let instance = match load_hydrated_computer_instance(state, &instance.id) {
            Ok(instance) => instance,
            Err(error) => {
                return Err(rollback_failed_computer_creation(
                    state,
                    &instance.id,
                    &instance_storage_root,
                    "create",
                    error.to_string(),
                )
                .await)
            }
        };
        let runtime = match state
            .computer_registry
            .upsert_runtime(instance.clone())
            .await
        {
            Ok(runtime) => runtime,
            Err(error) => {
                return Err(rollback_failed_computer_creation(
                    state,
                    &instance.id,
                    &instance_storage_root,
                    "create",
                    error,
                )
                .await)
            }
        };

        Ok(status_from_instance(&instance, &runtime).await)
    }
    .await;
    record_computer_profile_activity(
        state,
        Some(&computer_id),
        "create",
        trigger,
        started,
        &result,
    )
    .await;
    result
}

#[tauri::command]
pub async fn rename_computer_instance(
    state: State<'_, AppState>,
    request: RenameComputerInstanceRequest,
) -> Result<ComputerInstanceStatus, String> {
    rename_computer_instance_core(&state, request).await
}

pub async fn rename_computer_instance_core(
    state: &AppState,
    request: RenameComputerInstanceRequest,
) -> Result<ComputerInstanceStatus, String> {
    rename_computer_instance_with_trigger(state, request, "user").await
}

pub(crate) async fn rename_computer_instance_with_trigger(
    state: &AppState,
    request: RenameComputerInstanceRequest,
    trigger: &str,
) -> Result<ComputerInstanceStatus, String> {
    let started = std::time::Instant::now();
    let computer_id = request.id.clone();
    let result = rename_computer_instance_transaction(state, request).await;
    record_computer_profile_activity(
        state,
        Some(&computer_id),
        "update",
        trigger,
        started,
        &result,
    )
    .await;
    result
}

async fn rename_computer_instance_transaction(
    state: &AppState,
    request: RenameComputerInstanceRequest,
) -> Result<ComputerInstanceStatus, String> {
    use crate::services::computer::MAX_MCP_START_CONCURRENCY;
    if let Some(concurrency) = request.mcp_start_concurrency {
        if concurrency == 0 || concurrency > MAX_MCP_START_CONCURRENCY {
            return Err(format!(
                "MCP start concurrency must be between 1 and {MAX_MCP_START_CONCURRENCY}"
            ));
        }
    }
    let _operation_guard = state.computer_registry.operation_lease(&request.id).await;
    let name = normalize_name(&request.name)?;
    let previous = state
        .config
        .get_computer_instance(&request.id)
        .map_err(|error| error.to_string())?;
    let updated = state
        .config
        .update_computer_instance(&request.id, |instance| {
            instance.name = name;
            instance.description = normalize_optional_text(request.description);
            if let Some(concurrency) = request.mcp_start_concurrency {
                instance.mcp_start_concurrency = concurrency;
            }
        })
        .map_err(|error| error.to_string())?;
    let runtime = apply_updated_computer_instance(state, previous, updated.clone()).await?;

    Ok(status_from_instance(&updated, &runtime).await)
}

#[tauri::command]
pub async fn duplicate_computer_instance(
    state: State<'_, AppState>,
    request: DuplicateComputerInstanceRequest,
) -> Result<ComputerInstanceStatus, String> {
    duplicate_computer_instance_core(&state, request).await
}

pub async fn duplicate_computer_instance_core(
    state: &AppState,
    request: DuplicateComputerInstanceRequest,
) -> Result<ComputerInstanceStatus, String> {
    duplicate_computer_instance_with_trigger(state, request, "user").await
}

pub(crate) async fn duplicate_computer_instance_with_trigger(
    state: &AppState,
    mut request: DuplicateComputerInstanceRequest,
    trigger: &str,
) -> Result<ComputerInstanceStatus, String> {
    let started = std::time::Instant::now();
    request.name = match normalize_name(&request.name) {
        Ok(name) => name,
        Err(error) => {
            let result = Err(error);
            record_computer_profile_activity(state, None, "duplicate", trigger, started, &result)
                .await;
            return result;
        }
    };
    let transaction_state = state.clone();
    let transaction_computer_id = generate_instance_id();
    let transaction_trigger = trigger.to_string();
    let result = tokio::spawn(async move {
        let result = duplicate_computer_instance_transaction(
            &transaction_state,
            request,
            transaction_computer_id.clone(),
        )
        .await;
        record_computer_profile_activity(
            &transaction_state,
            Some(&transaction_computer_id),
            "duplicate",
            &transaction_trigger,
            started,
            &result,
        )
        .await;
        result
    })
    .await
    .map_err(|error| format!("Duplicate Computer transaction task failed: {error}"))?;
    result
}

async fn duplicate_computer_instance_transaction(
    state: &AppState,
    request: DuplicateComputerInstanceRequest,
    duplicate_id: ComputerInstanceId,
) -> Result<ComputerInstanceStatus, String> {
    let target_override = normalize_optional_text(request.connection_target_id);
    if let Some(target_id) = target_override.as_ref() {
        state
            .config
            .get_manual_smcp_target(target_id)
            .map_err(|error| error.to_string())?;
    }
    // Snapshot the source under its own transaction gate, then release it before copying files.
    // The source authority fields are refreshed again at publication, so Manager Context cleanup
    // can proceed while the potentially slow copy is running.
    let source_guard = state
        .computer_registry
        .operation_lease(&request.source_id)
        .await;
    let mut instance = state
        .config
        .get_computer_instance(&request.source_id)
        .map_err(|error| error.to_string())?;
    let source_id = instance.id.clone();
    let source_runtime = state
        .computer_registry
        .runtime(&source_id)
        .await
        .ok_or_else(|| format!("Computer instance not found: {source_id}"))?;
    let skill_snapshot = source_runtime.acquire_skill_snapshot_lease().await?;
    let source_skill_root = skill_snapshot.configured_skill_home();
    instance.id = duplicate_id;
    instance.name = request.name;
    instance.description = normalize_optional_text(request.description);
    instance.local_skills_root = None;
    instance.remote_control = RemoteControlPolicy::default();
    instance.command_line = CommandLineToolPolicy::default();
    let destination_skill_root = state.config.default_local_skills_root(&instance.id);
    let destination_storage_root = state.config.computer_instance_storage_root(&instance.id);
    if !request.copy_robot_binding {
        instance.robot_binding = None;
    }
    if let Some(target_id) = target_override.as_ref() {
        instance.connection_policy.target = Some(ComputerConnectionTarget::manual_smcp(target_id));
    }
    drop(source_guard);
    state
        .config
        .add_computer_instance(instance.clone())
        .map_err(|error| error.to_string())?;
    if let Err(error) = state.sdk_config.duplicate(&source_id, &instance.id) {
        drop(skill_snapshot);
        return Err(rollback_failed_computer_creation(
            state,
            &instance.id,
            &destination_storage_root,
            "duplicate",
            error.to_string(),
        )
        .await);
    }
    if let Err(error) = prepare_duplicate_skill_home(
        &source_skill_root,
        &destination_skill_root,
        request.skill_home_mode,
    )
    .await
    {
        drop(skill_snapshot);
        return Err(rollback_failed_computer_creation(
            state,
            &instance.id,
            &destination_storage_root,
            "duplicate",
            error,
        )
        .await);
    }
    drop(skill_snapshot);
    // Only runtime publication participates in the cross-Computer membership transaction. Re-read
    // the source after taking membership in the canonical order and refresh authority-bearing
    // fields so a departing Manager Context cannot be copied after its cleanup snapshot.
    let membership_guard = state.computer_registry.membership_lease().await;
    let source_guard = state
        .computer_registry
        .operation_lease(&request.source_id)
        .await;
    let target_guard = state.connection_target_lock.lock().await;
    if let Some(target_id) = target_override.as_ref() {
        if let Err(error) = state.config.get_manual_smcp_target(target_id) {
            drop(target_guard);
            drop(source_guard);
            drop(membership_guard);
            return Err(rollback_failed_computer_creation(
                state,
                &instance.id,
                &destination_storage_root,
                "duplicate",
                format!("connection target changed before publication: {error}"),
            )
            .await);
        }
    }
    let latest_source = match state.config.get_computer_instance(&request.source_id) {
        Ok(source) => source,
        Err(error) => {
            drop(target_guard);
            drop(source_guard);
            drop(membership_guard);
            return Err(rollback_failed_computer_creation(
                state,
                &instance.id,
                &destination_storage_root,
                "duplicate",
                format!("source Computer changed before publication: {error}"),
            )
            .await);
        }
    };
    let mut inherited_manager_contexts = Vec::new();
    if let Some(ComputerConnectionTarget::ManagerRobot { context_key, .. }) =
        latest_source.connection_policy.target.as_ref()
    {
        inherited_manager_contexts.push(context_key.clone());
    }
    if let Some(context_key) = latest_source
        .robot_binding
        .as_ref()
        .and_then(|binding| binding.context_key.clone())
    {
        if !inherited_manager_contexts.contains(&context_key) {
            inherited_manager_contexts.push(context_key);
        }
    }
    let departing_inherited_contexts = inherited_manager_contexts
        .into_iter()
        .filter(|context| {
            state
                .computer_registry
                .is_manager_context_departing(context)
        })
        .collect::<Vec<_>>();
    let latest_policy = if target_override.is_none() {
        Some(latest_source.connection_policy)
    } else {
        None
    };
    let latest_binding = request
        .copy_robot_binding
        .then_some(latest_source.robot_binding)
        .flatten();
    instance = match state
        .config
        .update_computer_instance(&instance.id, |candidate| {
            if let Some(policy) = latest_policy.clone() {
                candidate.connection_policy = policy;
            }
            candidate.robot_binding = latest_binding.clone();
            for context in &departing_inherited_contexts {
                crate::commands::manager::make_departing_manager_binding_dormant(
                    candidate, context,
                );
            }
        }) {
        Ok(instance) => instance,
        Err(error) => {
            drop(target_guard);
            drop(source_guard);
            drop(membership_guard);
            return Err(rollback_failed_computer_creation(
                state,
                &instance.id,
                &destination_storage_root,
                "duplicate",
                error.to_string(),
            )
            .await);
        }
    };
    let duplicate_id = instance.id.clone();
    let instance = match state.hydrate_computer_instance(instance) {
        Ok(instance) => instance,
        Err(error) => {
            drop(target_guard);
            drop(source_guard);
            drop(membership_guard);
            return Err(rollback_failed_computer_creation(
                state,
                &duplicate_id,
                &destination_storage_root,
                "duplicate",
                error.to_string(),
            )
            .await);
        }
    };
    let runtime = match state
        .computer_registry
        .upsert_runtime(instance.clone())
        .await
    {
        Ok(runtime) => runtime,
        Err(error) => {
            drop(target_guard);
            drop(source_guard);
            drop(membership_guard);
            return Err(rollback_failed_computer_creation(
                state,
                &duplicate_id,
                &destination_storage_root,
                "duplicate",
                error,
            )
            .await);
        }
    };
    drop(target_guard);
    drop(source_guard);
    drop(membership_guard);

    Ok(status_from_instance(&instance, &runtime).await)
}

#[tauri::command]
pub async fn delete_computer_instance(
    state: State<'_, AppState>,
    id: ComputerInstanceId,
) -> Result<(), String> {
    delete_computer_instance_core(&state, id).await
}

async fn record_computer_profile_activity<T>(
    state: &AppState,
    computer_id: Option<&str>,
    operation: &str,
    trigger: &str,
    started: std::time::Instant,
    result: &Result<T, String>,
) {
    let (level, outcome, message, error) = match result {
        Ok(_) => (
            ActivityLevel::Info,
            ActivityOutcome::Succeeded,
            format!("Computer profile {operation} succeeded"),
            None,
        ),
        Err(error) => (
            ActivityLevel::Warn,
            ActivityOutcome::Failed,
            format!("Computer profile {operation} failed"),
            Some(redact_text(error)),
        ),
    };
    let mut activity = match computer_id {
        Some(computer_id) => ActivityEventDraft::computer(
            computer_id,
            level,
            "computer",
            "computer_profile",
            operation,
            outcome,
            message,
        ),
        None => ActivityEventDraft::client(
            level,
            "computer",
            "computer_profile",
            operation,
            outcome,
            message,
        ),
    };
    activity.fields = Some(serde_json::json!({
        "app_version": env!("CARGO_PKG_VERSION"),
        "trigger": trigger,
        "target_id": computer_id,
        "duration_ms": started.elapsed().as_millis(),
        "error": error,
    }));
    activity.correlation_id = Some(uuid::Uuid::new_v4().to_string());
    if let Err(error) = state.observability.record_activity_async(activity).await {
        log::error!("failed to persist Computer profile {operation} activity: {error}");
    }
}

pub async fn delete_computer_instance_core(
    state: &AppState,
    id: ComputerInstanceId,
) -> Result<(), String> {
    delete_computer_instance_with_trigger(state, id, "user").await
}

pub(crate) async fn delete_computer_instance_with_trigger(
    state: &AppState,
    id: ComputerInstanceId,
    trigger: &str,
) -> Result<(), String> {
    let started = std::time::Instant::now();
    let result = delete_computer_instance_transaction(state, &id).await;
    record_computer_profile_activity(state, Some(&id), "delete", trigger, started, &result).await;
    result
}

async fn delete_computer_instance_transaction(
    state: &AppState,
    id: &ComputerInstanceId,
) -> Result<(), String> {
    let _operation_guard = state.computer_registry.operation_lease(id).await;
    let instance_storage_root = state.config.computer_instance_storage_root(id);
    let persisted_instance = state
        .config
        .get_computer_instance(id)
        .map_err(|error| error.to_string())?;
    // Retire and drain the runtime before reading or mutating InputEntry storage. This prevents
    // an in-flight SDK resolver from adopting legacy metadata or reading a value while Computer
    // deletion snapshots and removes the authoritative Entry set.
    let prepared_removal = state.computer_registry.prepare_runtime_removal(id).await?;
    let input_storage = snapshot_computer_input_storage(state, &persisted_instance)?;
    if let Err(error) = delete_computer_input_storage(state, id, &input_storage) {
        let rollback = restore_computer_input_storage(state, id, &input_storage);
        drop(prepared_removal);
        return match rollback {
            Ok(()) => Err(error),
            Err(rollback_error) => Err(format!(
                "{error}; additionally failed to restore Computer input storage: {rollback_error}"
            )),
        };
    }
    let quarantined_storage = match quarantine_computer_instance_storage(&instance_storage_root)
        .await
    {
        Ok(quarantined_storage) => quarantined_storage,
        Err(error) => {
            drop(prepared_removal);
            return match restore_computer_input_storage(state, id, &input_storage) {
                    Ok(()) => Err(error),
                    Err(rollback_error) => Err(format!(
                        "{error}; additionally failed to restore Computer input storage: {rollback_error}"
                    )),
                };
        }
    };
    if let Err(error) = state.config.remove_computer_instance(id) {
        let mut rollback_errors = Vec::new();
        if let Some(quarantined) = quarantined_storage.as_ref() {
            if let Err(restore_error) =
                restore_quarantined_computer_storage(quarantined, &instance_storage_root).await
            {
                rollback_errors.push(format!("restore SDK storage: {restore_error}"));
            }
        }
        if let Err(restore_error) = restore_computer_input_storage(state, id, &input_storage) {
            rollback_errors.push(format!("restore Computer input storage: {restore_error}"));
        }
        drop(prepared_removal);
        if rollback_errors.is_empty() {
            return Err(error.to_string());
        }
        return Err(format!(
            "Failed to remove Computer profile: {error}; additionally failed to rollback: {}",
            rollback_errors.join("; ")
        ));
    }
    if let Some(prepared_removal) = prepared_removal {
        if let Err(error) = state
            .computer_registry
            .commit_runtime_removal(prepared_removal)
            .await
        {
            let mut rollback_errors = Vec::new();
            if let Err(restore_error) = state
                .config
                .add_computer_instance(persisted_instance.clone())
            {
                rollback_errors.push(format!("restore Computer profile: {restore_error}"));
            }
            if let Some(quarantined) = quarantined_storage.as_ref() {
                if let Err(restore_error) =
                    restore_quarantined_computer_storage(quarantined, &instance_storage_root).await
                {
                    rollback_errors.push(format!("restore SDK storage: {restore_error}"));
                }
            }
            if let Err(restore_error) = restore_computer_input_storage(state, id, &input_storage) {
                rollback_errors.push(format!("restore Computer input storage: {restore_error}"));
            }
            if rollback_errors.is_empty() {
                return Err(error);
            }
            return Err(format!(
                "Failed to shutdown Computer runtime: {error}; additionally failed to rollback: {}",
                rollback_errors.join("; ")
            ));
        }
    }
    cleanup_quarantined_computer_storage(quarantined_storage).await;
    if let Err(error) = state.config.load_computer_instances() {
        log::warn!(
            "Computer '{id}' was deleted, but stale Client Control target cleanup failed: {error}"
        );
    }

    Ok(())
}

#[derive(Debug)]
enum ComputerInputStorageSnapshot {
    Value {
        id: String,
        value: Option<serde_json::Value>,
    },
    Secret {
        id: String,
        secret: Option<String>,
    },
}

fn snapshot_computer_input_storage(
    state: &AppState,
    instance: &ComputerInstance,
) -> Result<Vec<ComputerInputStorageSnapshot>, String> {
    let store = InputEntryStore::for_computer(
        state.config.as_ref(),
        instance.id.clone(),
        state.secret_store.clone(),
    );
    let preferred: BTreeMap<_, _> = state
        .sdk_config
        .load_input_definitions(&instance.id)
        .into_iter()
        .filter(|input| input.supports_persistent_value())
        .map(|input| {
            (
                input.id().to_string(),
                if input.is_secret() {
                    InputEntryStorageKind::Secret
                } else {
                    InputEntryStorageKind::Value
                },
            )
        })
        .collect();
    // Reconcile legacy storage with provenance intact, then let InputEntry metadata alone define
    // which backends this Computer owns. In particular, a current password definition must not
    // grant Keychain authority over a V1 index entry, which was always plain.
    store.migrate_legacy(
        input_value_index::load_with_provenance(state.config.as_ref(), &instance.id)?,
        &preferred,
    )?;
    let mut snapshots = Vec::new();
    for entry in store.list()? {
        snapshots.push(if entry.secret {
            ComputerInputStorageSnapshot::Secret {
                secret: keychain::get_input_secret(
                    state.secret_store.as_ref(),
                    &instance.id,
                    &entry.key,
                )
                .map_err(|error| error.to_string())?,
                id: entry.key,
            }
        } else {
            ComputerInputStorageSnapshot::Value {
                value: entry.value,
                id: entry.key,
            }
        });
    }
    Ok(snapshots)
}

fn delete_computer_input_storage(
    state: &AppState,
    instance_id: &str,
    snapshots: &[ComputerInputStorageSnapshot],
) -> Result<(), String> {
    for snapshot in snapshots {
        match snapshot {
            ComputerInputStorageSnapshot::Value { id, .. } => {
                InputValueStore::for_computer(state.config.as_ref(), instance_id).delete(id)?;
            }
            ComputerInputStorageSnapshot::Secret { id, .. } => {
                keychain::delete_input_secret(state.secret_store.as_ref(), instance_id, id)
                    .map_err(|error| error.to_string())?;
            }
        }
    }
    Ok(())
}

fn restore_computer_input_storage(
    state: &AppState,
    instance_id: &str,
    snapshots: &[ComputerInputStorageSnapshot],
) -> Result<(), String> {
    for snapshot in snapshots {
        match snapshot {
            ComputerInputStorageSnapshot::Value { id, value } => match value {
                Some(value) => {
                    InputValueStore::for_computer(state.config.as_ref(), instance_id).set(id, value)
                }
                None => {
                    InputValueStore::for_computer(state.config.as_ref(), instance_id).delete(id)
                }
            },
            ComputerInputStorageSnapshot::Secret { id, secret } => match secret {
                Some(secret) => {
                    keychain::set_input_secret(state.secret_store.as_ref(), instance_id, id, secret)
                }
                None => keychain::delete_input_secret(state.secret_store.as_ref(), instance_id, id),
            }
            .map_err(|error| error.to_string()),
        }?;
    }
    Ok(())
}

#[tauri::command]
pub async fn start_computer_instance(
    app: AppHandle,
    state: State<'_, AppState>,
    id: ComputerInstanceId,
) -> Result<ComputerInstanceStatus, RuntimeActionError> {
    start_computer_instance_interactive_core(Some(&app), &state, id).await
}

pub async fn start_computer_instance_interactive_core(
    app: Option<&AppHandle>,
    state: &AppState,
    id: ComputerInstanceId,
) -> Result<ComputerInstanceStatus, RuntimeActionError> {
    start_computer_instance_core_with_mode(app, state, id, RuntimeInputInteractionMode::Interactive)
        .await
}

pub async fn start_computer_instance_core(
    app: Option<&AppHandle>,
    state: &AppState,
    id: ComputerInstanceId,
) -> Result<ComputerInstanceStatus, RuntimeActionError> {
    start_computer_instance_core_with_mode(
        app,
        state,
        id,
        RuntimeInputInteractionMode::NonInteractive,
    )
    .await
}

async fn start_computer_instance_core_with_mode(
    app: Option<&AppHandle>,
    state: &AppState,
    id: ComputerInstanceId,
    interaction_mode: RuntimeInputInteractionMode,
) -> Result<ComputerInstanceStatus, RuntimeActionError> {
    let started = std::time::Instant::now();
    let result =
        start_computer_instance_transaction(app, state, id.clone(), interaction_mode).await;
    record_computer_runtime_activity(state, &id, "start", started, &result).await;
    result
}

async fn start_computer_instance_transaction(
    app: Option<&AppHandle>,
    state: &AppState,
    id: ComputerInstanceId,
    interaction_mode: RuntimeInputInteractionMode,
) -> Result<ComputerInstanceStatus, RuntimeActionError> {
    let operation_guard = state.computer_registry.operation_lease(&id).await;
    let instance = state
        .config
        .get_computer_instance(&id)
        .map_err(|error| RuntimeActionError::runtime(error.to_string()))?;
    let current_runtime =
        state.computer_registry.runtime(&id).await.ok_or_else(|| {
            RuntimeActionError::runtime(format!("Computer instance not found: {id}"))
        })?;
    current_runtime
        .ensure_runtime_action(ComputerRuntimeAction::Start)
        .await
        .map_err(RuntimeActionError::from)?;
    let instance = state
        .hydrate_computer_instance(instance)
        .map_err(|error| RuntimeActionError::runtime(error.to_string()))?;
    let mut instance = preflight_command_line_tool(state, instance).await?;
    let runtime = state
        .computer_registry
        .update_runtime_instance_typed(instance.clone())
        .await
        .map_err(RuntimeActionError::from)?;
    // Atomically downgrade after snapshot construction: replacement remains excluded while
    // independent shared operations on this Computer can progress during Runtime Input.
    let shared_operation_guard = operation_guard.downgrade();
    let start_result = match interaction_mode {
        RuntimeInputInteractionMode::Interactive => {
            runtime
                .with_runtime_input_interaction(interaction_mode, runtime.start_interactive())
                .await
        }
        RuntimeInputInteractionMode::NonInteractive => {
            runtime
                .with_runtime_input_interaction(interaction_mode, runtime.start())
                .await
        }
    };
    drop(shared_operation_guard);
    if let Err(error) = start_result {
        let command_line_failed = error.is_command_line_start_failure();
        let error = RuntimeActionError::from(error);
        if instance.command_line.enabled && command_line_failed {
            let _rollback_guard = state.computer_registry.operation_lease(&id).await;
            state
                .computer_registry
                .ensure_current_runtime(&runtime)
                .await
                .map_err(RuntimeActionError::runtime)?;
            instance = disable_command_line_after_failure(state, &id, &error.to_string())
                .await
                .map_err(|restore_error| {
                    RuntimeActionError::runtime(format!(
                        "{error}; command line policy rollback failed: {restore_error}"
                    ))
                })?;
        } else {
            return Err(error);
        }
    }
    record_mcp_start_failure_activities(state, &id, &runtime).await;
    if instance.connection_policy.auto_connect {
        if let Some(target) = instance.connection_policy.target.as_ref() {
            if let Err(error) =
                connect_computer_connection_target_by_policy(app, state, &id, target).await
            {
                if let Err(persist_error) = state
                    .observability
                    .record_activity_async(ActivityEventDraft::computer(
                        &id,
                        ActivityLevel::Warn,
                        "connection",
                        "smcp_connection",
                        "auto_connect",
                        ActivityOutcome::Failed,
                        crate::services::observability::redact_text(&format!(
                            "Auto connect failed: {error}"
                        )),
                    ))
                    .await
                {
                    log::error!("failed to persist auto-connect failure activity: {persist_error}");
                }
            }
        }
    }

    Ok(status_from_instance(&instance, &runtime).await)
}

#[tauri::command]
pub async fn stop_computer_instance(
    state: State<'_, AppState>,
    id: ComputerInstanceId,
) -> Result<ComputerInstanceStatus, String> {
    stop_computer_instance_core(&state, id).await
}

pub async fn stop_computer_instance_core(
    state: &AppState,
    id: ComputerInstanceId,
) -> Result<ComputerInstanceStatus, String> {
    let started = std::time::Instant::now();
    let result = stop_computer_instance_transaction(state, &id).await;
    record_computer_runtime_activity(state, &id, "stop", started, &result).await;
    result
}

async fn stop_computer_instance_transaction(
    state: &AppState,
    id: &ComputerInstanceId,
) -> Result<ComputerInstanceStatus, String> {
    let _operation_guard = state.computer_registry.operation_lease(id).await;
    let instance = state
        .config
        .get_computer_instance(id)
        .map_err(|error| error.to_string())?;
    let runtime = state
        .computer_registry
        .runtime(id)
        .await
        .ok_or_else(|| format!("Computer instance not found: {id}"))?;
    runtime
        .ensure_runtime_action(ComputerRuntimeAction::Stop)
        .await
        .map_err(|error| error.to_string())?;
    runtime.try_shutdown().await?;

    Ok(status_from_instance(&instance, &runtime).await)
}

#[tauri::command]
pub async fn restart_computer_instance(
    app: AppHandle,
    state: State<'_, AppState>,
    id: ComputerInstanceId,
) -> Result<ComputerInstanceStatus, RuntimeActionError> {
    restart_computer_instance_interactive_core(Some(&app), &state, id).await
}

pub async fn restart_computer_instance_interactive_core(
    app: Option<&AppHandle>,
    state: &AppState,
    id: ComputerInstanceId,
) -> Result<ComputerInstanceStatus, RuntimeActionError> {
    restart_computer_instance_core_with_mode(
        app,
        state,
        id,
        RuntimeInputInteractionMode::Interactive,
    )
    .await
}

pub async fn restart_computer_instance_core(
    app: Option<&AppHandle>,
    state: &AppState,
    id: ComputerInstanceId,
) -> Result<ComputerInstanceStatus, RuntimeActionError> {
    restart_computer_instance_core_with_mode(
        app,
        state,
        id,
        RuntimeInputInteractionMode::NonInteractive,
    )
    .await
}

async fn restart_computer_instance_core_with_mode(
    app: Option<&AppHandle>,
    state: &AppState,
    id: ComputerInstanceId,
    interaction_mode: RuntimeInputInteractionMode,
) -> Result<ComputerInstanceStatus, RuntimeActionError> {
    let started = std::time::Instant::now();
    let result =
        restart_computer_instance_transaction(app, state, id.clone(), interaction_mode).await;
    record_computer_runtime_activity(state, &id, "restart", started, &result).await;
    result
}

async fn restart_computer_instance_transaction(
    app: Option<&AppHandle>,
    state: &AppState,
    id: ComputerInstanceId,
    interaction_mode: RuntimeInputInteractionMode,
) -> Result<ComputerInstanceStatus, RuntimeActionError> {
    let operation_guard = state.computer_registry.operation_lease(&id).await;
    let instance = state
        .config
        .get_computer_instance(&id)
        .map_err(|error| RuntimeActionError::runtime(error.to_string()))?;
    let current_runtime =
        state.computer_registry.runtime(&id).await.ok_or_else(|| {
            RuntimeActionError::runtime(format!("Computer instance not found: {id}"))
        })?;
    current_runtime
        .ensure_runtime_action(ComputerRuntimeAction::Restart)
        .await
        .map_err(RuntimeActionError::from)?;
    let instance = state
        .hydrate_computer_instance(instance)
        .map_err(|error| RuntimeActionError::runtime(error.to_string()))?;
    let mut instance = preflight_command_line_tool(state, instance).await?;
    let runtime = state
        .computer_registry
        .update_runtime_instance_typed(instance.clone())
        .await
        .map_err(RuntimeActionError::from)?;
    let shared_operation_guard = operation_guard.downgrade();
    let restart_result = match interaction_mode {
        RuntimeInputInteractionMode::Interactive => {
            runtime
                .with_runtime_input_interaction(interaction_mode, runtime.restart_interactive())
                .await
        }
        RuntimeInputInteractionMode::NonInteractive => {
            runtime
                .with_runtime_input_interaction(interaction_mode, runtime.restart())
                .await
        }
    };
    drop(shared_operation_guard);
    if let Err(error) = restart_result {
        let command_line_failed = error.is_command_line_start_failure();
        let error = RuntimeActionError::from(error);
        if instance.command_line.enabled && command_line_failed {
            let _rollback_guard = state.computer_registry.operation_lease(&id).await;
            state
                .computer_registry
                .ensure_current_runtime(&runtime)
                .await
                .map_err(RuntimeActionError::runtime)?;
            instance = disable_command_line_after_failure(state, &id, &error.to_string())
                .await
                .map_err(|restore_error| {
                    RuntimeActionError::runtime(format!(
                        "{error}; command line policy rollback failed: {restore_error}"
                    ))
                })?;
        } else {
            return Err(error);
        }
    }
    record_mcp_start_failure_activities(state, &id, &runtime).await;
    if instance.connection_policy.auto_connect {
        if let Some(target) = instance.connection_policy.target.as_ref() {
            connect_computer_connection_target_by_policy(app, state, &id, target)
                .await
                .map_err(RuntimeActionError::runtime)?;
        }
    }

    Ok(status_from_instance(&instance, &runtime).await)
}

async fn record_computer_runtime_activity<T, E>(
    state: &AppState,
    computer_id: &str,
    operation: &str,
    started: std::time::Instant,
    result: &Result<T, E>,
) where
    E: std::fmt::Display,
{
    let (level, outcome, message, error) = match result {
        Ok(_) => (
            ActivityLevel::Info,
            ActivityOutcome::Succeeded,
            format!("Computer runtime {operation} succeeded"),
            None,
        ),
        Err(error) => (
            ActivityLevel::Warn,
            ActivityOutcome::Failed,
            format!("Computer runtime {operation} failed"),
            Some(redact_text(&error.to_string())),
        ),
    };
    let mut activity = ActivityEventDraft::computer(
        computer_id,
        level,
        "computer",
        "runtime_lifecycle",
        operation,
        outcome,
        message,
    );
    activity.fields = Some(serde_json::json!({
        "app_version": env!("CARGO_PKG_VERSION"),
        "trigger": "command",
        "target_id": computer_id,
        "duration_ms": started.elapsed().as_millis(),
        "error": error,
    }));
    activity.correlation_id = Some(uuid::Uuid::new_v4().to_string());
    if let Err(error) = state.observability.record_activity_async(activity).await {
        log::error!("failed to persist Computer runtime {operation} activity: {error}");
    }
}

async fn record_mcp_start_failure_activities(
    state: &AppState,
    instance_id: &str,
    runtime: &crate::services::computer::ComputerInstanceRuntime,
) {
    let problems = runtime.runtime_snapshot().await.problems;
    for problem in problems.into_iter().filter(|problem| {
        problem.source == ComputerRuntimeProblemSource::Mcp && problem.operation == "start"
    }) {
        let error_summary = problem
            .technical_detail
            .as_deref()
            .map(redact_text)
            .unwrap_or_else(|| "MCP server failed to start".to_string());
        for capability in problem.affected_capabilities {
            let ComputerRuntimeAffectedCapability::McpServer { bundle_id, name } = capability
            else {
                continue;
            };
            let server_label = name.as_deref().unwrap_or(&bundle_id);
            let mut activity = ActivityEventDraft::computer(
                instance_id,
                ActivityLevel::Error,
                "mcp",
                "mcp_server_lifecycle",
                "start",
                ActivityOutcome::Failed,
                format!("Server start failed: {server_label}: {error_summary}"),
            );
            activity.fields = Some(serde_json::json!({
                "bundle_id": bundle_id,
                "server_name": name,
                "error": error_summary.clone(),
            }));
            if let Err(error) = state.observability.record_activity_async(activity).await {
                log::error!("failed to persist MCP startup failure activity: {error}");
            }
        }
    }
}

async fn preflight_command_line_tool(
    state: &AppState,
    instance: ComputerInstance,
) -> Result<ComputerInstance, RuntimeActionError> {
    if !instance.command_line.enabled {
        return Ok(instance);
    }
    if let Err(error) = command_line_server_config(
        &instance.command_line,
        &state.config.computer_instance_storage_root(&instance.id),
    ) {
        return disable_command_line_after_failure(state, &instance.id, &error)
            .await
            .map_err(|restore_error| {
                RuntimeActionError::runtime(format!(
                    "Command line tool preflight failed: {error}; {restore_error}"
                ))
            });
    }
    Ok(instance)
}

async fn disable_command_line_after_failure(
    state: &AppState,
    instance_id: &str,
    failure: &str,
) -> Result<ComputerInstance, String> {
    let error_summary = redact_text(failure);
    let result = async {
        let restored = state
            .config
            .update_computer_instance(instance_id, |instance| {
                instance.command_line.enabled = false;
            })
            .map_err(|restore_error| {
                format!("failed to disable the persisted setting: {restore_error}")
            })?;
        let restored = state.hydrate_computer_instance(restored).map_err(|error| {
            format!("setting was disabled, but Computer inputs could not be loaded: {error}")
        })?;
        state
            .computer_registry
            .update_runtime_instance(restored.clone())
            .await
            .map_err(|restore_error| {
                format!(
                    "setting was disabled in persisted settings, but runtime rollback failed: {restore_error}"
                )
            })?;
        Ok(restored)
    }
    .await;

    let mut activity = ActivityEventDraft::computer(
        instance_id,
        ActivityLevel::Error,
        "mcp",
        "built_in_command_line",
        "start",
        ActivityOutcome::Failed,
        format!("Built-in command line tool failed to start: {error_summary}"),
    );
    activity.fields = Some(serde_json::json!({
        "bundle_id": COMMAND_LINE_BUNDLE_ID,
        "error": error_summary,
        "policy_rolled_back": result.is_ok(),
    }));
    if let Err(error) = state.observability.record_activity_async(activity).await {
        log::error!("failed to persist built-in command line startup failure: {error}");
    }
    result
}

#[tauri::command]
pub async fn update_computer_connection_policy(
    state: State<'_, AppState>,
    request: UpdateComputerConnectionPolicyRequest,
) -> Result<ComputerInstanceStatus, String> {
    update_computer_connection_policy_core(&state, request).await
}

pub async fn update_computer_connection_policy_core(
    state: &AppState,
    request: UpdateComputerConnectionPolicyRequest,
) -> Result<ComputerInstanceStatus, String> {
    if let Some(ComputerConnectionTarget::ManagerRobot { context_key, .. }) =
        request.target.as_ref()
    {
        let generation = state
            .manager_context
            .capture_authenticated_generation()
            .await
            .map_err(|error| error.to_string())?;
        let current_context = state
            .manager_context
            .context_key_for_generation(generation)
            .await
            .map_err(|error| error.to_string())?;
        if &current_context != context_key {
            return Err("Manager Robot target does not belong to the active Context".to_string());
        }
        return state
            .manager_context
            .commit_for_authenticated_generation(generation, || async {
                persist_computer_connection_policy(state, request)
                    .await
                    .map_err(ManagerError::InvalidResponse)
            })
            .await
            .map_err(|error| error.to_string());
    }
    persist_computer_connection_policy(state, request).await
}

async fn persist_computer_connection_policy(
    state: &AppState,
    request: UpdateComputerConnectionPolicyRequest,
) -> Result<ComputerInstanceStatus, String> {
    let _operation_guard = state.computer_registry.operation_lease(&request.id).await;
    let _target_guard = state.connection_target_lock.lock().await;
    validate_connection_target_reference(state, request.target.as_ref())?;
    let previous = state
        .config
        .get_computer_instance(&request.id)
        .map_err(|error| error.to_string())?;

    let updated = state
        .config
        .update_computer_instance(&request.id, |instance| {
            apply_selected_connection_policy(instance, &request);
        })
        .map_err(|error| error.to_string())?;
    let runtime = apply_updated_computer_instance(state, previous, updated.clone()).await?;

    Ok(status_from_instance(&updated, &runtime).await)
}

fn apply_selected_connection_policy(
    instance: &mut ComputerInstance,
    request: &UpdateComputerConnectionPolicyRequest,
) {
    instance.connection_policy = ComputerConnectionPolicy {
        target: request.target.clone(),
        auto_connect: request.auto_connect && request.target.is_some(),
    };
    match request.target.as_ref() {
        Some(ComputerConnectionTarget::ManagerRobot {
            context_key,
            employee_id,
            last_resolved_robot_account_id,
        }) => {
            let mut binding = instance
                .robot_binding
                .clone()
                .filter(|binding| {
                    binding.context_key.as_ref() == Some(context_key)
                        && binding.employee_id == *employee_id
                })
                .unwrap_or_else(|| RobotBindingMetadata::active(context_key.clone(), *employee_id));
            binding.state = ManagerRobotBindingState::Active;
            if last_resolved_robot_account_id.is_some() {
                binding.last_resolved_robot_account_id = last_resolved_robot_account_id.clone();
            }
            instance.robot_binding = Some(binding);
        }
        Some(ComputerConnectionTarget::ManualSmcp { .. }) | None => {
            if let Some(binding) = instance.robot_binding.as_mut() {
                if binding.state == ManagerRobotBindingState::Active {
                    binding.state = ManagerRobotBindingState::Dormant;
                }
            }
        }
    }
}

#[tauri::command]
pub async fn update_computer_skill_home(
    state: State<'_, AppState>,
    request: UpdateComputerSkillHomeRequest,
) -> Result<ComputerInstanceStatus, String> {
    update_computer_skill_home_core(&state, request).await
}

pub async fn update_computer_skill_home_core(
    state: &AppState,
    request: UpdateComputerSkillHomeRequest,
) -> Result<ComputerInstanceStatus, String> {
    let _operation_guard = state.computer_registry.operation_lease(&request.id).await;
    let previous = state
        .config
        .get_computer_instance(&request.id)
        .map_err(|error| error.to_string())?;
    let root = normalize_optional_text(request.local_skills_root).map(PathBuf::from);

    let updated = state
        .config
        .update_computer_instance(&request.id, |instance| {
            instance.local_skills_root = root;
        })
        .map_err(|error| error.to_string())?;
    let runtime = apply_updated_computer_instance(state, previous, updated.clone()).await?;

    Ok(status_from_instance(&updated, &runtime).await)
}

#[tauri::command]
pub async fn connect_computer_connection_target(
    app: AppHandle,
    state: State<'_, AppState>,
    id: ComputerInstanceId,
) -> Result<(), String> {
    connect_computer_connection_target_core(&app, &state, id).await
}

pub async fn connect_computer_connection_target_core(
    app: &AppHandle,
    state: &AppState,
    id: ComputerInstanceId,
) -> Result<(), String> {
    let instance = state
        .config
        .get_computer_instance(&id)
        .map_err(|error| error.to_string())?;
    let target = instance
        .connection_policy
        .target
        .as_ref()
        .ok_or_else(|| "No connection target selected for this Computer".to_string())?;

    connect_computer_connection_target_by_policy(Some(app), state, &id, target).await
}

#[tauri::command]
pub async fn disconnect_computer_connection_target(
    state: State<'_, AppState>,
    id: ComputerInstanceId,
) -> Result<(), String> {
    disconnect_smcp_core(&state, &id).await
}

async fn connect_computer_connection_target_by_policy(
    _app: Option<&AppHandle>,
    state: &AppState,
    id: &str,
    target: &ComputerConnectionTarget,
) -> Result<(), String> {
    match target {
        ComputerConnectionTarget::ManualSmcp { .. } => {
            connect_connection_target_for_policy_inner(state, id, target).await
        }
        ComputerConnectionTarget::ManagerRobot {
            context_key,
            employee_id,
            ..
        } => connect_manager_robot_target_for_policy(state, id, context_key, *employee_id, target)
            .await
            .map_err(|error| error.to_string()),
    }
}

fn validate_connection_target_reference(
    state: &AppState,
    target: Option<&ComputerConnectionTarget>,
) -> Result<(), String> {
    match target {
        Some(ComputerConnectionTarget::ManualSmcp { id }) => {
            state
                .config
                .get_manual_smcp_target(id)
                .map_err(|error| error.to_string())?;
            Ok(())
        }
        Some(ComputerConnectionTarget::ManagerRobot { .. }) => Ok(()),
        None => Ok(()),
    }
}

async fn status_from_instance(
    instance: &ComputerInstance,
    runtime: &crate::services::computer::ComputerInstanceRuntime,
) -> ComputerInstanceStatus {
    let runtime_snapshot = runtime.runtime_snapshot().await;
    let mcp_server_count = runtime_snapshot.mcp_servers;
    let connection_state = runtime.connection_snapshot().await;
    let connection_context = connection_state.context.clone();
    let connected = connection_state.status == ClientConnectionStatus::Connected;
    ComputerInstanceStatus {
        id: instance.id.clone(),
        name: instance.name.clone(),
        description: instance.description.clone(),
        local_skills_root: instance.local_skills_root.clone(),
        default_skill_home: runtime.default_skill_home(),
        configured_skill_home: runtime.configured_skill_home(),
        effective_skill_home: runtime.sdk_skill_home().await,
        running: runtime_snapshot.is_running(),
        runtime: runtime_snapshot,
        connection_state: connection_state.clone(),
        connected,
        client_connection_present: connection_context.is_some(),
        connection_revision: connection_state.revision,
        connection_context: connection_context.clone(),
        mcp_server_count,
        robot_binding: instance.robot_binding.clone(),
        connection_policy: instance.connection_policy.clone(),
        remote_control: instance.remote_control.clone(),
        command_line: instance.command_line.clone(),
        mcp_start_concurrency: instance.mcp_start_concurrency,
        connection: connected.then_some(connection_context).flatten(),
    }
}

fn normalize_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Computer instance name cannot be empty".to_string());
    }
    Ok(name.to_string())
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim().to_string();
        if value.is_empty() {
            None
        } else {
            Some(value)
        }
    })
}

fn generate_instance_id() -> ComputerInstanceId {
    format!("computer-{}", uuid::Uuid::new_v4())
}

async fn prepare_duplicate_skill_home(
    source: &Path,
    destination: &Path,
    mode: DuplicateSkillHomeMode,
) -> Result<(), String> {
    let source = source.to_path_buf();
    let destination = destination.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let source_user_skills = source.join("user");
        let destination_user_skills = destination.join("user");
        if mode == DuplicateSkillHomeMode::Copy && source_user_skills.exists() {
            validate_duplicate_skill_copy(&source_user_skills, &destination_user_skills)?;
        }

        fs::create_dir_all(&destination).map_err(|error| {
            format!(
                "Failed to create duplicate skill directory {}: {}",
                destination.display(),
                error
            )
        })?;

        if mode == DuplicateSkillHomeMode::Copy && source_user_skills.exists() {
            if let Err(error) = fs::create_dir_all(&destination_user_skills)
                .map_err(|error| error.to_string())
                .and_then(|_| {
                    copy_directory_contents(&source_user_skills, &destination_user_skills)
                })
            {
                if let Err(cleanup_error) = cleanup_duplicate_skill_destination(&destination) {
                    return Err(format!(
                        "Failed to copy skills from {} to {}: {}; cleanup failed: {}",
                        source_user_skills.display(),
                        destination_user_skills.display(),
                        error,
                        cleanup_error
                    ));
                }
                return Err(format!(
                    "Failed to copy skills from {} to {}: {}; created directory was cleaned up",
                    source_user_skills.display(),
                    destination_user_skills.display(),
                    error
                ));
            }
        }
        Ok(())
    })
    .await
    .map_err(|error| format!("Failed to prepare duplicate skill directory: {error}"))?
}

async fn quarantine_computer_instance_storage(path: &Path) -> Result<Option<PathBuf>, String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        if !path.exists() {
            return Ok(None);
        }
        let parent = path.parent().ok_or_else(|| {
            format!(
                "Computer instance storage has no parent: {}",
                path.display()
            )
        })?;
        let trash = parent.join(".trash");
        fs::create_dir_all(&trash).map_err(|error| {
            format!(
                "Failed to create Computer storage trash {}: {}",
                trash.display(),
                error
            )
        })?;
        let quarantined = trash.join(format!("deleting-{}", uuid::Uuid::new_v4()));
        fs::rename(&path, &quarantined).map_err(|error| {
            format!(
                "Failed to atomically quarantine Computer storage {}: {}",
                path.display(),
                error
            )
        })?;
        Ok(Some(quarantined))
    })
    .await
    .map_err(|error| format!("Failed to quarantine Computer instance storage: {error}"))?
}

async fn restore_quarantined_computer_storage(
    quarantined: &Path,
    destination: &Path,
) -> Result<(), String> {
    let quarantined = quarantined.to_path_buf();
    let destination = destination.to_path_buf();
    tokio::task::spawn_blocking(move || {
        fs::rename(&quarantined, &destination).map_err(|error| {
            format!(
                "failed to restore {} to {}: {}",
                quarantined.display(),
                destination.display(),
                error
            )
        })
    })
    .await
    .map_err(|error| format!("Failed to restore Computer instance storage: {error}"))?
}

async fn cleanup_quarantined_computer_storage(path: Option<PathBuf>) {
    let Some(path) = path else {
        return;
    };
    let trash_root = path.parent().map(Path::to_path_buf);
    match tokio::task::spawn_blocking({
        let path = path.clone();
        move || fs::remove_dir_all(path)
    })
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(error)) => log::warn!(
            "Computer was deleted, but quarantined SDK storage {} could not be cleaned up: {}",
            path.display(),
            error
        ),
        Err(error) => log::warn!(
            "Computer was deleted, but quarantined SDK storage cleanup could not run for {}: {}",
            path.display(),
            error
        ),
    }
    if let Some(trash_root) = trash_root {
        let _ = tokio::task::spawn_blocking(move || fs::remove_dir(trash_root)).await;
    }
}

fn load_hydrated_computer_instance(
    state: &AppState,
    instance_id: &str,
) -> Result<ComputerInstance, String> {
    let instance = state
        .config
        .get_computer_instance(instance_id)
        .map_err(|error| error.to_string())?;
    state
        .hydrate_computer_instance(instance)
        .map_err(|error| error.to_string())
}

async fn rollback_failed_computer_creation(
    state: &AppState,
    instance_id: &str,
    storage_root: &Path,
    operation: &str,
    primary_error: String,
) -> String {
    let mut rollback_errors = Vec::new();
    if let Err(error) = state.sdk_config.delete(instance_id) {
        rollback_errors.push(format!("delete SDK config: {error}"));
    }
    if let Err(error) = state.config.remove_computer_instance(instance_id) {
        rollback_errors.push(format!("remove persisted Computer profile: {error}"));
    }
    match quarantine_computer_instance_storage(storage_root).await {
        Ok(quarantined) => cleanup_quarantined_computer_storage(quarantined).await,
        Err(error) => rollback_errors.push(format!("clean target instance storage: {error}")),
    }

    if rollback_errors.is_empty() {
        format!("Failed to {operation} Computer instance: {primary_error}")
    } else {
        format!(
            "Failed to {operation} Computer instance: {primary_error}; additionally failed to rollback: {}",
            rollback_errors.join("; ")
        )
    }
}

fn validate_duplicate_skill_copy(source: &Path, destination: &Path) -> Result<(), String> {
    if !source.is_dir() {
        return Err(format!(
            "Failed to copy skills from {} to {}: source is not a directory",
            source.display(),
            destination.display()
        ));
    }

    let source = fs::canonicalize(source).map_err(|error| {
        format!(
            "Failed to resolve source skill directory {}: {}",
            source.display(),
            error
        )
    })?;
    let destination = absolute_destination_path(destination)?;

    if destination == source || destination.starts_with(&source) {
        return Err(format!(
            "Cannot copy skills from {} to {}: destination is inside source directory",
            source.display(),
            destination.display()
        ));
    }

    Ok(())
}

fn absolute_destination_path(path: &Path) -> Result<std::path::PathBuf, String> {
    let mut missing_components = Vec::new();
    let mut cursor = path;

    while !cursor.exists() {
        let name = cursor.file_name().ok_or_else(|| {
            format!(
                "Failed to resolve duplicate skill destination {}: no existing ancestor",
                path.display()
            )
        })?;
        missing_components.push(name.to_os_string());
        cursor = cursor.parent().ok_or_else(|| {
            format!(
                "Failed to resolve duplicate skill destination {}: no parent directory",
                path.display()
            )
        })?;
    }

    let mut absolute = fs::canonicalize(cursor).map_err(|error| {
        format!(
            "Failed to resolve duplicate skill destination ancestor {}: {}",
            cursor.display(),
            error
        )
    })?;
    for component in missing_components.iter().rev() {
        absolute.push(component);
    }
    Ok(absolute)
}

fn cleanup_duplicate_skill_destination(destination: &Path) -> Result<(), std::io::Error> {
    if destination.exists() {
        fs::remove_dir_all(destination)?;
    }
    if let Some(parent) = destination.parent() {
        if !parent.exists() {
            return Ok(());
        }
        let mut entries = fs::read_dir(parent)?;
        if entries.next().is_none() {
            fs::remove_dir(parent)?;
        }
    }
    Ok(())
}

fn copy_directory_contents(source: &Path, destination: &Path) -> Result<(), String> {
    for entry in fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_dir() {
            fs::create_dir_all(&destination_path).map_err(|error| error.to_string())?;
            copy_directory_contents(&source_path, &destination_path)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::client_control::{
        update_remote_control_policy_core, UpdateRemoteControlPolicyRequest,
    };
    use crate::services::computer::ComputerRuntimeState;
    use crate::services::config::ConfigService;
    use crate::services::input_entry_store::InputEntryStorageKind;
    use crate::services::keychain::{InMemorySecretStore, KeychainError, SecretStore};
    use crate::services::manager_context::ManagerContextKey;
    use crate::services::manager_environment::ManagerEnvironment;
    use crate::services::observability::{
        ActivityOutcome, ActivityQuery, ActivityScope, ActivityScopeFilter, ObservabilityService,
    };
    use crate::services::settings::SettingsService;
    use crate::services::storage::write_json_atomically;
    use a2c_smcp::smcp_computer::settings::config::{ConfigEdit, ConfigEntity, EditIntent};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use tempfile::TempDir;

    #[derive(Default)]
    struct FailNextDeleteSecretStore {
        inner: InMemorySecretStore,
        fail_next_delete: AtomicBool,
    }

    impl SecretStore for FailNextDeleteSecretStore {
        fn set_secret(&self, key: &str, secret: &str) -> Result<(), KeychainError> {
            self.inner.set_secret(key, secret)
        }

        fn get_secret(&self, key: &str) -> Result<Option<String>, KeychainError> {
            self.inner.get_secret(key)
        }

        fn delete_secret(&self, key: &str) -> Result<(), KeychainError> {
            if self.fail_next_delete.swap(false, Ordering::SeqCst) {
                return Err(KeychainError::Store("injected delete failure".to_string()));
            }
            self.inner.delete_secret(key)
        }
    }

    #[derive(Default)]
    struct CountingSecretStore {
        inner: InMemorySecretStore,
        get_calls: AtomicUsize,
        delete_calls: AtomicUsize,
    }

    impl CountingSecretStore {
        fn peek_input(&self, instance_id: &str, input_id: &str) -> Option<String> {
            self.inner
                .get_secret(&keychain::input_secret_key(instance_id, input_id))
                .unwrap()
        }
    }

    impl SecretStore for CountingSecretStore {
        fn set_secret(&self, key: &str, secret: &str) -> Result<(), KeychainError> {
            self.inner.set_secret(key, secret)
        }

        fn get_secret(&self, key: &str) -> Result<Option<String>, KeychainError> {
            self.get_calls.fetch_add(1, Ordering::SeqCst);
            self.inner.get_secret(key)
        }

        fn delete_secret(&self, key: &str) -> Result<(), KeychainError> {
            self.delete_calls.fetch_add(1, Ordering::SeqCst);
            self.inner.delete_secret(key)
        }
    }

    fn test_state() -> (AppState, TempDir) {
        let dir = TempDir::new().unwrap();
        let config = ConfigService::new(dir.path().to_path_buf()).unwrap();
        config
            .add_computer_instance(ComputerInstance::new("computer-a", "Computer A"))
            .unwrap();
        let log_service = ObservabilityService::new(dir.path()).unwrap();
        let settings_service = SettingsService::new(dir.path().to_path_buf());
        (AppState::new(config, log_service, settings_service), dir)
    }

    #[tokio::test]
    async fn computer_profile_lifecycle_is_audited_in_target_scope_for_shared_entrypoints() {
        let (state, _dir) = test_state();
        let created = create_computer_instance_with_trigger(
            &state,
            CreateComputerInstanceRequest {
                name: "Remote Computer".to_string(),
                description: None,
            },
            "client_control",
        )
        .await
        .unwrap();
        delete_computer_instance_with_trigger(&state, created.id.clone(), "client_control")
            .await
            .unwrap();

        let activity = state
            .observability
            .query_activity(&ActivityQuery {
                scope: ActivityScopeFilter::Computer {
                    computer_id: created.id.clone(),
                },
                ..ActivityQuery::default()
            })
            .unwrap();
        assert_eq!(activity.total, 2);
        assert_eq!(activity.items[0].operation, "delete");
        assert_eq!(activity.items[1].operation, "create");
        assert!(activity.items.iter().all(|event| {
            event.category == "computer"
                && event.outcome == ActivityOutcome::Succeeded
                && event
                    .fields
                    .as_ref()
                    .and_then(|fields| fields["trigger"].as_str())
                    == Some("client_control")
        }));
    }

    #[tokio::test]
    async fn computer_profile_validation_failure_is_redacted_and_client_scoped() {
        let (state, _dir) = test_state();
        let result = create_computer_instance_with_trigger(
            &state,
            CreateComputerInstanceRequest {
                name: "   ".to_string(),
                description: Some("token=must-not-be-recorded".to_string()),
            },
            "user",
        )
        .await;
        assert!(result.is_err());

        let activity = state
            .observability
            .query_activity(&ActivityQuery {
                scope: ActivityScopeFilter::ClientOnly,
                ..ActivityQuery::default()
            })
            .unwrap();
        assert_eq!(activity.total, 1);
        assert_eq!(activity.items[0].operation, "create");
        assert_eq!(activity.items[0].outcome, ActivityOutcome::Failed);
        assert!(!serde_json::to_string(&activity.items[0])
            .unwrap()
            .contains("must-not-be-recorded"));
    }

    #[tokio::test]
    async fn computer_runtime_core_records_success_and_failure_in_computer_scope() {
        let (state, _dir) = test_state();
        start_computer_instance_core(None, &state, "computer-a".to_string())
            .await
            .unwrap();
        stop_computer_instance_core(&state, "computer-a".to_string())
            .await
            .unwrap();
        assert!(crate::commands::connection::connect_connection_target_core(
            &state,
            "computer-a",
            "missing-target",
        )
        .await
        .is_err());
        assert!(
            start_computer_instance_core(None, &state, "missing-computer".to_string())
                .await
                .is_err()
        );

        let successful = state
            .observability
            .query_activity(&ActivityQuery {
                scope: ActivityScopeFilter::Computer {
                    computer_id: "computer-a".to_string(),
                },
                ..ActivityQuery::default()
            })
            .unwrap();
        let operations = successful
            .items
            .iter()
            .filter(|event| event.event_type == "runtime_lifecycle")
            .map(|event| event.operation.as_str())
            .collect::<Vec<_>>();
        assert_eq!(operations, vec!["stop", "start"]);
        assert!(successful.items.iter().all(|event| {
            event.event_type != "runtime_lifecycle" || event.outcome == ActivityOutcome::Succeeded
        }));
        let connection_failure = successful
            .items
            .iter()
            .find(|event| event.operation == "connect_manual")
            .expect("manual connection failure activity");
        assert_eq!(connection_failure.category, "connection");
        assert_eq!(connection_failure.outcome, ActivityOutcome::Failed);

        let failed = state
            .observability
            .query_activity(&ActivityQuery {
                scope: ActivityScopeFilter::Computer {
                    computer_id: "missing-computer".to_string(),
                },
                ..ActivityQuery::default()
            })
            .unwrap();
        assert_eq!(failed.total, 1);
        assert_eq!(failed.items[0].event_type, "runtime_lifecycle");
        assert_eq!(failed.items[0].outcome, ActivityOutcome::Failed);
    }

    #[tokio::test]
    async fn duplicate_failure_after_target_id_generation_keeps_computer_scope() {
        let (state, _dir) = test_state();
        let result = duplicate_computer_instance_with_trigger(
            &state,
            DuplicateComputerInstanceRequest {
                source_id: "missing-source".to_string(),
                name: "Duplicate".to_string(),
                description: None,
                copy_robot_binding: false,
                connection_target_id: None,
                skill_home_mode: DuplicateSkillHomeMode::Empty,
            },
            "client_control",
        )
        .await;
        assert!(result.is_err());

        let activity = state
            .observability
            .query_activity(&ActivityQuery::default())
            .unwrap();
        let event = activity
            .items
            .iter()
            .find(|event| event.operation == "duplicate")
            .expect("duplicate failure activity");
        let ActivityScope::Computer { computer_id } = &event.scope else {
            panic!("generated duplicate target must use Computer scope");
        };
        assert_eq!(event.outcome, ActivityOutcome::Failed);
        assert_eq!(
            event
                .fields
                .as_ref()
                .and_then(|fields| fields["target_id"].as_str()),
            Some(computer_id.as_str())
        );
    }

    #[tokio::test]
    async fn list_is_observational_while_an_uncommitted_profile_exists() {
        let (state, _dir) = test_state();
        let state = Arc::new(state);
        let creation_membership = state.computer_registry.membership_lease().await;
        state
            .config
            .add_computer_instance(ComputerInstance::new(
                "computer-pending",
                "Computer Pending",
            ))
            .unwrap();

        let list_state = state.clone();
        let list = tokio::time::timeout(std::time::Duration::from_secs(2), async move {
            list_computer_instances_core(&list_state).await
        })
        .await
        .expect("list must not wait for a membership transaction")
        .unwrap();
        assert!(list.iter().all(|status| status.id != "computer-pending"));

        // Model create/duplicate rollback after its profile write but before runtime publication.
        state
            .config
            .remove_computer_instance("computer-pending")
            .unwrap();
        drop(creation_membership);

        assert!(state
            .computer_registry
            .runtime("computer-pending")
            .await
            .is_none());
    }

    #[tokio::test]
    async fn deleting_computer_removes_definitionless_plain_and_secret_entries() {
        let (state, _dir) = test_state();
        let storage_root = state.config.computer_instance_storage_root("computer-a");
        let entries = InputEntryStore::for_computer(
            state.config.as_ref(),
            "computer-a".to_string(),
            state.secret_store.clone(),
        );
        entries
            .upsert("plain", Some(serde_json::json!("value")), false)
            .unwrap();
        entries
            .upsert("token", Some(serde_json::json!("top-secret")), true)
            .unwrap();
        assert!(state
            .sdk_config
            .load_input_definitions("computer-a")
            .is_empty());

        delete_computer_instance_core(&state, "computer-a".to_string())
            .await
            .unwrap();

        assert!(!storage_root.exists());
        assert_eq!(
            keychain::get_input_secret(state.secret_store.as_ref(), "computer-a", "token").unwrap(),
            None
        );
        assert!(state.config.get_computer_instance("computer-a").is_err());
    }

    #[tokio::test]
    async fn deleting_computer_never_grants_v1_plain_entries_keychain_authority() {
        let dir = TempDir::new().unwrap();
        let config = ConfigService::new(dir.path().to_path_buf()).unwrap();
        config
            .add_computer_instance(ComputerInstance::new("computer-a", "Computer A"))
            .unwrap();
        let secrets = Arc::new(CountingSecretStore::default());
        let state = AppState::new_with_secret_store(
            config,
            ObservabilityService::new(dir.path()).unwrap(),
            SettingsService::new(dir.path().to_path_buf()),
            secrets.clone(),
        );
        crate::commands::inputs::add_or_update_input_core(
            &state,
            "computer-a",
            crate::commands::inputs::InputDefinition::PromptString {
                id: "credential".to_string(),
                label: Some("Credential".to_string()),
                description: None,
                default: None,
                password: Some(true),
            },
        )
        .await
        .unwrap();
        let storage_root = state.config.computer_instance_storage_root("computer-a");
        InputValueStore::from_storage_root(&storage_root)
            .set("credential", &serde_json::json!("owned-plain"))
            .unwrap();
        write_json_atomically(
            &storage_root.join("input_value_ids.json"),
            &serde_json::json!({
                "schema_version": 1,
                "ids": ["credential"]
            }),
        )
        .unwrap();
        keychain::set_input_secret(
            secrets.as_ref(),
            "computer-a",
            "credential",
            "unowned-secret",
        )
        .unwrap();

        delete_computer_instance_core(&state, "computer-a".to_string())
            .await
            .unwrap();

        assert_eq!(secrets.get_calls.load(Ordering::SeqCst), 0);
        assert_eq!(secrets.delete_calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            secrets.peek_input("computer-a", "credential").as_deref(),
            Some("unowned-secret")
        );
        assert!(state.config.get_computer_instance("computer-a").is_err());
    }

    #[tokio::test]
    async fn deleting_computer_cleans_entry_metadata_when_backend_values_are_missing() {
        let (state, _dir) = test_state();
        let storage_root = state.config.computer_instance_storage_root("computer-a");
        let entries = InputEntryStore::for_computer(
            state.config.as_ref(),
            "computer-a".to_string(),
            state.secret_store.clone(),
        );
        entries
            .upsert("plain", Some(serde_json::json!("value")), false)
            .unwrap();
        entries
            .upsert("secret", Some(serde_json::json!("top-secret")), true)
            .unwrap();
        InputValueStore::from_storage_root(&storage_root)
            .delete("plain")
            .unwrap();
        keychain::delete_input_secret(state.secret_store.as_ref(), "computer-a", "secret").unwrap();

        delete_computer_instance_core(&state, "computer-a".to_string())
            .await
            .unwrap();

        assert!(!storage_root.exists());
        assert!(state.config.get_computer_instance("computer-a").is_err());
    }

    #[tokio::test]
    async fn failed_entry_cleanup_restores_computer_profile_and_all_entry_values() {
        let dir = TempDir::new().unwrap();
        let config = ConfigService::new(dir.path().to_path_buf()).unwrap();
        config
            .add_computer_instance(ComputerInstance::new("computer-a", "Computer A"))
            .unwrap();
        let secrets = Arc::new(FailNextDeleteSecretStore::default());
        let state = AppState::new_with_secret_store(
            config,
            ObservabilityService::new(dir.path()).unwrap(),
            SettingsService::new(dir.path().to_path_buf()),
            secrets.clone(),
        );
        let instance = state.config.get_computer_instance("computer-a").unwrap();
        state
            .computer_registry
            .upsert_runtime(instance)
            .await
            .unwrap();
        let entries = InputEntryStore::for_computer(
            state.config.as_ref(),
            "computer-a".to_string(),
            secrets.clone(),
        );
        entries
            .upsert("plain", Some(serde_json::json!("value")), false)
            .unwrap();
        entries
            .upsert("token", Some(serde_json::json!("top-secret")), true)
            .unwrap();
        secrets.fail_next_delete.store(true, Ordering::SeqCst);

        let error = delete_computer_instance_core(&state, "computer-a".to_string())
            .await
            .unwrap_err();

        assert!(error.contains("injected delete failure"));
        assert!(state.config.get_computer_instance("computer-a").is_ok());
        assert_eq!(
            entries
                .resolve("plain", InputEntryStorageKind::Value)
                .unwrap(),
            Some(serde_json::json!("value"))
        );
        assert_eq!(
            entries
                .resolve("token", InputEntryStorageKind::Secret)
                .unwrap(),
            Some(serde_json::json!("top-secret"))
        );
    }

    #[tokio::test]
    async fn update_computer_skill_home_saves_without_restarting_runtime() {
        let (state, dir) = test_state();
        let custom_root = dir.path().join("custom-skill-home");
        let default_root = state.config.default_local_skills_root("computer-a");

        let updated = update_computer_skill_home_core(
            &state,
            UpdateComputerSkillHomeRequest {
                id: "computer-a".to_string(),
                local_skills_root: Some(custom_root.to_string_lossy().to_string()),
            },
        )
        .await
        .unwrap();

        assert_eq!(updated.local_skills_root, Some(custom_root.clone()));
        assert_eq!(updated.default_skill_home, default_root.clone());
        assert_eq!(updated.configured_skill_home, custom_root.clone());
        assert_eq!(updated.effective_skill_home, default_root.clone());

        let started = start_computer_instance_core(None, &state, "computer-a".to_string())
            .await
            .unwrap();
        assert_eq!(started.effective_skill_home, custom_root.clone());
        let generation_before_save = started.runtime.generation;

        let restored = update_computer_skill_home_core(
            &state,
            UpdateComputerSkillHomeRequest {
                id: "computer-a".to_string(),
                local_skills_root: Some("   ".to_string()),
            },
        )
        .await
        .unwrap();

        assert_eq!(restored.local_skills_root, None);
        assert_eq!(restored.default_skill_home, default_root.clone());
        assert_eq!(restored.configured_skill_home, default_root.clone());
        assert_eq!(restored.effective_skill_home, custom_root);
        assert_eq!(restored.runtime.generation, generation_before_save);

        let restarted = restart_computer_instance_core(None, &state, "computer-a".to_string())
            .await
            .unwrap();
        assert_eq!(restarted.effective_skill_home, default_root);
        assert!(restarted.runtime.generation > generation_before_save);
    }

    #[tokio::test]
    async fn computer_status_counts_only_servers_materialized_into_the_sdk_handle() {
        let (state, _dir) = test_state();
        state
            .sdk_config
            .update(
                "computer-a",
                &[ConfigEdit::new(
                    ConfigEntity::McpServer("snapshot-only".to_string()),
                    EditIntent::Upsert(serde_json::json!({
                        "type": "stdio",
                        "server_parameters": {"command": "node"}
                    })),
                )],
            )
            .unwrap();
        assert!(state
            .config
            .get_computer_instance("computer-a")
            .unwrap()
            .mcp_servers
            .is_empty());

        let before_start = get_computer_instance_status_core(&state, "computer-a".to_string())
            .await
            .unwrap();
        assert_eq!(before_start.mcp_server_count, 0);

        let after_start = start_computer_instance_core(None, &state, "computer-a".to_string())
            .await
            .unwrap();
        assert_eq!(after_start.mcp_server_count, 1);
    }

    #[tokio::test]
    async fn computer_start_is_not_blocked_by_a_missing_sdk_input_definition() {
        let (state, _dir) = test_state();
        state
            .sdk_config
            .update(
                "computer-a",
                &[ConfigEdit::new(
                    ConfigEntity::McpServer("missing-input".to_string()),
                    EditIntent::Upsert(serde_json::json!({
                        "type": "stdio",
                        "server_parameters": {
                            "command": "node",
                            "args": ["${input:workspace}"]
                        }
                    })),
                )],
            )
            .unwrap();

        let status = start_computer_instance_core(None, &state, "computer-a".to_string())
            .await
            .expect("an MCP input failure must not fail Computer startup");

        assert!(status.running);
    }

    #[tokio::test]
    async fn computer_status_includes_internal_client_control_in_mcp_counts() {
        let (state, _dir) = test_state();
        update_remote_control_policy_core(
            &state,
            UpdateRemoteControlPolicyRequest {
                computer_id: "computer-a".to_string(),
                policy: RemoteControlPolicy {
                    enabled: true,
                    ..RemoteControlPolicy::default()
                },
            },
        )
        .await
        .unwrap();

        let activity = state
            .observability
            .query_activity(&ActivityQuery {
                scope: ActivityScopeFilter::Computer {
                    computer_id: "computer-a".to_string(),
                },
                ..ActivityQuery::default()
            })
            .unwrap();
        let policy_event = activity
            .items
            .iter()
            .find(|event| event.event_type == "client_control_policy")
            .expect("policy update should be visible in Computer activity");
        assert_eq!(
            policy_event.scope,
            ActivityScope::Computer {
                computer_id: "computer-a".to_string()
            }
        );
        assert_eq!(policy_event.category, "security");
        assert_eq!(policy_event.outcome, ActivityOutcome::Succeeded);

        let stopped = get_computer_instance_status_core(&state, "computer-a".to_string())
            .await
            .unwrap();
        assert_eq!(stopped.mcp_server_count, 1);
        assert_eq!(stopped.runtime.mcp_servers, 1);
        assert_eq!(stopped.runtime.active_mcp_servers, 0);

        let started = start_computer_instance_core(None, &state, "computer-a".to_string())
            .await
            .unwrap();

        assert_eq!(started.mcp_server_count, 1);
        assert_eq!(started.runtime.mcp_servers, 1);
        assert_eq!(started.runtime.active_mcp_servers, 1);

        state
            .sdk_config
            .update(
                "computer-a",
                &[ConfigEdit::new(
                    ConfigEntity::McpServer("snapshot-only".to_string()),
                    EditIntent::Upsert(serde_json::json!({
                        "type": "stdio",
                        "server_parameters": {"command": "node"},
                        "disabled": true
                    })),
                )],
            )
            .unwrap();
        let restarted = restart_computer_instance_core(None, &state, "computer-a".to_string())
            .await
            .unwrap();
        assert_eq!(restarted.mcp_server_count, 2);
        assert_eq!(restarted.runtime.mcp_servers, 2);
    }

    #[tokio::test]
    async fn start_command_rejects_an_already_running_runtime() {
        let (state, _dir) = test_state();
        let started = start_computer_instance_core(None, &state, "computer-a".to_string())
            .await
            .unwrap();
        state
            .config
            .update_computer_instance("computer-a", |instance| {
                instance.description = Some("stale persisted metadata".to_string());
            })
            .unwrap();
        let runtime = state.computer_registry.runtime("computer-a").await.unwrap();
        let config_revision_before_rejected_start =
            runtime.runtime_snapshot().await.config_revision;

        let error = start_computer_instance_core(None, &state, "computer-a".to_string())
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            RuntimeActionError::ActionUnavailable {
                action,
                lifecycle,
                disabled_reason,
                ..
            } if action == "start"
                && lifecycle == ComputerRuntimeState::Started.to_string()
                && disabled_reason == "already_running"
        ));
        assert_eq!(runtime.runtime_generation(), started.runtime.generation);
        assert_eq!(
            runtime.runtime_snapshot().await.config_revision,
            config_revision_before_rejected_start,
            "a rejected start must not synchronize newly persisted inputs into the active SDK handle"
        );
    }

    #[test]
    fn explicit_manager_selection_rebinds_needs_rebind_metadata_to_active_context() {
        let mut instance = ComputerInstance::new("computer-a", "Computer A");
        instance.robot_binding = Some(RobotBindingMetadata::needs_rebind(
            7,
            Some("legacy-account".to_string()),
        ));
        let context_key = ManagerContextKey {
            environment: ManagerEnvironment::Staging,
            account_id: "account-a".to_string(),
            organization_id: "organization-a".to_string(),
        };
        let request = UpdateComputerConnectionPolicyRequest {
            id: instance.id.clone(),
            target: Some(ComputerConnectionTarget::manager_robot(
                context_key.clone(),
                42,
                Some("robot-account-42".to_string()),
            )),
            auto_connect: true,
        };

        apply_selected_connection_policy(&mut instance, &request);

        assert_eq!(instance.connection_policy.target, request.target);
        assert!(instance.connection_policy.auto_connect);
        assert_eq!(
            instance.robot_binding,
            Some(RobotBindingMetadata {
                context_key: Some(context_key),
                state: ManagerRobotBindingState::Active,
                employee_id: 42,
                robot_id: None,
                last_resolved_robot_account_id: Some("robot-account-42".to_string()),
                namespace: None,
                robot_name: None,
            })
        );
    }

    #[test]
    fn selecting_manual_target_preserves_it_and_dormants_historical_manager_binding() {
        let context_key = ManagerContextKey {
            environment: ManagerEnvironment::Staging,
            account_id: "account-a".to_string(),
            organization_id: "organization-a".to_string(),
        };
        let mut instance = ComputerInstance::new("computer-a", "Computer A");
        instance.robot_binding = Some(RobotBindingMetadata::active(context_key, 42));
        let request = UpdateComputerConnectionPolicyRequest {
            id: instance.id.clone(),
            target: Some(ComputerConnectionTarget::manual_smcp("manual-a")),
            auto_connect: true,
        };

        apply_selected_connection_policy(&mut instance, &request);

        assert_eq!(instance.connection_policy.target, request.target);
        assert!(instance.connection_policy.auto_connect);
        assert_eq!(
            instance.robot_binding.as_ref().map(|binding| binding.state),
            Some(ManagerRobotBindingState::Dormant)
        );
    }
}
