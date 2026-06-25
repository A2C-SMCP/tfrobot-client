use crate::services::manager_client::{ExchangedToken, ManagerClient, ManagerError};
use crate::AppState;
use serde::{Deserialize, Serialize};
use smcp_computer::mcp_clients::model::MCPServerInput;
use smcp_computer::mcp_clients::MCPServerManager;
use smcp_computer::socketio_client::SmcpComputerClient;
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
/// [`reconnect_with_token`] 验证 build-失败-回滚路径。
#[derive(Clone)]
pub struct ManagerConnectionParams {
    /// Socket.IO 服务端 URL（connection-info.socketBaseURL）。
    pub url: String,
    /// Computer 名（Manager 下发）。
    pub computer_name: String,
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
}

/// 预刷新换连接的结果：要么换上了新连接（持有需关闭的旧 client），要么连接已被换/断
/// （持有需丢弃的新 client）。用枚举让 `new_client` 在两个分支都被移动，绕开条件移动借用错误。
/// `pub` 供集成测试验证 generation 守卫。
pub enum SwapResult {
    /// 成功原地替换；内含需关闭的旧 client。
    Replaced(SmcpComputerClient),
    /// 连接已不属于本代（用户断开 / 改连别的机器人）；内含需丢弃的新 client。
    Stale(SmcpComputerClient),
}

/// Connection Profile stored to disk (API Key stored separately in Keychain)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionProfile {
    pub name: String,
    pub url: String,
    #[serde(default = "default_namespace")]
    pub namespace: String,
    pub office_id: String,
    pub computer_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_ref: Option<String>,
    #[serde(default)]
    pub headers: std::collections::HashMap<String, String>,
    #[serde(default = "default_true")]
    pub auto_connect: bool,
    #[serde(default = "default_true")]
    pub auto_reconnect: bool,
}

fn default_true() -> bool {
    true
}

fn default_namespace() -> String {
    "/smcp".to_string()
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
}

/// List all saved connection profiles
#[tauri::command]
pub async fn list_profiles(state: State<'_, AppState>) -> Result<Vec<ConnectionProfile>, String> {
    state.config.load_profiles().map_err(|e| e.to_string())
}

/// Save (create or update) a connection profile
#[tauri::command]
pub async fn save_profile(
    state: State<'_, AppState>,
    profile: ConnectionProfile,
    api_key: Option<String>,
) -> Result<(), String> {
    let name = profile.name.clone();
    log::info!("Saving connection profile: {}", name);

    // Store API key in keychain if provided
    if let Some(key) = &api_key {
        if !key.is_empty() {
            let keychain_id = format!("profile:{}", name);
            crate::services::keychain::save_credential(&keychain_id, key)
                .map_err(|e| e.to_string())?;
        }
    }

    let mut profiles = state.config.load_profiles().map_err(|e| e.to_string())?;
    profiles.retain(|p| p.name != name);
    profiles.push(profile);
    state
        .config
        .save_profiles(&profiles)
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Delete a connection profile
#[tauri::command]
pub async fn delete_profile(state: State<'_, AppState>, name: String) -> Result<(), String> {
    log::info!("Deleting connection profile: {}", name);

    let keychain_id = format!("profile:{}", name);
    let _ = crate::services::keychain::delete_credential(&keychain_id);

    let mut profiles = state.config.load_profiles().map_err(|e| e.to_string())?;
    profiles.retain(|p| p.name != name);
    state
        .config
        .save_profiles(&profiles)
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Connect to SMCP server using a saved profile
#[tauri::command]
pub async fn connect_smcp(state: State<'_, AppState>, profile_name: String) -> Result<(), String> {
    connect_smcp_core(&state, profile_name).await
}

pub async fn connect_smcp_core(state: &AppState, profile_name: String) -> Result<(), String> {
    log::info!("Connecting with profile: {}", profile_name);

    let profiles = state.config.load_profiles().map_err(|e| e.to_string())?;
    let profile = profiles
        .iter()
        .find(|p| p.name == profile_name)
        .ok_or_else(|| format!("Profile not found: {}", profile_name))?
        .clone();

    // Retrieve API key from keychain
    let keychain_id = format!("profile:{}", profile.name);
    let api_key =
        crate::services::keychain::get_credential(&keychain_id).map_err(|e| e.to_string())?;

    // Share the same manager Arc with SmcpComputerClient
    let manager = state.manager.clone();
    let inputs = state.inputs.clone();

    // smcp-computer #86：连接面鉴权唯一走 Socket.IO auth dict（字段名 `token`），HTTP header 退役鉴权、
    // 仅承载路由（profile.headers，如 X-TF-*）。profile 中保存的密钥包成 `{"token": <secret>}` 注入 auth dict。
    // Connection auth lives solely in the Socket.IO auth dict (`token`); HTTP headers are routing-only.
    let auth_payload = api_key
        .filter(|k| !k.is_empty())
        .map(|tok| serde_json::json!({ "token": tok }));

    let client = smcp_computer::socketio_client::SmcpComputerClient::new(
        &profile.url,
        manager,
        profile.computer_name.clone(),
        auth_payload,
        inputs,
        Some(profile.headers.clone()),
    )
    .await
    .map_err(|e| e.to_string())?;

    if let Err(e) = client.join_office(&profile.office_id).await {
        disconnect_smcp_client(client, "after join_office failure").await;
        return Err(e.to_string());
    }

    let new_connection = ConnectionState {
        client,
        profile_name: profile.name.clone(),
        url: profile.url.clone(),
        office_id: profile.office_id.clone(),
        computer_name: profile.computer_name.clone(),
        connected_at: chrono::Utc::now(),
        generation: next_generation(),
        // 手动 profile 连接用静态密钥、不做 token-exchange，故无预刷新任务。
        refresh_task: None,
    };

    let previous_connection = {
        let mut conn = state.connection.write().await;
        conn.replace(new_connection)
    };
    if let Some(connection) = previous_connection {
        close_smcp_connection(connection).await;
    }

    log::info!("Connected to SMCP server: {}", profile.url);
    let _ = state.log_service.write(
        "info",
        "connection",
        &format!("Connected to {}", profile.url),
        None,
    );
    Ok(())
}

/// Disconnect from SMCP server
#[tauri::command]
pub async fn disconnect_smcp(state: State<'_, AppState>) -> Result<(), String> {
    disconnect_smcp_core(&state).await
}

async fn disconnect_smcp_core(state: &AppState) -> Result<(), String> {
    log::info!("Disconnecting from SMCP server");

    let existing_connection = {
        let mut conn = state.connection.write().await;
        conn.take()
    };
    if let Some(connection) = existing_connection {
        close_smcp_connection(connection).await;
    }

    let _ = state
        .log_service
        .write("info", "connection", "Disconnected from SMCP server", None);
    Ok(())
}

pub async fn close_smcp_connection(connection: ConnectionState) {
    let ConnectionState {
        client,
        office_id,
        refresh_task,
        ..
    } = connection;

    // 先停掉预刷新任务，避免它在我们关闭连接的同时又去重连。
    if let Some(task) = refresh_task {
        task.abort();
    }

    match timeout(
        SMCP_CONNECTION_CLOSE_TIMEOUT,
        client.leave_office(&office_id),
    )
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(e)) => log::warn!("Error leaving office: {}", e),
        Err(_) => log::warn!(
            "Timed out leaving SMCP office after {:?}",
            SMCP_CONNECTION_CLOSE_TIMEOUT
        ),
    }

    disconnect_smcp_client(client, "from SMCP server").await;
}

async fn disconnect_smcp_client(
    client: smcp_computer::socketio_client::SmcpComputerClient,
    context: &str,
) {
    match timeout(SMCP_CONNECTION_CLOSE_TIMEOUT, client.disconnect()).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => log::warn!("Error disconnecting {}: {}", context, e),
        Err(_) => log::warn!(
            "Timed out disconnecting {} after {:?}",
            context,
            SMCP_CONNECTION_CLOSE_TIMEOUT,
        ),
    }
}

/// Get current connection status
#[tauri::command]
pub async fn get_connection_status(
    state: State<'_, AppState>,
) -> Result<ConnectionStatusInfo, String> {
    let conn = state.connection.read().await;
    match conn.as_ref() {
        Some(c) => Ok(ConnectionStatusInfo {
            connected: true,
            url: Some(c.url.clone()),
            office_id: Some(c.office_id.clone()),
            computer_name: Some(c.computer_name.clone()),
            connected_at: Some(c.connected_at.to_rfc3339()),
            profile_name: Some(c.profile_name.clone()),
        }),
        None => Ok(ConnectionStatusInfo {
            connected: false,
            url: None,
            office_id: None,
            computer_name: None,
            connected_at: None,
            profile_name: None,
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
pub async fn manager_connect_smcp(
    app: AppHandle,
    state: State<'_, AppState>,
    employee_id: u64,
    robot_account_id: u64,
    scope: Option<String>,
) -> Result<(), ManagerError> {
    log::info!(
        "manager_connect_smcp: employee_id={employee_id} robot_account_id={robot_account_id}"
    );

    // 1) 握手参数
    let info = state.manager_client.get_connection_info(employee_id).await?;
    let url = info.socket_base_url.clone();
    if url.trim().is_empty() {
        return Err(ManagerError::InvalidResponse(
            "connection-info missing socketBaseURL".into(),
        ));
    }
    let office_id = info.rid.clone().filter(|s| !s.is_empty()).ok_or_else(|| {
        ManagerError::InvalidResponse("connection-info missing rid (office_id)".into())
    })?;
    let computer_name = info
        .computer_name
        .clone()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ManagerError::InvalidResponse("connection-info missing computerName".into())
        })?;

    let params = ManagerConnectionParams {
        url,
        computer_name,
        office_id,
        // routingHeaders 为纯路由头（X-TF-*），verbatim 注入 HTTP header。连接面鉴权唯一走
        // Socket.IO auth dict（字段 `token`，#86），凭据不进网关可读的 header（TFRC-20）。
        routing_headers: info.routing_headers.clone(),
        employee_id,
        robot_account_id,
        scope,
    };

    // 2) 换短 JWT
    let token = state
        .manager_client
        .exchange_token(&robot_account_id.to_string(), params.scope.clone())
        .await?;

    // 3) 连接 + 入库 + 起预刷新任务
    establish_manager_connection(&app, state.inner(), params, token).await
}

/// 用给定参数 + 短 JWT 构建 [`SmcpComputerClient`] 并 join_office。失败时尽力清理已建客户端。
async fn build_and_join(
    manager: &Arc<RwLock<Option<MCPServerManager>>>,
    inputs: &Arc<RwLock<HashMap<String, MCPServerInput>>>,
    params: &ManagerConnectionParams,
    jwt: &str,
) -> Result<SmcpComputerClient, String> {
    // 连接面鉴权唯一走 Socket.IO auth dict（字段名 `token`，smcp-computer #86）。
    let auth_payload = serde_json::json!({ "token": jwt });

    let client = SmcpComputerClient::new(
        &params.url,
        manager.clone(),
        params.computer_name.clone(),
        Some(auth_payload),
        inputs.clone(),
        Some(params.routing_headers.clone()),
    )
    .await
    .map_err(|e| e.to_string())?;

    if let Err(e) = client.join_office(&params.office_id).await {
        disconnect_smcp_client(client, "after manager join_office failure").await;
        return Err(e.to_string());
    }
    Ok(client)
}

/// 首连：构建连接、替换 AppState、起预刷新任务、emit 状态变更事件。
async fn establish_manager_connection(
    app: &AppHandle,
    state: &AppState,
    params: ManagerConnectionParams,
    token: ExchangedToken,
) -> Result<(), ManagerError> {
    // 先把新连接建成功，再替换/关旧——build 失败则保留旧连接（与手动 profile 切换同一不变量）。
    // 用户发起的连接通常切到「不同机器人」（office 不同），不会撞 room；同机器人重连由 UI 阻止
    // （已连接时显示「断开」），其 token 预刷新走 spawn_refresh_task 的 leave-first 路径。
    let client = build_and_join(&state.manager, &state.inputs, &params, &token.access_token)
        .await
        .map_err(|e| ManagerError::NetworkError(format!("SMCP connect failed: {e}")))?;

    let generation = next_generation();
    let refresh_task = spawn_refresh_task(app, state, params.clone(), generation, token.expires_in);

    let new_connection = ConnectionState {
        client,
        profile_name: format!("manager:{}", params.employee_id),
        url: params.url.clone(),
        office_id: params.office_id.clone(),
        computer_name: params.computer_name.clone(),
        connected_at: chrono::Utc::now(),
        generation,
        refresh_task: Some(refresh_task),
    };

    let previous = {
        let mut conn = state.connection.write().await;
        conn.replace(new_connection)
    };
    if let Some(previous) = previous {
        close_smcp_connection(previous).await;
    }

    let _ = state.log_service.write(
        "info",
        "connection",
        &format!(
            "Connected to {} (robot {})",
            params.url, params.robot_account_id
        ),
        None,
    );
    emit_connection_changed(app);
    Ok(())
}

/// 后台预刷新重连任务：`expires_in - 60s` 重新 exchange → 原地换新连接 → 关旧连接。
///
/// SMCP 长连接 token 不能热刷新（握手时绑定一次），只能 teardown+reconnect。任务整段生命周期由
/// [`ConnectionState::refresh_task`] 持有，连接被关闭/替换时 abort。换连接前用 `generation` 确认
/// 「仍是我这条连接」，避免与用户期间手动断开/改连竞态时误覆盖。
fn spawn_refresh_task(
    app: &AppHandle,
    state: &AppState,
    params: ManagerConnectionParams,
    generation: u64,
    initial_expires_in: i64,
) -> tokio::task::JoinHandle<()> {
    let manager_client = state.manager_client.clone();
    let connection = state.connection.clone();
    let manager = state.manager.clone();
    let inputs = state.inputs.clone();
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

            match refresh_cycle(&manager_client, &connection, &manager, &inputs, &params, generation)
                .await
            {
                // 成功：emit/log 副作用在此（refresh_cycle 不做副作用，便于测试），按新 TTL 排下次。
                RefreshOutcome::Renewed(new_ttl) => {
                    let _ = log_service.write(
                        "info",
                        "connection",
                        "Pre-refreshed SMCP token and reconnected",
                        None,
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
                // 暂时性失败（503 / 网络 / build 失败已回滚旧连接）→ 约 RETRY_SECS 后再试，
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
    /// 暂时性失败，短退避后重试（旧连接仍在 / 已回滚）。
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
    manager: &Arc<RwLock<Option<MCPServerManager>>>,
    inputs: &Arc<RwLock<HashMap<String, MCPServerInput>>>,
    params: &ManagerConnectionParams,
    generation: u64,
) -> RefreshOutcome {
    let token = match manager_client
        .exchange_token(&params.robot_account_id.to_string(), params.scope.clone())
        .await
    {
        Ok(t) => t,
        Err(ManagerError::Unauthorized) => return RefreshOutcome::Unauthorized,
        Err(e @ ManagerError::SigningUnavailable { .. }) | Err(e @ ManagerError::NetworkError(_)) => {
            log::warn!("Token pre-refresh retryable error: {e}");
            return RefreshOutcome::Retry;
        }
        Err(e) => {
            log::error!("Token pre-refresh failed permanently: {e}; stopping refresh");
            return RefreshOutcome::Stop;
        }
    };
    reconnect_with_token(connection, manager, inputs, params, generation, &token).await
}

/// 用已拿到的短 JWT 重建连接：leave 旧 room → build 新连接 → 成功原地换 / 失败回滚旧连接。
///
/// **不依赖 AppHandle / ManagerClient**（emit/log 留给调用方），便于集成测试 build-失败-回滚路径。
/// 顺序 **leave-first**：同机器人重连必须先让旧连接离开 room，否则 server 拒绝重复实例
/// （同 `(office_id, computer_name)`）。**build 失败则回滚**：把旧连接重新 join_office（旧 token 还
/// 有约 lead 秒有效），避免「已 leave 却仍标记 connected」的僵尸窗口。`generation` 守卫防与用户手动
/// 断开/改连竞态。`pub` 供集成测试。
pub async fn reconnect_with_token(
    connection: &Arc<RwLock<Option<ConnectionState>>>,
    manager: &Arc<RwLock<Option<MCPServerManager>>>,
    inputs: &Arc<RwLock<HashMap<String, MCPServerInput>>>,
    params: &ManagerConnectionParams,
    generation: u64,
    token: &ExchangedToken,
) -> RefreshOutcome {
    // 让旧连接离开 room（&self，read 锁就地调用）。
    // #5 已知有界折衷：leave_office 的 .await 跨持 read 锁，上限 SMCP_CONNECTION_CLOSE_TIMEOUT (5s)；
    // 期间用户主动 disconnect（需 write 锁）会被阻塞至多 5s。彻底消除需把 client 改 Arc 共享，范围外。
    {
        let guard = connection.read().await;
        match guard.as_ref() {
            Some(cs) if cs.generation == generation => {
                let _ = timeout(
                    SMCP_CONNECTION_CLOSE_TIMEOUT,
                    cs.client.leave_office(&params.office_id),
                )
                .await;
            }
            _ => return RefreshOutcome::Gone,
        }
    }

    // build 新连接（room 槽位已释放）。失败 → 回滚：旧连接重新 join_office（旧 token 仍有效约 lead
    // 秒），避免僵尸窗口；随后 Retry。
    let new_client = match build_and_join(manager, inputs, params, &token.access_token).await {
        Ok(c) => c,
        Err(e) => {
            log::warn!("Pre-refresh reconnect failed: {e}; rolling back old connection");
            let guard = connection.read().await;
            if let Some(cs) = guard.as_ref() {
                if cs.generation == generation {
                    let _ = timeout(
                        SMCP_CONNECTION_CLOSE_TIMEOUT,
                        cs.client.join_office(&params.office_id),
                    )
                    .await;
                }
            }
            return RefreshOutcome::Retry;
        }
    };

    // 原地换上新 client —— 仅当仍是我们这一代；旧 client 已 leave_office，直接 disconnect。
    match try_install_refreshed_client(connection, generation, new_client).await {
        SwapResult::Replaced(old) => {
            disconnect_smcp_client(old, "old connection after pre-refresh").await;
            RefreshOutcome::Renewed(token.expires_in)
        }
        SwapResult::Stale(new) => {
            disconnect_smcp_client(new, "discarded pre-refresh (connection replaced)").await;
            RefreshOutcome::Gone
        }
    }
}

/// 把预刷新得到的新 client **原地**装入当前连接——仅当代际匹配（仍是同一条逻辑连接）。
/// 返回 [`SwapResult::Replaced`]（内含需关闭的旧 client）或 [`SwapResult::Stale`]（连接已被换/断，
/// 内含需丢弃的新 client）。`pub` 供集成测试验证 generation 守卫。
pub async fn try_install_refreshed_client(
    connection: &Arc<RwLock<Option<ConnectionState>>>,
    generation: u64,
    new_client: SmcpComputerClient,
) -> SwapResult {
    let mut guard = connection.write().await;
    match guard.as_mut() {
        Some(cs) if cs.generation == generation => {
            cs.connected_at = chrono::Utc::now();
            SwapResult::Replaced(std::mem::replace(&mut cs.client, new_client))
        }
        _ => SwapResult::Stale(new_client),
    }
}

/// 通知前端连接状态变化（前端据此 refetch `get_connection_status`）。
fn emit_connection_changed(app: &AppHandle) {
    let _ = app.emit("connection", ());
}

/// Active connection state held in AppState
pub struct ConnectionState {
    pub client: SmcpComputerClient,
    pub profile_name: String,
    pub url: String,
    pub office_id: String,
    pub computer_name: String,
    pub connected_at: chrono::DateTime<chrono::Utc>,
    /// 代际号；Manager 驱动连接由预刷新任务用它确认连接归属。手动 profile 连接也分配（不复用）。
    pub generation: u64,
    /// Manager 驱动连接的预刷新重连后台任务句柄；手动 profile 连接为 `None`。
    /// 连接被关闭/替换时 abort，停止其挂起的 sleep / 重连。
    pub refresh_task: Option<tokio::task::JoinHandle<()>>,
}
