//! TFRSManager HTTP 客户端。
//!
//! 封装桌面端与 TFRSManager 的 3 条核心 REST 调用（登录 / 列表 / connection-info），
//! 负责 JWT 生命周期（keychain 持久化 + 401 清理）与错误分类。
//!
//! 本模块不依赖 Tauri 运行时，便于单元测试。上层命令负责在 `ManagerError::Unauthorized`
//! 时向前端 emit 事件。

use std::sync::Arc;

use reqwest::{header, StatusCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::sync::RwLock;

use crate::services::keychain;

/// 环境变量：TFRSManager Base URL。无内置默认值；未配置且命令未显式传入 → `MissingBaseUrl`。
pub const BASE_URL_ENV: &str = "TFRS_MANAGER_BASE_URL";

/// keychain 中 Manager JWT 条目的用户名前缀；实际 key 为 `manager_jwt:{sha256(base_url)[..16]}`。
const KEYCHAIN_KEY_PREFIX: &str = "manager_jwt:";

// ───────────────────────── 错误 ─────────────────────────

/// 面向前端的错误分类。`serde` 采用 `tag + content` 让 UI 可按 `kind` 分支。
#[derive(Debug, Error, Serialize, Clone)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum ManagerError {
    /// 连接不上 Manager（DNS / TCP / TLS 层失败）。
    #[error("Network error: {0}")]
    NetworkError(String),

    /// 401：JWT 过期或无效。客户端侧已自动清理 keychain 中对应条目。
    #[error("Unauthorized: Manager JWT expired or invalid")]
    Unauthorized,

    /// 403：权限不足（Manager 拒绝当前账号访问该资源）。
    #[error("Forbidden: insufficient permissions")]
    Forbidden,

    /// 402：欠费 / 服务熔断。`redirect_url` 指向续费页（若 Manager 提供）。
    #[error("Payment required: {message}")]
    PaymentRequired {
        message: String,
        redirect_url: Option<String>,
    },

    /// 404：资源不存在（机器人已删除 / id 错误）。
    #[error("Not found")]
    NotFound,

    /// 其他 HTTP 非成功状态。
    #[error("HTTP {status}: {body}")]
    Other { status: u16, body: String },

    /// 未登录即调用了需要 session 的命令（list / connection-info / select-account）。
    #[error("No active Manager session; call manager_login first")]
    NoSession,

    /// 登录或 select-account 时既未显式传 base_url，也未设置 TFRS_MANAGER_BASE_URL。
    #[error("TFRS_MANAGER_BASE_URL not configured and no base_url was provided")]
    MissingBaseUrl,

    /// 服务器返回的 body 无法反序列化为预期 DTO。
    #[error("Invalid Manager response: {0}")]
    InvalidResponse(String),

    /// keychain 读写失败（平台相关）。
    #[error("Keychain error: {0}")]
    KeychainError(String),
}

impl From<keychain::KeychainError> for ManagerError {
    fn from(e: keychain::KeychainError) -> Self {
        ManagerError::KeychainError(e.to_string())
    }
}

// ───────────────────────── DTOs ─────────────────────────
//
// 契约对齐：TFRSManager 所有 user-facing 接口统一使用 `{code, message, data}` envelope，
// 分页接口进一步使用扁平 `{total, page, pageSize, items}`。
// 参考：tfrsmanager/docs/local-dev/client-uat-guide.md §5.0。

/// Server 统一响应 envelope。`data` 可能是业务体或 null（错误）。
#[derive(Debug, Deserialize)]
struct ApiEnvelope<T> {
    #[serde(default)]
    #[allow(dead_code)]
    code: i32,
    #[serde(default)]
    #[allow(dead_code)]
    message: String,
    data: T,
}

/// 扁平分页响应（14 接口已整改统一）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListResponse<T> {
    #[serde(default)]
    #[allow(dead_code)]
    total: i64,
    #[serde(default)]
    #[allow(dead_code)]
    page: i64,
    #[serde(default)]
    #[allow(dead_code)]
    page_size: i64,
    #[serde(default = "Vec::new")]
    items: Vec<T>,
}

/// 登录请求体。`POST /auth/login-by-password` —— server 接 `phone` 字段，不是 `username`。
#[derive(Debug, Serialize)]
struct LoginRequestBody<'a> {
    phone: &'a str,
    password: &'a str,
}

/// select-account 请求体。`POST /auth/select-account`。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SelectAccountRequestBody<'a> {
    temp_token: &'a str,
    account_id: u64,
}

/// 登录成功后 Manager 下发的用户/账户信息（扁平 4 字段，**无嵌套 user 对象**）。
/// 结构与 select-account 成功响应一致。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UserInfo {
    pub user_id: u64,
    pub account_id: u64,
    pub account_name: String,
}

/// 单账户登录 / select-account 的成功响应 data 体（含 token，未暴露给前端）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthenticatedPayload {
    token: String,
    #[serde(flatten)]
    user: UserInfo,
}

/// 多账户候选项。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountOption {
    pub account_id: u64,
    pub account_name: String,
    #[serde(default)]
    pub nickname: String,
    #[serde(default)]
    pub organization_id: u64,
    #[serde(default)]
    pub organization_name: String,
    #[serde(default)]
    pub organization_type: String,
}

/// 多账户登录响应 data 体。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MultiAccountPayload {
    #[serde(default)]
    #[allow(dead_code)]
    message: String,
    temp_token: String,
    #[serde(default)]
    #[allow(dead_code)]
    expires_in: i32,
    accounts: Vec<AccountOption>,
}

/// 登录响应的 data 体——两种形态，按内容区分。
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum LoginData {
    SingleAccount(AuthenticatedPayload),
    MultiAccount(MultiAccountPayload),
}

/// 前端可感知的登录结果（JWT 不透出；存 keychain + 内存 session）。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LoginResult {
    /// 登录成功，JWT 已存 keychain。
    Authenticated { user: UserInfo },
    /// 命中多账户，需要前端让用户挑选账号后调 `manager_select_account`。
    AccountSelectionRequired { accounts: Vec<AccountOption> },
}

/// 数字员工列表项（`GET /api/v1/digital-employees`，data.items 元素）。
/// 按真实响应补全业务字段，UI 可展示；未列出的字段由 serde 自动忽略。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DigitalEmployeeBrief {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub robot_id: Option<String>,
    /// `running` / `stopped` / `init_failed` / … 完整状态集见 UAT guide。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_display_name: Option<String>,
    /// `tfrserver` / `tfropenclaw` 等。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cluster_name: Option<String>,
}

/// connection-info 响应 data 体（`GET /api/v1/digital-employees/{id}/connection-info`）。
/// 字段与 TFRM-18 / UAT guide §5.5 对齐；`routingHeaders` 是客户端注入 smcp-computer 的权威契约。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInfoResponse {
    // Manager 侧字段为 `socketBaseURL`（全大写 URL），默认 camelCase 转换不覆盖此形态。
    #[serde(rename = "socketBaseURL")]
    pub socket_base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sio_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub robot_type: Option<String>,
    /// Socket.IO 应用层 namespace（固定 `/smcp`，但 Manager 可能仍回传）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smcp_namespace: Option<String>,
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub computer_name: Option<String>,
    /// **客户端必须整 dict 注入 smcp-computer 的 headers 参数**，不要自组装 header 名。
    /// 唯一的 snake_case 例外：`routingHeaders.access_token`（物化契约，下游 server 按此校验）。
    pub routing_headers: std::collections::HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

/// 402 欠费响应。兼容两种形态用同一个结构：
/// - envelope：`{code, message, data: {message?, redirectUrl?}}` —— `data` 非空
/// - 裸对象：`{message?, redirectUrl?}` —— `data` 缺省
///
/// 解析时优先使用 `data` 内的字段（envelope 语义），回退到顶层字段（裸对象语义）。
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PaymentRequiredData {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    redirect_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PaymentRequiredAny {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    redirect_url: Option<String>,
    #[serde(default)]
    data: Option<PaymentRequiredData>,
}

// ───────────────────────── 内存 session ─────────────────────────

/// 仅在内存中持有、进程退出即丢失的 session 状态。
///
/// JWT 同步落在 keychain；`base_url` 仅在内存（前端每次启动都需重新走登录流程或显式指定）。
#[derive(Debug, Clone)]
struct Session {
    base_url: String,
    jwt: String,
    /// 多账户登录待确认时的临时 session token；完成 `select_account` 后置为 None。
    pending_session_token: Option<String>,
}

// ───────────────────────── 客户端 ─────────────────────────

pub struct ManagerClient {
    http: reqwest::Client,
    session: Arc<RwLock<Option<Session>>>,
}

/// 把 reqwest 错误的 source chain 展平成一行便于前端展示与诊断。
/// reqwest 的 `Display` 只给顶层消息（如 "error sending request for url (...)"），
/// 真实原因（connect refused / connection reset / TLS 错）在 `source()` 链里。
fn flatten_reqwest_err(e: reqwest::Error) -> String {
    let mut parts: Vec<String> = vec![e.to_string()];
    let mut src: Option<&(dyn std::error::Error + 'static)> = std::error::Error::source(&e);
    while let Some(s) = src {
        parts.push(s.to_string());
        src = s.source();
    }
    parts.join(" -> ")
}

impl ManagerClient {
    pub fn new() -> Self {
        let user_agent = format!(
            "tfrobot-client/{} (Tauri; {})",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS
        );
        // Manager 场景下更偏向"每个请求独立连接"，不用连接池：
        // - 避免 Go server idle 关停导致 pool 里的 stale 连接拿出来直接 fail
        // - Manager 调用本身很少（login + list + connection-info），性能影响可忽略
        let http = reqwest::Client::builder()
            .user_agent(user_agent)
            .pool_max_idle_per_host(0)
            .tcp_keepalive(std::time::Duration::from_secs(10))
            .build()
            .expect("reqwest::Client::builder should not fail with rustls + default settings");
        Self {
            http,
            session: Arc::new(RwLock::new(None)),
        }
    }

    /// 仅供测试：注入自定义 reqwest::Client（用于 mock server 断言 User-Agent 等）。
    #[cfg(test)]
    pub fn with_http_client(http: reqwest::Client) -> Self {
        Self {
            http,
            session: Arc::new(RwLock::new(None)),
        }
    }

    // ───────── 工具 ─────────

    fn resolve_base_url(arg: Option<String>) -> Result<String, ManagerError> {
        match arg.filter(|s| !s.trim().is_empty()) {
            Some(u) => Ok(strip_trailing_slash(u)),
            None => match std::env::var(BASE_URL_ENV) {
                Ok(v) if !v.trim().is_empty() => Ok(strip_trailing_slash(v)),
                _ => Err(ManagerError::MissingBaseUrl),
            },
        }
    }

    fn keychain_key(base_url: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(base_url.as_bytes());
        let hex = hex::encode(hasher.finalize());
        format!("{KEYCHAIN_KEY_PREFIX}{}", &hex[..16])
    }

    async fn require_session(&self) -> Result<Session, ManagerError> {
        self.session
            .read()
            .await
            .clone()
            .ok_or(ManagerError::NoSession)
    }

    /// 用当前 session 的 JWT 构造鉴权 header。
    fn bearer(jwt: &str) -> String {
        format!("Bearer {jwt}")
    }

    // ───────── HTTP 错误归一化 ─────────

    async fn classify_error(&self, resp: reqwest::Response) -> ManagerError {
        let status = resp.status();
        match status {
            StatusCode::UNAUTHORIZED => {
                // 401 → 清理 keychain 与内存 session
                self.clear_session_on_auth_failure().await;
                ManagerError::Unauthorized
            }
            StatusCode::FORBIDDEN => ManagerError::Forbidden,
            StatusCode::NOT_FOUND => ManagerError::NotFound,
            StatusCode::PAYMENT_REQUIRED => {
                let body = resp.text().await.unwrap_or_default();
                let (message, redirect_url) = parse_payment_required(&body);
                ManagerError::PaymentRequired { message, redirect_url }
            }
            s => {
                let body = resp.text().await.unwrap_or_default();
                ManagerError::Other {
                    status: s.as_u16(),
                    body,
                }
            }
        }
    }

    async fn clear_session_on_auth_failure(&self) {
        let base_url = {
            let guard = self.session.read().await;
            guard.as_ref().map(|s| s.base_url.clone())
        };
        if let Some(url) = base_url {
            let key = Self::keychain_key(&url);
            let _ = keychain::delete_credential(&key); // 失败不阻塞 401 路径
        }
        *self.session.write().await = None;
    }

    // ───────── 公共 API ─────────

    /// `POST {base}/auth/login-by-password` — 登录。
    ///
    /// server 按 `phone` 字段接收（不是 `username`）。响应 envelope `{code, message, data}`，
    /// data 为两种形态之一：单账户 `{token, userId, accountId, accountName}` 或
    /// 多账户 `{message, tempToken, expiresIn, accounts}`。
    pub async fn login(
        &self,
        base_url: Option<String>,
        phone: &str,
        password: &str,
    ) -> Result<LoginResult, ManagerError> {
        let base = Self::resolve_base_url(base_url)?;
        let url = format!("{base}/auth/login-by-password");

        let resp = self
            .http
            .post(&url)
            .json(&LoginRequestBody { phone, password })
            .send()
            .await
            .map_err(|e| ManagerError::NetworkError(flatten_reqwest_err(e)))?;

        if !resp.status().is_success() {
            return Err(self.classify_error(resp).await);
        }

        let envelope: ApiEnvelope<LoginData> = resp
            .json()
            .await
            .map_err(|e| ManagerError::InvalidResponse(e.to_string()))?;

        match envelope.data {
            LoginData::SingleAccount(payload) => {
                // 写 keychain + 建立内存 session（清空 pending token）
                let key = Self::keychain_key(&base);
                keychain::save_credential(&key, &payload.token)?;
                *self.session.write().await = Some(Session {
                    base_url: base,
                    jwt: payload.token,
                    pending_session_token: None,
                });
                Ok(LoginResult::Authenticated { user: payload.user })
            }
            LoginData::MultiAccount(payload) => {
                // 多账户：记住 base_url + pending token；JWT 尚未产生，不写 keychain
                *self.session.write().await = Some(Session {
                    base_url: base,
                    jwt: String::new(),
                    pending_session_token: Some(payload.temp_token),
                });
                Ok(LoginResult::AccountSelectionRequired {
                    accounts: payload.accounts,
                })
            }
        }
    }

    /// `POST {base}/auth/select-account` — 多账户登录二次确认。
    ///
    /// 必须在 `login()` 返回 `AccountSelectionRequired` 之后调用。
    /// 请求体字段：`{tempToken, accountId}`；响应同单账户登录。
    pub async fn select_account(&self, account_id: u64) -> Result<UserInfo, ManagerError> {
        let session = self.require_session().await?;
        let pending = session
            .pending_session_token
            .clone()
            .ok_or(ManagerError::NoSession)?;
        let url = format!("{}/auth/select-account", session.base_url);

        let resp = self
            .http
            .post(&url)
            .json(&SelectAccountRequestBody {
                temp_token: &pending,
                account_id,
            })
            .send()
            .await
            .map_err(|e| ManagerError::NetworkError(flatten_reqwest_err(e)))?;

        if !resp.status().is_success() {
            return Err(self.classify_error(resp).await);
        }

        let envelope: ApiEnvelope<AuthenticatedPayload> = resp
            .json()
            .await
            .map_err(|e| ManagerError::InvalidResponse(e.to_string()))?;

        let key = Self::keychain_key(&session.base_url);
        keychain::save_credential(&key, &envelope.data.token)?;
        *self.session.write().await = Some(Session {
            base_url: session.base_url,
            jwt: envelope.data.token,
            pending_session_token: None,
        });
        Ok(envelope.data.user)
    }

    /// `GET {base}/api/v1/digital-employees` — 当前账号的数字员工列表。
    ///
    /// 响应走标准分页 envelope：`{code, message, data: {total, page, pageSize, items}}`。
    pub async fn list_digital_employees(&self) -> Result<Vec<DigitalEmployeeBrief>, ManagerError> {
        let session = self.require_session().await?;
        if session.jwt.is_empty() {
            return Err(ManagerError::NoSession);
        }
        let url = format!("{}/api/v1/digital-employees", session.base_url);

        let resp = self
            .http
            .get(&url)
            .header(header::AUTHORIZATION, Self::bearer(&session.jwt))
            .send()
            .await
            .map_err(|e| ManagerError::NetworkError(flatten_reqwest_err(e)))?;

        if !resp.status().is_success() {
            return Err(self.classify_error(resp).await);
        }
        let envelope: ApiEnvelope<ListResponse<DigitalEmployeeBrief>> = resp
            .json()
            .await
            .map_err(|e| ManagerError::InvalidResponse(e.to_string()))?;
        Ok(envelope.data.items)
    }

    /// `GET {base}/api/v1/digital-employees/{id}/connection-info` — 拿 SMCP 握手参数。
    /// `id` 是 Manager 侧的数字主键（DigitalEmployeeBrief.id）。
    pub async fn get_connection_info(
        &self,
        id: u64,
    ) -> Result<ConnectionInfoResponse, ManagerError> {
        let session = self.require_session().await?;
        if session.jwt.is_empty() {
            return Err(ManagerError::NoSession);
        }
        let url = format!(
            "{}/api/v1/digital-employees/{id}/connection-info",
            session.base_url
        );

        let resp = self
            .http
            .get(&url)
            .header(header::AUTHORIZATION, Self::bearer(&session.jwt))
            .send()
            .await
            .map_err(|e| ManagerError::NetworkError(flatten_reqwest_err(e)))?;

        if !resp.status().is_success() {
            return Err(self.classify_error(resp).await);
        }
        let envelope: ApiEnvelope<ConnectionInfoResponse> = resp
            .json()
            .await
            .map_err(|e| ManagerError::InvalidResponse(e.to_string()))?;
        Ok(envelope.data)
    }

    /// 本地登出：清 keychain + 内存 session。无服务端 logout API。
    pub async fn logout(&self) -> Result<(), ManagerError> {
        let base_url = {
            let guard = self.session.read().await;
            guard.as_ref().map(|s| s.base_url.clone())
        };
        if let Some(url) = base_url {
            let key = Self::keychain_key(&url);
            keychain::delete_credential(&key)?;
        }
        *self.session.write().await = None;
        Ok(())
    }

    /// 测试/自检用：当前是否已有 session。
    pub async fn has_session(&self) -> bool {
        self.session.read().await.is_some()
    }
}

impl Default for ManagerClient {
    fn default() -> Self {
        Self::new()
    }
}

fn strip_trailing_slash(url: String) -> String {
    if url.ends_with('/') {
        url.trim_end_matches('/').to_string()
    } else {
        url
    }
}

/// 解析 402 响应体。兼容两种形态：
/// - envelope：`{code, message, data: {message?, redirectUrl?}}`
/// - 裸对象：`{message?, redirectUrl?}`
/// 缺省返回 `("Payment required", None)`。
fn parse_payment_required(body: &str) -> (String, Option<String>) {
    if let Ok(any) = serde_json::from_str::<PaymentRequiredAny>(body) {
        // 两种形态共用一个 struct：data 存在时走 envelope 语义，否则走裸对象语义。
        let (data_msg, data_url) = any
            .data
            .map(|d| (d.message, d.redirect_url))
            .unwrap_or((None, None));
        let msg = data_msg
            .or(any.message)
            .unwrap_or_else(|| "Payment required".to_string());
        let url = data_url.or(any.redirect_url);
        return (msg, url);
    }
    ("Payment required".to_string(), None)
}

// ───────────────────────── 单元测试 ─────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// `resolve_base_url` 依赖进程全局环境变量，cargo 默认并行测试会互相踩踏。
    /// 合并为一个顺序执行的测试，保证 set/unset 之间不被别的用例插入。
    #[test]
    fn resolve_base_url_priority_and_error_cases() {
        // 1) 显式 arg 优先于 env（env 存在也不看）
        std::env::set_var(BASE_URL_ENV, "https://env.example.com/");
        let got =
            ManagerClient::resolve_base_url(Some("https://arg.example.com/".to_string())).unwrap();
        assert_eq!(got, "https://arg.example.com");

        // 2) arg 为 None 时退回 env（同时验证尾斜杠被剥离）
        let got = ManagerClient::resolve_base_url(None).unwrap();
        assert_eq!(got, "https://env.example.com");

        // 3) env 未设置且 arg 为 None → MissingBaseUrl
        std::env::remove_var(BASE_URL_ENV);
        let err = ManagerClient::resolve_base_url(None).unwrap_err();
        assert!(matches!(err, ManagerError::MissingBaseUrl));

        // 4) 纯空白 arg 等同于未提供
        let err = ManagerClient::resolve_base_url(Some("   ".to_string())).unwrap_err();
        assert!(matches!(err, ManagerError::MissingBaseUrl));
    }

    #[test]
    fn keychain_key_is_deterministic_and_prefixed() {
        let k1 = ManagerClient::keychain_key("https://manager.example.com");
        let k2 = ManagerClient::keychain_key("https://manager.example.com");
        assert_eq!(k1, k2);
        assert!(k1.starts_with(KEYCHAIN_KEY_PREFIX));
        assert_eq!(k1.len(), KEYCHAIN_KEY_PREFIX.len() + 16);
    }

    #[test]
    fn keychain_key_differs_per_base_url() {
        let a = ManagerClient::keychain_key("https://a.example.com");
        let b = ManagerClient::keychain_key("https://b.example.com");
        assert_ne!(a, b);
    }

    #[test]
    fn login_data_deserializes_single_account_branch() {
        // Server 返回 envelope 包裹；data 是扁平 4 字段。
        let json = r#"{
            "code": 200,
            "message": "success",
            "data": {
                "token": "jwt-abc",
                "userId": 9,
                "accountId": 16,
                "accountName": "client_uat"
            }
        }"#;
        let env: ApiEnvelope<LoginData> = serde_json::from_str(json).unwrap();
        match env.data {
            LoginData::SingleAccount(payload) => {
                assert_eq!(payload.token, "jwt-abc");
                assert_eq!(payload.user.user_id, 9);
                assert_eq!(payload.user.account_id, 16);
                assert_eq!(payload.user.account_name, "client_uat");
            }
            _ => panic!("expected SingleAccount branch"),
        }
    }

    #[test]
    fn login_data_deserializes_multi_account_branch() {
        let json = r#"{
            "code": 200,
            "message": "success",
            "data": {
                "message": "请选择要登录的账户",
                "tempToken": "temp-xyz",
                "expiresIn": 300,
                "accounts": [
                    {"accountId": 2, "accountName": "testuser2_enterprise", "nickname": "测试用户2",
                     "organizationId": 2, "organizationName": "测试企业", "organizationType": "enterprise"},
                    {"accountId": 3, "accountName": "testuser2_personal", "nickname": "测试用户2",
                     "organizationId": 1, "organizationName": "one-person-org-1", "organizationType": "personal"}
                ]
            }
        }"#;
        let env: ApiEnvelope<LoginData> = serde_json::from_str(json).unwrap();
        match env.data {
            LoginData::MultiAccount(payload) => {
                assert_eq!(payload.temp_token, "temp-xyz");
                assert_eq!(payload.expires_in, 300);
                assert_eq!(payload.accounts.len(), 2);
                assert_eq!(payload.accounts[0].account_id, 2);
                assert_eq!(payload.accounts[0].organization_type, "enterprise");
                assert_eq!(payload.accounts[1].account_name, "testuser2_personal");
            }
            _ => panic!("expected MultiAccount branch"),
        }
    }

    #[test]
    fn list_response_deserializes_paginated_employees() {
        let json = r#"{
            "code": 200,
            "message": "success",
            "data": {
                "total": 1,
                "page": 1,
                "pageSize": 20,
                "items": [
                    {"id": 11, "name": "本地联调员工", "robotId": "5f4b...", "status": "running",
                     "templateType": "tfrserver", "templateDisplayName": "智能客服机器人", "namespace": "tfrobotserver"}
                ]
            }
        }"#;
        let env: ApiEnvelope<ListResponse<DigitalEmployeeBrief>> =
            serde_json::from_str(json).unwrap();
        assert_eq!(env.data.total, 1);
        assert_eq!(env.data.items.len(), 1);
        assert_eq!(env.data.items[0].id, 11);
        assert_eq!(env.data.items[0].robot_id.as_deref(), Some("5f4b..."));
        assert_eq!(env.data.items[0].status.as_deref(), Some("running"));
        assert_eq!(
            env.data.items[0].template_type.as_deref(),
            Some("tfrserver")
        );
    }

    #[test]
    fn list_response_tolerates_empty_items() {
        let json = r#"{
            "code": 200, "message": "success",
            "data": {"total": 0, "page": 1, "pageSize": 20, "items": []}
        }"#;
        let env: ApiEnvelope<ListResponse<DigitalEmployeeBrief>> =
            serde_json::from_str(json).unwrap();
        assert_eq!(env.data.total, 0);
        assert!(env.data.items.is_empty());
    }

    #[test]
    fn parse_payment_required_handles_envelope_and_bare_shapes() {
        // 包在 envelope 里
        let (msg, url) = parse_payment_required(
            r#"{"code":402,"message":"outer","data":{"message":"欠费","redirectUrl":"https://pay/x"}}"#,
        );
        assert_eq!(msg, "欠费");
        assert_eq!(url.as_deref(), Some("https://pay/x"));

        // envelope 但 data 没有 message → fallback 到 outer message
        let (msg, url) = parse_payment_required(
            r#"{"code":402,"message":"outer","data":{"redirectUrl":"https://pay/y"}}"#,
        );
        assert_eq!(msg, "outer");
        assert_eq!(url.as_deref(), Some("https://pay/y"));

        // 裸对象
        let (msg, url) = parse_payment_required(r#"{"message":"欠费","redirectUrl":"https://x"}"#);
        assert_eq!(msg, "欠费");
        assert_eq!(url.as_deref(), Some("https://x"));

        // 完全空
        let (msg, url) = parse_payment_required("");
        assert_eq!(msg, "Payment required");
        assert!(url.is_none());
    }

    #[test]
    fn connection_info_deserializes_full_payload() {
        let json = r#"{
            "socketBaseURL": "https://staging.turingfocus.cn",
            "sioPath": "/socket.io/",
            "namespace": "tenant-acme",
            "rid": "robot-xxx",
            "robotType": "tfrobot",
            "smcpNamespace": "/smcp",
            "accessToken": "admin-secret-plain",
            "computerName": "desktop-001",
            "routingHeaders": {
                "X-TF-Namespace": "tenant-acme",
                "X-TF-RobotId": "robot-xxx",
                "X-TF-RobotType": "tfrobot",
                "access_token": "admin-secret-plain"
            },
            "expiresAt": "2026-04-23T12:00:00Z"
        }"#;
        let parsed: ConnectionInfoResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.socket_base_url, "https://staging.turingfocus.cn");
        assert_eq!(parsed.sio_path.as_deref(), Some("/socket.io/"));
        assert_eq!(parsed.smcp_namespace.as_deref(), Some("/smcp"));
        assert_eq!(parsed.access_token, "admin-secret-plain");
        assert_eq!(parsed.routing_headers.len(), 4);
        assert_eq!(
            parsed.routing_headers.get("access_token").map(String::as_str),
            Some("admin-secret-plain")
        );
    }

    #[test]
    fn connection_info_tolerates_missing_optional_fields() {
        // Manager 可能暂未返回 smcpNamespace / expiresAt / computerName 等字段——不应反序列化失败。
        let json = r#"{
            "socketBaseURL": "https://s.example.com",
            "accessToken": "tok",
            "routingHeaders": {}
        }"#;
        let parsed: ConnectionInfoResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.socket_base_url, "https://s.example.com");
        assert!(parsed.sio_path.is_none());
        assert!(parsed.smcp_namespace.is_none());
        assert!(parsed.expires_at.is_none());
    }

    #[test]
    fn manager_error_serializes_with_kind_tag() {
        let e = ManagerError::PaymentRequired {
            message: "arrears".into(),
            redirect_url: Some("https://pay.example.com".into()),
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v.get("kind").and_then(|x| x.as_str()), Some("payment_required"));
        assert_eq!(
            v.pointer("/detail/message").and_then(|x| x.as_str()),
            Some("arrears")
        );
        assert_eq!(
            v.pointer("/detail/redirect_url").and_then(|x| x.as_str()),
            Some("https://pay.example.com")
        );
    }

    #[test]
    fn manager_error_unit_variants_serialize_with_kind_only() {
        let e = ManagerError::Unauthorized;
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v.get("kind").and_then(|x| x.as_str()), Some("unauthorized"));
    }

    #[tokio::test]
    async fn require_session_errors_when_none() {
        let c = ManagerClient::new();
        let err = c.require_session().await.unwrap_err();
        assert!(matches!(err, ManagerError::NoSession));
    }

    #[tokio::test]
    async fn has_session_reflects_state_transitions() {
        let c = ManagerClient::new();
        assert!(!c.has_session().await);
        *c.session.write().await = Some(Session {
            base_url: "https://x".into(),
            jwt: "j".into(),
            pending_session_token: None,
        });
        assert!(c.has_session().await);
        // logout() 应清理内存 session（keychain 条目不存在时 delete 会成功 noop）
        c.logout().await.unwrap();
        assert!(!c.has_session().await);
    }

    #[test]
    fn strip_trailing_slash_normalizes() {
        assert_eq!(strip_trailing_slash("https://x.com/".to_string()), "https://x.com");
        assert_eq!(strip_trailing_slash("https://x.com".to_string()), "https://x.com");
        assert_eq!(
            strip_trailing_slash("https://x.com/path/".to_string()),
            "https://x.com/path"
        );
    }
}
