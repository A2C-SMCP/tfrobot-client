//! Narrow Tauri boundary for the managed Chat Kit workspace.

use tauri::State;

use crate::services::chat_session::{
    ChatHttpResponse, ChatResourceError, ChatResourceHandle, ChatSessionCredential,
    ChatSessionDescriptor, ChatUploadFile,
};
use crate::services::manager_client::ManagerError;
use crate::AppState;

pub fn install_resource_diagnostics(
    app: tauri::AppHandle,
    service: &crate::services::chat_session::ChatSessionService,
) {
    use tauri::Emitter;
    let mut errors = service.subscribe_resource_errors();
    tauri::async_runtime::spawn(async move {
        loop {
            match errors.recv().await {
                Ok(error) => {
                    let _ = app.emit_to("main", "chat-resource-error", error);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

#[tauri::command]
pub async fn chat_open_session(
    state: State<'_, AppState>,
    employee_id: u64,
    selection_revision: u64,
) -> Result<ChatSessionDescriptor, ManagerError> {
    state
        .chat_sessions
        .open(employee_id, selection_revision)
        .await
}

#[tauri::command]
pub async fn chat_get_recent_robot(
    state: State<'_, AppState>,
) -> Result<Option<u64>, ManagerError> {
    state.chat_sessions.recent_employee().await
}

#[tauri::command]
pub async fn chat_remember_robot(
    state: State<'_, AppState>,
    lease_id: String,
) -> Result<(), ManagerError> {
    state.chat_sessions.remember(&lease_id).await
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

/// Reserve a cancellation slot before the frontend starts transferring bytes.
#[tauri::command]
pub async fn chat_prepare_transfer(
    state: State<'_, AppState>,
    lease_id: String,
) -> Result<String, ManagerError> {
    state.chat_sessions.prepare_transfer(&lease_id).await
}

#[tauri::command]
pub async fn chat_cancel_transfer(
    state: State<'_, AppState>,
    lease_id: String,
    transfer_id: String,
) -> Result<(), ManagerError> {
    state.chat_sessions.cancel_transfer(&lease_id, &transfer_id);
    Ok(())
}

#[tauri::command]
pub async fn chat_upload_request(
    state: State<'_, AppState>,
    lease_id: String,
    transfer_id: String,
    url: String,
    file: ChatUploadFile,
) -> Result<ChatHttpResponse, ManagerError> {
    state
        .chat_sessions
        .upload(&lease_id, &transfer_id, &url, file)
        .await
}

#[tauri::command]
pub async fn chat_resolve_resource(
    state: State<'_, AppState>,
    lease_id: String,
    uri: String,
) -> Result<ChatResourceHandle, ChatResourceError> {
    state.chat_sessions.register_resource(&lease_id, &uri).await
}

#[tauri::command]
pub async fn chat_release_resource(
    state: State<'_, AppState>,
    lease_id: String,
    resource_id: String,
) -> Result<(), ChatResourceError> {
    state
        .chat_sessions
        .release_resource(&lease_id, &resource_id);
    Ok(())
}

#[tauri::command]
pub async fn chat_save_resource(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    lease_id: String,
    resource_id: String,
    name: Option<String>,
    open: bool,
) -> Result<(), ChatResourceError> {
    use tauri_plugin_dialog::DialogExt;
    use tauri_plugin_opener::OpenerExt;
    let service = &state.chat_sessions;
    let name = service
        .resource_file_name(&lease_id, &resource_id, name.as_deref())
        .await?;
    if open {
        return service
            .open_resource(&lease_id, &resource_id, &name, |path| {
                app.opener()
                    .open_path(path.to_string_lossy(), None::<&str>)
                    .map_err(|_| ChatResourceError::Unsupported)
            })
            .await;
    }
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_file_name(name)
        .save_file(move |path| {
            let _ = sender.send(path);
        });
    let destination = service
        .await_resource_destination(&lease_id, &resource_id, async {
            receiver
                .await
                .map_err(|_| ChatResourceError::Save)?
                .map(|path| path.into_path().map_err(|_| ChatResourceError::Save))
                .transpose()
        })
        .await?;
    if let Some(destination) = destination {
        service
            .save_resource(&lease_id, &resource_id, &destination)
            .await?;
    }
    Ok(())
}
