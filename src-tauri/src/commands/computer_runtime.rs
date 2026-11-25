use crate::services::computer::ClientConnectionStateSnapshot;
use crate::services::computer_runtime_events::{
    ComputerRuntimeEventSink, ComputerRuntimeSnapshot, ComputerRuntimeStatusEvent,
    COMPUTER_RUNTIME_STATUS_EVENT,
};
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputerRuntimeSnapshotRecord {
    pub instance_id: String,
    pub snapshot: ComputerRuntimeSnapshot,
    pub connection: ClientConnectionStateSnapshot,
}

#[derive(Clone)]
struct TauriComputerRuntimeEventSink {
    app: AppHandle,
}

impl ComputerRuntimeEventSink for TauriComputerRuntimeEventSink {
    fn emit(&self, event: &ComputerRuntimeStatusEvent) -> Result<(), String> {
        self.app
            .emit(COMPUTER_RUNTIME_STATUS_EVENT, event)
            .map_err(|error| error.to_string())
    }
}

async fn runtime_snapshot_records(state: &AppState) -> Vec<ComputerRuntimeSnapshotRecord> {
    state
        .computer_registry
        .runtime_observations()
        .await
        .into_iter()
        .map(
            |(instance_id, snapshot, connection)| ComputerRuntimeSnapshotRecord {
                instance_id,
                snapshot,
                connection,
            },
        )
        .collect()
}

#[tauri::command]
pub async fn enable_computer_runtime_events(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<ComputerRuntimeSnapshotRecord>, String> {
    state
        .computer_registry
        .set_runtime_ui_event_sink(Arc::new(TauriComputerRuntimeEventSink { app }))
        .await;
    Ok(runtime_snapshot_records(&state).await)
}

#[tauri::command]
pub async fn get_computer_runtime_snapshots(
    state: State<'_, AppState>,
) -> Result<Vec<ComputerRuntimeSnapshotRecord>, String> {
    Ok(runtime_snapshot_records(&state).await)
}

/// The registry write lease stabilizes the saved configuration and runtime handoff together.
#[tauri::command]
pub async fn get_computer_config_adoption(
    state: State<'_, AppState>,
    id: String,
) -> Result<crate::services::computer::config_adoption::ConfigAdoptionStatus, String> {
    get_computer_config_adoption_core(&state, &id).await
}

pub async fn get_computer_config_adoption_core(
    state: &AppState,
    id: &str,
) -> Result<crate::services::computer::ConfigAdoptionStatus, String> {
    let _operation = state.computer_registry.operation_lease(id).await;
    let runtime = state
        .computer_registry
        .runtime(id)
        .await
        .ok_or_else(|| "Computer not found".to_string())?;
    Ok(runtime.config_adoption_status().await)
}
