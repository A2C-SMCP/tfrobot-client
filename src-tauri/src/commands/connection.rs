use crate::commands::runtime_sync::apply_updated_computer_instance;
use crate::services::computer::{
    ClientConnectionOperation, ClientConnectionOperationTarget, ClientConnectionOperationToken,
    ClientConnectionStateSnapshot, ClientConnectionStatus, ComputerConnectionPolicy,
    ComputerConnectionTarget, ComputerConnectionTargetType, ComputerInstance,
    ComputerInstanceRuntime, ComputerRuntimeAction, ComputerRuntimeState, RobotBindingMetadata,
    SmcpReconnectOutcome,
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
use tokio::time::{timeout, Duration};

const SMCP_CONNECTION_CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

/// 短 JWT 过期前多少秒触发预刷新重连（TFRC-11：`expires_at - 60s`）。
const TOKEN_PREREFRESH_LEAD_SECS: i64 = 60;
/// 预刷新失败（503 签名未就位 / 网络抖动）时的重试间隔；小于 lead，保证过期前多次重试。
const TOKEN_REFRESH_RETRY_SECS: u64 = 10;
/// A reconnect that has torn down the old socket must not leave the UI in an unbounded
/// transitional state. Three retries give transient outages 10s/20s/40s windows to recover.
const TOKEN_REFRESH_MAX_RETRIES: u32 = 3;
const TOKEN_REFRESH_MAX_RETRY_SECS: u64 = 40;

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
    pub robot_account_id: String,
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

#[derive(Debug, Clone)]
struct ConnectionProfileSnapshot {
    name: String,
    connection_policy: ComputerConnectionPolicy,
    robot_binding: Option<RobotBindingMetadata>,
}

impl ConnectionProfileSnapshot {
    fn capture(instance: &ComputerInstance) -> Self {
        Self {
            name: instance.name.clone(),
            connection_policy: instance.connection_policy.clone(),
            robot_binding: instance.robot_binding.clone(),
        }
    }

    fn ensure_unchanged(&self, current: &ComputerInstance) -> Result<(), String> {
        if current.name == self.name
            && current.connection_policy == self.connection_policy
            && current.robot_binding == self.robot_binding
        {
            return Ok(());
        }
        Err(format!(
            "Computer profile {} changed while the connection was being established",
            current.id
        ))
    }
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

/// Backward-compatible connection status response.
///
/// Existing consumers keep their historical top-level fields while TFRC-73 consumers use the
/// canonical, versioned `connection_state`.
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
    pub connection_state: ClientConnectionStateSnapshot,
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
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
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
    delete_manual_smcp_target_core(&state, &target_id).await
}

pub async fn delete_manual_smcp_target_core(
    state: &AppState,
    target_id: &str,
) -> Result<(), String> {
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    for runtime in state.computer_registry.list_runtimes().await {
        let connection = runtime.connection_state_snapshot().await;
        if connection
            .as_ref()
            .and_then(|connection| connection.target_id.as_deref())
            == Some(target_id)
        {
            return Err("Cannot delete a manual SMCP target while it is connected".to_string());
        }
    }

    state
        .config
        .delete_manual_smcp_target(target_id)
        .map_err(|e| e.to_string())?;
    delete_manual_smcp_target_api_key(state.secret_store.as_ref(), target_id);
    Ok(())
}

fn delete_manual_smcp_target_api_key(
    secret_store: &dyn crate::services::keychain::SecretStore,
    target_id: &str,
) {
    secret_store.delete_secret_best_effort(&manual_target_keychain_id(target_id));
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
    connect_connection_target_with_policy(state, instance_id, target_id, None).await
}

pub async fn connect_connection_target_for_policy_core(
    state: &AppState,
    instance_id: &str,
    target: &ComputerConnectionTarget,
) -> Result<(), String> {
    connect_connection_target_with_policy(state, instance_id, &target.id, Some(target)).await
}

async fn connect_connection_target_with_policy(
    state: &AppState,
    instance_id: &str,
    target_id: &str,
    required_policy_target: Option<&ComputerConnectionTarget>,
) -> Result<(), String> {
    // The connection mutation is a two-phase transaction. Validate its authoritative inputs and
    // publish the operation token under the Computer lifecycle lock, then release the lock before
    // network I/O. The commit phase reacquires the lock and rejects stale targets, credentials,
    // runtimes, or operation tokens.
    let lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let instance_id = require_instance_id(instance_id)?.to_string();
    let instance = state
        .config
        .get_computer_instance(&instance_id)
        .map_err(|error| error.to_string())?;
    ensure_required_policy_target(&instance, required_policy_target)?;
    let profile_snapshot = ConnectionProfileSnapshot::capture(&instance);
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
        .runtime(&instance_id)
        .await
        .ok_or_else(|| format!("Computer instance not found: {instance_id}"))?;
    if matches!(
        check_connection_target_allowed(state, &instance_id, &target.id, &target.office_id)
            .await
            .map_err(|error| error.to_string())?,
        ManagerConnectionDecision::AlreadyConnected
    ) {
        return Ok(());
    }
    let operation_token = begin_connect_operation_locked(
        &runtime,
        ClientConnectionOperationTarget {
            source_type: SOURCE_MANUAL_SMCP.to_string(),
            target_id: Some(target.id.clone()),
            employee_id: None,
        },
    )
    .await?;
    drop(lifecycle_guard);
    finish_connect_preparation(&runtime, operation_token).await?;

    let connect_result = async {
        let _reservation =
            reserve_connection_target(state, &instance_id, &target.id, &target.office_id)?;
        let result = async {
            if matches!(
                check_connection_target_allowed(state, &instance_id, &target.id, &target.office_id)
                    .await
                    .map_err(|e| e.to_string())?,
                ManagerConnectionDecision::AlreadyConnected
            ) {
                return Ok(());
            }
            runtime.ensure_connection_operation(operation_token).await?;

            let auth_payload = api_key
                .as_deref()
                .filter(|key| !key.is_empty())
                .map(|token| serde_json::json!({ "token": token }));
            let computer_name = runtime.instance.name.clone();
            runtime
                .connect_and_install_smcp_socketio(
                    operation_token,
                    &target.url,
                    auth_payload,
                    target.headers.clone(),
                    Some(target.namespace.clone()),
                    &target.office_id,
                    &computer_name,
                    ConnectionState {
                        profile_name: target.name.clone(),
                        url: target.url.clone(),
                        office_id: target.office_id.clone(),
                        computer_name: computer_name.clone(),
                        connected_at: chrono::Utc::now(),
                        source_type: SOURCE_MANUAL_SMCP.to_string(),
                        target_id: Some(target.id.clone()),
                        target_name: Some(target.name.clone()),
                        employee_id: None,
                        generation: next_generation(),
                    },
                )
                .await?;
            commit_manual_connection_target(
                state,
                &runtime,
                operation_token,
                &instance_id,
                &target,
                api_key.as_deref(),
                &profile_snapshot,
            )
            .await
        }
        .await;
        if result.is_err() {
            let _ = runtime
                .clear_smcp_connection_for_operation(operation_token)
                .await;
        }
        result
    }
    .await;
    if let Err(error) = connect_result {
        runtime
            .fail_connection_operation_for_token(operation_token, error.clone(), true)
            .await;
        return Err(error);
    }
    if !runtime
        .complete_connection_operation_for_token(operation_token)
        .await
    {
        return Err(
            "Connection operation was superseded by a runtime lifecycle change".to_string(),
        );
    }
    let _ = state.log_service.write_for_instance(
        "info",
        "connection",
        &format!("Connected to manual SMCP target {}", target.name),
        None,
        Some(&instance_id),
    );
    Ok(())
}

async fn commit_manual_connection_target(
    state: &AppState,
    runtime: &ComputerInstanceRuntime,
    operation_token: ClientConnectionOperationToken,
    instance_id: &str,
    expected_target: &ManualSmcpTarget,
    expected_api_key: Option<&str>,
    expected_profile: &ConnectionProfileSnapshot,
) -> Result<(), String> {
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    runtime.ensure_connection_operation(operation_token).await?;
    state
        .computer_registry
        .ensure_current_runtime(runtime)
        .await?;
    let current_target = state
        .config
        .get_manual_smcp_target(&expected_target.id)
        .map_err(|error| error.to_string())?;
    if current_target != *expected_target {
        return Err(format!(
            "Manual SMCP target {} changed while the connection was being established",
            expected_target.id
        ));
    }
    let current_api_key = state
        .secret_store
        .get_secret(&manual_target_keychain_id(&expected_target.id))
        .map_err(|error| error.to_string())?;
    if current_api_key.as_deref() != expected_api_key {
        return Err(format!(
            "Manual SMCP target {} credentials changed while the connection was being established",
            expected_target.id
        ));
    }

    let previous = state
        .config
        .get_computer_instance(instance_id)
        .map_err(|error| error.to_string())?;
    expected_profile.ensure_unchanged(&previous)?;
    let updated = state
        .config
        .update_computer_instance(instance_id, |instance| {
            instance.connection_policy.target = Some(ComputerConnectionTarget {
                target_type: ComputerConnectionTargetType::ManualSmcp,
                id: expected_target.id.clone(),
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
        let connection = runtime.connection_state_snapshot().await;
        let Some(connection) = connection.as_ref() else {
            continue;
        };
        if !connection_snapshot_blocks_target(runtime_state, runtime.instance.id == instance_id) {
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

fn connection_snapshot_blocks_target(state: ComputerRuntimeState, same_instance: bool) -> bool {
    matches!(
        state,
        ComputerRuntimeState::Connecting
            | ComputerRuntimeState::Connected
            | ComputerRuntimeState::JoinedOffice
            | ComputerRuntimeState::Syncing
            | ComputerRuntimeState::Degraded
            | ComputerRuntimeState::Disconnecting
    ) || (state == ComputerRuntimeState::Started && !same_instance)
}

async fn clear_unhealthy_connection_snapshot(
    runtime: &ComputerInstanceRuntime,
    token: ClientConnectionOperationToken,
) -> Result<(), String> {
    if runtime.has_smcp_transport().await
        && !runtime.is_connected().await
        && !runtime.clear_smcp_connection_for_operation(token).await?
    {
        return Err(
            "Connection operation was superseded before stale transport cleanup".to_string(),
        );
    }
    Ok(())
}

async fn begin_connect_operation_locked(
    runtime: &ComputerInstanceRuntime,
    operation_target: ClientConnectionOperationTarget,
) -> Result<ClientConnectionOperationToken, String> {
    // A logical connection must still reject a second connect. Without authority, however, a
    // failed disconnect may have left an orphan SDK transport; enter the backend transition first
    // and clean that transport before validating the fresh connect action.
    if runtime.has_connection_state().await {
        runtime
            .ensure_runtime_action(ComputerRuntimeAction::Connect)
            .await
            .map_err(|error| error.to_string())?;
    }
    let token = runtime
        .begin_connection_operation(ClientConnectionOperation::Connect, Some(operation_target))
        .await?;
    Ok(token)
}

async fn finish_connect_preparation(
    runtime: &ComputerInstanceRuntime,
    token: ClientConnectionOperationToken,
) -> Result<(), String> {
    // Transport teardown can wait for the SDK/socket close timeout. Keep it outside the global
    // Computer lifecycle transaction and use the token to reject a superseded cleanup.
    if let Err(error) = clear_unhealthy_connection_snapshot(runtime, token).await {
        runtime
            .fail_connection_operation_for_token(token, error.clone(), true)
            .await;
        return Err(error);
    }
    if let Err(error) = runtime
        .ensure_runtime_action(ComputerRuntimeAction::Connect)
        .await
    {
        let message = error.to_string();
        runtime
            .fail_connection_operation_for_token(token, message.clone(), false)
            .await;
        return Err(message);
    }
    if let Err(error) = runtime.ensure_connection_operation(token).await {
        runtime
            .fail_connection_operation_for_token(token, error.clone(), false)
            .await;
        return Err(error);
    }
    Ok(())
}

fn ensure_required_policy_target(
    instance: &ComputerInstance,
    required_policy_target: Option<&ComputerConnectionTarget>,
) -> Result<(), String> {
    let Some(required) = required_policy_target else {
        return Ok(());
    };
    if instance.connection_policy.target.as_ref() == Some(required) {
        return Ok(());
    }
    Err(format!(
        "Computer connection policy for {} changed before the connection could start",
        instance.id
    ))
}

/// Disconnect from SMCP server
#[tauri::command]
pub async fn disconnect_smcp(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<(), String> {
    disconnect_smcp_core(&state, &instance_id).await
}

pub async fn disconnect_smcp_core(state: &AppState, instance_id: &str) -> Result<(), String> {
    // Publish the operation while the authoritative runtime is protected, release the global
    // lifecycle lock for socket teardown, then reacquire it for token-guarded settlement.
    let lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let instance_id = require_instance_id(instance_id)?.to_string();
    log::info!("Disconnecting instance {} from SMCP server", instance_id);
    let runtime = state
        .computer_registry
        .runtime(&instance_id)
        .await
        .ok_or_else(|| format!("Computer instance not found: {instance_id}"))?;
    state
        .computer_registry
        .ensure_current_runtime(&runtime)
        .await?;

    let current = runtime.connection_snapshot().await;
    if !current.actions.disconnect.enabled {
        return Err(format!(
            "Connection disconnect action is unavailable: {:?}",
            current.actions.disconnect.disabled_reason
        ));
    }
    let operation_token = runtime
        .begin_connection_operation(
            ClientConnectionOperation::Disconnect,
            current
                .context
                .as_ref()
                .map(|context| ClientConnectionOperationTarget {
                    source_type: context.source_type.clone(),
                    target_id: context.target_id.clone(),
                    employee_id: context.employee_id,
                }),
        )
        .await?;
    if let Err(error) = runtime
        .ensure_runtime_action(ComputerRuntimeAction::Disconnect)
        .await
    {
        let message = error.to_string();
        runtime
            .reconcile_disconnect_failure_for_token(operation_token, message.clone())
            .await;
        return Err(message);
    }
    drop(lifecycle_guard);

    if runtime.has_smcp_transport().await {
        if let Err(error) = close_smcp_transport(&runtime).await {
            let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
            if state
                .computer_registry
                .ensure_current_runtime(&runtime)
                .await
                .is_ok()
                && runtime
                    .ensure_connection_operation(operation_token)
                    .await
                    .is_ok()
            {
                runtime
                    .reconcile_disconnect_failure_for_token(operation_token, error.clone())
                    .await;
            }
            return Err(error);
        }
    }
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    state
        .computer_registry
        .ensure_current_runtime(&runtime)
        .await?;
    runtime.ensure_connection_operation(operation_token).await?;
    if !runtime
        .complete_connection_operation_for_token(operation_token)
        .await
    {
        return Err(
            "Disconnect operation was superseded by a runtime lifecycle change".to_string(),
        );
    }

    let _ = state.log_service.write_for_instance(
        "info",
        "connection",
        "Disconnected from SMCP server",
        None,
        Some(&instance_id),
    );
    Ok(())
}

pub async fn close_smcp_connection(
    runtime: &ComputerInstanceRuntime,
    _connection: ConnectionState,
) -> Result<(), String> {
    close_smcp_transport(runtime).await
}

async fn close_smcp_transport(runtime: &ComputerInstanceRuntime) -> Result<(), String> {
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
    let connection_state = runtime.connection_snapshot().await;
    let context = connection_state.context.as_ref();
    Ok(ConnectionStatusInfo {
        connected: connection_state.status == ClientConnectionStatus::Connected,
        url: context.map(|value| value.url.clone()),
        office_id: context.map(|value| value.office_id.clone()),
        computer_name: context.map(|value| value.computer_name.clone()),
        connected_at: context.map(|value| value.connected_at.clone()),
        profile_name: context.map(|value| value.profile_name.clone()),
        source_type: context.map(|value| value.source_type.clone()),
        target_id: context.and_then(|value| value.target_id.clone()),
        target_name: context.and_then(|value| value.target_name.clone()),
        employee_id: context.and_then(|value| value.employee_id),
        connection_state,
    })
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
    robot_account_id: String,
    robot_id: Option<String>,
    robot_name: Option<String>,
    namespace: Option<String>,
    scope: Option<String>,
) -> Result<(), ManagerError> {
    let instance_id = require_instance_id(&instance_id)
        .map_err(ManagerError::InvalidResponse)?
        .to_string();
    let (runtime, operation_token, profile_snapshot) =
        begin_manager_connect(state.inner(), &instance_id, employee_id, None).await?;
    log::info!(
        "manager_connect_smcp: employee_id={employee_id} robot_account_id={robot_account_id}"
    );
    let result = async {
        let employee =
            validate_manager_robot_account(state.inner(), employee_id, &robot_account_id).await?;
        runtime
            .ensure_connection_operation(operation_token)
            .await
            .map_err(ManagerError::InvalidResponse)?;

        // 1) 握手参数
        let info = state
            .manager_client
            .get_connection_info(employee_id)
            .await?;
        runtime
            .ensure_connection_operation(operation_token)
            .await
            .map_err(ManagerError::InvalidResponse)?;
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
            robot_account_id: robot_account_id.clone(),
            scope,
            robot_binding: RobotBindingMetadata {
                employee_id,
                robot_id: robot_id
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| employee.robot_id.clone())
                    .or_else(|| Some(office_id.clone())),
                robot_account_id: Some(robot_account_id.clone()),
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
            .exchange_token(&robot_account_id, params.scope.clone())
            .await?;
        runtime
            .ensure_connection_operation(operation_token)
            .await
            .map_err(ManagerError::InvalidResponse)?;

        // 3) 连接 + 入库 + 起预刷新任务
        establish_manager_connection(
            &app,
            state.inner(),
            &runtime,
            operation_token,
            params,
            token,
            &profile_snapshot,
        )
        .await
    }
    .await;
    finish_manager_connect(runtime, operation_token, result).await
}

pub(crate) async fn connect_manager_robot_target_for_policy(
    app: &AppHandle,
    state: &AppState,
    instance_id: &str,
    employee_id: u64,
    robot_account_id: String,
    required_policy_target: &ComputerConnectionTarget,
) -> Result<(), ManagerError> {
    let (runtime, operation_token, profile_snapshot) = begin_manager_connect(
        state,
        instance_id,
        employee_id,
        Some(required_policy_target),
    )
    .await?;
    let result = async {
        let employee =
            validate_manager_robot_account(state, employee_id, &robot_account_id).await?;
        runtime
            .ensure_connection_operation(operation_token)
            .await
            .map_err(ManagerError::InvalidResponse)?;
        let token = state
            .manager_client
            .exchange_token(&robot_account_id, None)
            .await?;
        runtime
            .ensure_connection_operation(operation_token)
            .await
            .map_err(ManagerError::InvalidResponse)?;
        let info = state
            .manager_client
            .get_connection_info(employee_id)
            .await?;
        runtime
            .ensure_connection_operation(operation_token)
            .await
            .map_err(ManagerError::InvalidResponse)?;
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
            robot_account_id: robot_account_id.clone(),
            scope: None,
            robot_binding: RobotBindingMetadata {
                employee_id,
                robot_id: employee
                    .robot_id
                    .clone()
                    .or_else(|| Some(office_id.clone())),
                robot_account_id: Some(robot_account_id.clone()),
                namespace: employee
                    .namespace
                    .clone()
                    .or_else(|| info.namespace.clone()),
                robot_name: Some(employee.name.clone()),
            },
        };

        establish_manager_connection(
            app,
            state,
            &runtime,
            operation_token,
            params,
            token,
            &profile_snapshot,
        )
        .await
    }
    .await;
    finish_manager_connect(runtime, operation_token, result).await
}

async fn begin_manager_connect(
    state: &AppState,
    instance_id: &str,
    employee_id: u64,
    required_policy_target: Option<&ComputerConnectionTarget>,
) -> Result<
    (
        ComputerInstanceRuntime,
        ClientConnectionOperationToken,
        ConnectionProfileSnapshot,
    ),
    ManagerError,
> {
    let lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    let instance = state
        .config
        .get_computer_instance(instance_id)
        .map_err(|error| ManagerError::InvalidResponse(error.to_string()))?;
    ensure_required_policy_target(&instance, required_policy_target)
        .map_err(ManagerError::InvalidResponse)?;
    let profile_snapshot = ConnectionProfileSnapshot::capture(&instance);
    let runtime = state
        .computer_registry
        .runtime(instance_id)
        .await
        .ok_or_else(|| {
            ManagerError::InvalidResponse(format!("Computer instance not found: {instance_id}"))
        })?;
    state
        .computer_registry
        .ensure_current_runtime(&runtime)
        .await
        .map_err(ManagerError::InvalidResponse)?;
    let operation_token = begin_connect_operation_locked(
        &runtime,
        ClientConnectionOperationTarget {
            source_type: SOURCE_MANAGER_ROBOT.to_string(),
            target_id: Some(manager_target_id(employee_id)),
            employee_id: Some(employee_id),
        },
    )
    .await
    .map_err(ManagerError::InvalidResponse)?;
    drop(lifecycle_guard);
    finish_connect_preparation(&runtime, operation_token)
        .await
        .map_err(ManagerError::InvalidResponse)?;
    Ok((runtime, operation_token, profile_snapshot))
}

async fn finish_manager_connect(
    runtime: ComputerInstanceRuntime,
    operation_token: ClientConnectionOperationToken,
    result: Result<(), ManagerError>,
) -> Result<(), ManagerError> {
    match result {
        Ok(()) => {
            if runtime
                .complete_connection_operation_for_token(operation_token)
                .await
            {
                Ok(())
            } else {
                Err(ManagerError::InvalidResponse(
                    "Connection operation was superseded by a runtime lifecycle change".into(),
                ))
            }
        }
        Err(error) => {
            let retryable = matches!(
                &error,
                ManagerError::SigningUnavailable { .. } | ManagerError::NetworkError(_)
            );
            let message = error.to_string();
            let _ = runtime
                .clear_smcp_connection_for_operation(operation_token)
                .await;
            runtime
                .fail_connection_operation_for_token(operation_token, message, retryable)
                .await;
            Err(error)
        }
    }
}

async fn validate_manager_robot_account(
    state: &AppState,
    employee_id: u64,
    robot_account_id: &str,
) -> Result<DigitalEmployeeBrief, ManagerError> {
    let employees = state.manager_client.list_digital_employees().await?;
    validate_manager_robot_account_from_list(&employees, employee_id, robot_account_id)
}

fn validate_manager_robot_account_from_list(
    employees: &[DigitalEmployeeBrief],
    employee_id: u64,
    robot_account_id: &str,
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
    let actual_robot_account_id = employee.robot_account_id.as_deref().ok_or_else(|| {
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
    operation_token: ClientConnectionOperationToken,
    params: &ManagerConnectionParams,
    jwt: &str,
    generation: u64,
) -> Result<(), String> {
    // 连接面鉴权唯一走 Socket.IO auth dict（字段名 `token`，smcp-computer #86）。
    let auth_payload = serde_json::json!({ "token": jwt });
    runtime
        .connect_and_install_smcp_socketio(
            operation_token,
            &params.url,
            Some(auth_payload),
            params.routing_headers.clone(),
            None,
            &params.office_id,
            &runtime.instance.name,
            ConnectionState {
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
            },
        )
        .await
}

/// 首连：构建连接、替换 AppState、起预刷新任务、emit 状态变更事件。
async fn establish_manager_connection(
    app: &AppHandle,
    state: &AppState,
    runtime: &ComputerInstanceRuntime,
    operation_token: ClientConnectionOperationToken,
    params: ManagerConnectionParams,
    token: ExchangedToken,
    expected_profile: &ConnectionProfileSnapshot,
) -> Result<(), ManagerError> {
    let instance_id = runtime.instance.id.as_str();
    let _reservation = reserve_connection_target(
        state,
        instance_id,
        &manager_target_id(params.employee_id),
        &params.office_id,
    )
    .map_err(ManagerError::InvalidResponse)?;
    let result = async {
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
            commit_robot_binding(
                state,
                runtime,
                operation_token,
                instance_id,
                &params.robot_binding,
                expected_profile,
            )
            .await?;
            return Ok(());
        }
        runtime
            .ensure_connection_operation(operation_token)
            .await
            .map_err(ManagerError::InvalidResponse)?;
        // First connection work runs under the instance lifecycle coordinator. The reservation
        // above prevents only the same Robot/Office identity from being claimed concurrently.
        let generation = next_generation();
        build_and_join(
            runtime,
            operation_token,
            &params,
            &token.access_token,
            generation,
        )
        .await
        .map_err(|e| ManagerError::NetworkError(format!("SMCP connect failed: {e}")))?;

        commit_robot_binding(
            state,
            runtime,
            operation_token,
            instance_id,
            &params.robot_binding,
            expected_profile,
        )
        .await?;
        runtime
            .ensure_connection_operation(operation_token)
            .await
            .map_err(ManagerError::InvalidResponse)?;
        let refresh_task = spawn_refresh_task(
            app,
            state,
            runtime.clone(),
            params.clone(),
            instance_id.to_string(),
            generation,
            token.expires_in,
        );
        runtime
            .set_refresh_task_for_generation(generation, refresh_task)
            .await
            .map_err(ManagerError::InvalidResponse)
    }
    .await;
    if result.is_err() {
        let _ = runtime
            .clear_smcp_connection_for_operation(operation_token)
            .await;
    }
    result?;
    runtime
        .ensure_connection_operation(operation_token)
        .await
        .map_err(ManagerError::InvalidResponse)?;

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
    Ok(())
}

async fn commit_robot_binding(
    state: &AppState,
    runtime: &ComputerInstanceRuntime,
    operation_token: ClientConnectionOperationToken,
    instance_id: &str,
    robot_binding: &RobotBindingMetadata,
    expected_profile: &ConnectionProfileSnapshot,
) -> Result<(), ManagerError> {
    let _lifecycle_guard = state.computer_lifecycle_lock.lock().await;
    runtime
        .ensure_connection_operation(operation_token)
        .await
        .map_err(ManagerError::InvalidResponse)?;
    state
        .computer_registry
        .ensure_current_runtime(runtime)
        .await
        .map_err(ManagerError::InvalidResponse)?;
    let previous = state
        .config
        .get_computer_instance(instance_id)
        .map_err(|error| ManagerError::InvalidResponse(error.to_string()))?;
    expected_profile
        .ensure_unchanged(&previous)
        .map_err(ManagerError::InvalidResponse)?;
    let updated = state
        .config
        .update_computer_instance(instance_id, |instance| {
            instance.robot_binding = Some(robot_binding.clone());
            instance.connection_policy.target = Some(ComputerConnectionTarget {
                target_type: ComputerConnectionTargetType::ManagerRobot,
                id: robot_binding.employee_id.to_string(),
                robot_account_id: robot_binding.robot_account_id.clone(),
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

#[derive(Debug)]
struct ConnectionTargetReservation {
    reservations: Arc<std::sync::Mutex<HashMap<String, String>>>,
    instance_id: String,
    keys: Vec<String>,
}

impl Drop for ConnectionTargetReservation {
    fn drop(&mut self) {
        let Ok(mut reservations) = self.reservations.lock() else {
            log::error!("Connection target reservation map was poisoned during release");
            return;
        };
        for key in &self.keys {
            if reservations.get(key) == Some(&self.instance_id) {
                reservations.remove(key);
            }
        }
    }
}

/// Reserves only the target identities while connection work is in flight. The mutex itself is
/// held for this short map mutation; unrelated Computers then perform HTTP and Socket.IO work
/// concurrently under their instance lifecycle coordinators.
fn reserve_connection_target(
    state: &AppState,
    instance_id: &str,
    target_id: &str,
    office_id: &str,
) -> Result<ConnectionTargetReservation, String> {
    reserve_connection_target_in(
        &state.connection_target_reservations,
        instance_id,
        target_id,
        office_id,
    )
}

fn reserve_connection_target_in(
    reservation_map: &Arc<std::sync::Mutex<HashMap<String, String>>>,
    instance_id: &str,
    target_id: &str,
    office_id: &str,
) -> Result<ConnectionTargetReservation, String> {
    let mut keys = vec![format!("target:{target_id}"), format!("office:{office_id}")];
    keys.sort();
    keys.dedup();
    let mut reservations = reservation_map
        .lock()
        .map_err(|_| "Connection target reservation map is unavailable".to_string())?;
    if let Some(owner) = keys.iter().find_map(|key| reservations.get(key)) {
        return Err(format!(
            "Robot connection is already being established by Computer instance {owner}"
        ));
    }
    for key in &keys {
        reservations.insert(key.clone(), instance_id.to_string());
    }
    drop(reservations);
    Ok(ConnectionTargetReservation {
        reservations: reservation_map.clone(),
        instance_id: instance_id.to_string(),
        keys,
    })
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
        let mut next_wait = refresh_wait_secs(expires_in);
        let mut retry_attempt = 0;
        loop {
            // #1: 把过短/退化 TTL（含 server 漏发=0）夹到安全下限，避免 wait≈1s 的重连风暴。
            if expires_in <= TOKEN_PREREFRESH_LEAD_SECS {
                log::warn!(
                    "Token expires_in={expires_in}s <= lead={TOKEN_PREREFRESH_LEAD_SECS}s; \
                     clamping refresh cadence to avoid a reconnect storm"
                );
            }
            tokio::time::sleep(Duration::from_secs(next_wait)).await;
            if !runtime.begin_reconnect_for_generation(generation).await {
                return;
            }

            match refresh_cycle(&manager_client, &runtime, &params, generation).await {
                // 成功：emit/log 副作用在此（refresh_cycle 不做副作用，便于测试），按新 TTL 排下次。
                RefreshOutcome::Renewed(new_ttl) => {
                    if !runtime.complete_reconnect_for_generation(generation).await {
                        return;
                    }
                    let _ = log_service.write_for_instance(
                        "info",
                        "connection",
                        "Pre-refreshed SMCP token and reconnected",
                        None,
                        Some(&instance_id),
                    );
                    expires_in = new_ttl;
                    next_wait = refresh_wait_secs(expires_in);
                    retry_attempt = 0;
                }
                // session 失效：通知前端重新登录，停止刷新。
                RefreshOutcome::Unauthorized => {
                    log::warn!("Token pre-refresh: session unauthorized; stopping refresh");
                    settle_refresh_terminal(
                        &runtime,
                        generation,
                        RefreshTerminalOutcome::Unauthorized,
                    )
                    .await;
                    let _ = app.emit("manager:auth-expired", ());
                    return;
                }
                // 连接已被替换/断开，或永久错误 → 本任务退场。
                RefreshOutcome::Gone => {
                    settle_refresh_terminal(&runtime, generation, RefreshTerminalOutcome::Gone)
                        .await;
                    return;
                }
                RefreshOutcome::Stop => {
                    settle_refresh_terminal(&runtime, generation, RefreshTerminalOutcome::Stop)
                        .await;
                    return;
                }
                // 暂时性失败（503 / 网络 / build 失败）→ 约 RETRY_SECS 后再试，
                // 不再重睡整个 lead 窗口（修复僵尸窗口 + 错误的重睡间隔）。
                RefreshOutcome::Retry => {
                    if retry_attempt >= TOKEN_REFRESH_MAX_RETRIES {
                        settle_refresh_terminal(
                            &runtime,
                            generation,
                            RefreshTerminalOutcome::Exhausted,
                        )
                        .await;
                        return;
                    }
                    retry_attempt += 1;
                    runtime
                        .record_reconnect_retry(
                            generation,
                            format!(
                                "SMCP reconnect failed; retry {retry_attempt}/{TOKEN_REFRESH_MAX_RETRIES}"
                            ),
                        )
                        .await;
                    next_wait = refresh_retry_delay_secs(retry_attempt, generation);
                    expires_in = TOKEN_PREREFRESH_LEAD_SECS + next_wait as i64;
                }
            }
        }
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RefreshTerminalOutcome {
    Unauthorized,
    Gone,
    Stop,
    Exhausted,
}

/// One terminal settlement table shared by orchestration and tests. Every exit either clears the
/// reconnect operation for its generation or clears the failed connection authority with an
/// actionable terminal error.
pub(crate) async fn settle_refresh_terminal(
    runtime: &ComputerInstanceRuntime,
    generation: u64,
    outcome: RefreshTerminalOutcome,
) -> bool {
    match outcome {
        RefreshTerminalOutcome::Gone => runtime.abort_reconnect_for_generation(generation).await,
        RefreshTerminalOutcome::Unauthorized => {
            runtime
                .terminate_reconnect_for_generation(
                    generation,
                    "Manager session expired during SMCP token refresh".to_string(),
                    false,
                )
                .await
        }
        RefreshTerminalOutcome::Stop => {
            runtime
                .terminate_reconnect_for_generation(
                    generation,
                    "SMCP token refresh failed permanently".to_string(),
                    false,
                )
                .await
        }
        RefreshTerminalOutcome::Exhausted => {
            runtime
                .terminate_reconnect_for_generation(
                    generation,
                    "SMCP reconnect retry limit exhausted".to_string(),
                    true,
                )
                .await
        }
    }
}

/// #1: 距下次预刷新的等待秒数。把过短/退化 TTL 夹到安全下限 `lead+retry`——degenerate（含 0）→ 约
/// `retry` 秒一次，杜绝 `wait≈1s` 的 teardown/reconnect 风暴。
fn refresh_wait_secs(expires_in: i64) -> u64 {
    let floor = TOKEN_PREREFRESH_LEAD_SECS + TOKEN_REFRESH_RETRY_SECS as i64;
    (expires_in.max(floor) - TOKEN_PREREFRESH_LEAD_SECS).max(1) as u64
}

fn refresh_retry_delay_secs(attempt: u32, generation: u64) -> u64 {
    let exponential = TOKEN_REFRESH_RETRY_SECS
        .saturating_mul(1_u64 << attempt.saturating_sub(1))
        .min(TOKEN_REFRESH_MAX_RETRY_SECS);
    // Stable ±20% jitter prevents clients with identical token TTLs from retrying in lockstep
    // without adding a runtime RNG dependency.
    let jitter_bucket = (generation.wrapping_add(attempt as u64 * 17) % 41) as i64 - 20;
    ((exponential as i64 * (100 + jitter_bucket)) / 100).max(1) as u64
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
    runtime: &ComputerInstanceRuntime,
    params: &ManagerConnectionParams,
    generation: u64,
) -> RefreshOutcome {
    let token = match manager_client
        .exchange_token(&params.robot_account_id, params.scope.clone())
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
    reconnect_with_token(runtime, params, generation, &token).await
}

/// 用已拿到的短 JWT 重建连接：runtime lifecycle lock 内断开旧 SDK Socket.IO → 重连 → 成功刷新快照。
///
/// **不依赖 AppHandle / ManagerClient**（emit/log 留给调用方），便于集成测试 build 失败路径。
/// 顺序 **disconnect-first**：同机器人重连必须先释放 room，否则 server 拒绝重复实例
/// （同 `(office_id, connection.computer_name)`）。build 失败返回 Retry；SDK lifecycle 回到 Started，
/// client runtime diagnostic 则保留失败原因，避免业务层把已断开的 socket 误判为健康连接。
/// `generation` 守卫防与用户手动断开/改连竞态。
/// `pub` 供集成测试。
pub async fn reconnect_with_token(
    runtime: &ComputerInstanceRuntime,
    params: &ManagerConnectionParams,
    generation: u64,
    token: &ExchangedToken,
) -> RefreshOutcome {
    let auth_payload = serde_json::json!({ "token": token.access_token });
    let computer_name = {
        let connection = runtime.connection_state_snapshot().await;
        match connection.as_ref() {
            Some(connection) if connection.generation == generation => {
                connection.computer_name.clone()
            }
            _ => return RefreshOutcome::Gone,
        }
    };
    match runtime
        .reconnect_smcp_socketio_for_generation(
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
    runtime: &ComputerInstanceRuntime,
    generation: u64,
) -> SwapResult {
    if runtime
        .refresh_connection_timestamp_for_generation(generation)
        .await
    {
        SwapResult::Replaced
    } else {
        SwapResult::Stale
    }
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
    use crate::services::keychain::{InMemorySecretStore, SecretStore};

    #[test]
    fn connection_status_serialization_preserves_legacy_fields_and_canonical_state() {
        let response: ConnectionStatusInfo = serde_json::from_value(serde_json::json!({
            "connected": false,
            "url": null,
            "office_id": null,
            "computer_name": null,
            "connected_at": null,
            "profile_name": null,
            "source_type": null,
            "target_id": null,
            "target_name": null,
            "employee_id": null,
            "connection_state": {
                "status": "connecting",
                "present": false,
                "revision": 4,
                "context": null,
                "operation": "connect",
                "last_error": null,
                "actions": {
                    "connect": {
                        "enabled": false,
                        "disabled_reason": "transition_in_progress"
                    },
                    "disconnect": {
                        "enabled": false,
                        "disabled_reason": "transition_in_progress"
                    }
                }
            }
        }))
        .unwrap();

        let value = serde_json::to_value(response).unwrap();
        assert_eq!(value["connected"], false);
        assert!(value.get("url").is_some());
        assert_eq!(value["connection_state"]["status"], "connecting");
        assert_eq!(value["connection_state"]["revision"], 4);
    }

    #[test]
    fn target_reservations_allow_unrelated_computers_and_reject_identity_conflicts() {
        let reservations = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let first =
            reserve_connection_target_in(&reservations, "computer-a", "target-a", "office-a")
                .unwrap();
        let second =
            reserve_connection_target_in(&reservations, "computer-b", "target-b", "office-b")
                .unwrap();

        let target_conflict =
            reserve_connection_target_in(&reservations, "computer-c", "target-a", "office-c")
                .unwrap_err();
        assert!(target_conflict.contains("computer-a"));
        let office_conflict =
            reserve_connection_target_in(&reservations, "computer-c", "target-c", "office-b")
                .unwrap_err();
        assert!(office_conflict.contains("computer-b"));

        drop(first);
        reserve_connection_target_in(&reservations, "computer-c", "target-a", "office-c").unwrap();
        drop(second);
    }

    #[test]
    fn reconnect_retry_backoff_is_bounded_with_deterministic_jitter() {
        let generation = 42;
        let delays = (1..=TOKEN_REFRESH_MAX_RETRIES)
            .map(|attempt| refresh_retry_delay_secs(attempt, generation))
            .collect::<Vec<_>>();

        assert!((8..=12).contains(&delays[0]));
        assert!((16..=24).contains(&delays[1]));
        assert!((32..=48).contains(&delays[2]));
        assert_eq!(
            delays,
            (1..=TOKEN_REFRESH_MAX_RETRIES)
                .map(|attempt| refresh_retry_delay_secs(attempt, generation))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn deleting_manual_target_api_key_cleans_up_target_scoped_secret() {
        let store = InMemorySecretStore::default();
        let key = manual_target_keychain_id("target-a");
        store.set_secret(&key, "api-key").unwrap();

        delete_manual_smcp_target_api_key(&store, "target-a");

        assert_eq!(store.get_secret(&key).unwrap(), None);
    }

    #[test]
    fn validate_manager_robot_account_accepts_matching_employee() {
        let employees: Vec<DigitalEmployeeBrief> = serde_json::from_value(serde_json::json!([
            { "id": 41, "name": "old", "robotAccountId": "org-1:account-41" },
            {
                "id": 42,
                "name": "target",
                "robotAccountId": "org-1:account-42",
                "robotId": "robot-a",
                "namespace": "tf"
            }
        ]))
        .unwrap();

        let employee =
            validate_manager_robot_account_from_list(&employees, 42, "org-1:account-42").unwrap();

        assert_eq!(employee.id, 42);
        assert_eq!(
            employee.robot_account_id.as_deref(),
            Some("org-1:account-42")
        );
        assert_eq!(employee.robot_id.as_deref(), Some("robot-a"));
    }

    #[test]
    fn validate_manager_robot_account_rejects_mismatched_account() {
        let employees: Vec<DigitalEmployeeBrief> = serde_json::from_value(serde_json::json!([
            { "id": 42, "name": "target", "robotAccountId": "org-1:account-42" }
        ]))
        .unwrap();

        let err = validate_manager_robot_account_from_list(&employees, 42, "org-1:account-43")
            .unwrap_err();

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

        let err = validate_manager_robot_account_from_list(&employees, 42, "4200").unwrap_err();

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

    #[test]
    fn failed_refresh_reserves_target_for_other_instances_but_allows_owner_recovery() {
        assert!(connection_snapshot_blocks_target(
            ComputerRuntimeState::Started,
            false
        ));
        assert!(!connection_snapshot_blocks_target(
            ComputerRuntimeState::Started,
            true
        ));
    }
}
