use crate::services::client_control::{
    ClientControlError, ClientControlErrorCode, RemoteControlPolicy, TargetContract, ToolGroup,
    ToolId, ToolRisk,
};
use crate::services::observability::{
    redact_text, ActivityEventDraft, ActivityLevel, ActivityOutcome,
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
    state.client_control.persisted_policy(computer_id)
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
    let operation_state = state.clone();
    tokio::spawn(async move {
        update_remote_control_policy_transaction(&operation_state, request).await
    })
    .await
    .map_err(|error| {
        ClientControlError::new(
            ClientControlErrorCode::OperationFailed,
            format!("Client Control policy transaction task failed: {error}"),
        )
    })?
}

async fn update_remote_control_policy_transaction(
    state: &AppState,
    request: UpdateRemoteControlPolicyRequest,
) -> Result<RemoteControlPolicy, ClientControlError> {
    let started = std::time::Instant::now();
    let computer_id = request.computer_id.clone();
    let previous = state.client_control.persisted_policy(&computer_id).ok();
    let result = state
        .client_control
        .update_policy_local(&request.computer_id, request.policy)
        .await;
    let changed_keys = result
        .as_ref()
        .ok()
        .map(|updated| changed_policy_keys(previous.as_ref(), updated))
        .unwrap_or_default();
    if !changed_keys.is_empty() || result.is_err() {
        let (level, outcome, message, error_code, error) = match &result {
            Ok(_) => (
                ActivityLevel::Info,
                ActivityOutcome::Succeeded,
                "Client Control policy updated",
                None,
                None,
            ),
            Err(error) => (
                ActivityLevel::Warn,
                ActivityOutcome::Failed,
                "Client Control policy update failed",
                serde_json::to_value(error.code).ok(),
                Some(redact_text(&error.message)),
            ),
        };
        let mut activity = ActivityEventDraft::computer(
            &computer_id,
            level,
            "security",
            "client_control_policy",
            "update",
            outcome,
            message,
        );
        activity.fields = Some(serde_json::json!({
            "app_version": env!("CARGO_PKG_VERSION"),
            "trigger": "user",
            "changed_keys": changed_keys,
            "duration_ms": started.elapsed().as_millis(),
            "error_code": error_code,
            "error": error,
        }));
        activity.correlation_id = Some(uuid::Uuid::new_v4().to_string());
        if let Err(error) = state.observability.record_activity_async(activity).await {
            log::error!("failed to persist Client Control policy activity: {error}");
        }
    }
    result
}

fn changed_policy_keys(
    previous: Option<&RemoteControlPolicy>,
    next: &RemoteControlPolicy,
) -> Vec<&'static str> {
    let Some(previous) = previous else {
        return vec!["enabled", "tool_scope", "target_scope"];
    };
    let mut keys = Vec::new();
    if previous.enabled != next.enabled {
        keys.push("enabled");
    }
    if previous.tool_scope != next.tool_scope {
        keys.push("tool_scope");
    }
    if previous.target_scope != next.target_scope {
        keys.push("target_scope");
    }
    keys
}
