//! TFRSManager Tauri commands.
//!
//! Authentication and authenticated requests are delegated to the backend-owned
//! [`ManagerContextCoordinator`]. Commands never construct identity context in the webview.

use tauri::{AppHandle, Emitter, State};

use crate::services::manager_client::{DigitalEmployeeBrief, LoginResult, ManagerError, UserInfo};
use crate::services::manager_context::{
    ManagerContextEventSink, ManagerContextSnapshot, RestoredManagerSession,
    MANAGER_AUTH_EXPIRED_EVENT, MANAGER_CONTEXT_CHANGED_EVENT,
};
use crate::services::manager_environment::ManagerEnvironment;
use crate::AppState;

pub(crate) struct TauriManagerContextEventSink {
    app: AppHandle,
}

impl TauriManagerContextEventSink {
    pub(crate) fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl ManagerContextEventSink for TauriManagerContextEventSink {
    fn emit_context_changed(&self, snapshot: &ManagerContextSnapshot) -> Result<(), String> {
        self.app
            .emit(MANAGER_CONTEXT_CHANGED_EVENT, snapshot)
            .map_err(|error| error.to_string())
    }

    fn emit_auth_expired(&self) -> Result<(), String> {
        self.app
            .emit(MANAGER_AUTH_EXPIRED_EVENT, ())
            .map_err(|error| error.to_string())
    }
}

#[tauri::command]
pub async fn manager_get_context(
    state: State<'_, AppState>,
) -> Result<ManagerContextSnapshot, ManagerError> {
    Ok(state.manager_context.snapshot().await)
}

#[tauri::command]
pub async fn manager_login(
    state: State<'_, AppState>,
    environment: ManagerEnvironment,
    identifier: String,
    password: String,
) -> Result<LoginResult, ManagerError> {
    log::info!("manager_login: environment={environment:?}");
    state
        .manager_context
        .login(environment, &identifier, &password)
        .await
}

#[tauri::command]
pub async fn manager_select_account(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<UserInfo, ManagerError> {
    log::info!("manager_select_account: account_id={account_id}");
    state.manager_context.select_account(&account_id).await
}

#[tauri::command]
pub async fn manager_restore_session(
    state: State<'_, AppState>,
) -> Result<Option<RestoredManagerSession>, ManagerError> {
    state.manager_context.restore_session().await
}

#[tauri::command]
pub async fn manager_list_digital_employees(
    state: State<'_, AppState>,
) -> Result<Vec<DigitalEmployeeBrief>, ManagerError> {
    state.manager_context.list_digital_employees().await
}

#[tauri::command]
pub async fn manager_logout(state: State<'_, AppState>) -> Result<(), ManagerError> {
    log::info!("manager_logout");
    state.manager_context.logout().await
}
