use crate::commands::connection::{
    connect_connection_target_core, connect_manager_robot_target_core, disconnect_smcp_core,
};
use crate::commands::runtime_sync::apply_updated_computer_instance;
use crate::services::computer::{
    ComputerConnectionPolicy, ComputerConnectionTarget, ComputerConnectionTargetType,
    ComputerInstance, ComputerInstanceId, ConnectionStateSummary, RobotBindingMetadata,
};
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputerInstanceStatus {
    pub id: ComputerInstanceId,
    pub name: String,
    pub description: Option<String>,
    pub local_skills_root: Option<PathBuf>,
    pub effective_skill_home: PathBuf,
    pub running: bool,
    pub connected: bool,
    pub mcp_server_count: usize,
    pub robot_binding: Option<RobotBindingMetadata>,
    pub connection_policy: ComputerConnectionPolicy,
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
) -> Result<Vec<ComputerInstanceStatus>, String> {
    list_computer_instances_core(&state).await
}

pub async fn list_computer_instances_core(
    state: &AppState,
) -> Result<Vec<ComputerInstanceStatus>, String> {
    let config = state
        .config
        .load_computer_instances()
        .map_err(|error| error.to_string())?;
    let mut statuses = Vec::with_capacity(config.instances.len());

    for instance in config.instances {
        let runtime = match state.computer_registry.runtime(&instance.id).await {
            Some(runtime) => runtime,
            None => {
                state
                    .computer_registry
                    .upsert_runtime(instance.clone())
                    .await
            }
        };
        statuses.push(status_from_instance(&instance, &runtime).await);
    }

    Ok(statuses)
}

#[tauri::command]
pub async fn get_computer_instance_status(
    state: State<'_, AppState>,
    id: ComputerInstanceId,
) -> Result<ComputerInstanceStatus, String> {
    get_computer_instance_status_core(&state, id).await
}

pub async fn get_computer_instance_status_core(
    state: &AppState,
    id: ComputerInstanceId,
) -> Result<ComputerInstanceStatus, String> {
    let config = state
        .config
        .load_computer_instances()
        .map_err(|error| error.to_string())?;
    let instance = config
        .instances
        .into_iter()
        .find(|instance| instance.id == id)
        .ok_or_else(|| format!("Computer instance not found: {id}"))?;
    let runtime = match state.computer_registry.runtime(&instance.id).await {
        Some(runtime) => runtime,
        None => {
            state
                .computer_registry
                .upsert_runtime(instance.clone())
                .await
        }
    };

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
        robot_binding: None,
    };

    state
        .config
        .add_computer_instance(instance.clone())
        .map_err(|error| error.to_string())?;
    let runtime = state
        .computer_registry
        .upsert_runtime(instance.clone())
        .await;

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
        instance.connection_policy.target = Some(ComputerConnectionTarget {
            target_type: ComputerConnectionTargetType::ManualSmcp,
            id: target_id,
            robot_account_id: None,
        });
    }
    state
        .config
        .add_computer_instance(instance.clone())
        .map_err(|error| error.to_string())?;
    if let Err(error) = state.sdk_config.duplicate(&source_id, &instance.id) {
        return Err(rollback_failed_duplicate(
            state,
            &instance.id,
            &destination_storage_root,
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
        return Err(rollback_failed_duplicate(
            state,
            &instance.id,
            &destination_storage_root,
            error,
        )
        .await);
    }
    let runtime = state
        .computer_registry
        .upsert_runtime(instance.clone())
        .await;

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
    let instance_storage_root = state.config.computer_instance_storage_root(&id);
    state
        .config
        .remove_computer_instance(&id)
        .map_err(|error| error.to_string())?;

    if let Some(runtime) = state.computer_registry.remove_runtime(&id).await {
        runtime.shutdown().await;
    }
    cleanup_computer_instance_storage(&instance_storage_root).await?;

    Ok(())
}

#[tauri::command]
pub async fn start_computer_instance(
    app: AppHandle,
    state: State<'_, AppState>,
    id: ComputerInstanceId,
) -> Result<ComputerInstanceStatus, String> {
    start_computer_instance_core(Some(&app), &state, id).await
}

pub async fn start_computer_instance_core(
    app: Option<&AppHandle>,
    state: &AppState,
    id: ComputerInstanceId,
) -> Result<ComputerInstanceStatus, String> {
    let instance = state
        .config
        .get_computer_instance(&id)
        .map_err(|error| error.to_string())?;
    let runtime = state
        .computer_registry
        .update_runtime_instance(instance.clone())
        .await?;
    runtime.start().await?;
    if instance.connection_policy.auto_connect {
        if let Some(target) = instance.connection_policy.target.as_ref() {
            if let Err(error) =
                connect_computer_connection_target_by_policy(app, state, &id, target).await
            {
                let _ = state.log_service.write_for_instance(
                    "warn",
                    "connection",
                    &format!("Auto connect failed: {error}"),
                    None,
                    Some(&id),
                );
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
    let instance = state
        .config
        .get_computer_instance(&id)
        .map_err(|error| error.to_string())?;
    let runtime = state
        .computer_registry
        .runtime(&id)
        .await
        .ok_or_else(|| format!("Computer instance not found: {id}"))?;
    runtime.shutdown().await;

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
    validate_connection_target_reference(state, request.target.as_ref())?;
    let previous = state
        .config
        .get_computer_instance(&request.id)
        .map_err(|error| error.to_string())?;

    let updated = state
        .config
        .update_computer_instance(&request.id, |instance| {
            instance.connection_policy = ComputerConnectionPolicy {
                target: request.target.clone(),
                auto_connect: request.auto_connect,
            };
        })
        .map_err(|error| error.to_string())?;
    let runtime = apply_updated_computer_instance(state, previous, updated.clone()).await?;

    Ok(status_from_instance(&updated, &runtime).await)
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
    let runtime = state
        .computer_registry
        .runtime(&id)
        .await
        .ok_or_else(|| format!("Computer instance not found: {id}"))?;
    if !runtime.is_running().await {
        return Err("Computer must be running before connecting".to_string());
    }
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
    app: Option<&AppHandle>,
    state: &AppState,
    id: &str,
    target: &ComputerConnectionTarget,
) -> Result<(), String> {
    match target.target_type {
        ComputerConnectionTargetType::ManualSmcp => {
            connect_connection_target_core(state, id, &target.id).await
        }
        ComputerConnectionTargetType::ManagerRobot => {
            let app = app.ok_or_else(|| {
                "Manager Robot auto connect requires an application handle".to_string()
            })?;
            let employee_id = target
                .id
                .parse::<u64>()
                .map_err(|_| "Manager Robot target id must be a numeric employee id".to_string())?;
            let robot_account_id = target
                .robot_account_id
                .ok_or_else(|| "Manager Robot target missing robotAccountId".to_string())?;
            connect_manager_robot_target_core(app, state, id, employee_id, robot_account_id)
                .await
                .map_err(|error| error.to_string())
        }
    }
}

fn validate_connection_target_reference(
    state: &AppState,
    target: Option<&ComputerConnectionTarget>,
) -> Result<(), String> {
    match target {
        Some(ComputerConnectionTarget {
            target_type: ComputerConnectionTargetType::ManualSmcp,
            id,
            ..
        }) => {
            state
                .config
                .get_manual_smcp_target(id)
                .map_err(|error| error.to_string())?;
            Ok(())
        }
        Some(ComputerConnectionTarget {
            target_type: ComputerConnectionTargetType::ManagerRobot,
            id,
            robot_account_id,
        }) => {
            id.parse::<u64>()
                .map_err(|_| "Manager Robot target id must be a numeric employee id".to_string())?;
            robot_account_id
                .ok_or_else(|| "Manager Robot target missing robotAccountId".to_string())?;
            Ok(())
        }
        None => Ok(()),
    }
}

async fn status_from_instance(
    instance: &ComputerInstance,
    runtime: &crate::services::computer::ComputerInstanceRuntime,
) -> ComputerInstanceStatus {
    ComputerInstanceStatus {
        id: instance.id.clone(),
        name: instance.name.clone(),
        description: instance.description.clone(),
        local_skills_root: instance.local_skills_root.clone(),
        effective_skill_home: runtime.sdk_skill_home().await,
        running: runtime.is_running().await,
        connected: runtime.is_connected().await,
        mcp_server_count: runtime.mcp_server_inventory_count().await,
        robot_binding: instance.robot_binding.clone(),
        connection_policy: instance.connection_policy.clone(),
        connection: runtime.connection_status().await,
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

async fn cleanup_computer_instance_storage(path: &Path) -> Result<(), String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || match fs::remove_dir_all(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "Failed to delete computer instance storage {}: {}",
            path.display(),
            error
        )),
    })
    .await
    .map_err(|error| format!("Failed to delete computer instance storage: {error}"))?
}

async fn rollback_failed_duplicate(
    state: &AppState,
    instance_id: &str,
    storage_root: &Path,
    primary_error: String,
) -> String {
    let mut rollback_errors = Vec::new();
    if let Err(error) = state.sdk_config.delete(instance_id) {
        rollback_errors.push(format!("delete SDK config: {error}"));
    }
    if let Err(error) = state.config.remove_computer_instance(instance_id) {
        rollback_errors.push(format!("remove persisted Computer profile: {error}"));
    }
    if let Err(error) = cleanup_computer_instance_storage(storage_root).await {
        rollback_errors.push(format!("clean target instance storage: {error}"));
    }

    if rollback_errors.is_empty() {
        format!("Failed to duplicate Computer instance: {primary_error}")
    } else {
        format!(
            "Failed to duplicate Computer instance: {primary_error}; additionally failed to rollback: {}",
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
    use crate::services::config::ConfigService;
    use crate::services::logger::LogService;
    use crate::services::settings::SettingsService;
    use a2c_smcp::smcp_computer::settings::config::{ConfigEdit, ConfigEntity, EditIntent};
    use tempfile::TempDir;

    fn test_state() -> (AppState, TempDir) {
        let dir = TempDir::new().unwrap();
        let config = ConfigService::new(dir.path().to_path_buf()).unwrap();
        config
            .add_computer_instance(ComputerInstance::new("computer-a", "Computer A"))
            .unwrap();
        let log_service = LogService::new(dir.path()).unwrap();
        let settings_service = SettingsService::new(dir.path().to_path_buf());
        (AppState::new(config, log_service, settings_service), dir)
    }

    #[tokio::test]
    async fn update_computer_skill_home_updates_runtime_and_can_restore_default() {
        let (state, dir) = test_state();
        let custom_root = dir.path().join("custom-skill-home");

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
        assert_eq!(updated.effective_skill_home, custom_root);

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
        assert_eq!(
            restored.effective_skill_home,
            state.config.default_local_skills_root("computer-a")
        );
    }

    #[tokio::test]
    async fn computer_status_counts_sdk_snapshot_servers_not_legacy_profile_servers() {
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

        let status = get_computer_instance_status_core(&state, "computer-a".to_string())
            .await
            .unwrap();
        assert_eq!(status.mcp_server_count, 1);
    }
}
