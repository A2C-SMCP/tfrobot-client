use crate::commands::runtime_sync::apply_updated_computer_instance;
use crate::services::computer::{
    ClientConnectionOperation, ClientConnectionOperationTarget, ClientConnectionOperationToken,
    ClientConnectionStateSnapshot, ClientConnectionStatus, ComputerConnectionPolicy,
    ComputerConnectionTarget, ComputerInstance, ComputerInstanceRuntime, ComputerRuntimeAction,
    ComputerRuntimeState, ManagerRobotBindingState, RobotBindingMetadata,
};
use crate::services::config::normalize_manual_smcp_target;
use crate::services::connection_targets::{manual_target_keychain_id, ManualSmcpTarget};
use crate::services::manager_client::{
    ConnectionInfoResponse, DigitalEmployeeBrief, ExchangedToken, ManagerError,
};
use crate::services::manager_context::{ManagerContextCoordinator, ManagerContextKey};
use crate::services::observability::{ActivityEventDraft, ActivityLevel, ActivityOutcome};
use crate::AppState;
use a2c_smcp::smcp_computer::computer::SocketIoAuthProvider;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use tauri::State;
use tokio::time::{timeout, Duration};

const SMCP_CONNECTION_CLOSE_TIMEOUT: Duration = Duration::from_secs(5);
const MANAGER_AUTH_RETRY_INITIAL_DELAY: Duration = Duration::from_millis(250);
const MANAGER_AUTH_RETRY_MAX_DELAY: Duration = Duration::from_secs(5);

type ManagerTokenExchange = Arc<
    dyn Fn() -> Pin<Box<dyn Future<Output = Result<ExchangedToken, ManagerError>> + Send + 'static>>
        + Send
        + Sync,
>;

/// 单调代际号：连接快照与 Manager generation 共同隔离手动断开、改连及身份切换。
static CONNECTION_GENERATION: AtomicU64 = AtomicU64::new(1);

fn next_generation() -> u64 {
    CONNECTION_GENERATION.fetch_add(1, Ordering::Relaxed)
}

/// Manager 驱动连接所需的解析结果。
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

struct ManagerConnectionAuthority {
    manager_generation: u64,
    params: ManagerConnectionParams,
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
    list_manual_smcp_targets_core(&state)
}

pub fn list_manual_smcp_targets_core(state: &AppState) -> Result<Vec<ManualSmcpTarget>, String> {
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
    save_manual_smcp_target_core(&state, target, api_key_action).await
}

pub async fn save_manual_smcp_target_core(
    state: &AppState,
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
    connect_connection_target_for_policy_inner(state, instance_id, target).await
}

pub(crate) async fn connect_connection_target_for_policy_inner(
    state: &AppState,
    instance_id: &str,
    target: &ComputerConnectionTarget,
) -> Result<(), String> {
    let ComputerConnectionTarget::ManualSmcp { id } = target else {
        return Err("Expected a Manual SMCP connection target".to_string());
    };
    connect_connection_target_with_policy(state, instance_id, id, Some(target)).await
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
    let instance_id = require_instance_id(instance_id)?.to_string();
    let operation_guard = state.computer_registry.operation_lease(&instance_id).await;
    let lifecycle_guard = state.computer_lifecycle_lock.lock().await;
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
    drop(operation_guard);
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
                    None,
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
    if let Err(error) = state
        .observability
        .record_activity_async(ActivityEventDraft::computer(
            &instance_id,
            ActivityLevel::Info,
            "connection",
            "smcp_connection",
            "connect_manual",
            ActivityOutcome::Succeeded,
            format!("Connected to manual SMCP target {}", target.name),
        ))
        .await
    {
        log::error!("failed to persist connection activity: {error}");
    }
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
    let _operation_guard = state.computer_registry.operation_lease(instance_id).await;
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
            instance.connection_policy.target = Some(ComputerConnectionTarget::manual_smcp(
                expected_target.id.clone(),
            ));
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
    let instance_id = require_instance_id(instance_id)?.to_string();
    let operation_guard = state.computer_registry.operation_lease(&instance_id).await;
    let lifecycle_guard = state.computer_lifecycle_lock.lock().await;
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
    drop(operation_guard);

    if runtime.has_smcp_transport().await {
        if let Err(error) = close_smcp_transport(&runtime).await {
            let _operation_guard = state.computer_registry.operation_lease(&instance_id).await;
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
    let _operation_guard = state.computer_registry.operation_lease(&instance_id).await;
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

    if let Err(error) = state
        .observability
        .record_activity_async(ActivityEventDraft::computer(
            &instance_id,
            ActivityLevel::Info,
            "connection",
            "smcp_connection",
            "disconnect",
            ActivityOutcome::Succeeded,
            "Disconnected from SMCP server",
        ))
        .await
    {
        log::error!("failed to persist disconnection activity: {error}");
    }
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
    get_connection_status_core(&state, &instance_id).await
}

pub async fn get_connection_status_core(
    state: &AppState,
    instance_id: &str,
) -> Result<ConnectionStatusInfo, String> {
    let instance_id = require_instance_id(instance_id)?;
    let _operation_guard = state.computer_registry.operation_lease(instance_id).await;
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
/// 全路径：`connection-info → exchange_token(robotAccountId, token_profile=session) → 动态
/// Socket.IO auth provider → 连接`。连接面鉴权**唯一**走 auth dict（字段名 `token`）；
/// `routingHeaders` 仅作 HTTP 路由。SDK 每次真实重连前重新调用 provider，不主动拆健康连接。
///
/// 前端只提交 employee 主键。robotAccountId、路由和连接地址必须在捕获的 Manager Context
/// generation 下重新解析，前端缓存与 profile 中的最近快照都不具备授权语义。
#[tauri::command]
pub async fn manager_connect_smcp(
    state: State<'_, AppState>,
    instance_id: String,
    employee_id: u64,
    scope: Option<String>,
) -> Result<(), ManagerError> {
    manager_connect_smcp_core(&state, &instance_id, employee_id, scope).await
}

pub async fn manager_connect_smcp_core(
    state: &AppState,
    instance_id: &str,
    employee_id: u64,
    scope: Option<String>,
) -> Result<(), ManagerError> {
    let instance_id = require_instance_id(instance_id)
        .map_err(ManagerError::InvalidResponse)?
        .to_string();
    let (runtime, operation_token, profile_snapshot) =
        begin_manager_connect(state, &instance_id, employee_id, None).await?;
    let result = async {
        let manager_generation = state
            .manager_context
            .capture_authenticated_generation()
            .await?;
        let context_key = state
            .manager_context
            .context_key_for_generation(manager_generation)
            .await?;
        let params = resolve_manager_connection_params(
            &state.manager_context,
            manager_generation,
            &context_key,
            employee_id,
            scope,
        )
        .await?;
        log::info!(
            "manager_connect_smcp: employee_id={employee_id} resolved_robot_account_id={}",
            params.robot_account_id
        );
        runtime
            .ensure_connection_operation(operation_token)
            .await
            .map_err(ManagerError::InvalidResponse)?;

        // 换短 JWT时只使用同一 generation 下刚解析出的 robotAccountId。
        let token = state
            .manager_context
            .exchange_token_for_generation(
                manager_generation,
                &params.robot_account_id,
                params.scope.clone(),
            )
            .await?;
        runtime
            .ensure_connection_operation(operation_token)
            .await
            .map_err(ManagerError::InvalidResponse)?;
        state
            .manager_context
            .ensure_authenticated_generation(manager_generation)
            .await?;

        // 连接与入库均位于 generation commit boundary 内。
        establish_manager_connection(
            state,
            &runtime,
            operation_token,
            ManagerConnectionAuthority {
                manager_generation,
                params,
            },
            token,
            &profile_snapshot,
        )
        .await
    }
    .await;
    finish_manager_connect(runtime, operation_token, result).await
}

pub(crate) async fn connect_manager_robot_target_for_policy(
    state: &AppState,
    instance_id: &str,
    expected_context_key: &ManagerContextKey,
    employee_id: u64,
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
        let manager_generation = match state
            .manager_context
            .capture_authenticated_generation()
            .await
        {
            Ok(generation) => generation,
            Err(error @ (ManagerError::NoSession | ManagerError::Unauthorized)) => {
                mark_manager_binding_dormant(
                    state,
                    &runtime,
                    operation_token,
                    instance_id,
                    required_policy_target,
                    expected_context_key,
                    employee_id,
                )
                .await?;
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        let current_context_key = state
            .manager_context
            .context_key_for_generation(manager_generation)
            .await?;
        if &current_context_key != expected_context_key {
            mark_manager_binding_dormant(
                state,
                &runtime,
                operation_token,
                instance_id,
                required_policy_target,
                expected_context_key,
                employee_id,
            )
            .await?;
            return Err(ManagerError::ContextChanged);
        }
        let params = resolve_manager_connection_params(
            &state.manager_context,
            manager_generation,
            expected_context_key,
            employee_id,
            None,
        )
        .await?;
        runtime
            .ensure_connection_operation(operation_token)
            .await
            .map_err(ManagerError::InvalidResponse)?;
        let token = state
            .manager_context
            .exchange_token_for_generation(
                manager_generation,
                &params.robot_account_id,
                params.scope.clone(),
            )
            .await?;
        runtime
            .ensure_connection_operation(operation_token)
            .await
            .map_err(ManagerError::InvalidResponse)?;
        state
            .manager_context
            .ensure_authenticated_generation(manager_generation)
            .await?;

        establish_manager_connection(
            state,
            &runtime,
            operation_token,
            ManagerConnectionAuthority {
                manager_generation,
                params,
            },
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
    let operation_guard = state.computer_registry.operation_lease(instance_id).await;
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
    drop(operation_guard);
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

async fn mark_manager_binding_dormant(
    state: &AppState,
    runtime: &ComputerInstanceRuntime,
    operation_token: ClientConnectionOperationToken,
    instance_id: &str,
    required_policy_target: &ComputerConnectionTarget,
    expected_context_key: &ManagerContextKey,
    employee_id: u64,
) -> Result<(), ManagerError> {
    let _operation_guard = state.computer_registry.operation_lease(instance_id).await;
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
    if previous.connection_policy.target.as_ref() != Some(required_policy_target) {
        return Ok(());
    }
    let updated = state
        .config
        .update_computer_instance(instance_id, |instance| {
            make_manager_binding_dormant(
                instance,
                required_policy_target,
                expected_context_key,
                employee_id,
            );
        })
        .map_err(|error| ManagerError::InvalidResponse(error.to_string()))?;
    apply_updated_computer_instance(state, previous, updated)
        .await
        .map_err(ManagerError::InvalidResponse)?;
    Ok(())
}

fn make_manager_binding_dormant(
    instance: &mut ComputerInstance,
    required_policy_target: &ComputerConnectionTarget,
    expected_context_key: &ManagerContextKey,
    employee_id: u64,
) -> bool {
    let last_resolved_robot_account_id = match required_policy_target {
        ComputerConnectionTarget::ManagerRobot {
            context_key,
            employee_id: target_employee_id,
            last_resolved_robot_account_id,
        } if context_key == expected_context_key && *target_employee_id == employee_id => {
            last_resolved_robot_account_id.clone()
        }
        _ => return false,
    };
    if instance.connection_policy.target.as_ref() != Some(required_policy_target) {
        return false;
    }
    let mut binding = instance
        .robot_binding
        .clone()
        .filter(|binding| {
            binding.context_key.as_ref() == Some(expected_context_key)
                && binding.employee_id == employee_id
        })
        .unwrap_or_else(|| RobotBindingMetadata {
            context_key: Some(expected_context_key.clone()),
            state: ManagerRobotBindingState::Dormant,
            employee_id,
            robot_id: None,
            last_resolved_robot_account_id,
            namespace: None,
            robot_name: None,
        });
    binding.state = ManagerRobotBindingState::Dormant;
    instance.robot_binding = Some(binding);
    instance.connection_policy.auto_connect = false;
    true
}

async fn resolve_manager_connection_params(
    manager_context: &ManagerContextCoordinator,
    manager_generation: u64,
    expected_context_key: &ManagerContextKey,
    employee_id: u64,
    scope: Option<String>,
) -> Result<ManagerConnectionParams, ManagerError> {
    let current_context_key = manager_context
        .context_key_for_generation(manager_generation)
        .await?;
    if &current_context_key != expected_context_key {
        return Err(ManagerError::ContextChanged);
    }
    let employees = manager_context
        .list_digital_employees_for_generation(manager_generation)
        .await?;
    let (employee, robot_account_id) =
        resolve_manager_robot_account_from_list(employees, employee_id)?;
    let connection_info = manager_context
        .get_connection_info_for_generation(manager_generation, employee_id)
        .await?;
    manager_context
        .ensure_authenticated_generation(manager_generation)
        .await?;
    manager_connection_params_from_resolved(
        expected_context_key.clone(),
        employee,
        robot_account_id,
        connection_info,
        scope,
    )
}

fn resolve_manager_robot_account_from_list(
    employees: Vec<DigitalEmployeeBrief>,
    employee_id: u64,
) -> Result<(DigitalEmployeeBrief, String), ManagerError> {
    let employee = employees
        .into_iter()
        .find(|item| item.id == employee_id)
        .ok_or_else(|| {
            ManagerError::InvalidResponse(format!(
                "Manager Robot target employee {employee_id} is not visible"
            ))
        })?;
    let robot_account_id = employee.robot_account_id.clone().ok_or_else(|| {
        ManagerError::InvalidResponse(format!(
            "robotAccountId missing for Manager Robot target employee {employee_id}"
        ))
    })?;
    Ok((employee, robot_account_id))
}

fn manager_connection_params_from_resolved(
    context_key: ManagerContextKey,
    employee: DigitalEmployeeBrief,
    robot_account_id: String,
    connection_info: ConnectionInfoResponse,
    scope: Option<String>,
) -> Result<ManagerConnectionParams, ManagerError> {
    let url = connection_info.socket_base_url.trim().to_string();
    if url.is_empty() {
        return Err(ManagerError::InvalidResponse(
            "connection-info missing socketBaseURL".into(),
        ));
    }
    let office_id = connection_info
        .rid
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ManagerError::InvalidResponse("connection-info missing rid (office_id)".into())
        })?;
    Ok(ManagerConnectionParams {
        url,
        office_id: office_id.clone(),
        routing_headers: connection_info.routing_headers,
        employee_id: employee.id,
        robot_account_id: robot_account_id.clone(),
        scope,
        robot_binding: RobotBindingMetadata {
            context_key: Some(context_key),
            state: ManagerRobotBindingState::Active,
            employee_id: employee.id,
            robot_id: employee.robot_id.or(Some(office_id)),
            last_resolved_robot_account_id: Some(robot_account_id),
            namespace: employee.namespace.or(connection_info.namespace),
            robot_name: Some(employee.name),
        },
    })
}

fn manager_socketio_auth_provider(
    manager_context: Arc<ManagerContextCoordinator>,
    manager_generation: u64,
    robot_account_id: String,
    scope: Option<String>,
    initial_access_token: String,
) -> SocketIoAuthProvider {
    let exchange: ManagerTokenExchange = Arc::new(move || {
        let manager_context = manager_context.clone();
        let robot_account_id = robot_account_id.clone();
        let scope = scope.clone();
        Box::pin(async move {
            manager_context
                .exchange_token_for_generation(manager_generation, &robot_account_id, scope)
                .await
        })
    });
    let refresh_provider = retrying_manager_socketio_auth_provider(
        exchange,
        MANAGER_AUTH_RETRY_INITIAL_DELAY,
        MANAGER_AUTH_RETRY_MAX_DELAY,
    );
    seeded_socketio_auth_provider(initial_access_token, refresh_provider)
}

fn retrying_manager_socketio_auth_provider(
    exchange: ManagerTokenExchange,
    initial_delay: Duration,
    max_delay: Duration,
) -> SocketIoAuthProvider {
    Arc::new(move || {
        let exchange = exchange.clone();
        Box::pin(async move {
            let mut retry_delay = initial_delay;
            loop {
                match exchange().await {
                    Ok(token) => return serde_json::json!({ "token": token.access_token }),
                    Err(ManagerError::Unauthorized | ManagerError::NoSession) => {
                        // Manager Context owns auth-expired publication and credential cleanup. Its
                        // registered consumer disconnects Manager-owned runtimes; returning an empty
                        // auth dict ensures no stale credential is replayed meanwhile.
                        log::warn!(
                            "Manager Socket.IO auth stopped because the session is unavailable"
                        );
                        return serde_json::json!({});
                    }
                    Err(ManagerError::ContextChanged) => {
                        log::info!("Manager Socket.IO auth ignored a stale generation");
                        return serde_json::json!({});
                    }
                    Err(
                        ManagerError::SigningUnavailable { .. } | ManagerError::NetworkError(_),
                    ) => {
                        log::warn!(
                            "Manager Socket.IO auth exchange failed transiently; retrying in {} ms",
                            retry_delay.as_millis()
                        );
                        tokio::time::sleep(retry_delay).await;
                        retry_delay = retry_delay.saturating_mul(2).min(max_delay);
                    }
                    Err(_) => {
                        log::error!("Manager Socket.IO auth exchange failed permanently");
                        return serde_json::json!({});
                    }
                }
            }
        })
    })
}

fn seeded_socketio_auth_provider(
    initial_access_token: String,
    refresh_provider: SocketIoAuthProvider,
) -> SocketIoAuthProvider {
    // The seed is consumed by the SDK's initial CONNECT while the Manager generation commit lock
    // is held. Later invocations happen only for SDK-driven network reconnects and perform a fresh
    // generation-bound lookup through foundation-ts' cache/retry policy.
    let initial_access_token = Arc::new(StdMutex::new(Some(initial_access_token)));
    Arc::new(move || {
        let seeded = match initial_access_token.lock() {
            Ok(mut token) => token.take(),
            Err(_) => {
                log::error!("Manager Socket.IO auth seed lock is unavailable");
                None
            }
        };
        if let Some(access_token) = seeded {
            Box::pin(async move { serde_json::json!({ "token": access_token }) })
        } else {
            refresh_provider()
        }
    })
}

/// 用动态 auth provider 通过 SDK Computer 建立 Socket.IO 连接并 join_office。
async fn build_and_join(
    runtime: &ComputerInstanceRuntime,
    operation_token: ClientConnectionOperationToken,
    params: &ManagerConnectionParams,
    auth_provider: SocketIoAuthProvider,
    generation: u64,
) -> Result<(), String> {
    runtime
        .connect_and_install_smcp_socketio(
            operation_token,
            &params.url,
            Some(auth_provider),
            None,
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

/// 首连：构建连接、替换 AppState、emit 状态变更事件。
async fn establish_manager_connection(
    state: &AppState,
    runtime: &ComputerInstanceRuntime,
    operation_token: ClientConnectionOperationToken,
    authority: ManagerConnectionAuthority,
    token: ExchangedToken,
    expected_profile: &ConnectionProfileSnapshot,
) -> Result<(), ManagerError> {
    let ManagerConnectionAuthority {
        manager_generation,
        params,
    } = authority;
    let instance_id = runtime.instance.id.as_str();
    let _reservation = reserve_connection_target(
        state,
        instance_id,
        &manager_target_id(params.employee_id),
        &params.office_id,
    )
    .map_err(ManagerError::InvalidResponse)?;
    let result = async {
        runtime
            .ensure_connection_operation(operation_token)
            .await
            .map_err(ManagerError::InvalidResponse)?;
        let generation = next_generation();
        state
            .manager_context
            .commit_for_authenticated_generation(manager_generation, || async {
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

                // The first provider result is the token exchanged immediately before this
                // transaction. Seeding it avoids re-entering the Manager transaction lock from
                // the SDK callback. Every later reconnect exchanges through the generation-bound
                // Manager Context.
                let auth_provider = manager_socketio_auth_provider(
                    state.manager_context.clone(),
                    manager_generation,
                    params.robot_account_id.clone(),
                    params.scope.clone(),
                    token.access_token.clone(),
                );
                build_and_join(runtime, operation_token, &params, auth_provider, generation)
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
                    .map_err(ManagerError::InvalidResponse)
            })
            .await
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

    if let Err(error) = state
        .observability
        .record_activity_async(ActivityEventDraft::computer(
            instance_id,
            ActivityLevel::Info,
            "connection",
            "smcp_connection",
            "connect_manager",
            ActivityOutcome::Succeeded,
            format!(
                "Connected to {} (robot {})",
                params.url, params.robot_account_id
            ),
        ))
        .await
    {
        log::error!("failed to persist manager connection activity: {error}");
    }
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
    let context_key = robot_binding.context_key.clone().ok_or_else(|| {
        ManagerError::InvalidResponse(
            "active Manager Robot binding is missing its Context key".to_string(),
        )
    })?;
    if robot_binding.state != ManagerRobotBindingState::Active {
        return Err(ManagerError::InvalidResponse(
            "only an active Manager Robot binding can be committed".to_string(),
        ));
    }
    let _operation_guard = state.computer_registry.operation_lease(instance_id).await;
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
            instance.connection_policy.target = Some(ComputerConnectionTarget::manager_robot(
                context_key,
                robot_binding.employee_id,
                robot_binding.last_resolved_robot_account_id.clone(),
            ));
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
    /// Logical connection generation used by lifecycle and ownership guards. Values are never
    /// reused for either Manager-driven or manual profile connections.
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
    fn deleting_manual_target_api_key_cleans_up_target_scoped_secret() {
        let store = InMemorySecretStore::default();
        let key = manual_target_keychain_id("target-a");
        store.set_secret(&key, "api-key").unwrap();

        delete_manual_smcp_target_api_key(&store, "target-a");

        assert_eq!(store.get_secret(&key).unwrap(), None);
    }

    #[test]
    fn manager_robot_resolution_uses_the_latest_visible_account() {
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

        let (employee, robot_account_id) =
            resolve_manager_robot_account_from_list(employees, 42).unwrap();

        assert_eq!(employee.id, 42);
        assert_eq!(
            employee.robot_account_id.as_deref(),
            Some("org-1:account-42")
        );
        assert_eq!(employee.robot_id.as_deref(), Some("robot-a"));
        assert_eq!(robot_account_id, "org-1:account-42");
    }

    #[test]
    fn manager_connection_params_use_fresh_discovery_instead_of_profile_diagnostics() {
        let employees: Vec<DigitalEmployeeBrief> = serde_json::from_value(serde_json::json!([
            {
                "id": 42,
                "name": "renamed-target",
                "robotAccountId": "new-robot-account",
                "robotId": "new-robot-id",
                "namespace": "new-namespace"
            }
        ]))
        .unwrap();
        let (employee, robot_account_id) =
            resolve_manager_robot_account_from_list(employees, 42).unwrap();
        let connection_info: ConnectionInfoResponse = serde_json::from_value(serde_json::json!({
            "socketBaseURL": "https://new-smcp.example.com",
            "rid": "new-office",
            "namespace": "connection-namespace",
            "routingHeaders": { "X-TF-Route": "new-route" }
        }))
        .unwrap();
        let context_key = ManagerContextKey {
            environment: crate::services::manager_environment::ManagerEnvironment::Staging,
            account_id: "account-a".to_string(),
            organization_id: "organization-a".to_string(),
        };

        let params = manager_connection_params_from_resolved(
            context_key.clone(),
            employee,
            robot_account_id,
            connection_info,
            None,
        )
        .unwrap();

        assert_eq!(params.url, "https://new-smcp.example.com");
        assert_eq!(params.office_id, "new-office");
        assert_eq!(params.robot_account_id, "new-robot-account");
        assert_eq!(
            params.routing_headers.get("X-TF-Route").map(String::as_str),
            Some("new-route")
        );
        assert_eq!(params.robot_binding.context_key, Some(context_key));
        assert_eq!(
            params
                .robot_binding
                .last_resolved_robot_account_id
                .as_deref(),
            Some("new-robot-account")
        );
        assert_eq!(
            params.robot_binding.robot_id.as_deref(),
            Some("new-robot-id")
        );
        assert_eq!(
            params.robot_binding.namespace.as_deref(),
            Some("new-namespace")
        );
    }

    #[test]
    fn mismatched_context_makes_the_scoped_binding_dormant_and_disables_auto_connect() {
        let context_key = ManagerContextKey {
            environment: crate::services::manager_environment::ManagerEnvironment::Staging,
            account_id: "account-a".to_string(),
            organization_id: "organization-a".to_string(),
        };
        let target = ComputerConnectionTarget::manager_robot(
            context_key.clone(),
            42,
            Some("diagnostic-only".to_string()),
        );
        let mut instance = ComputerInstance::new("computer-a", "Computer A");
        instance.connection_policy.target = Some(target.clone());
        instance.connection_policy.auto_connect = true;
        instance.robot_binding = Some(RobotBindingMetadata {
            context_key: Some(context_key.clone()),
            state: ManagerRobotBindingState::Active,
            employee_id: 42,
            robot_id: Some("robot-a".to_string()),
            last_resolved_robot_account_id: Some("old-account".to_string()),
            namespace: None,
            robot_name: Some("Robot A".to_string()),
        });

        assert!(make_manager_binding_dormant(
            &mut instance,
            &target,
            &context_key,
            42,
        ));

        assert!(!instance.connection_policy.auto_connect);
        assert_eq!(
            instance.robot_binding.as_ref().map(|binding| binding.state),
            Some(ManagerRobotBindingState::Dormant)
        );
        assert_eq!(
            instance
                .robot_binding
                .as_ref()
                .and_then(|binding| binding.last_resolved_robot_account_id.as_deref()),
            Some("old-account")
        );
    }

    #[test]
    fn manager_robot_resolution_rejects_missing_account() {
        let employees: Vec<DigitalEmployeeBrief> = serde_json::from_value(serde_json::json!([
            { "id": 42, "name": "target", "robotAccountId": null }
        ]))
        .unwrap();

        let err = resolve_manager_robot_account_from_list(employees, 42).unwrap_err();

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
    fn failed_connection_authority_reserves_target_for_others_but_allows_owner_recovery() {
        assert!(connection_snapshot_blocks_target(
            ComputerRuntimeState::Started,
            false
        ));
        assert!(!connection_snapshot_blocks_target(
            ComputerRuntimeState::Started,
            true
        ));
    }

    #[tokio::test]
    async fn manager_auth_provider_consumes_seed_once_then_refreshes_each_connect() {
        let refresh_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let refresh_provider: SocketIoAuthProvider = {
            let refresh_count = refresh_count.clone();
            Arc::new(move || {
                let sequence = refresh_count.fetch_add(1, Ordering::SeqCst) + 1;
                Box::pin(async move { serde_json::json!({ "token": format!("fresh-{sequence}") }) })
            })
        };
        let provider = seeded_socketio_auth_provider("initial".to_string(), refresh_provider);

        assert_eq!(provider().await, serde_json::json!({ "token": "initial" }));
        assert_eq!(provider().await, serde_json::json!({ "token": "fresh-1" }));
        assert_eq!(provider().await, serde_json::json!({ "token": "fresh-2" }));
        assert_eq!(refresh_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn manager_auth_provider_retries_transient_exchange_without_emitting_empty_auth() {
        let exchange_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let exchange: ManagerTokenExchange = {
            let exchange_count = exchange_count.clone();
            Arc::new(move || {
                let attempt = exchange_count.fetch_add(1, Ordering::SeqCst);
                Box::pin(async move {
                    if attempt == 0 {
                        Err(ManagerError::NetworkError(
                            "temporary bridge outage".to_string(),
                        ))
                    } else {
                        Ok(ExchangedToken {
                            access_token: "fresh-after-retry".to_string(),
                            token_type: "Bearer".to_string(),
                            expires_in: 300,
                            scope: None,
                        })
                    }
                })
            })
        };
        let provider = retrying_manager_socketio_auth_provider(
            exchange,
            Duration::from_millis(1),
            Duration::from_millis(2),
        );

        let auth = timeout(Duration::from_secs(1), provider())
            .await
            .expect("transient exchange should recover");

        assert_eq!(auth, serde_json::json!({ "token": "fresh-after-retry" }));
        assert_eq!(exchange_count.load(Ordering::SeqCst), 2);
        assert_ne!(auth, serde_json::json!({}));
    }
}
