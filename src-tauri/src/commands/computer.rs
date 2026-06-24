use crate::commands::connection::{
    connect_connection_target_core, connect_manager_robot_target_core, disconnect_smcp_core,
};
use crate::services::computer::{
    ComputerConnectionPolicy, ComputerConnectionTarget, ComputerConnectionTargetType,
    ComputerInstance, ComputerInstanceId, ConnectionStateSummary, RobotBindingMetadata,
};
use crate::AppState;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputerInstanceStatus {
    pub id: ComputerInstanceId,
    pub name: String,
    pub description: Option<String>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateComputerConnectionPolicyRequest {
    pub id: ComputerInstanceId,
    pub target: Option<ComputerConnectionTarget>,
    pub auto_connect: bool,
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
            Some(_) => {
                state
                    .computer_registry
                    .update_runtime_instance(instance.clone())
                    .await
            }
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
    let runtime = state
        .computer_registry
        .update_runtime_instance(instance.clone())
        .await;

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
    let updated = state
        .config
        .update_computer_instance(&request.id, |instance| {
            instance.name = name;
            instance.description = normalize_optional_text(request.description);
        })
        .map_err(|error| error.to_string())?;
    let runtime = state
        .computer_registry
        .update_runtime_instance(updated.clone())
        .await;

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
    instance.id = generate_instance_id();
    instance.name = name;
    instance.description = normalize_optional_text(request.description);
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
    state
        .config
        .remove_computer_instance(&id)
        .map_err(|error| error.to_string())?;

    if let Some(runtime) = state.computer_registry.remove_runtime(&id).await {
        runtime.shutdown().await;
    }

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
        .await;
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

    let updated = state
        .config
        .update_computer_instance(&request.id, |instance| {
            instance.connection_policy = ComputerConnectionPolicy {
                target: request.target.clone(),
                auto_connect: request.auto_connect,
            };
        })
        .map_err(|error| error.to_string())?;
    let runtime = state
        .computer_registry
        .update_runtime_instance(updated.clone())
        .await;

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
        running: runtime.is_running().await,
        connected: runtime.is_connected().await,
        mcp_server_count: instance.mcp_servers.len(),
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
