use crate::services::client_control::{
    ClientControlError, RemoteControlPolicy, TargetContract, ToolGroup, ToolId, ToolRisk,
};
use crate::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientControlToolView {
    pub id: ToolId,
    pub group: ToolGroup,
    pub risk: ToolRisk,
    pub target: TargetContract,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateRemoteControlPolicyRequest {
    pub computer_id: String,
    pub policy: RemoteControlPolicy,
}

#[tauri::command]
pub fn get_client_control_catalog(state: State<'_, AppState>) -> Vec<ClientControlToolView> {
    get_client_control_catalog_core(&state)
}

pub fn get_client_control_catalog_core(state: &AppState) -> Vec<ClientControlToolView> {
    state
        .client_control
        .catalog()
        .into_iter()
        .map(|tool| ClientControlToolView {
            id: tool.id,
            group: tool.group,
            risk: tool.risk,
            target: tool.target,
            input_schema: tool.input_schema(),
        })
        .collect()
}

#[tauri::command]
pub fn get_remote_control_policy(
    state: State<'_, AppState>,
    computer_id: String,
) -> Result<RemoteControlPolicy, ClientControlError> {
    get_remote_control_policy_core(&state, &computer_id)
}

pub fn get_remote_control_policy_core(
    state: &AppState,
    computer_id: &str,
) -> Result<RemoteControlPolicy, ClientControlError> {
    state.client_control.policy(computer_id)
}

#[tauri::command]
pub async fn update_remote_control_policy(
    state: State<'_, AppState>,
    request: UpdateRemoteControlPolicyRequest,
) -> Result<RemoteControlPolicy, ClientControlError> {
    update_remote_control_policy_core(&state, request).await
}

pub async fn update_remote_control_policy_core(
    state: &AppState,
    request: UpdateRemoteControlPolicyRequest,
) -> Result<RemoteControlPolicy, ClientControlError> {
    state
        .client_control
        .update_policy_local(&request.computer_id, request.policy)
        .await
}
