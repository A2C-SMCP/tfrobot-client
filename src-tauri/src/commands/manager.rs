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
    DigitalEmployeeBrief, LoginResult, ManagerError, UserInfo,
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
    phone: String,
    password: String,
) -> Result<LoginResult, ManagerError> {
    log::info!(
        "manager_login: base_url_provided={} phone={}",
        base_url.is_some(),
        phone
    );
    state
        .manager_client
        .login(base_url, &phone, &password)
        .await
        .inspect_err(|e| maybe_emit_auth_expired(&app, e))
}

#[tauri::command]
pub async fn manager_select_account(
    state: State<'_, AppState>,
    app: AppHandle,
    account_id: u64,
) -> Result<UserInfo, ManagerError> {
    log::info!("manager_select_account: account_id={}", account_id);
    state
        .manager_client
        .select_account(account_id)
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

// 注：原 `manager_get_connection_info` Tauri 命令已移除——切到后端编排（`manager_connect_smcp`）后
// 前端不再直接拉 connection-info（避免短 JWT/密钥流入 JS 层）。`ManagerClient::get_connection_info`
// 方法仍由 `manager_connect_smcp` 内部使用。

#[tauri::command]
pub async fn manager_logout(state: State<'_, AppState>) -> Result<(), ManagerError> {
    log::info!("manager_logout");
    state.manager_client.logout().await
}
