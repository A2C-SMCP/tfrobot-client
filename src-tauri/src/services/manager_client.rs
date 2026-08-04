//! TFRSManager HTTP 客户端。
//!
//! 封装桌面端与 TFRSManager 的 3 条核心 REST 调用（登录 / 列表 / connection-info），
//! 负责 JWT/keychain transport 生命周期与 HTTP 错误分类；401 状态提交由
//! `ManagerContextCoordinator` 统一线性化。
//!
//! 本模块不依赖 Tauri 运行时，便于单元测试。生产调用必须经过 Manager Context coordinator。

use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use reqwest::{header, StatusCode};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::sync::RwLock;

use crate::services::keychain::{self, SecretStore, SystemSecretStore};
use crate::services::manager_environment::ManagerEnvironment;

/// keychain 中 Manager JWT 条目的用户名前缀；实际 key 为 `manager_jwt:{sha256(base_url)[..16]}`。
const KEYCHAIN_KEY_PREFIX: &str = "manager_jwt:";

/// TFRM-167 落地的权限失效错误码：业务接口对「不可见 / 已变更」资源返回 HTTP 404，
/// 同时在响应体顶层带 `errorCode = ERR_NOT_FOUND_OR_NO_PERMISSION`。
/// 数字 `code=404` 与 `message` 不变，仅新增此 `errorCode` 字段。
pub const ERR_NOT_FOUND_OR_NO_PERMISSION: &str = "ERR_NOT_FOUND_OR_NO_PERMISSION";

/// RFC 8693 token-exchange 的 grant type（TFRC-11 / C1）。
const GRANT_TYPE_TOKEN_EXCHANGE: &str = "urn:ietf:params:oauth:grant-type:token-exchange";
/// subject_token 类型：User JWT（区别于 PAT 的 `...:token-type:access_token`）。
const SUBJECT_TOKEN_TYPE_JWT: &str = "urn:ietf:params:oauth:token-type:jwt";

// ───────────────────────── 错误 ─────────────────────────

/// 面向前端的错误分类。`serde` 采用 `tag + content` 让 UI 可按 `kind` 分支。
#[derive(Debug, Error, Serialize, Clone)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum ManagerError {
    /// 连接不上 Manager（DNS / TCP / TLS 层失败）。
    #[error("Network error: {0}")]
    NetworkError(String),

    /// 401：JWT 过期或无效。此层只分类；coordinator 负责清理和发布状态事件。
    #[error("Unauthorized: Manager JWT expired or invalid")]
    Unauthorized,

    /// 登录接口返回 401。它是凭据业务错误，不代表已有 Manager session 过期。
    #[error("Invalid Manager credentials: {message}")]
    InvalidCredentials { message: String },

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

    /// 404 + 顶层 `errorCode = ERR_NOT_FOUND_OR_NO_PERMISSION`：资源不可见或权限被回收
    /// （典型场景：部门调岗后该机器人对当前 viewer 不再可见）。
    /// 前端据此清本地缓存 + refetch + 从列表剔除该项（TFRM-56）。
    #[error("Not found or no permission (visibility revoked)")]
    NotFoundOrNoPermission,

    /// 其他 HTTP 非成功状态。
    #[error("HTTP {status}: {body}")]
    Other { status: u16, body: String },

    /// RFC 8693 token-exchange 失败：RFC 6749 §5.2 错误体 `{error, error_description}`。
    /// `error` 形如 `invalid_grant` / `invalid_target` / `invalid_scope` / `unsupported_grant_type`。
    #[error("Token exchange failed: {error}")]
    TokenExchange {
        error: String,
        description: Option<String>,
    },

    /// 503 `temporarily_unavailable`：签名子系统未就位（ACCESS_TOKEN_SIGNING_* 未配置 / flag 未开）。
    /// 短期重试可恢复——上层应带 jitter/退避重试，而非当作硬错误弹登录。
    #[error("Token signing temporarily unavailable")]
    SigningUnavailable { message: Option<String> },

    /// 未登录即调用了需要 session 的命令（list / connection-info / select-account）。
    #[error("No active Manager session; call manager_login first")]
    NoSession,

    /// 请求使用的 Manager Context 已被登录、选账户、恢复或登出操作替换。
    #[error("Manager context changed while the request was in flight")]
    ContextChanged,

    /// 内部调用未提供 Manager 地址。生产命令必须由环境枚举映射出地址。
    #[error("Manager base URL was not resolved from an environment")]
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

/// 登录请求体。FrontPortal 契约允许手机号或邮箱二选一。
#[derive(Debug, Serialize)]
struct LoginRequestBody<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    phone: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<&'a str>,
    password: &'a str,
}

/// Account IDs are opaque in the current public contract, while older Manager deployments used
/// positive JSON numbers. Preserve public strings exactly and retain numeric wire compatibility
/// for legacy IDs that round-trip canonically through `u64`.
#[derive(Debug, Serialize)]
#[serde(untagged)]
enum WireAccountId<'a> {
    LegacyNumber(u64),
    Opaque(&'a str),
}

/// select-account 请求体。`POST /auth/select-account`。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SelectAccountRequestBody<'a> {
    temp_token: &'a str,
    account_id: WireAccountId<'a>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SwitchAccountRequestBody<'a> {
    account_id: WireAccountId<'a>,
}

/// 登录成功后 Manager 下发的用户/账户信息（扁平 4 字段，**无嵌套 user 对象**）。
/// 结构与 select-account 成功响应一致。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserInfo {
    #[serde(deserialize_with = "super::serde_compat::deserialize_opaque_id")]
    pub user_id: String,
    #[serde(deserialize_with = "super::serde_compat::deserialize_opaque_id")]
    pub account_id: String,
    pub account_name: String,
}

/// `GET /api/v1/auth/me` 的脱敏身份响应。
///
/// JWT 与其他凭据不会进入此 DTO，因此它可以安全地用于 Manager Context 快照与事件。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManagerCurrentUser {
    #[serde(deserialize_with = "super::serde_compat::deserialize_opaque_id")]
    pub id: String,
    #[serde(default)]
    pub nickname: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub phone: String,
    #[serde(default)]
    pub account_avatar: String,
    #[serde(deserialize_with = "super::serde_compat::deserialize_opaque_id")]
    pub account_id: String,
    pub account_name: String,
    #[serde(default)]
    pub employee_no: String,
    #[serde(deserialize_with = "super::serde_compat::deserialize_opaque_id")]
    pub organization_id: String,
    pub organization_name: String,
    pub organization_type: String,
    #[serde(default)]
    pub permissions: Vec<String>,
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
    #[serde(deserialize_with = "super::serde_compat::deserialize_opaque_id")]
    pub account_id: String,
    pub account_name: String,
    #[serde(default)]
    pub nickname: String,
    #[serde(default)]
    #[serde(deserialize_with = "super::serde_compat::deserialize_opaque_id")]
    pub organization_id: String,
    #[serde(default)]
    pub organization_name: String,
    #[serde(default)]
    pub organization_type: String,
}

/// Account available to the currently authenticated user.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManagerAccountSummary {
    #[serde(deserialize_with = "super::serde_compat::deserialize_opaque_id")]
    pub account_id: String,
    pub account_name: String,
    #[serde(default)]
    pub nickname: String,
    #[serde(deserialize_with = "super::serde_compat::deserialize_opaque_id")]
    pub organization_id: String,
    pub organization_name: String,
    #[serde(default)]
    pub organization_type: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub avatar: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManagerAccountListPayload {
    #[serde(default)]
    accounts: Vec<ManagerAccountSummary>,
}

/// Redacted identity returned by passwordless account switching. The token is consumed internally.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SwitchedManagerAccount {
    #[serde(deserialize_with = "super::serde_compat::deserialize_opaque_id")]
    pub user_id: String,
    #[serde(deserialize_with = "super::serde_compat::deserialize_opaque_id")]
    pub account_id: String,
    pub account_name: String,
    #[serde(deserialize_with = "super::serde_compat::deserialize_opaque_id")]
    pub organization_id: String,
    pub organization_name: String,
    #[serde(default)]
    pub organization_type: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwitchAccountPayload {
    token: String,
    #[serde(flatten)]
    identity: SwitchedManagerAccount,
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

/// 登录成功但尚无关联账户。客户端提示用户先在 FrontPortal 完成组织初始化。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OnboardingPayload {
    #[allow(dead_code)]
    token: String,
    #[serde(deserialize_with = "super::serde_compat::deserialize_opaque_id")]
    user_id: String,
    needs_onboarding: bool,
}

/// 登录响应的 data 体，按字段形态区分。
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum LoginData {
    SingleAccount(AuthenticatedPayload),
    MultiAccount(MultiAccountPayload),
    Onboarding(OnboardingPayload),
}

/// 前端可感知的登录结果（JWT 不透出；写 keychain 采用 best-effort，内存 session 必须建立）。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LoginResult {
    /// 登录成功，JWT 已进入内存 session；keychain 持久化失败不阻断当前会话。
    Authenticated { user: UserInfo },
    /// 命中多账户，需要前端让用户挑选账号后调 `manager_select_account`。
    AccountSelectionRequired { accounts: Vec<AccountOption> },
    /// 用户身份有效，但尚未创建或加入任何组织账户。
    OnboardingRequired {
        #[serde(rename = "userId")]
        user_id: String,
    },
}

/// 部门祖先链元素（`departments[].ancestors[]` 元素）。
/// 根→叶有序，末元素即本部门；按序 join `name` 即面包屑。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DepartmentAncestor {
    #[serde(deserialize_with = "super::serde_compat::deserialize_opaque_id")]
    pub id: String,
    pub name: String,
}

/// 部门归属（`departments[]` 元素）。契约见 TFRM-167/168：
/// - `ancestors` **含自身**、根→叶有序（如 总公司 / 研发中心 / 平台组）
/// - `path` 与后端 `Department.Path` 同源（形如 `/1/3/7/`）
/// - **不受可见性 flag 控制**，始终返回真实归属
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DepartmentRef {
    #[serde(deserialize_with = "super::serde_compat::deserialize_opaque_id")]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub ancestors: Vec<DepartmentAncestor>,
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
    /// 机器人自身账号 ID（`AccountType=robot` 的 Account.ID，不透明字符串）；token-exchange 的
    /// audience = `robot:<robotAccountId>`（TFRM-183 暴露，**nullable**：历史/未回填实例为 null —
    /// 这类机器人不能做 token-exchange 连接，前端应禁用其连接按钮）。
    /// 注意与 `account_id`（创建人 ID）和 `robot_id`/rid（SMCP 路由串）区分。
    #[serde(
        default,
        alias = "robot_account_id",
        deserialize_with = "super::serde_compat::deserialize_optional_opaque_id",
        skip_serializing_if = "Option::is_none"
    )]
    pub robot_account_id: Option<String>,
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
    /// 部门归属（TFRM-56：含祖先链，用于客户端面包屑展示）。
    /// 后端保证为 `[]` 非 null；旧响应缺字段时 serde 默认空 vec（向后兼容）。
    #[serde(default)]
    pub departments: Vec<DepartmentRef>,
}

/// connection-info 响应 data 体（`GET /api/v1/digital-employees/{id}/connection-info`）。
/// 字段与 TFRM-18 / UAT guide §5.5 对齐；`routingHeaders` 是客户端注入 smcp-computer 的权威契约。
///
/// TFRC-20（C2）退役连接面内嵌令牌：不再消费 `accessToken` / `expiresAt`——连接面鉴权唯一走
/// token-exchange 换发的短 JWT（注入 Socket.IO auth dict 字段 `token`，TFRC-11/C1）。M6（TFRM-161）
/// 后 Manager 也不再下发这两个字段；过渡期旧响应仍含它们时，serde 静默忽略（无 `deny_unknown_fields`）。
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub computer_name: Option<String>,
    /// 纯路由头（`X-TF-*`）：客户端整 dict verbatim 注入 smcp-computer 的 headers 参数，
    /// 不要自组装 header 名。连接面鉴权不再走此处（TFRC-20：凭据走 Socket.IO auth dict）。
    #[serde(default, deserialize_with = "deserialize_routing_headers")]
    pub routing_headers: HashMap<String, String>,
}

fn deserialize_routing_headers<'de, D>(deserializer: D) -> Result<HashMap<String, String>, D::Error>
where
    D: Deserializer<'de>,
{
    let headers = HashMap::<String, String>::deserialize(deserializer)?;
    Ok(strip_auth_headers(headers))
}

fn strip_auth_headers(headers: HashMap<String, String>) -> HashMap<String, String> {
    headers
        .into_iter()
        .filter(|(key, _)| !is_auth_header_name(key))
        .collect()
}

pub(crate) fn is_auth_header_name(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace('_', "-");
    normalized.contains("token")
        || normalized == "authorization"
        || normalized == "cookie"
        || normalized == "x-api-key"
}

// ───────── RFC 8693 token-exchange（TFRC-11 / C1，前置 TFRM-158/M4） ─────────

/// `POST /api/v1/oauth/token` 请求体（`application/x-www-form-urlencoded`）。
/// 公开端点：subject_token 在表单里，无需 Authorization header。**不引 oauth2 crate**，手写表单。
#[derive(Debug, Serialize)]
struct TokenExchangeRequest<'a> {
    grant_type: &'a str,
    subject_token: &'a str,
    subject_token_type: &'a str,
    audience: String,
    /// 空格分隔的可选 scope；None 时整字段不发送（server 缺省授予全部可用能力）。
    #[serde(skip_serializing_if = "Option::is_none")]
    scope: Option<String>,
}

/// token-exchange 成功响应（OAuth 标准 JSON）。
#[derive(Debug, Deserialize)]
struct TokenExchangeResponse {
    access_token: String,
    #[serde(default)]
    token_type: String,
    #[serde(default)]
    expires_in: i64,
    #[serde(default)]
    #[allow(dead_code)]
    issued_token_type: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

/// 换取到的短 JWT 及元数据。`access_token` 是注入 Socket.IO `auth` dict（字段名 `token`）的
/// 连接面凭据。**故意不实现 `Serialize`**——短 JWT 不应原样透传给前端。
#[derive(Debug, Clone, PartialEq)]
pub struct ExchangedToken {
    pub access_token: String,
    pub token_type: String,
    /// 有效期秒数（server 默认 300）。上层据此计算 `expires_at - 60s` 预刷新重连。
    pub expires_in: i64,
    pub scope: Option<String>,
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
/// JWT 落在按 canonical environment URL 隔离的 keychain 条目；JSON 只持久化环境与非敏感用户元数据。
#[derive(Debug, Clone)]
struct Session {
    generation: u64,
    base_url: String,
    environment: Option<ManagerEnvironment>,
    jwt: String,
    /// 多账户登录待确认时的临时 session token；完成 `select_account` 后置为 None。
    pending_session_token: Option<String>,
}

pub(crate) struct ManagerRequestOutcome<T> {
    pub generation: u64,
    pub result: Result<T, ManagerError>,
}

// ───────────────────────── 客户端 ─────────────────────────

pub struct ManagerClient {
    http: reqwest::Client,
    session: Arc<RwLock<Option<Session>>>,
    session_generation: AtomicU64,
    secret_store: Arc<dyn SecretStore>,
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
        Self::new_with_secret_store(Arc::new(SystemSecretStore))
    }

    pub fn new_with_secret_store(secret_store: Arc<dyn SecretStore>) -> Self {
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
            session_generation: AtomicU64::new(0),
            secret_store,
        }
    }

    /// 仅供测试：注入自定义 reqwest::Client（用于 mock server 断言 User-Agent 等）。
    pub fn with_http_client(http: reqwest::Client) -> Self {
        Self::with_http_client_and_secret_store(
            http,
            Arc::new(crate::services::keychain::InMemorySecretStore::default()),
        )
    }

    pub fn with_http_client_and_secret_store(
        http: reqwest::Client,
        secret_store: Arc<dyn SecretStore>,
    ) -> Self {
        Self {
            http,
            session: Arc::new(RwLock::new(None)),
            session_generation: AtomicU64::new(0),
            secret_store,
        }
    }

    // ───────── 工具 ─────────

    fn resolve_base_url(arg: Option<String>) -> Result<String, ManagerError> {
        match arg.filter(|s| !s.trim().is_empty()) {
            Some(u) => Ok(strip_trailing_slash(u)),
            None => Err(ManagerError::MissingBaseUrl),
        }
    }

    fn keychain_key(base_url: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(base_url.as_bytes());
        let hex = hex::encode(hasher.finalize());
        format!("{KEYCHAIN_KEY_PREFIX}{}", &hex[..16])
    }

    fn save_manager_jwt(&self, base_url: &str, token: &str) -> Result<(), ManagerError> {
        let key = Self::keychain_key(base_url);
        self.secret_store.set_secret(&key, token)?;
        Ok(())
    }

    fn delete_manager_jwt(&self, base_url: &str) -> Result<(), ManagerError> {
        let key = Self::keychain_key(base_url);
        self.secret_store.delete_secret(&key)?;
        Ok(())
    }

    async fn commit_authenticated_session(
        &self,
        base_url: String,
        environment: Option<ManagerEnvironment>,
        token: String,
    ) -> Result<(), ManagerError> {
        let mut guard = self.session.write().await;
        let previous_base_url = guard.as_ref().map(|session| session.base_url.clone());

        // Keychain write and session publication share the session lock. A same-generation 401
        // cannot clear the new session between these two side effects, and an old-generation 401
        // cannot delete the new environment key after publication.
        self.save_manager_jwt(&base_url, &token)?;
        if let Some(previous) = previous_base_url.filter(|previous| previous != &base_url) {
            if let Err(error) = self.delete_manager_jwt(&previous) {
                self.secret_store
                    .delete_secret_best_effort(&Self::keychain_key(&base_url));
                return Err(error);
            }
        }

        let generation = self.session_generation.fetch_add(1, Ordering::AcqRel) + 1;
        *guard = Some(Session {
            generation,
            base_url,
            environment,
            jwt: token,
            pending_session_token: None,
        });
        Ok(())
    }

    async fn commit_non_authenticated_session(
        &self,
        base_url: String,
        environment: Option<ManagerEnvironment>,
        pending_session_token: Option<String>,
    ) -> Result<(), ManagerError> {
        let mut guard = self.session.write().await;
        let previous_base_url = guard.as_ref().map(|session| session.base_url.clone());
        self.delete_manager_jwt(&base_url)?;
        if let Some(previous) = previous_base_url.filter(|previous| previous != &base_url) {
            self.delete_manager_jwt(&previous)?;
        }

        let generation = self.session_generation.fetch_add(1, Ordering::AcqRel) + 1;
        *guard = pending_session_token.map(|pending_session_token| Session {
            generation,
            base_url,
            environment,
            jwt: String::new(),
            pending_session_token: Some(pending_session_token),
        });
        Ok(())
    }

    async fn require_session(&self) -> Result<Session, ManagerError> {
        self.session
            .read()
            .await
            .clone()
            .ok_or(ManagerError::NoSession)
    }

    pub async fn current_environment(&self) -> Option<ManagerEnvironment> {
        self.session
            .read()
            .await
            .as_ref()
            .and_then(|s| s.environment)
    }

    pub async fn restore_session(
        &self,
        environment: ManagerEnvironment,
    ) -> Result<bool, ManagerError> {
        self.restore_session_from_base_url(environment, environment.base_url().to_string())
            .await
    }

    pub(crate) async fn restore_session_from_base_url(
        &self,
        environment: ManagerEnvironment,
        normalized_base_url: String,
    ) -> Result<bool, ManagerError> {
        // Hold the session write lock across keychain read and generation publication. Otherwise
        // a late 401 could clear the old session and delete this JWT after it was read but before
        // the restored generation became visible.
        let mut session = self.session.write().await;
        let key = Self::keychain_key(&normalized_base_url);
        let Some(jwt) = self.secret_store.get_secret(&key)? else {
            return Ok(false);
        };
        if jwt.trim().is_empty() {
            return Ok(false);
        }
        let generation = self.session_generation.fetch_add(1, Ordering::AcqRel) + 1;
        *session = Some(Session {
            generation,
            base_url: normalized_base_url,
            environment: Some(environment),
            jwt,
            pending_session_token: None,
        });
        Ok(true)
    }

    /// 用当前 session 的 JWT 构造鉴权 header。
    fn bearer(jwt: &str) -> String {
        format!("Bearer {jwt}")
    }

    // ───────── HTTP 错误归一化 ─────────

    async fn classify_error(&self, resp: reqwest::Response) -> ManagerError {
        let status = resp.status();
        match status {
            // Transport only classifies the response. The Manager Context coordinator owns the
            // generation check and the atomic session/keychain/metadata/context transition.
            StatusCode::UNAUTHORIZED => ManagerError::Unauthorized,
            StatusCode::FORBIDDEN => ManagerError::Forbidden,
            StatusCode::NOT_FOUND => {
                // 普通 404 与「不可见/权限回收」404 同状态码，靠响应体顶层 `errorCode` 区分。
                let body = resp.text().await.unwrap_or_default();
                if extract_error_code(&body).as_deref() == Some(ERR_NOT_FOUND_OR_NO_PERMISSION) {
                    ManagerError::NotFoundOrNoPermission
                } else {
                    ManagerError::NotFound
                }
            }
            StatusCode::PAYMENT_REQUIRED => {
                let body = resp.text().await.unwrap_or_default();
                let (message, redirect_url) = parse_payment_required(&body);
                ManagerError::PaymentRequired {
                    message,
                    redirect_url,
                }
            }
            s => {
                ManagerError::Other {
                    status: s.as_u16(),
                    // Do not serialize arbitrary upstream bodies across Tauri IPC. Error payloads
                    // can echo credentials or tokens even when the expected API contract never
                    // does; status + a stable summary are sufficient for the frontend.
                    body: "Unexpected Manager response".to_string(),
                }
            }
        }
    }

    pub(crate) async fn clear_session_for_generation(&self, expected_generation: u64) -> bool {
        let mut guard = self.session.write().await;
        if let Some(session) = guard
            .as_ref()
            .filter(|session| session.generation == expected_generation)
        {
            // The session and its environment-scoped keychain entry form one transition. Keep
            // the lock until both are cleared so a new generation cannot publish a JWT between
            // the generation check and this deletion.
            let key = Self::keychain_key(&session.base_url);
            self.secret_store.delete_secret_best_effort(&key);
            *guard = None;
            return true;
        }
        false
    }

    pub(crate) fn current_session_generation(&self) -> u64 {
        self.session_generation.load(Ordering::Acquire)
    }

    // ───────── 公共 API ─────────

    /// `POST {base}/auth/login-by-password` — 登录。
    ///
    /// server 按 `phone` 或 `email` 字段接收。响应 envelope `{code, message, data}`，
    /// data 为两种形态之一：单账户 `{token, userId, accountId, accountName}` 或
    /// 多账户 `{message, tempToken, expiresIn, accounts}`。
    pub async fn login(
        &self,
        base_url: Option<String>,
        identifier: &str,
        password: &str,
    ) -> Result<LoginResult, ManagerError> {
        let base = Self::resolve_base_url(base_url)?;
        let environment = ManagerEnvironment::from_base_url(&base);
        let url = format!("{base}/auth/login-by-password");
        let (phone, email) = if identifier.contains('@') {
            (None, Some(identifier))
        } else {
            (Some(identifier), None)
        };

        let resp = self
            .http
            .post(&url)
            .json(&LoginRequestBody {
                phone,
                email,
                password,
            })
            .send()
            .await
            .map_err(|e| ManagerError::NetworkError(flatten_reqwest_err(e)))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            let body = resp.text().await.unwrap_or_default();
            return Err(ManagerError::InvalidCredentials {
                message: extract_message(&body)
                    .unwrap_or_else(|| "用户名、邮箱或密码错误".to_string()),
            });
        }
        if !resp.status().is_success() {
            return Err(self.classify_error(resp).await);
        }

        let envelope: ApiEnvelope<LoginData> = resp
            .json()
            .await
            .map_err(|e| ManagerError::InvalidResponse(e.to_string()))?;

        match envelope.data {
            LoginData::SingleAccount(payload) => {
                let AuthenticatedPayload { token, user } = payload;
                self.commit_authenticated_session(base, environment, token)
                    .await?;
                Ok(LoginResult::Authenticated { user })
            }
            LoginData::MultiAccount(payload) => {
                // 多账户：旧 JWT 不能在崩溃恢复时绕过当前账户选择。
                self.commit_non_authenticated_session(base, environment, Some(payload.temp_token))
                    .await?;
                Ok(LoginResult::AccountSelectionRequired {
                    accounts: payload.accounts,
                })
            }
            LoginData::Onboarding(payload) if payload.needs_onboarding => {
                self.commit_non_authenticated_session(base, environment, None)
                    .await?;
                Ok(LoginResult::OnboardingRequired {
                    user_id: payload.user_id,
                })
            }
            LoginData::Onboarding(_) => Err(ManagerError::InvalidResponse(
                "onboarding response did not set needsOnboarding=true".to_string(),
            )),
        }
    }

    /// `POST {base}/auth/select-account` — 多账户登录二次确认。
    ///
    /// 必须在 `login()` 返回 `AccountSelectionRequired` 之后调用。
    /// 请求体字段：`{tempToken, accountId}`；响应同单账户登录。
    pub async fn select_account(&self, account_id: &str) -> Result<UserInfo, ManagerError> {
        let wire_account_id = wire_account_id(account_id, "select-account")?;
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
                account_id: wire_account_id,
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

        let AuthenticatedPayload { token, user } = envelope.data;
        self.commit_authenticated_session(session.base_url, session.environment, token)
            .await?;
        Ok(user)
    }

    /// `GET {base}/api/v1/auth/me` — 获取 JWT 所属的完整用户、账户与组织身份。
    pub async fn get_current_user(&self) -> Result<ManagerCurrentUser, ManagerError> {
        self.get_current_user_outcome().await?.result
    }

    pub(crate) async fn get_current_user_outcome(
        &self,
    ) -> Result<ManagerRequestOutcome<ManagerCurrentUser>, ManagerError> {
        let session = self.require_session().await?;
        let generation = session.generation;
        let result = async {
            if session.jwt.is_empty() {
                return Err(ManagerError::NoSession);
            }
            let url = format!("{}/api/v1/auth/me", session.base_url);
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
            let envelope: ApiEnvelope<ManagerCurrentUser> = resp
                .json()
                .await
                .map_err(|e| ManagerError::InvalidResponse(e.to_string()))?;
            Ok(envelope.data)
        }
        .await;
        Ok(ManagerRequestOutcome { generation, result })
    }

    /// `GET {base}/api/v1/accounts/my` — list accounts available to the current user.
    pub(crate) async fn list_accounts_outcome(
        &self,
    ) -> Result<ManagerRequestOutcome<Vec<ManagerAccountSummary>>, ManagerError> {
        let session = self.require_session().await?;
        let generation = session.generation;
        let result = async {
            if session.jwt.is_empty() {
                return Err(ManagerError::NoSession);
            }
            let url = format!("{}/api/v1/accounts/my", session.base_url);
            let response = self
                .http
                .get(url)
                .header(header::AUTHORIZATION, Self::bearer(&session.jwt))
                .send()
                .await
                .map_err(|error| ManagerError::NetworkError(flatten_reqwest_err(error)))?;
            if !response.status().is_success() {
                return Err(self.classify_error(response).await);
            }
            let envelope: ApiEnvelope<ManagerAccountListPayload> = response
                .json()
                .await
                .map_err(|error| ManagerError::InvalidResponse(error.to_string()))?;
            Ok(envelope.data.accounts)
        }
        .await;
        Ok(ManagerRequestOutcome { generation, result })
    }

    /// `POST {base}/api/v1/auth/switch-account` — switch using the current JWT, without password.
    pub(crate) async fn switch_account(
        &self,
        account_id: &str,
    ) -> Result<SwitchedManagerAccount, ManagerError> {
        let wire_account_id = wire_account_id(account_id, "switch-account")?;
        let session = self.require_session().await?;
        if session.jwt.is_empty() {
            return Err(ManagerError::NoSession);
        }
        let url = format!("{}/api/v1/auth/switch-account", session.base_url);
        let response = self
            .http
            .post(url)
            .header(header::AUTHORIZATION, Self::bearer(&session.jwt))
            .json(&SwitchAccountRequestBody {
                account_id: wire_account_id,
            })
            .send()
            .await
            .map_err(|error| ManagerError::NetworkError(flatten_reqwest_err(error)))?;
        if !response.status().is_success() {
            return Err(self.classify_error(response).await);
        }
        let envelope: ApiEnvelope<SwitchAccountPayload> = response
            .json()
            .await
            .map_err(|error| ManagerError::InvalidResponse(error.to_string()))?;
        let SwitchAccountPayload { token, identity } = envelope.data;
        if identity.account_id != account_id {
            return Err(ManagerError::InvalidResponse(
                "switch-account response accountId does not match the request".to_string(),
            ));
        }
        self.commit_authenticated_session(session.base_url, session.environment, token)
            .await?;
        Ok(identity)
    }

    /// `GET {base}/api/v1/digital-employees` — 当前账号的数字员工列表。
    ///
    /// 响应走标准分页 envelope：`{code, message, data: {total, page, pageSize, items}}`。
    pub async fn list_digital_employees(&self) -> Result<Vec<DigitalEmployeeBrief>, ManagerError> {
        self.list_digital_employees_outcome().await?.result
    }

    pub(crate) async fn list_digital_employees_outcome(
        &self,
    ) -> Result<ManagerRequestOutcome<Vec<DigitalEmployeeBrief>>, ManagerError> {
        let session = self.require_session().await?;
        let generation = session.generation;
        let result = async {
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
        .await;
        Ok(ManagerRequestOutcome { generation, result })
    }

    /// `GET {base}/api/v1/digital-employees/{id}/connection-info` — 拿 SMCP 握手参数。
    /// `id` 是 Manager 侧的数字主键（DigitalEmployeeBrief.id）。
    pub async fn get_connection_info(
        &self,
        id: u64,
    ) -> Result<ConnectionInfoResponse, ManagerError> {
        self.get_connection_info_outcome(id).await?.result
    }

    pub(crate) async fn get_connection_info_outcome(
        &self,
        id: u64,
    ) -> Result<ManagerRequestOutcome<ConnectionInfoResponse>, ManagerError> {
        let session = self.require_session().await?;
        let generation = session.generation;
        let result = async {
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
        .await;
        Ok(ManagerRequestOutcome { generation, result })
    }

    /// `POST {base}/api/v1/oauth/token` — RFC 8693 token-exchange（C1 / TFRM-153）。
    ///
    /// 用当前 session 的 User JWT 作 subject，换取目标机器人的短 JWT（server 默认 5min）。
    /// 公开端点：subject_token 在 `application/x-www-form-urlencoded` 表单里，无需 Authorization
    /// header（但本方法要求已登录以取 User JWT）。**不引 oauth2 crate**，手写表单。
    ///
    /// - `robot_account_id`：目标机器人账号 ID（audience = `robot:<id>`）。
    /// - `scope`：可选、空格分隔；None 时不发该字段（server 缺省授予全部可用能力）。
    ///
    /// 错误归类：400 → [`ManagerError::TokenExchange`]（RFC 6749 §5.2）；503 →
    /// [`ManagerError::SigningUnavailable`]（签名未就位，可重试）；401/402/404 等沿用通用归一化
    /// （401 只在此层归类；session/context 清理由 Manager Context coordinator 事务提交）。
    pub async fn exchange_token(
        &self,
        robot_account_id: &str,
        scope: Option<String>,
    ) -> Result<ExchangedToken, ManagerError> {
        self.exchange_token_outcome(robot_account_id, scope)
            .await?
            .result
    }

    pub(crate) async fn exchange_token_outcome(
        &self,
        robot_account_id: &str,
        scope: Option<String>,
    ) -> Result<ManagerRequestOutcome<ExchangedToken>, ManagerError> {
        let session = self.require_session().await?;
        let generation = session.generation;
        let result = async {
            if session.jwt.is_empty() {
                return Err(ManagerError::NoSession);
            }
            let url = format!("{}/api/v1/oauth/token", session.base_url);

            let form = TokenExchangeRequest {
                grant_type: GRANT_TYPE_TOKEN_EXCHANGE,
                subject_token: &session.jwt,
                subject_token_type: SUBJECT_TOKEN_TYPE_JWT,
                audience: format!("robot:{robot_account_id}"),
                scope,
            };

            let resp = self
                .http
                .post(&url)
                .form(&form)
                .send()
                .await
                .map_err(|e| ManagerError::NetworkError(flatten_reqwest_err(e)))?;

            let status = resp.status();
            if status.is_success() {
                let body: TokenExchangeResponse = resp
                    .json()
                    .await
                    .map_err(|e| ManagerError::InvalidResponse(e.to_string()))?;
                return Ok(ExchangedToken {
                    access_token: body.access_token,
                    token_type: body.token_type,
                    expires_in: body.expires_in,
                    scope: body.scope,
                });
            }

            // 错误分流：400 = RFC 6749 §5.2 OAuth 错误体；503 = 签名子系统未就位；其余沿用通用归一化
            // （401 交 coordinator 清 session、402 欠费、404 等）。
            match status {
                StatusCode::BAD_REQUEST => {
                    let body = resp.text().await.unwrap_or_default();
                    let (error, description) = parse_oauth_error(&body);
                    Err(ManagerError::TokenExchange { error, description })
                }
                StatusCode::SERVICE_UNAVAILABLE => {
                    let body = resp.text().await.unwrap_or_default();
                    let (_error, description) = parse_oauth_error(&body);
                    Err(ManagerError::SigningUnavailable {
                        message: description,
                    })
                }
                _ => Err(self.classify_error(resp).await),
            }
        }
        .await;
        Ok(ManagerRequestOutcome { generation, result })
    }

    /// 本地登出：清 keychain + 内存 session。无服务端 logout API。
    pub async fn logout(&self) -> Result<(), ManagerError> {
        let mut guard = self.session.write().await;
        self.session_generation.fetch_add(1, Ordering::AcqRel);
        let delete_result = guard
            .as_ref()
            .map(|session| self.delete_manager_jwt(&session.base_url))
            .transpose();
        *guard = None;
        delete_result.map(|_| ())
    }

    /// 测试/自检用：当前是否已有 session。
    pub async fn has_session(&self) -> bool {
        self.session.read().await.is_some()
    }

    /// Drops only the in-memory projection after a transient restore validation failure. The
    /// keychain JWT remains available for a later retry and is still revalidated by `/auth/me`.
    pub(crate) async fn suspend_restored_session(&self) {
        let mut guard = self.session.write().await;
        self.session_generation.fetch_add(1, Ordering::AcqRel);
        *guard = None;
    }
}

impl Default for ManagerClient {
    fn default() -> Self {
        Self::new()
    }
}

/// 从响应体提取顶层 `errorCode`（TFRM-167：用于区分权限失效 404 与普通 404）。
/// body 非 JSON 或无该字段时返回 None（回退为普通 404 语义，向后兼容）。
fn extract_error_code(body: &str) -> Option<String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ErrorCodeProbe {
        #[serde(default)]
        error_code: Option<String>,
    }
    serde_json::from_str::<ErrorCodeProbe>(body)
        .ok()
        .and_then(|p| p.error_code)
        .filter(|s| !s.is_empty())
}

/// 登录错误响应沿用 Manager 的用户可读 message，避免把凭据错误误报成会话过期。
fn extract_message(body: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct MessageProbe {
        #[serde(default)]
        message: Option<String>,
    }
    serde_json::from_str::<MessageProbe>(body)
        .ok()
        .and_then(|probe| probe.message)
        .filter(|message| !message.trim().is_empty())
}

/// 解析 OAuth / RFC 6749 §5.2 错误体 `{error, error_description}`（token-exchange 用）。
/// 非 JSON 或缺 `error` 字段时回退 `("invalid_request", None)`。
fn parse_oauth_error(body: &str) -> (String, Option<String>) {
    #[derive(Deserialize)]
    struct OAuthError {
        #[serde(default)]
        error: String,
        #[serde(default)]
        error_description: Option<String>,
    }
    serde_json::from_str::<OAuthError>(body)
        .ok()
        .filter(|e| !e.error.is_empty())
        .map(|e| (e.error, e.error_description))
        .unwrap_or_else(|| ("invalid_request".to_string(), None))
}

fn strip_trailing_slash(url: String) -> String {
    if url.ends_with('/') {
        url.trim_end_matches('/').to_string()
    } else {
        url
    }
}

fn wire_account_id<'a>(
    account_id: &'a str,
    operation: &str,
) -> Result<WireAccountId<'a>, ManagerError> {
    if account_id.trim().is_empty() {
        return Err(ManagerError::InvalidResponse(format!(
            "{operation} requires a non-empty accountId"
        )));
    }
    match account_id.parse::<u64>() {
        Ok(value) if value > 0 && value.to_string() == account_id => {
            Ok(WireAccountId::LegacyNumber(value))
        }
        _ => Ok(WireAccountId::Opaque(account_id)),
    }
}

/// 解析 402 响应体。兼容两种形态：
/// - envelope：`{code, message, data: {message?, redirectUrl?}}`
/// - 裸对象：`{message?, redirectUrl?}`
///
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
    use crate::services::keychain::KeychainError;
    use std::sync::{Condvar, Mutex as StdMutex};

    #[derive(Default)]
    struct BlockingDeleteState {
        secrets: HashMap<String, String>,
        block_next_delete: bool,
        delete_entered: bool,
        release_delete: bool,
    }

    #[derive(Default)]
    struct BlockingDeleteSecretStore {
        state: StdMutex<BlockingDeleteState>,
        changed: Condvar,
    }

    impl BlockingDeleteSecretStore {
        fn arm_delete(&self) {
            let mut state = self.state.lock().unwrap();
            state.block_next_delete = true;
            state.delete_entered = false;
            state.release_delete = false;
        }

        fn wait_until_delete_entered(&self) {
            let mut state = self.state.lock().unwrap();
            while !state.delete_entered {
                state = self.changed.wait(state).unwrap();
            }
        }

        fn release_delete(&self) {
            let mut state = self.state.lock().unwrap();
            state.release_delete = true;
            self.changed.notify_all();
        }
    }

    impl SecretStore for BlockingDeleteSecretStore {
        fn set_secret(&self, key: &str, secret: &str) -> Result<(), KeychainError> {
            self.state
                .lock()
                .unwrap()
                .secrets
                .insert(key.to_string(), secret.to_string());
            Ok(())
        }

        fn get_secret(&self, key: &str) -> Result<Option<String>, KeychainError> {
            Ok(self.state.lock().unwrap().secrets.get(key).cloned())
        }

        fn delete_secret(&self, key: &str) -> Result<(), KeychainError> {
            let mut state = self.state.lock().unwrap();
            if state.block_next_delete {
                state.block_next_delete = false;
                state.delete_entered = true;
                self.changed.notify_all();
                while !state.release_delete {
                    state = self.changed.wait(state).unwrap();
                }
            }
            state.secrets.remove(key);
            Ok(())
        }
    }

    fn test_manager_client() -> ManagerClient {
        ManagerClient::new_with_secret_store(
            crate::services::keychain::InMemorySecretStore::shared(),
        )
    }

    #[test]
    fn resolve_base_url_normalizes_explicit_url_and_rejects_missing_value() {
        let got =
            ManagerClient::resolve_base_url(Some("https://arg.example.com/".to_string())).unwrap();
        assert_eq!(got, "https://arg.example.com");

        let err = ManagerClient::resolve_base_url(None).unwrap_err();
        assert!(matches!(err, ManagerError::MissingBaseUrl));

        let err = ManagerClient::resolve_base_url(Some("   ".to_string())).unwrap_err();
        assert!(matches!(err, ManagerError::MissingBaseUrl));
    }

    #[test]
    fn account_id_wire_contract_preserves_every_noncanonical_nonempty_string() {
        let cases = [
            ("1", serde_json::json!(1)),
            ("18446744073709551615", serde_json::json!(u64::MAX)),
            ("0", serde_json::json!("0")),
            ("00", serde_json::json!("00")),
            ("01", serde_json::json!("01")),
            ("+0", serde_json::json!("+0")),
            (
                "18446744073709551616",
                serde_json::json!("18446744073709551616"),
            ),
            ("acct_public_42", serde_json::json!("acct_public_42")),
        ];

        for (account_id, expected) in cases {
            let select = serde_json::to_value(SelectAccountRequestBody {
                temp_token: "temp-token",
                account_id: wire_account_id(account_id, "select-account").unwrap(),
            })
            .unwrap();
            let switch = serde_json::to_value(SwitchAccountRequestBody {
                account_id: wire_account_id(account_id, "switch-account").unwrap(),
            })
            .unwrap();

            assert_eq!(
                select["accountId"], expected,
                "select accountId={account_id}"
            );
            assert_eq!(
                switch["accountId"], expected,
                "switch accountId={account_id}"
            );
        }

        for account_id in ["", " ", "\t\r\n"] {
            assert!(wire_account_id(account_id, "select-account").is_err());
            assert!(wire_account_id(account_id, "switch-account").is_err());
        }
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
                assert_eq!(payload.user.user_id, "9");
                assert_eq!(payload.user.account_id, "16");
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
                assert_eq!(payload.accounts[0].account_id, "2");
                assert_eq!(payload.accounts[0].organization_id, "2");
                assert_eq!(payload.accounts[0].organization_type, "enterprise");
                assert_eq!(payload.accounts[1].account_name, "testuser2_personal");
            }
            _ => panic!("expected MultiAccount branch"),
        }
    }

    #[test]
    fn onboarding_login_result_uses_the_camel_case_ipc_contract() {
        let value = serde_json::to_value(LoginResult::OnboardingRequired {
            user_id: "99".to_string(),
        })
        .unwrap();
        assert_eq!(value["kind"], "onboarding_required");
        assert_eq!(value["userId"], "99");
        assert!(value.get("user_id").is_none());
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
    fn digital_employee_brief_deserializes_departments_with_ancestors() {
        // TFRM-167/168 契约：departments[].ancestors 含自身、根→叶有序。
        let json = r#"{
            "id": 11, "name": "客服机器人", "robotId": "r-1",
            "departments": [{
                "id": 7, "name": "平台组", "path": "/1/3/7/",
                "ancestors": [
                    {"id": 1, "name": "总公司"},
                    {"id": 3, "name": "研发中心"},
                    {"id": 7, "name": "平台组"}
                ]
            }]
        }"#;
        let emp: DigitalEmployeeBrief = serde_json::from_str(json).unwrap();
        assert_eq!(emp.departments.len(), 1);
        let dept = &emp.departments[0];
        assert_eq!(dept.id, "7");
        assert_eq!(dept.path, "/1/3/7/");
        assert_eq!(dept.ancestors.len(), 3);
        assert_eq!(dept.ancestors[0].name, "总公司");
        assert_eq!(dept.ancestors[2].name, "平台组");
        // 面包屑 = ancestors.name join " / "
        let crumb: Vec<&str> = dept.ancestors.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(crumb.join(" / "), "总公司 / 研发中心 / 平台组");
    }

    #[test]
    fn digital_employee_brief_defaults_empty_departments_when_field_absent() {
        // 旧响应 / flag-off 且未回填：缺 departments 字段 → 空 vec，不应反序列化失败。
        let json = r#"{"id": 12, "name": "robot-2"}"#;
        let emp: DigitalEmployeeBrief = serde_json::from_str(json).unwrap();
        assert!(emp.departments.is_empty());
    }

    #[test]
    fn digital_employee_brief_preserves_string_department_ids_from_manager() {
        let emp: DigitalEmployeeBrief = serde_json::from_str(
            r#"{
                "id": 11,
                "name": "robot",
                "departments": [{
                    "id": "org-legacy-18:department-7",
                    "name": "平台组",
                    "ancestors": [{
                        "id": "org-legacy-18:department-1",
                        "name": "总公司"
                    }]
                }]
            }"#,
        )
        .expect("Manager string department IDs should deserialize");

        let serialized = serde_json::to_value(emp).unwrap();
        assert_eq!(
            serialized["departments"][0]["id"],
            serde_json::json!("org-legacy-18:department-7")
        );
        assert_eq!(
            serialized["departments"][0]["ancestors"][0]["id"],
            serde_json::json!("org-legacy-18:department-1")
        );
    }

    #[test]
    fn digital_employee_brief_deserializes_robot_account_id() {
        // TFRM-183：robotAccountId（camelCase, nullable）= 机器人账号 ID，token-exchange audience 用。
        let json = r#"{"id": 11, "name": "robot", "robotId": "rid-1", "robotAccountId": 4242}"#;
        let emp: DigitalEmployeeBrief = serde_json::from_str(json).unwrap();
        assert_eq!(emp.robot_account_id.as_deref(), Some("4242"));
        // 与 rid/robot_id 区分：robotId 是路由串，robotAccountId 是不透明账号 ID。
        assert_eq!(emp.robot_id.as_deref(), Some("rid-1"));

        // Manager 若返回 snake_case，也必须保留；Tauri 再序列化给前端时会转回 robotAccountId。
        let snake_case: DigitalEmployeeBrief =
            serde_json::from_str(r#"{"id": 14, "name": "robot", "robot_account_id": 5252}"#)
                .unwrap();
        assert_eq!(snake_case.robot_account_id.as_deref(), Some("5252"));

        // 缺字段（历史实例）→ None
        let absent: DigitalEmployeeBrief =
            serde_json::from_str(r#"{"id": 12, "name": "old"}"#).unwrap();
        assert!(absent.robot_account_id.is_none());
        // 显式 null → None
        let null_val: DigitalEmployeeBrief =
            serde_json::from_str(r#"{"id": 13, "name": "n", "robotAccountId": null}"#).unwrap();
        assert!(null_val.robot_account_id.is_none());
    }

    #[test]
    fn digital_employee_brief_preserves_string_robot_account_id_from_manager() {
        // staging 实际契约：机器人账号 ID 与登录账号 ID 一样是 opaque string，
        // 不能假设为数字主键，否则一个不兼容条目会导致整个列表解析失败。
        let emp: DigitalEmployeeBrief = serde_json::from_str(
            r#"{"id": 15, "name": "robot", "robotAccountId": "org-legacy-18:account-24"}"#,
        )
        .expect("Manager string robotAccountId should deserialize");

        let serialized = serde_json::to_value(emp).unwrap();
        assert_eq!(
            serialized["robotAccountId"],
            serde_json::json!("org-legacy-18:account-24")
        );
    }

    #[test]
    fn extract_error_code_picks_up_visibility_revoked_marker() {
        // 顶层 errorCode（camelCase），数字 code 与 message 不影响提取。
        let body =
            r#"{"code":404,"message":"not found","errorCode":"ERR_NOT_FOUND_OR_NO_PERMISSION"}"#;
        assert_eq!(
            extract_error_code(body).as_deref(),
            Some(ERR_NOT_FOUND_OR_NO_PERMISSION)
        );
    }

    #[test]
    fn extract_error_code_returns_none_for_plain_404_and_non_json() {
        // 普通 404（无 errorCode）→ None → 上层落普通 NotFound
        assert!(extract_error_code(r#"{"code":404,"message":"not found","data":null}"#).is_none());
        // 空 errorCode 视为无标记
        assert!(extract_error_code(r#"{"errorCode":""}"#).is_none());
        // 非 JSON body 不应 panic
        assert!(extract_error_code("plain text 404").is_none());
        assert!(extract_error_code("").is_none());
    }

    #[test]
    fn not_found_or_no_permission_serializes_with_kind_tag() {
        let v = serde_json::to_value(ManagerError::NotFoundOrNoPermission).unwrap();
        assert_eq!(
            v.get("kind").and_then(|x| x.as_str()),
            Some("not_found_or_no_permission")
        );
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
        // M6（TFRM-161）后 connection-info 仅返元数据 + 纯路由头（X-TF-*），不含 accessToken / expiresAt。
        let json = r#"{
            "socketBaseURL": "https://staging.turingfocus.cn",
            "sioPath": "/socket.io/",
            "namespace": "tenant-acme",
            "rid": "robot-xxx",
            "robotType": "tfrobot",
            "smcpNamespace": "/smcp",
            "computerName": "desktop-001",
            "routingHeaders": {
                "X-TF-Namespace": "tenant-acme",
                "X-TF-RobotId": "robot-xxx",
                "X-TF-RobotType": "tfrobot"
            }
        }"#;
        let parsed: ConnectionInfoResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.socket_base_url, "https://staging.turingfocus.cn");
        assert_eq!(parsed.sio_path.as_deref(), Some("/socket.io/"));
        assert_eq!(parsed.smcp_namespace.as_deref(), Some("/smcp"));
        assert_eq!(parsed.computer_name.as_deref(), Some("desktop-001"));
        assert_eq!(parsed.routing_headers.len(), 3);
        // 连接面鉴权不再随路由头下发（TFRC-20：凭据走 Socket.IO auth dict）。
        assert!(!parsed.routing_headers.contains_key("access_token"));
    }

    #[test]
    fn connection_info_ignores_legacy_token_fields() {
        // flag-day 过渡期：旧 Manager 仍可能下发 accessToken / expiresAt（含 routingHeaders.access_token）。
        // 瘦身后的 DTO 无对应字段，serde 必须静默忽略而非反序列化失败（无 `deny_unknown_fields`）。
        let json = r#"{
            "socketBaseURL": "https://staging.turingfocus.cn",
            "rid": "robot-xxx",
            "accessToken": "admin-secret-plain",
            "computerName": "desktop-001",
            "routingHeaders": {
                "X-TF-Namespace": "tenant-acme",
                "access_token": "admin-secret-plain"
            },
            "expiresAt": "2026-04-23T12:00:00Z"
        }"#;
        let parsed: ConnectionInfoResponse = serde_json::from_str(json).unwrap();
        // 顶层 legacy 鉴权字段（accessToken/expiresAt）被瘦身 DTO 忽略；
        // nested routingHeaders.access_token 也必须在 DTO 边界剔除，避免进入 HTTP header。
        assert_eq!(parsed.socket_base_url, "https://staging.turingfocus.cn");
        assert_eq!(parsed.rid.as_deref(), Some("robot-xxx"));
        assert_eq!(parsed.computer_name.as_deref(), Some("desktop-001"));
        assert_eq!(
            parsed
                .routing_headers
                .get("X-TF-Namespace")
                .map(String::as_str),
            Some("tenant-acme")
        );
        assert!(!parsed.routing_headers.contains_key("access_token"));
        assert_eq!(parsed.routing_headers.len(), 1);
    }

    #[test]
    fn connection_info_tolerates_missing_optional_fields() {
        // Manager 可能暂未返回 smcpNamespace / computerName 等字段——不应反序列化失败。
        let json = r#"{
            "socketBaseURL": "https://s.example.com",
            "routingHeaders": {}
        }"#;
        let parsed: ConnectionInfoResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.socket_base_url, "https://s.example.com");
        assert!(parsed.sio_path.is_none());
        assert!(parsed.smcp_namespace.is_none());
        assert!(parsed.computer_name.is_none());
    }

    #[test]
    fn connection_info_strips_auth_like_routing_headers() {
        let json = r#"{
            "socketBaseURL": "https://s.example.com",
            "routingHeaders": {
                "X-TF-Namespace": "tenant-acme",
                "X-TF-RobotId": "robot-xxx",
                "Access_Token": "legacy-secret",
                "Authorization": "Bearer legacy-secret",
                "cookie": "sid=legacy-secret",
                "x-api-key": "legacy-secret"
            }
        }"#;

        let parsed: ConnectionInfoResponse = serde_json::from_str(json).unwrap();

        assert_eq!(parsed.routing_headers.len(), 2);
        assert_eq!(
            parsed
                .routing_headers
                .get("X-TF-Namespace")
                .map(String::as_str),
            Some("tenant-acme")
        );
        assert_eq!(
            parsed
                .routing_headers
                .get("X-TF-RobotId")
                .map(String::as_str),
            Some("robot-xxx")
        );
        assert!(!parsed.routing_headers.contains_key("Access_Token"));
        assert!(!parsed.routing_headers.contains_key("Authorization"));
        assert!(!parsed.routing_headers.contains_key("cookie"));
        assert!(!parsed.routing_headers.contains_key("x-api-key"));
    }

    #[test]
    fn manager_error_serializes_with_kind_tag() {
        let e = ManagerError::PaymentRequired {
            message: "arrears".into(),
            redirect_url: Some("https://pay.example.com".into()),
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(
            v.get("kind").and_then(|x| x.as_str()),
            Some("payment_required")
        );
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

    #[test]
    fn token_exchange_response_deserializes_oauth_json() {
        let json = r#"{
            "access_token": "short-robot-jwt",
            "issued_token_type": "urn:ietf:params:oauth:token-type:access_token",
            "token_type": "Bearer",
            "expires_in": 300,
            "scope": "smcp:connect tools:call"
        }"#;
        let r: TokenExchangeResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.access_token, "short-robot-jwt");
        assert_eq!(r.token_type, "Bearer");
        assert_eq!(r.expires_in, 300);
        assert_eq!(r.scope.as_deref(), Some("smcp:connect tools:call"));
    }

    #[test]
    fn parse_oauth_error_extracts_error_and_description_with_fallback() {
        let (e, d) =
            parse_oauth_error(r#"{"error":"invalid_scope","error_description":"unknown scope x"}"#);
        assert_eq!(e, "invalid_scope");
        assert_eq!(d.as_deref(), Some("unknown scope x"));

        // 缺 description → None
        let (e2, d2) = parse_oauth_error(r#"{"error":"invalid_grant"}"#);
        assert_eq!(e2, "invalid_grant");
        assert!(d2.is_none());

        // 非 JSON / 空 error → 回退 invalid_request
        let (e3, d3) = parse_oauth_error("not json at all");
        assert_eq!(e3, "invalid_request");
        assert!(d3.is_none());
        let (e4, _) = parse_oauth_error(r#"{"error":""}"#);
        assert_eq!(e4, "invalid_request");
    }

    #[test]
    fn token_exchange_and_signing_errors_serialize_with_kind_tag() {
        let e = ManagerError::TokenExchange {
            error: "invalid_target".into(),
            description: Some("no such robot".into()),
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(
            v.get("kind").and_then(|x| x.as_str()),
            Some("token_exchange")
        );
        assert_eq!(
            v.pointer("/detail/error").and_then(|x| x.as_str()),
            Some("invalid_target")
        );
        assert_eq!(
            v.pointer("/detail/description").and_then(|x| x.as_str()),
            Some("no such robot")
        );

        let e2 = ManagerError::SigningUnavailable { message: None };
        let v2 = serde_json::to_value(&e2).unwrap();
        assert_eq!(
            v2.get("kind").and_then(|x| x.as_str()),
            Some("signing_unavailable")
        );
    }

    #[tokio::test]
    async fn require_session_errors_when_none() {
        let c = test_manager_client();
        let err = c.require_session().await.unwrap_err();
        assert!(matches!(err, ManagerError::NoSession));
    }

    #[tokio::test]
    async fn restore_session_returns_none_without_persisted_jwt() {
        let c = test_manager_client();
        let restored = c.restore_session(ManagerEnvironment::Beta).await.unwrap();
        assert!(!restored);
        assert!(!c.has_session().await);
    }

    #[tokio::test]
    async fn has_session_reflects_state_transitions() {
        let c = test_manager_client();
        assert!(!c.has_session().await);
        *c.session.write().await = Some(Session {
            generation: 1,
            base_url: "https://x".into(),
            environment: None,
            jwt: "j".into(),
            pending_session_token: None,
        });
        assert!(c.has_session().await);
        // logout() 应清理内存 session（keychain 条目不存在时 delete 会成功 noop）
        c.logout().await.unwrap();
        assert!(!c.has_session().await);
    }

    #[tokio::test]
    async fn stale_unauthorized_response_does_not_clear_a_newer_session() {
        let c = test_manager_client();
        *c.session.write().await = Some(Session {
            generation: 2,
            base_url: "https://new.example.com".into(),
            environment: Some(ManagerEnvironment::Prod),
            jwt: "new-jwt".into(),
            pending_session_token: None,
        });

        assert!(!c.clear_session_for_generation(1).await);

        let session = c.require_session().await.unwrap();
        assert_eq!(session.generation, 2);
        assert_eq!(session.jwt, "new-jwt");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn auth_failure_keychain_delete_is_linearized_before_new_session_publish() {
        let store = Arc::new(BlockingDeleteSecretStore::default());
        let client = Arc::new(ManagerClient::new_with_secret_store(store.clone()));
        let base_url = "https://same-environment.example.com".to_string();
        let key = ManagerClient::keychain_key(&base_url);
        store.set_secret(&key, "old-jwt").unwrap();
        client.session_generation.store(1, Ordering::Release);
        *client.session.write().await = Some(Session {
            generation: 1,
            base_url: base_url.clone(),
            environment: Some(ManagerEnvironment::Staging),
            jwt: "old-jwt".into(),
            pending_session_token: None,
        });
        store.arm_delete();

        let clear = tokio::spawn({
            let client = client.clone();
            async move { client.clear_session_for_generation(1).await }
        });
        tokio::task::spawn_blocking({
            let store = store.clone();
            move || store.wait_until_delete_entered()
        })
        .await
        .unwrap();

        let publish = tokio::spawn({
            let client = client.clone();
            let base_url = base_url.clone();
            async move {
                client
                    .commit_authenticated_session(
                        base_url,
                        Some(ManagerEnvironment::Staging),
                        "new-jwt".into(),
                    )
                    .await
                    .unwrap();
            }
        });
        tokio::task::yield_now().await;
        store.release_delete();
        clear.await.unwrap();
        publish.await.unwrap();

        assert_eq!(store.get_secret(&key).unwrap().as_deref(), Some("new-jwt"));
        assert_eq!(client.require_session().await.unwrap().generation, 2);
    }

    #[test]
    fn strip_trailing_slash_normalizes() {
        assert_eq!(
            strip_trailing_slash("https://x.com/".to_string()),
            "https://x.com"
        );
        assert_eq!(
            strip_trailing_slash("https://x.com".to_string()),
            "https://x.com"
        );
        assert_eq!(
            strip_trailing_slash("https://x.com/path/".to_string()),
            "https://x.com/path"
        );
    }
}
