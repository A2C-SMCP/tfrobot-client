use crate::commands::runtime_sync::apply_updated_computer_instance;
use crate::services::computer::{
    ComputerConnectionTarget, ComputerConnectionTargetType, ComputerInstanceRuntime,
    ComputerRuntimeState, RobotBindingMetadata, SmcpReconnectOutcome,
};
use crate::services::config::normalize_manual_smcp_target;
use crate::services::connection_targets::{manual_target_keychain_id, ManualSmcpTarget};
use crate::services::manager_client::{
    DigitalEmployeeBrief, ExchangedToken, ManagerClient, ManagerError,
};
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::RwLock;
use tokio::time::{timeout, Duration};

const SMCP_CONNECTION_CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

/// 短 JWT 过期前多少秒触发预刷新重连（TFRC-11：`expires_at - 60s`）。
const TOKEN_PREREFRESH_LEAD_SECS: i64 = 60;
/// 预刷新失败（503 签名未就位 / 网络抖动）时的重试间隔；小于 lead，保证过期前多次重试。
const TOKEN_REFRESH_RETRY_SECS: u64 = 10;

/// 单调代际号：后台预刷新任务在原地替换连接前，确认「这仍是我建立的那条连接」，
/// 避免与「用户期间手动断开 / 改连别的机器人」竞态时误覆盖新连接。
static CONNECTION_GENERATION: AtomicU64 = AtomicU64::new(1);

fn next_generation() -> u64 {
    CONNECTION_GENERATION.fetch_add(1, Ordering::Relaxed)
}

/// Manager 驱动连接重建所需的参数（首连 + 预刷新重连共用）。`pub` 供集成测试构造，传给
/// [`reconnect_with_token`] 验证成功、失败和 stale generation 路径。
#[derive(Clone)]
pub struct ManagerConnectionParams {
    /// Socket.IO 服务端 URL（connection-info.socketBaseURL）。
    pub url: String,
    /// SMCP office_id（= connection-info.rid，等价机器人 robotId）。
    pub office_id: String,
    /// 路由 HTTP headers（X-TF-*，**非鉴权**；鉴权走 auth dict）。
    pub routing_headers: HashMap<String, String>,
    /// digital-employee 主键（用于状态标识 / 日志）。
    pub employee_id: u64,
    /// 机器人账号 ID（audience = `robot:<id>`，TFRM-183 暴露）。
    pub robot_account_id: u64,
    /// 可选 scope（None = server 缺省全部能力）。
    pub scope: Option<String>,
    /// Persisted Robot binding metadata for the target ComputerInstance.
    pub robot_binding: RobotBindingMetadata,
}

/// 预刷新换连接的结果：要么当前 business snapshot 仍属本代并已刷新时间戳，
/// 要么连接已被用户断开 / 改连，刷新任务应退出。
/// `pub` 供集成测试验证 generation 守卫。
pub enum SwapResult {
    /// 成功刷新当前连接快照。
    Replaced,
    /// 连接已不属于本代（用户断开 / 改连别的机器人）。
    Stale,
}

#[derive(Debug)]
enum ManagerConnectionDecision {
    Proceed,
    AlreadyConnected,
}

const SOURCE_MANUAL_SMCP: &str = "manual_smcp";
const SOURCE_MANAGER_ROBOT: &str = "manager_robot";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ManualSmcpApiKeyAction {
    #[default]
    Unchanged,
    Set {
        value: String,
    },
    Clear,
}

/// Connection status returned to frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStatusInfo {
    pub connected: bool,
    pub url: Option<String>,
    pub office_id: Option<String>,
    pub computer_name: Option<String>,
    pub connected_at: Option<String>,
    pub profile_name: Option<String>,
    pub source_type: Option<String>,
    pub target_id: Option<String>,
    pub target_name: Option<String>,
    pub employee_id: Option<u64>,
}

/// List globally managed manual SMCP targets.
#[tauri::command]
pub async fn list_manual_smcp_targets(
    state: State<'_, AppState>,
) -> Result<Vec<ManualSmcpTarget>, String> {
    state
        .config
        .list_manual_smcp_targets()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_manual_smcp_target(
    state: State<'_, AppState>,
    target: ManualSmcpTarget,
    api_key_action: Option<ManualSmcpApiKeyAction>,
) -> Result<ManualSmcpTarget, String> {
    let target = normalize_manual_smcp_target(target);
    let credential_key = manual_target_keychain_id(&target.id);
    let saved = state
        .config
        .save_manual_smcp_target(target)
        .map_err(|e| e.to_string())?;

    match api_key_action.unwrap_or_default() {
        ManualSmcpApiKeyAction::Unchanged => {}
        ManualSmcpApiKeyAction::Set { value } => {
            let value = value.trim();
            if !value.is_empty() {
                state
                    .secret_store
                    .set_secret(&credential_key, value)
                    .map_err(|e| e.to_string())?;
            }
        }
        ManualSmcpApiKeyAction::Clear => {
            state
                .secret_store
                .delete_secret_best_effort(&credential_key);
        }
    }

    Ok(saved)
}

#[tauri::command]
pub async fn delete_manual_smcp_target(
    state: State<'_, AppState>,
    target_id: String,
) -> Result<(), String> {
    for runtime in state.computer_registry.list_runtimes().await {
        let guard = runtime.connection.read().await;
        if guard
            .as_ref()
            .and_then(|connection| connection.target_id.as_deref())
            == Some(target_id.as_str())
        {
            return Err("Cannot delete a manual SMCP target while it is connected".to_string());
        }
    }

    state
        .config
        .delete_manual_smcp_target(&target_id)
        .map_err(|e| e.to_string())?;
    state
        .secret_store
        .delete_secret_best_effort(&manual_target_keychain_id(&target_id));
    Ok(())
}

#[tauri::command]
pub async fn connect_connection_target(
    state: State<'_, AppState>,
    instance_id: String,
    target_id: String,
) -> Result<(), String> {
    connect_connection_target_core(&state, &instance_id, &target_id).await
}

pub async fn connect_connection_target_core(
    state: &AppState,
    instance_id: &str,
    target_id: &str,
) -> Result<(), String> {
    let instance_id = require_instance_id(instance_id)?;
    let target = state
        .config
        .get_manual_smcp_target(target_id)
        .map_err(|e| e.to_string())?;
    let api_key = state
        .secret_store
        .get_secret(&manual_target_keychain_id(&target.id))
        .map_err(|e| e.to_string())?;
    let runtime = state
        .computer_registry
        .runtime(instance_id)
        .await
        .ok_or_else(|| format!("Computer instance not found: {instance_id}"))?;
    if !runtime.is_running().await {
        return Err("Computer must be running before connecting".to_string());
    }

    {
        let _guard = state.connection_establish_lock.lock().await;
        if matches!(
            check_connection_target_allowed(state, instance_id, &target.id, &target.office_id)
                .await
                .map_err(|e| e.to_string())?,
            ManagerConnectionDecision::AlreadyConnected
        ) {
            return Ok(());
        }

        clear_unhealthy_connection_snapshot(&runtime).await?;

        let auth_payload = api_key
            .filter(|k| !k.is_empty())
            .map(|tok| serde_json::json!({ "token": tok }));
        let computer_name = runtime.instance.name.clone();
        runtime
            .connect_smcp_socketio(
                &target.url,
                auth_payload,
                target.headers.clone(),
                Some(target.namespace.clone()),
                &target.office_id,
                &computer_name,
            )
            .await?;

        let new_connection = ConnectionState {
            profile_name: target.name.clone(),
            url: target.url.clone(),
            office_id: target.office_id.clone(),
            computer_name,
            connected_at: chrono::Utc::now(),
            source_type: SOURCE_MANUAL_SMCP.to_string(),
            target_id: Some(target.id.clone()),
            target_name: Some(target.name.clone()),
            employee_id: None,
            generation: next_generation(),
        };

        let mut runtime_conn = runtime.connection.write().await;
        if runtime_conn.is_some() {
            return Err(
                "Computer instance already has a connection snapshot; disconnect before reconnecting"
                    .to_string(),
            );
        }
        *runtime_conn = Some(new_connection);
    }
    if let Err(error) = persist_manual_connection_target(state, instance_id, &target.id).await {
        let existing_connection = runtime.connection.write().await.take();
        if let Some(connection) = existing_connection {
            let _ = close_smcp_connection(&runtime, connection).await;
        }
        return Err(error);
    }
    let _ = state.log_service.write_for_instance(
        "info",
        "connection",
        &format!("Connected to manual SMCP target {}", target.name),
        None,
        Some(instance_id),
    );
    Ok(())
}

async fn persist_manual_connection_target(
    state: &AppState,
    instance_id: &str,
    target_id: &str,
) -> Result<(), String> {
    let previous = state
        .config
        .get_computer_instance(instance_id)
        .map_err(|error| error.to_string())?;
    let updated = state
        .config
        .update_computer_instance(instance_id, |instance| {
            instance.connection_policy.target = Some(ComputerConnectionTarget {
                target_type: ComputerConnectionTargetType::ManualSmcp,
                id: target_id.to_string(),
                robot_account_id: None,
            });
        })
        .map_err(|error| error.to_string())?;
    apply_updated_computer_instance(state, previous, updated).await?;
    Ok(())
}

async fn check_connection_target_allowed(
    state: &AppState,
    instance_id: &str,
    target_id: &str,
    office_id: &str,
) -> Result<ManagerConnectionDecision, ManagerError> {
    let runtimes = state.computer_registry.list_runtimes().await;
    for runtime in runtimes {
        let runtime_state = runtime.runtime_state().await;
        let guard = runtime.connection.read().await;
        let Some(connection) = guard.as_ref() else {
            continue;
        };
        if !connection_snapshot_blocks_target(runtime_state) {
            continue;
        }

        match manager_connection_decision(
            instance_id,
            &runtime.instance.id,
            &runtime.instance.name,
            connection.target_id.as_deref(),
            target_id,
            &connection.office_id,
            office_id,
        )? {
            ManagerConnectionDecision::Proceed => {}
            ManagerConnectionDecision::AlreadyConnected => {
                return Ok(ManagerConnectionDecision::AlreadyConnected)
            }
        }
    }

    Ok(ManagerConnectionDecision::Proceed)
}

fn connection_snapshot_blocks_target(state: ComputerRuntimeState) -> bool {
    !matches!(
        state,
        ComputerRuntimeState::Created | ComputerRuntimeState::Stopped | ComputerRuntimeState::Error
    )
}

async fn clear_unhealthy_connection_snapshot(
    runtime: &ComputerInstanceRuntime,
) -> Result<(), String> {
    let has_snapshot = runtime.connection.read().await.is_some();
    if has_snapshot && !runtime.is_connected().await {
        runtime.clear_smcp_connection().await?;
    }
    Ok(())
}

/// Disconnect from SMCP server
#[tauri::command]
pub async fn disconnect_smcp(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<(), String> {
    disconnect_smcp_core(&state, &instance_id).await
}

pub(crate) async fn disconnect_smcp_core(
    state: &AppState,
    instance_id: &str,
) -> Result<(), String> {
    let instance_id = require_instance_id(instance_id)?;
    log::info!("Disconnecting instance {} from SMCP server", instance_id);
    let runtime = state
        .computer_registry
        .runtime(instance_id)
        .await
        .ok_or_else(|| format!("Computer instance not found: {instance_id}"))?;

    let existing_connection = runtime.connection.read().await.as_ref().cloned();
    if let Some(connection) = existing_connection {
        close_smcp_connection(&runtime, connection).await?;
    }

    let _ = state.log_service.write_for_instance(
        "info",
        "connection",
        "Disconnected from SMCP server",
        None,
        Some(instance_id),
    );
    Ok(())
}

pub async fn close_smcp_connection(
    runtime: &ComputerInstanceRuntime,
    _connection: ConnectionState,
) -> Result<(), String> {
    let close_result = timeout(
        SMCP_CONNECTION_CLOSE_TIMEOUT,
        runtime.clear_smcp_connection(),
    )
    .await;
    match close_result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => {
            log::warn!("Error disconnecting SMCP socket: {}", e);
            Err(e)
        }
        Err(_) => {
            let message = format!(
                "Timed out disconnecting SMCP socket after {:?}",
                SMCP_CONNECTION_CLOSE_TIMEOUT,
            );
            log::warn!("{}", message);
            Err(message)
        }
    }
}

/// Get current connection status
#[tauri::command]
pub async fn get_connection_status(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<ConnectionStatusInfo, String> {
    let instance_id = require_instance_id(&instance_id)?;
    let runtime = state
        .computer_registry
        .runtime(instance_id)
        .await
        .ok_or_else(|| format!("Computer instance not found: {instance_id}"))?;
    let connection = runtime.connection_status().await;
    match connection {
        Some(c) => Ok(ConnectionStatusInfo {
            connected: runtime.is_connected().await,
            url: Some(c.url),
            office_id: Some(c.office_id),
            computer_name: Some(c.computer_name),
            connected_at: Some(c.connected_at),
            profile_name: Some(c.profile_name),
            source_type: Some(c.source_type),
            target_id: c.target_id,
            target_name: c.target_name,
            employee_id: c.employee_id,
        }),
        None => Ok(ConnectionStatusInfo {
            connected: false,
            url: None,
            office_id: None,
            computer_name: None,
            connected_at: None,
            profile_name: None,
            source_type: None,
            target_id: None,
            target_name: None,
            employee_id: None,
        }),
    }
}

// ───────────────────────── Manager 驱动连接（TFRC-11 / C1） ─────────────────────────

/// 建立一条 Manager 驱动的 SMCP 连接（token-exchange 全路径）。
///
/// 全路径：`connection-info → exchange_token(robotAccountId) → 短 JWT 注入 Socket.IO auth dict
/// → 连接`。连接面鉴权**唯一**走 auth dict（字段名 `token`，smcp-computer #86）；`routingHeaders`
/// 仅作 HTTP 路由（X-TF-*，非鉴权）。成功后后台起预刷新任务（`expires_in - 60s` teardown+重连）。
///
/// `robot_account_id` 取自 digital-employee 列表的 `robotAccountId`（TFRM-183，nullable —— 前端
/// 应对 null 项禁用连接）。错误沿用 [`ManagerError`]（前端按 `kind` 分支）。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn manager_connect_smcp(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    employee_id: u64,
    robot_account_id: u64,
    robot_id: Option<String>,
    robot_name: Option<String>,
    namespace: Option<String>,
    scope: Option<String>,
) -> Result<(), ManagerError> {
    let instance_id = require_instance_id(&instance_id)
        .map_err(ManagerError::InvalidResponse)?
        .to_string();
    log::info!(
        "manager_connect_smcp: employee_id={employee_id} robot_account_id={robot_account_id}"
    );
    let employee =
        validate_manager_robot_account(state.inner(), employee_id, robot_account_id).await?;

    // 1) 握手参数
    let info = state
        .manager_client
        .get_connection_info(employee_id)
        .await?;
    let url = info.socket_base_url.clone();
    if url.trim().is_empty() {
        return Err(ManagerError::InvalidResponse(
            "connection-info missing socketBaseURL".into(),
        ));
    }
    let office_id = info.rid.clone().filter(|s| !s.is_empty()).ok_or_else(|| {
        ManagerError::InvalidResponse("connection-info missing rid (office_id)".into())
    })?;
    let params = ManagerConnectionParams {
        url,
        office_id: office_id.clone(),
        // routingHeaders 为纯路由头（X-TF-*），verbatim 注入 HTTP header。连接面鉴权唯一走
        // Socket.IO auth dict（字段 `token`，#86），凭据不进网关可读的 header（TFRC-20）。
        routing_headers: info.routing_headers.clone(),
        employee_id,
        robot_account_id,
        scope,
        robot_binding: RobotBindingMetadata {
            employee_id,
            robot_id: robot_id
                .filter(|s| !s.trim().is_empty())
                .or_else(|| employee.robot_id.clone())
                .or_else(|| Some(office_id.clone())),
            robot_account_id: Some(robot_account_id),
            namespace: namespace
                .filter(|s| !s.trim().is_empty())
                .or_else(|| employee.namespace.clone())
                .or_else(|| info.namespace.clone()),
            robot_name: robot_name
                .filter(|s| !s.trim().is_empty())
                .or_else(|| Some(employee.name.clone())),
        },
    };

    // 2) 换短 JWT
    let token = state
        .manager_client
        .exchange_token(&robot_account_id.to_string(), params.scope.clone())
        .await?;

    // 3) 连接 + 入库 + 起预刷新任务
    establish_manager_connection(&app, state.inner(), &instance_id, params, token).await
}

pub async fn connect_manager_robot_target_core(
    app: &AppHandle,
    state: &AppState,
    instance_id: &str,
    employee_id: u64,
    robot_account_id: u64,
) -> Result<(), ManagerError> {
    let employee = validate_manager_robot_account(state, employee_id, robot_account_id).await?;
    let token = state
        .manager_client
        .exchange_token(&robot_account_id.to_string(), None)
        .await?;
    let info = state
        .manager_client
        .get_connection_info(employee_id)
        .await?;
    let url = info.socket_base_url.clone();
    if url.trim().is_empty() {
        return Err(ManagerError::InvalidResponse(
            "connection-info missing socketBaseURL".into(),
        ));
    }
    let office_id = info.rid.clone().filter(|s| !s.is_empty()).ok_or_else(|| {
        ManagerError::InvalidResponse("connection-info missing rid (office_id)".into())
    })?;
    let params = ManagerConnectionParams {
        url,
        office_id: office_id.clone(),
        routing_headers: info.routing_headers.clone(),
        employee_id,
        robot_account_id,
        scope: None,
        robot_binding: RobotBindingMetadata {
            employee_id,
            robot_id: employee
                .robot_id
                .clone()
                .or_else(|| Some(office_id.clone())),
            robot_account_id: Some(robot_account_id),
            namespace: employee
                .namespace
                .clone()
                .or_else(|| info.namespace.clone()),
            robot_name: Some(employee.name.clone()),
        },
    };

    establish_manager_connection(app, state, instance_id, params, token).await
}

async fn validate_manager_robot_account(
    state: &AppState,
    employee_id: u64,
    robot_account_id: u64,
) -> Result<DigitalEmployeeBrief, ManagerError> {
    let employees = state.manager_client.list_digital_employees().await?;
    validate_manager_robot_account_from_list(&employees, employee_id, robot_account_id)
}

fn validate_manager_robot_account_from_list(
    employees: &[DigitalEmployeeBrief],
    employee_id: u64,
    robot_account_id: u64,
) -> Result<DigitalEmployeeBrief, ManagerError> {
    let employee = employees
        .iter()
        .find(|item| item.id == employee_id)
        .cloned()
        .ok_or_else(|| {
            ManagerError::InvalidResponse(format!(
                "Manager Robot target employee {employee_id} is not visible"
            ))
        })?;
    let actual_robot_account_id = employee.robot_account_id.ok_or_else(|| {
        ManagerError::InvalidResponse(format!(
            "robotAccountId missing for Manager Robot target employee {employee_id}"
        ))
    })?;
    if actual_robot_account_id != robot_account_id {
        return Err(ManagerError::InvalidResponse(format!(
            "robotAccountId mismatch for Manager Robot target employee {employee_id}"
        )));
    }
    Ok(employee)
}

/// 用给定参数 + 短 JWT 通过 SDK Computer 建立 Socket.IO 连接并 join_office。
async fn build_and_join(
    runtime: &ComputerInstanceRuntime,
    params: &ManagerConnectionParams,
    jwt: &str,
) -> Result<(), String> {
    // 连接面鉴权唯一走 Socket.IO auth dict（字段名 `token`，smcp-computer #86）。
    let auth_payload = serde_json::json!({ "token": jwt });
    runtime
        .connect_smcp_socketio(
            &params.url,
            Some(auth_payload),
            params.routing_headers.clone(),
            None,
            &params.office_id,
            &runtime.instance.name,
        )
        .await
}

/// 首连：构建连接、替换 AppState、起预刷新任务、emit 状态变更事件。
async fn establish_manager_connection(
    app: &AppHandle,
    state: &AppState,
    instance_id: &str,
    params: ManagerConnectionParams,
    token: ExchangedToken,
) -> Result<(), ManagerError> {
    let runtime = state
        .computer_registry
        .runtime(instance_id)
        .await
        .ok_or_else(|| {
            ManagerError::InvalidResponse(format!("Computer instance not found: {instance_id}"))
        })?;
    {
        let _guard = state.connection_establish_lock.lock().await;

        if matches!(
            check_connection_target_allowed(
                state,
                instance_id,
                &manager_target_id(params.employee_id),
                &params.office_id,
            )
            .await?,
            ManagerConnectionDecision::AlreadyConnected
        ) {
            persist_robot_binding(state, instance_id, &params.robot_binding).await?;
            emit_connection_changed(app);
            return Ok(());
        }
        clear_unhealthy_connection_snapshot(&runtime)
            .await
            .map_err(ManagerError::InvalidResponse)?;
        // 先把新连接建成功，再替换快照；不同 Robot 的切换由 guard 要求用户先断开。
        // 用户发起的连接通常切到「不同机器人」（office 不同），不会撞 room；同机器人重连由 UI 阻止
        // （已连接时显示「断开」），其 token 预刷新走 spawn_refresh_task 的 SDK 重连路径。
        build_and_join(&runtime, &params, &token.access_token)
            .await
            .map_err(|e| ManagerError::NetworkError(format!("SMCP connect failed: {e}")))?;

        let generation = next_generation();
        let refresh_task = spawn_refresh_task(
            app,
            state,
            runtime.connection.clone(),
            runtime.clone(),
            params.clone(),
            instance_id.to_string(),
            generation,
            token.expires_in,
        );
        runtime.set_refresh_task(refresh_task).await;

        let new_connection = ConnectionState {
            profile_name: format!("manager:{}", params.employee_id),
            url: params.url.clone(),
            office_id: params.office_id.clone(),
            computer_name: runtime.instance.name.clone(),
            connected_at: chrono::Utc::now(),
            source_type: SOURCE_MANAGER_ROBOT.to_string(),
            target_id: Some(manager_target_id(params.employee_id)),
            target_name: params.robot_binding.robot_name.clone(),
            employee_id: Some(params.employee_id),
            generation,
        };

        let mut runtime_conn = runtime.connection.write().await;
        if runtime_conn.is_some() {
            return Err(ManagerError::InvalidResponse(
                "Computer instance already has a connection snapshot; disconnect before reconnecting"
                    .to_string(),
            ));
        }
        *runtime_conn = Some(new_connection);
    }

    if let Err(error) = persist_robot_binding(state, instance_id, &params.robot_binding).await {
        let existing_connection = runtime.connection.write().await.take();
        if let Some(connection) = existing_connection {
            let _ = close_smcp_connection(&runtime, connection).await;
        }
        return Err(error);
    }

    let _ = state.log_service.write_for_instance(
        "info",
        "connection",
        &format!(
            "Connected to {} (robot {})",
            params.url, params.robot_account_id
        ),
        None,
        Some(instance_id),
    );
    emit_connection_changed(app);
    Ok(())
}

async fn persist_robot_binding(
    state: &AppState,
    instance_id: &str,
    robot_binding: &RobotBindingMetadata,
) -> Result<(), ManagerError> {
    let previous = state
        .config
        .get_computer_instance(instance_id)
        .map_err(|error| ManagerError::InvalidResponse(error.to_string()))?;
    let updated = state
        .config
        .update_computer_instance(instance_id, |instance| {
            instance.robot_binding = Some(robot_binding.clone());
            instance.connection_policy.target = Some(ComputerConnectionTarget {
                target_type: ComputerConnectionTargetType::ManagerRobot,
                id: robot_binding.employee_id.to_string(),
                robot_account_id: robot_binding.robot_account_id,
            });
        })
        .map_err(|error| ManagerError::InvalidResponse(error.to_string()))?;
    apply_updated_computer_instance(state, previous, updated)
        .await
        .map_err(ManagerError::InvalidResponse)?;
    Ok(())
}

fn manager_connection_decision(
    target_instance_id: &str,
    connected_instance_id: &str,
    connected_instance_name: &str,
    connected_target_id: Option<&str>,
    target_id: &str,
    connected_office_id: &str,
    target_office_id: &str,
) -> Result<ManagerConnectionDecision, ManagerError> {
    if connected_instance_id == target_instance_id {
        if connected_target_id == Some(target_id) {
            return Ok(ManagerConnectionDecision::AlreadyConnected);
        }
        return Err(ManagerError::InvalidResponse(
            "Computer instance is already connected to another Robot; disconnect before switching Robot"
                .to_string(),
        ));
    }

    if connected_target_id == Some(target_id) || connected_office_id == target_office_id {
        return Err(ManagerError::InvalidResponse(format!(
            "Robot is already connected by Computer instance {connected_instance_name}"
        )));
    }

    Ok(ManagerConnectionDecision::Proceed)
}

/// 后台预刷新重连任务：`expires_in - 60s` 重新 exchange → SDK 重连 → 刷新业务快照。
///
/// SMCP 长连接 token 不能热刷新（握手时绑定一次），只能 teardown+reconnect。任务整段生命周期由
/// [`ComputerInstanceRuntime`] 持有，连接被关闭/替换时 abort。换连接前用 `generation` 确认「仍是我
/// 这条连接」，避免与用户期间手动断开/改连竞态时误覆盖。
#[allow(clippy::too_many_arguments)]
fn spawn_refresh_task(
    app: &AppHandle,
    state: &AppState,
    connection: Arc<RwLock<Option<ConnectionState>>>,
    runtime: ComputerInstanceRuntime,
    params: ManagerConnectionParams,
    instance_id: String,
    generation: u64,
    initial_expires_in: i64,
) -> tokio::task::JoinHandle<()> {
    let manager_client = state.manager_client.clone();
    let log_service = state.log_service.clone();
    let app = app.clone();

    tokio::spawn(async move {
        let mut expires_in = initial_expires_in;
        loop {
            // #1: 把过短/退化 TTL（含 server 漏发=0）夹到安全下限，避免 wait≈1s 的重连风暴。
            if expires_in <= TOKEN_PREREFRESH_LEAD_SECS {
                log::warn!(
                    "Token expires_in={expires_in}s <= lead={TOKEN_PREREFRESH_LEAD_SECS}s; \
                     clamping refresh cadence to avoid a reconnect storm"
                );
            }
            let wait = refresh_wait_secs(expires_in);
            tokio::time::sleep(Duration::from_secs(wait)).await;

            match refresh_cycle(&manager_client, &connection, &runtime, &params, generation).await {
                // 成功：emit/log 副作用在此（refresh_cycle 不做副作用，便于测试），按新 TTL 排下次。
                RefreshOutcome::Renewed(new_ttl) => {
                    let _ = log_service.write_for_instance(
                        "info",
                        "connection",
                        "Pre-refreshed SMCP token and reconnected",
                        None,
                        Some(&instance_id),
                    );
                    emit_connection_changed(&app);
                    expires_in = new_ttl;
                }
                // session 失效：通知前端重新登录，停止刷新。
                RefreshOutcome::Unauthorized => {
                    log::warn!("Token pre-refresh: session unauthorized; stopping refresh");
                    let _ = app.emit("manager:auth-expired", ());
                    return;
                }
                // 连接已被替换/断开，或永久错误 → 本任务退场。
                RefreshOutcome::Gone | RefreshOutcome::Stop => return,
                // 暂时性失败（503 / 网络 / build 失败）→ 约 RETRY_SECS 后再试，
                // 不再重睡整个 lead 窗口（修复僵尸窗口 + 错误的重睡间隔）。
                RefreshOutcome::Retry => {
                    expires_in = TOKEN_PREREFRESH_LEAD_SECS + TOKEN_REFRESH_RETRY_SECS as i64
                }
            }
        }
    })
}

/// #1: 距下次预刷新的等待秒数。把过短/退化 TTL 夹到安全下限 `lead+retry`——degenerate（含 0）→ 约
/// `retry` 秒一次，杜绝 `wait≈1s` 的 teardown/reconnect 风暴。
fn refresh_wait_secs(expires_in: i64) -> u64 {
    let floor = TOKEN_PREREFRESH_LEAD_SECS + TOKEN_REFRESH_RETRY_SECS as i64;
    (expires_in.max(floor) - TOKEN_PREREFRESH_LEAD_SECS).max(1) as u64
}

/// 一次预刷新的结果。`pub` 供集成测试匹配 [`reconnect_with_token`] 的返回。
pub enum RefreshOutcome {
    /// 换上新连接，内含新 token 的 `expires_in`（秒）。
    Renewed(i64),
    /// 暂时性失败，短退避后重试；runtime 可能已进入 Error，等待后续重连恢复。
    Retry,
    /// 连接已被换/断，任务退场。
    Gone,
    /// 不可恢复（永久错误），停止刷新。
    Stop,
    /// session 失效——调用方应 emit `manager:auth-expired` 并停止刷新。
    Unauthorized,
}

/// 执行一次预刷新：exchange → [`reconnect_with_token`]。
/// 只做决策、不做 emit/log 副作用（交调用方按 [`RefreshOutcome`] 处理），便于无 AppHandle 环境测试。
async fn refresh_cycle(
    manager_client: &Arc<ManagerClient>,
    connection: &Arc<RwLock<Option<ConnectionState>>>,
    runtime: &ComputerInstanceRuntime,
    params: &ManagerConnectionParams,
    generation: u64,
) -> RefreshOutcome {
    let token = match manager_client
        .exchange_token(&params.robot_account_id.to_string(), params.scope.clone())
        .await
    {
        Ok(t) => t,
        Err(ManagerError::Unauthorized) => return RefreshOutcome::Unauthorized,
        Err(e @ ManagerError::SigningUnavailable { .. })
        | Err(e @ ManagerError::NetworkError(_)) => {
            log::warn!("Token pre-refresh retryable error: {e}");
            return RefreshOutcome::Retry;
        }
        Err(e) => {
            log::error!("Token pre-refresh failed permanently: {e}; stopping refresh");
            return RefreshOutcome::Stop;
        }
    };
    reconnect_with_token(connection, runtime, params, generation, &token).await
}

/// 用已拿到的短 JWT 重建连接：runtime lifecycle lock 内断开旧 SDK Socket.IO → 重连 → 成功刷新快照。
///
/// **不依赖 AppHandle / ManagerClient**（emit/log 留给调用方），便于集成测试 build 失败路径。
/// 顺序 **disconnect-first**：同机器人重连必须先释放 room，否则 server 拒绝重复实例
/// （同 `(office_id, connection.computer_name)`）。build 失败返回 Retry，同时 runtime 进入 Error 状态，
/// 避免业务层把已断开的 socket 误判为健康连接。`generation` 守卫防与用户手动断开/改连竞态。
/// `pub` 供集成测试。
pub async fn reconnect_with_token(
    connection: &Arc<RwLock<Option<ConnectionState>>>,
    runtime: &ComputerInstanceRuntime,
    params: &ManagerConnectionParams,
    generation: u64,
    token: &ExchangedToken,
) -> RefreshOutcome {
    let auth_payload = serde_json::json!({ "token": token.access_token });
    let computer_name = {
        let guard = connection.read().await;
        match guard.as_ref() {
            Some(connection) if connection.generation == generation => {
                connection.computer_name.clone()
            }
            _ => return RefreshOutcome::Gone,
        }
    };
    match runtime
        .reconnect_smcp_socketio_for_generation(
            connection,
            generation,
            &params.url,
            Some(auth_payload),
            params.routing_headers.clone(),
            None,
            &params.office_id,
            &computer_name,
            token.expires_in,
        )
        .await
    {
        Ok(SmcpReconnectOutcome::Reconnected { expires_in }) => RefreshOutcome::Renewed(expires_in),
        Ok(SmcpReconnectOutcome::Stale) => RefreshOutcome::Gone,
        Err(e) => {
            log::warn!("Pre-refresh reconnect failed: {e}");
            RefreshOutcome::Retry
        }
    }
}

/// 仅当代际匹配（仍是同一条逻辑连接）时刷新当前 connection snapshot。
/// 返回 [`SwapResult::Replaced`] 或 [`SwapResult::Stale`]（连接已被换/断）。
/// `pub` 供集成测试验证 generation 守卫。
pub async fn try_install_refreshed_client(
    connection: &Arc<RwLock<Option<ConnectionState>>>,
    generation: u64,
) -> SwapResult {
    let mut guard = connection.write().await;
    match guard.as_mut() {
        Some(cs) if cs.generation == generation => {
            cs.connected_at = chrono::Utc::now();
            SwapResult::Replaced
        }
        _ => SwapResult::Stale,
    }
}

/// 通知前端连接状态变化（前端据此 refetch `get_connection_status`）。
fn emit_connection_changed(app: &AppHandle) {
    let _ = app.emit("connection", ());
}

fn require_instance_id(instance_id: &str) -> Result<&str, String> {
    let instance_id = instance_id.trim();
    if instance_id.is_empty() {
        return Err("instance_id is required".to_string());
    }
    Ok(instance_id)
}

fn manager_target_id(employee_id: u64) -> String {
    format!("manager:{employee_id}")
}

/// Active connection state held in AppState
#[derive(Clone)]
pub struct ConnectionState {
    pub profile_name: String,
    pub url: String,
    pub office_id: String,
    pub computer_name: String,
    pub connected_at: chrono::DateTime<chrono::Utc>,
    pub source_type: String,
    pub target_id: Option<String>,
    pub target_name: Option<String>,
    pub employee_id: Option<u64>,
    /// 代际号；Manager 驱动连接由预刷新任务用它确认连接归属。手动 profile 连接也分配（不复用）。
    pub generation: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_manager_robot_account_accepts_matching_employee() {
        let employees: Vec<DigitalEmployeeBrief> = serde_json::from_value(serde_json::json!([
            { "id": 41, "name": "old", "robotAccountId": 4100 },
            {
                "id": 42,
                "name": "target",
                "robotAccountId": 4200,
                "robotId": "robot-a",
                "namespace": "tf"
            }
        ]))
        .unwrap();

        let employee = validate_manager_robot_account_from_list(&employees, 42, 4200).unwrap();

        assert_eq!(employee.id, 42);
        assert_eq!(employee.robot_account_id, Some(4200));
        assert_eq!(employee.robot_id.as_deref(), Some("robot-a"));
    }

    #[test]
    fn validate_manager_robot_account_rejects_mismatched_account() {
        let employees: Vec<DigitalEmployeeBrief> = serde_json::from_value(serde_json::json!([
            { "id": 42, "name": "target", "robotAccountId": 4200 }
        ]))
        .unwrap();

        let err = validate_manager_robot_account_from_list(&employees, 42, 4300).unwrap_err();

        assert!(
            matches!(err, ManagerError::InvalidResponse(message) if message.contains("robotAccountId mismatch"))
        );
    }

    #[test]
    fn validate_manager_robot_account_rejects_missing_account() {
        let employees: Vec<DigitalEmployeeBrief> = serde_json::from_value(serde_json::json!([
            { "id": 42, "name": "target", "robotAccountId": null }
        ]))
        .unwrap();

        let err = validate_manager_robot_account_from_list(&employees, 42, 4200).unwrap_err();

        assert!(
            matches!(err, ManagerError::InvalidResponse(message) if message.contains("robotAccountId missing"))
        );
    }

    #[test]
    fn manager_connection_decision_is_idempotent_for_same_instance_same_robot() {
        let decision = manager_connection_decision(
            "computer-a",
            "computer-a",
            "Computer A",
            Some("manager:1"),
            "manager:1",
            "robot-1",
            "robot-1",
        )
        .unwrap();

        assert!(matches!(
            decision,
            ManagerConnectionDecision::AlreadyConnected
        ));
    }

    #[test]
    fn manager_connection_decision_rejects_switching_robot_without_disconnect() {
        let err = manager_connection_decision(
            "computer-a",
            "computer-a",
            "Computer A",
            Some("manager:1"),
            "manager:2",
            "robot-1",
            "robot-2",
        )
        .unwrap_err();

        assert!(
            matches!(err, ManagerError::InvalidResponse(message) if message.contains("disconnect before switching Robot"))
        );
    }

    #[test]
    fn manager_connection_decision_rejects_same_instance_same_office_different_target() {
        let err = manager_connection_decision(
            "computer-a",
            "computer-a",
            "Computer A",
            Some("manual:old"),
            "manual:new",
            "robot-1",
            "robot-1",
        )
        .unwrap_err();

        assert!(
            matches!(err, ManagerError::InvalidResponse(message) if message.contains("disconnect before switching Robot"))
        );
    }

    #[test]
    fn manager_connection_decision_rejects_cross_instance_same_robot() {
        let err = manager_connection_decision(
            "computer-a",
            "computer-b",
            "Computer B",
            Some("manager:1"),
            "manager:1",
            "robot-1",
            "robot-1",
        )
        .unwrap_err();

        assert!(
            matches!(err, ManagerError::InvalidResponse(message) if message.contains("Computer B"))
        );
    }
}
