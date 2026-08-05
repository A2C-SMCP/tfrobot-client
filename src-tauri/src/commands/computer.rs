use crate::commands::connection::{
    connect_connection_target_for_policy_core, connect_manager_robot_target_for_policy,
    disconnect_smcp_core,
};
use crate::commands::runtime_error::RuntimeActionError;
use crate::commands::runtime_sync::apply_updated_computer_instance;
use crate::services::client_control::RemoteControlPolicy;
use crate::services::computer::{
    ClientConnectionStateSnapshot, ClientConnectionStatus, ComputerConnectionPolicy,
    ComputerConnectionTarget, ComputerInstance, ComputerInstanceId, ComputerRuntimeAction,
    ConnectionStateSummary, ManagerRobotBindingState, RobotBindingMetadata,
};
use crate::services::computer_runtime_events::ComputerRuntimeSnapshot;
use crate::services::keychain;
use crate::services::manager_client::ManagerError;
use crate::services::observability::{ActivityEventDraft, ActivityLevel, ActivityOutcome};
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let config = state
        .load_hydrated_computer_instances()
        .map_err(|error| RuntimeActionError::runtime(error.to_string()))?;
    let discovered_ids = config
        .instances
        .iter()
        .map(|instance| instance.id.clone())
        .collect::<HashSet<_>>();
    for runtime in state.computer_registry.list_runtimes().await {
        if !discovered_ids.contains(&runtime.instance.id) {
            state
                .computer_registry
                .remove_runtime(&runtime.instance.id)
                .await
                .map_err(RuntimeActionError::runtime)?;
        }
    }
    let mut statuses = Vec::with_capacity(config.instances.len());

    for instance in config.instances {
        // Listing is observational and must not trigger SDK governance reconciliation. A
        // lifecycle command or the instance-scoped status command performs typed synchronization,
        // where a missing input can be associated with the exact Computer and retried safely.
        let runtime = match state.computer_registry.runtime(&instance.id).await {
            Some(runtime) => runtime,
            None => state
                .computer_registry
                .upsert_runtime(instance.clone())
                .await
                .map_err(RuntimeActionError::runtime)?,
        };
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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let config = state
        .load_hydrated_computer_instances()
        .map_err(|error| RuntimeActionError::runtime(error.to_string()))?;
    let instance = config
        .instances
        .into_iter()
        .find(|instance| instance.id == id)
        .ok_or_else(|| RuntimeActionError::runtime(format!("Computer instance not found: {id}")))?;
    let runtime = state
        .computer_registry
        .update_runtime_instance_typed(instance.clone())
        .await
        .map_err(RuntimeActionError::from)?;

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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let name = normalize_name(&request.name)?;
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
        robot_binding: None,
    };

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
                error,
            )
            .await)
        }
    };
    let runtime = state
        .computer_registry
        .upsert_runtime(instance.clone())
        .await?;

    Ok(status_from_instance(&instance, &runtime).await)
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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let name = normalize_name(&request.name)?;
    let mut instance = state
        .config
        .get_computer_instance(&request.source_id)
        .map_err(|error| error.to_string())?;
    let source_id = instance.id.clone();
    let source_skill_root = instance
        .local_skills_root
        .clone()
        .unwrap_or_else(|| state.config.default_local_skills_root(&source_id));
    instance.id = generate_instance_id();
    instance.name = name;
    instance.description = normalize_optional_text(request.description);
    instance.local_skills_root = None;
    instance.remote_control = RemoteControlPolicy::default();
    let destination_skill_root = state.config.default_local_skills_root(&instance.id);
    let destination_storage_root = state.config.computer_instance_storage_root(&instance.id);
    if !request.copy_robot_binding {
        instance.robot_binding = None;
    }
    if let Some(target_id) = normalize_optional_text(request.connection_target_id) {
        state
            .config
            .get_manual_smcp_target(&target_id)
            .map_err(|error| error.to_string())?;
        instance.connection_policy.target = Some(ComputerConnectionTarget::manual_smcp(target_id));
    }
    state
        .config
        .add_computer_instance(instance.clone())
        .map_err(|error| error.to_string())?;
    if let Err(error) = state.sdk_config.duplicate(&source_id, &instance.id) {
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
        return Err(rollback_failed_computer_creation(
            state,
            &instance.id,
            &destination_storage_root,
            "duplicate",
            error,
        )
        .await);
    }
    let instance = match load_hydrated_computer_instance(state, &instance.id) {
        Ok(instance) => instance,
        Err(error) => {
            return Err(rollback_failed_computer_creation(
                state,
                &instance.id,
                &destination_storage_root,
                "duplicate",
                error,
            )
            .await)
        }
    };
    let runtime = state
        .computer_registry
        .upsert_runtime(instance.clone())
        .await?;

    Ok(status_from_instance(&instance, &runtime).await)
}

#[tauri::command]
pub async fn delete_computer_instance(
    state: State<'_, AppState>,
    id: ComputerInstanceId,
) -> Result<(), String> {
    delete_computer_instance_core(&state, id).await
}

pub async fn delete_computer_instance_core(
    state: &AppState,
    id: ComputerInstanceId,
) -> Result<(), String> {
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let instance_storage_root = state.config.computer_instance_storage_root(&id);
    let persisted_instance = state
        .config
        .get_computer_instance(&id)
        .map_err(|error| error.to_string())?;
    let input_storage = snapshot_computer_input_storage(state, &persisted_instance)?;
    if let Err(error) = delete_computer_input_storage(state, &id, &input_storage) {
        let rollback = restore_computer_input_storage(state, &id, &input_storage);
        return match rollback {
            Ok(()) => Err(error),
            Err(rollback_error) => Err(format!(
                "{error}; additionally failed to restore Computer input storage: {rollback_error}"
            )),
        };
    }
    let prepared_removal = match state.computer_registry.prepare_runtime_removal(&id).await {
        Ok(prepared_removal) => prepared_removal,
        Err(error) => {
            return match restore_computer_input_storage(state, &id, &input_storage) {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(format!(
                "{error}; additionally failed to restore Computer input storage: {rollback_error}"
            )),
            }
        }
    };
    let quarantined_storage = match quarantine_computer_instance_storage(&instance_storage_root)
        .await
    {
        Ok(quarantined_storage) => quarantined_storage,
        Err(error) => {
            drop(prepared_removal);
            return match restore_computer_input_storage(state, &id, &input_storage) {
                    Ok(()) => Err(error),
                    Err(rollback_error) => Err(format!(
                        "{error}; additionally failed to restore Computer input storage: {rollback_error}"
                    )),
                };
        }
    };
    if let Err(error) = state.config.remove_computer_instance(&id) {
        let mut rollback_errors = Vec::new();
        if let Some(quarantined) = quarantined_storage.as_ref() {
            if let Err(restore_error) =
                restore_quarantined_computer_storage(quarantined, &instance_storage_root).await
            {
                rollback_errors.push(format!("restore SDK storage: {restore_error}"));
            }
        }
        if let Err(restore_error) = restore_computer_input_storage(state, &id, &input_storage) {
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
            if let Err(restore_error) = restore_computer_input_storage(state, &id, &input_storage) {
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
struct ComputerInputStorageSnapshot {
    id: String,
    value: Option<serde_json::Value>,
    secret: Option<String>,
}

fn snapshot_computer_input_storage(
    state: &AppState,
    instance: &ComputerInstance,
) -> Result<Vec<ComputerInputStorageSnapshot>, String> {
    instance
        .inputs
        .iter()
        .map(|input| {
            let id = input.id().to_string();
            Ok(ComputerInputStorageSnapshot {
                value: keychain::get_input_value(state.secret_store.as_ref(), &instance.id, &id)
                    .map_err(|error| error.to_string())?,
                secret: keychain::get_input_secret(state.secret_store.as_ref(), &instance.id, &id)
                    .map_err(|error| error.to_string())?,
                id,
            })
        })
        .collect()
}

fn delete_computer_input_storage(
    state: &AppState,
    instance_id: &str,
    snapshots: &[ComputerInputStorageSnapshot],
) -> Result<(), String> {
    for snapshot in snapshots {
        keychain::delete_input_value(state.secret_store.as_ref(), instance_id, &snapshot.id)
            .map_err(|error| error.to_string())?;
        keychain::delete_input_secret(state.secret_store.as_ref(), instance_id, &snapshot.id)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn restore_computer_input_storage(
    state: &AppState,
    instance_id: &str,
    snapshots: &[ComputerInputStorageSnapshot],
) -> Result<(), String> {
    for snapshot in snapshots {
        match &snapshot.value {
            Some(value) => keychain::set_input_value(
                state.secret_store.as_ref(),
                instance_id,
                &snapshot.id,
                value,
            ),
            None => {
                keychain::delete_input_value(state.secret_store.as_ref(), instance_id, &snapshot.id)
            }
        }
        .map_err(|error| error.to_string())?;
        match &snapshot.secret {
            Some(secret) => keychain::set_input_secret(
                state.secret_store.as_ref(),
                instance_id,
                &snapshot.id,
                secret,
            ),
            None => keychain::delete_input_secret(
                state.secret_store.as_ref(),
                instance_id,
                &snapshot.id,
            ),
        }
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn start_computer_instance(
    app: AppHandle,
    state: State<'_, AppState>,
    id: ComputerInstanceId,
) -> Result<ComputerInstanceStatus, RuntimeActionError> {
    start_computer_instance_core(Some(&app), &state, id).await
}

pub async fn start_computer_instance_core(
    app: Option<&AppHandle>,
    state: &AppState,
    id: ComputerInstanceId,
) -> Result<ComputerInstanceStatus, RuntimeActionError> {
    let lifecycle_guard = state.computer_lifecycle_lock.lock().await;
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
    let runtime = state
        .computer_registry
        .update_runtime_instance_typed(instance.clone())
        .await
        .map_err(RuntimeActionError::from)?;
    runtime.start().await.map_err(RuntimeActionError::from)?;
    drop(lifecycle_guard);
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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let instance = state
        .config
        .get_computer_instance(&id)
        .map_err(|error| error.to_string())?;
    let runtime = state
        .computer_registry
        .runtime(&id)
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
    restart_computer_instance_core(Some(&app), &state, id).await
}

pub async fn restart_computer_instance_core(
    app: Option<&AppHandle>,
    state: &AppState,
    id: ComputerInstanceId,
) -> Result<ComputerInstanceStatus, RuntimeActionError> {
    let lifecycle_guard = state.computer_lifecycle_lock.lock().await;
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
    let runtime = state
        .computer_registry
        .update_runtime_instance_typed(instance.clone())
        .await
        .map_err(RuntimeActionError::from)?;
    runtime.restart().await.map_err(RuntimeActionError::from)?;
    drop(lifecycle_guard);
    if instance.connection_policy.auto_connect {
        if let Some(target) = instance.connection_policy.target.as_ref() {
            connect_computer_connection_target_by_policy(app, state, &id, target)
                .await
                .map_err(RuntimeActionError::runtime)?;
        }
    }

    Ok(status_from_instance(&instance, &runtime).await)
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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
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
            connect_connection_target_for_policy_core(state, id, target).await
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
    use crate::commands::inputs::InputDefinition;
    use crate::services::computer::ComputerRuntimeState;
    use crate::services::config::ConfigService;
    use crate::services::manager_context::ManagerContextKey;
    use crate::services::manager_environment::ManagerEnvironment;
    use crate::services::observability::ObservabilityService;
    use crate::services::settings::SettingsService;
    use a2c_smcp::smcp_computer::settings::config::{ConfigEdit, ConfigEntity, EditIntent};
    use tempfile::TempDir;

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
    async fn computer_status_excludes_internal_client_control_from_mcp_counts() {
        let (state, _dir) = test_state();
        state
            .config
            .update_computer_instance("computer-a", |instance| {
                instance.remote_control.enabled = true;
            })
            .unwrap();

        let stopped = get_computer_instance_status_core(&state, "computer-a".to_string())
            .await
            .unwrap();
        assert_eq!(stopped.mcp_server_count, 0);
        assert_eq!(stopped.runtime.mcp_servers, 0);
        assert_eq!(stopped.runtime.active_mcp_servers, 0);

        let started = start_computer_instance_core(None, &state, "computer-a".to_string())
            .await
            .unwrap();

        assert_eq!(started.mcp_server_count, 0);
        assert_eq!(started.runtime.mcp_servers, 0);
        assert_eq!(started.runtime.active_mcp_servers, 0);

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
        assert_eq!(restarted.mcp_server_count, 1);
        assert_eq!(restarted.runtime.mcp_servers, 1);
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
                instance.inputs = vec![InputDefinition::PromptString {
                    id: "new-input".to_string(),
                    label: "New Input".to_string(),
                    description: None,
                    default: None,
                    password: None,
                }];
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
