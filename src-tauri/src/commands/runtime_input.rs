use crate::services::runtime_input_bridge::{
    RuntimeInputCompletion, RuntimeInputRequest, RuntimeInputRequestSink,
    RUNTIME_INPUT_REQUEST_EVENT,
};
use crate::AppState;
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

pub(crate) struct TauriRuntimeInputRequestSink {
    app: AppHandle,
}

impl TauriRuntimeInputRequestSink {
    pub(crate) fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl RuntimeInputRequestSink for TauriRuntimeInputRequestSink {
    fn emit(&self, request: &RuntimeInputRequest) -> Result<(), String> {
        self.app
            .emit(RUNTIME_INPUT_REQUEST_EVENT, request)
            .map_err(|error| error.to_string())
    }
}

pub(crate) fn install_runtime_input_sink(app: AppHandle, state: &AppState) {
    state
        .computer_registry
        .runtime_input_bridge()
        .set_sink(Arc::new(TauriRuntimeInputRequestSink::new(app)));
}

#[tauri::command]
pub async fn runtime_input_bridge_ready(
    state: State<'_, AppState>,
    lease_id: String,
    ready: bool,
) -> Result<(), String> {
    state
        .computer_registry
        .runtime_input_bridge()
        .set_ready(&lease_id, ready);
    Ok(())
}

#[tauri::command]
pub async fn complete_runtime_input_request(
    state: State<'_, AppState>,
    request_id: String,
    completion: RuntimeInputCompletion,
) -> Result<(), RuntimeInputCompletionCommandError> {
    state
        .computer_registry
        .runtime_input_bridge()
        .complete(&request_id, completion)
        .await
        .map_err(|error| RuntimeInputCompletionCommandError {
            code: error.code(),
            terminal: true,
            message: error.to_string(),
        })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInputCompletionCommandError {
    code: &'static str,
    terminal: bool,
    message: String,
}
