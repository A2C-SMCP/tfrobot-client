//! TFRSManager Tauri 命令（前端 IPC 端点）。
//!
//! 本层只做两件事：
//! 1. 透传调用到 `services::manager_client::ManagerClient`；
//! 2. 在 `ManagerError::Unauthorized` 时向前端 emit `manager:auth-expired` 事件，让 UI 跳回登录页。
//!
//! 事件契约（前端侧文档）：
//! - `manager:auth-expired` — payload 为空字符串；收到即应清理本地 Manager 登录态并引导重新登录。

use tauri::{AppHandle, Emitter, State};

use crate::services::manager_client::{
    ConnectionInfoResponse, DigitalEmployeeBrief, LoginResult, ManagerError, UserInfo,
};
use crate::AppState;

/// 401 事件名。导出为 pub const 便于前端在单一来源引用（通过 get_app_info 之类的常量桥，后续 UI 可接）。
pub const AUTH_EXPIRED_EVENT: &str = "manager:auth-expired";

/// 对外：如果错误是 `Unauthorized`，顺手 emit 一次事件给前端。
fn maybe_emit_auth_expired(app: &AppHandle, err: &ManagerError) {
    if matches!(err, ManagerError::Unauthorized) {
        if let Err(e) = app.emit(AUTH_EXPIRED_EVENT, "") {
            log::warn!("failed to emit {AUTH_EXPIRED_EVENT}: {e}");
        }
    }
}

#[tauri::command]
pub async fn manager_login(
    state: State<'_, AppState>,
    app: AppHandle,
    base_url: Option<String>,
    username: String,
    password: String,
) -> Result<LoginResult, ManagerError> {
    log::info!(
        "manager_login: base_url_provided={} username={}",
        base_url.is_some(),
        username
    );
    state
        .manager_client
        .login(base_url, &username, &password)
        .await
        .inspect_err(|e| maybe_emit_auth_expired(&app, e))
}

#[tauri::command]
pub async fn manager_select_account(
    state: State<'_, AppState>,
    app: AppHandle,
    account_id: String,
) -> Result<UserInfo, ManagerError> {
    log::info!("manager_select_account: account_id={}", account_id);
    state
        .manager_client
        .select_account(&account_id)
        .await
        .inspect_err(|e| maybe_emit_auth_expired(&app, e))
}

#[tauri::command]
pub async fn manager_list_digital_employees(
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Vec<DigitalEmployeeBrief>, ManagerError> {
    state
        .manager_client
        .list_digital_employees()
        .await
        .inspect_err(|e| maybe_emit_auth_expired(&app, e))
}

#[tauri::command]
pub async fn manager_get_connection_info(
    state: State<'_, AppState>,
    app: AppHandle,
    id: String,
) -> Result<ConnectionInfoResponse, ManagerError> {
    log::info!("manager_get_connection_info: id={}", id);
    state
        .manager_client
        .get_connection_info(&id)
        .await
        .inspect_err(|e| maybe_emit_auth_expired(&app, e))
}

#[tauri::command]
pub async fn manager_logout(state: State<'_, AppState>) -> Result<(), ManagerError> {
    log::info!("manager_logout");
    state.manager_client.logout().await
}
