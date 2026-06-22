use crate::services::computer::{
    ComputerInstance, ComputerInstanceId, ConnectionStateSummary, RobotBindingMetadata,
};
use crate::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputerInstanceStatus {
    pub id: ComputerInstanceId,
    pub name: String,
    pub is_default: bool,
    pub running: bool,
    pub connected: bool,
    pub mcp_server_count: usize,
    pub robot_binding: Option<RobotBindingMetadata>,
    pub connection: Option<ConnectionStateSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateComputerInstanceRequest {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameComputerInstanceRequest {
    pub id: ComputerInstanceId,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateComputerInstanceRequest {
    pub source_id: ComputerInstanceId,
    pub name: String,
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
        statuses.push(status_from_instance(&instance, false, &runtime).await);
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

    Ok(status_from_instance(&instance, false, &runtime).await)
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
        ..ComputerInstance::default_instance()
    };

    state
        .config
        .add_computer_instance(instance.clone())
        .map_err(|error| error.to_string())?;
    let runtime = state
        .computer_registry
        .upsert_runtime(instance.clone())
        .await;

    Ok(status_from_instance(&instance, false, &runtime).await)
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
        .rename_computer_instance(&request.id, name)
        .map_err(|error| error.to_string())?;
    let runtime = state
        .computer_registry
        .update_runtime_instance(updated.clone())
        .await;

    Ok(status_from_instance(&updated, false, &runtime).await)
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

    state
        .config
        .add_computer_instance(instance.clone())
        .map_err(|error| error.to_string())?;
    let runtime = state
        .computer_registry
        .upsert_runtime(instance.clone())
        .await;

    Ok(status_from_instance(&instance, false, &runtime).await)
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
    state: State<'_, AppState>,
    id: ComputerInstanceId,
) -> Result<ComputerInstanceStatus, String> {
    start_computer_instance_core(&state, id).await
}

pub async fn start_computer_instance_core(
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

    Ok(status_from_instance(&instance, false, &runtime).await)
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

    Ok(status_from_instance(&instance, false, &runtime).await)
}

async fn status_from_instance(
    instance: &ComputerInstance,
    is_default: bool,
    runtime: &crate::services::computer::ComputerInstanceRuntime,
) -> ComputerInstanceStatus {
    ComputerInstanceStatus {
        id: instance.id.clone(),
        name: instance.name.clone(),
        is_default,
        running: runtime.is_running().await,
        connected: runtime.is_connected().await,
        mcp_server_count: instance.mcp_servers.len(),
        robot_binding: instance.robot_binding.clone(),
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

fn generate_instance_id() -> ComputerInstanceId {
    format!("computer-{}", uuid::Uuid::new_v4())
}
