//! Narrow Tauri boundary for the managed Chat Kit workspace.

use tauri::State;

use crate::services::chat_session::{
    ChatHttpResponse, ChatSessionCredential, ChatSessionDescriptor,
};
use crate::services::manager_client::ManagerError;
use crate::AppState;

#[tauri::command]
pub async fn chat_open_session(
    state: State<'_, AppState>,
    employee_id: u64,
) -> Result<ChatSessionDescriptor, ManagerError> {
    state.chat_sessions.open(employee_id).await
}

#[tauri::command]
pub async fn chat_get_session_token(
    state: State<'_, AppState>,
    lease_id: String,
) -> Result<ChatSessionCredential, ManagerError> {
    state.chat_sessions.credential(&lease_id).await
}

#[tauri::command]
pub async fn chat_invalidate_session(
    state: State<'_, AppState>,
    lease_id: String,
) -> Result<(), ManagerError> {
    state.chat_sessions.invalidate(&lease_id).await
}

#[tauri::command]
pub async fn chat_http_request(
    state: State<'_, AppState>,
    lease_id: String,
    url: String,
    method: String,
    body: Option<String>,
) -> Result<ChatHttpResponse, ManagerError> {
    state
        .chat_sessions
        .proxy(&lease_id, &url, &method, body)
        .await
}

#[tauri::command]
pub async fn chat_close_session(
    state: State<'_, AppState>,
    lease_id: String,
) -> Result<(), ManagerError> {
    state.chat_sessions.close(&lease_id).await;
    Ok(())
}
