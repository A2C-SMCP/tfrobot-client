//! Integration tests for `services::manager_client::ManagerClient`.
//!
//! 用轻量 `tokio::net::TcpListener` 模拟 Manager HTTP 接口：
//! - 每次请求读一个 HTTP 请求（到 `\r\n\r\n` + 可选 body），捕获请求行/headers/body；
//! - 按脚本回固定状态码 + body；
//! - 关闭连接让客户端推进。
//!
//! 响应 fixture 统一遵循 TFRSManager 实测契约：
//! - envelope `{code, message, data}`；错误响应同结构
//! - 分页扁平 `{total, page, pageSize, items}`
//! - 登录 data 扁平 `{token, userId, accountId, accountName}`（无嵌套 user）
//! - 多账户 data `{message, tempToken, expiresIn, accounts}`
//!
//! 不依赖 `hyper`/`warp` 等重量级 server，用轻量 `tokio::net::TcpListener` mock。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use tfrobot_client_lib::services::keychain::{InMemorySecretStore, SecretStore};
use tfrobot_client_lib::services::manager_client::{LoginResult, ManagerClient, ManagerError};
use tfrobot_client_lib::services::manager_context::{
    ManagerAuthState, ManagerContextCoordinator, ManagerContextEventSink, ManagerContextKey,
    ManagerContextLifecycleSink, ManagerContextSnapshot,
};
use tfrobot_client_lib::services::manager_environment::ManagerEnvironment;
use tfrobot_client_lib::services::settings::SettingsService;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, Notify};

#[derive(Clone, Debug, Default)]
struct CapturedRequest {
    request_line: String,
    headers: HashMap<String, String>,
    body: String,
}

/// 一段脚本化的响应：按顺序匹配 `path_contains`，回对应 status + body。
#[derive(Clone, Debug)]
struct ScriptedResponse {
    path_contains: &'static str,
    status_line: &'static str, // 例如 "HTTP/1.1 200 OK"
    body: String,
    content_type: &'static str, // 默认 "application/json"
    response_delay: Duration,
}

type Captured = Arc<Mutex<Vec<CapturedRequest>>>;

fn test_manager_client() -> ManagerClient {
    ManagerClient::new_with_secret_store(
        tfrobot_client_lib::services::keychain::InMemorySecretStore::shared(),
    )
}

#[derive(Default)]
struct RecordingContextEvents {
    snapshots: StdMutex<Vec<ManagerContextSnapshot>>,
    auth_expired: AtomicUsize,
}

#[derive(Default)]
struct RecordingLifecycle {
    contexts: StdMutex<Vec<Option<ManagerContextKey>>>,
}

#[async_trait::async_trait]
impl ManagerContextLifecycleSink for RecordingLifecycle {
    async fn cleanup_manager_context(
        &self,
        departing_context: Option<&ManagerContextKey>,
    ) -> Vec<String> {
        self.contexts
            .lock()
            .unwrap()
            .push(departing_context.cloned());
        Vec::new()
    }
}

#[derive(Default)]
struct BlockingFirstLifecycle {
    contexts: StdMutex<Vec<Option<ManagerContextKey>>>,
    entered: Notify,
    release: Notify,
}

#[async_trait::async_trait]
impl ManagerContextLifecycleSink for BlockingFirstLifecycle {
    async fn cleanup_manager_context(
        &self,
        departing_context: Option<&ManagerContextKey>,
    ) -> Vec<String> {
        let should_block = {
            let mut contexts = self.contexts.lock().unwrap();
            contexts.push(departing_context.cloned());
            contexts.len() == 1
        };
        if should_block {
            self.entered.notify_one();
            self.release.notified().await;
        }
        Vec::new()
    }
}

impl ManagerContextEventSink for RecordingContextEvents {
    fn emit_context_changed(&self, snapshot: &ManagerContextSnapshot) -> Result<(), String> {
        self.snapshots.lock().unwrap().push(snapshot.clone());
        Ok(())
    }

    fn emit_auth_expired(&self) -> Result<(), String> {
        self.auth_expired.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

fn test_context_coordinator(
    base_url: String,
    settings: Arc<SettingsService>,
    secret_store: Arc<dyn SecretStore>,
) -> ManagerContextCoordinator {
    ManagerContextCoordinator::new_with_base_url_override(
        Arc::new(ManagerClient::new_with_secret_store(secret_store)),
        settings,
        base_url,
    )
}

/// 启动一个 mock server，按顺序匹配 script 中第一个 `path_contains` 命中的项并回响应。
/// 未命中时回 500。listener 持续接受连接直到 test 结束（通过 JoinHandle drop）。
async fn spawn_mock_manager(
    script: Vec<ScriptedResponse>,
) -> (String, Captured, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let base_url = format!("http://{}", addr);

    let captured: Captured = Arc::new(Mutex::new(Vec::new()));
    let cap_clone = captured.clone();
    let script = Arc::new(Mutex::new(script));

    let handle = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let script = script.clone();
            let cap_for_conn = cap_clone.clone();
            tokio::spawn(async move {
                let mut buf = Vec::with_capacity(8192);
                let mut tmp = [0u8; 2048];
                let mut header_end = None;
                // 读 headers
                loop {
                    match tokio::time::timeout(Duration::from_secs(2), socket.read(&mut tmp)).await
                    {
                        Ok(Ok(0)) => break,
                        Ok(Ok(n)) => {
                            buf.extend_from_slice(&tmp[..n]);
                            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                                header_end = Some(pos + 4);
                                break;
                            }
                        }
                        _ => break,
                    }
                }
                let Some(hend) = header_end else {
                    return;
                };

                let raw_headers = String::from_utf8_lossy(&buf[..hend]).to_string();
                let mut lines = raw_headers.split("\r\n");
                let request_line = lines.next().unwrap_or("").to_string();
                let mut headers = HashMap::new();
                let mut content_length: usize = 0;
                for line in lines {
                    if line.is_empty() {
                        break;
                    }
                    if let Some((k, v)) = line.split_once(':') {
                        let lk = k.trim().to_ascii_lowercase();
                        let v = v.trim().to_string();
                        if lk == "content-length" {
                            content_length = v.parse().unwrap_or(0);
                        }
                        headers.insert(lk, v);
                    }
                }

                // 读 body
                let mut body = buf[hend..].to_vec();
                while body.len() < content_length {
                    let need = content_length - body.len();
                    let mut chunk = vec![0u8; need.min(2048)];
                    match tokio::time::timeout(Duration::from_secs(2), socket.read(&mut chunk))
                        .await
                    {
                        Ok(Ok(0)) => break,
                        Ok(Ok(n)) => body.extend_from_slice(&chunk[..n]),
                        _ => break,
                    }
                }
                let body_str = String::from_utf8_lossy(&body).to_string();

                cap_for_conn.lock().await.push(CapturedRequest {
                    request_line: request_line.clone(),
                    headers,
                    body: body_str,
                });

                // 匹配 script
                let matched = {
                    let mut script = script.lock().await;
                    let matched_index = script
                        .iter()
                        .position(|response| request_line.contains(response.path_contains));
                    matched_index.map(|index| {
                        let matched = script[index].clone();
                        let has_later_match = script[index + 1..]
                            .iter()
                            .any(|response| response.path_contains == matched.path_contains);
                        if has_later_match {
                            script.remove(index)
                        } else {
                            matched
                        }
                    })
                };
                let resp = match matched {
                    Some(r) => {
                        tokio::time::sleep(r.response_delay).await;
                        format!(
                            "{}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            r.status_line,
                            r.content_type,
                            r.body.len(),
                            r.body
                        )
                    }
                    None => "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
                };
                let _ = socket.write_all(resp.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });

    (base_url, captured, handle)
}

fn json_script(
    path: &'static str,
    status: &'static str,
    body: serde_json::Value,
) -> ScriptedResponse {
    ScriptedResponse {
        path_contains: path,
        status_line: status,
        body: body.to_string(),
        content_type: "application/json",
        response_delay: Duration::ZERO,
    }
}

fn raw_script(
    path: &'static str,
    status: &'static str,
    body: &str,
    content_type: &'static str,
) -> ScriptedResponse {
    ScriptedResponse {
        path_contains: path,
        status_line: status,
        body: body.to_string(),
        content_type,
        response_delay: Duration::ZERO,
    }
}

fn delayed_raw_script(
    path: &'static str,
    status: &'static str,
    body: &str,
    delay: Duration,
) -> ScriptedResponse {
    ScriptedResponse {
        path_contains: path,
        status_line: status,
        body: body.to_string(),
        content_type: "application/json",
        response_delay: delay,
    }
}

fn delayed_json_script(
    path: &'static str,
    status: &'static str,
    body: serde_json::Value,
    delay: Duration,
) -> ScriptedResponse {
    ScriptedResponse {
        path_contains: path,
        status_line: status,
        body: body.to_string(),
        content_type: "application/json",
        response_delay: delay,
    }
}

async fn wait_for_captured_request(captured: &Captured, path: &str) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if captured
                .lock()
                .await
                .iter()
                .any(|request| request.request_line.contains(path))
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("scripted request should reach the mock server");
}

/// 标准成功 envelope：`{code:200, message:"success", data}`。
fn envelope(data: serde_json::Value) -> serde_json::Value {
    serde_json::json!({"code": 200, "message": "success", "data": data})
}

#[tokio::test]
#[ignore = "requires TFRS_LIVE_IDENTIFIER and TFRS_LIVE_PASSWORD"]
async fn live_staging_login_contract_completes_the_manager_loop() {
    let identifier = std::env::var("TFRS_LIVE_IDENTIFIER").expect("TFRS_LIVE_IDENTIFIER");
    let password = std::env::var("TFRS_LIVE_PASSWORD").expect("TFRS_LIVE_PASSWORD");
    let client = test_manager_client();

    let login = client
        .login(
            Some(ManagerEnvironment::Staging.base_url().to_string()),
            &identifier,
            &password,
        )
        .await
        .expect("staging login contract");

    match login {
        LoginResult::Authenticated { .. } => {
            let current = client
                .get_current_user()
                .await
                .expect("staging /auth/me contract");
            assert!(!current.account_id.is_empty());
            assert!(!current.organization_id.is_empty());
            client
                .list_digital_employees()
                .await
                .expect("staging authenticated list contract");
        }
        LoginResult::AccountSelectionRequired { accounts } => {
            assert!(!accounts.is_empty(), "at least one Manager account");
            // A list contract mismatch can be account-specific because different accounts expose
            // different generations of robot records. Exercise every selectable account instead
            // of silently covering only the first one.
            for account in accounts {
                let account_client = test_manager_client();
                let refreshed_login = account_client
                    .login(
                        Some(ManagerEnvironment::Staging.base_url().to_string()),
                        &identifier,
                        &password,
                    )
                    .await
                    .expect("refresh staging login before account selection");
                assert!(matches!(
                    refreshed_login,
                    LoginResult::AccountSelectionRequired { .. }
                ));
                account_client
                    .select_account(&account.account_id)
                    .await
                    .expect("staging account selection contract");
                let current = account_client
                    .get_current_user()
                    .await
                    .expect("staging selected-account /auth/me contract");
                assert_eq!(current.account_id, account.account_id);
                assert_eq!(current.organization_id, account.organization_id);
                account_client
                    .list_digital_employees()
                    .await
                    .expect("staging authenticated list contract for selected account");
            }
        }
        LoginResult::OnboardingRequired { .. } => {
            panic!("test account unexpectedly requires onboarding")
        }
    }
}

/// 标准单账户登录 data（与 UAT guide §5.1 实测字面量对齐）。
fn single_account_login_data(token: &str) -> serde_json::Value {
    serde_json::json!({
        "token": token,
        "userId": 9,
        "accountId": 16,
        "accountName": "client_uat",
    })
}

fn current_user_data() -> serde_json::Value {
    serde_json::json!({
        "id": 9,
        "nickname": "Client UAT",
        "email": "client@example.com",
        "phone": "13800000000",
        "accountAvatar": "https://example.com/avatar.png",
        "accountId": 16,
        "accountName": "client_uat",
        "employeeNo": "000016",
        "organizationId": 9,
        "organizationName": "Client UAT Org",
        "organizationType": "enterprise",
        "permissions": ["robot:read"]
    })
}

fn switched_current_user_data() -> serde_json::Value {
    serde_json::json!({
        "id": 9,
        "nickname": "Client UAT",
        "email": "client@example.com",
        "phone": "13800000000",
        "accountAvatar": "https://example.com/switched-avatar.png",
        "accountId": 42,
        "accountName": "client_switched",
        "employeeNo": "000042",
        "organizationId": 84,
        "organizationName": "Switched Organization",
        "organizationType": "enterprise",
        "permissions": ["robot:read", "robot:connect"]
    })
}

// ───────────────────── 用例 ─────────────────────

#[tokio::test]
async fn login_success_writes_session_and_returns_authenticated() {
    let script = vec![json_script(
        "/auth/login-by-password",
        "HTTP/1.1 200 OK",
        envelope(single_account_login_data("jwt-happy-path")),
    )];
    let (base, captured, _h) = spawn_mock_manager(script).await;

    let client = test_manager_client();
    let result = client
        .login(Some(base.clone()), "13800138008", "Test@123456")
        .await
        .expect("login should succeed");

    match result {
        LoginResult::Authenticated { user } => {
            assert_eq!(user.user_id, "9");
            assert_eq!(user.account_id, "16");
            assert_eq!(user.account_name, "client_uat");
        }
        _ => panic!("expected Authenticated variant"),
    }
    assert!(client.has_session().await);

    // 验证请求特征
    let reqs = captured.lock().await;
    assert_eq!(reqs.len(), 1);
    assert!(reqs[0]
        .request_line
        .starts_with("POST /auth/login-by-password"));
    let ua = reqs[0]
        .headers
        .get("user-agent")
        .cloned()
        .unwrap_or_default();
    assert!(
        ua.starts_with("tfrobot-client/"),
        "User-Agent should be prefixed with tfrobot-client/ but was: {ua}"
    );
    assert!(ua.contains("(Tauri;"));
    // 请求体用 `phone` 字段，不是 `username`
    let parsed: serde_json::Value =
        serde_json::from_str(&reqs[0].body).expect("login body should be JSON");
    assert_eq!(parsed["phone"], "13800138008");
    assert_eq!(parsed["password"], "Test@123456");
    assert!(parsed.get("username").is_none());

    // 清理
    let _ = client.logout().await;
}

#[tokio::test]
async fn current_user_returns_complete_redacted_context_identity() {
    let (base, captured, _handle) = spawn_mock_manager(vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("jwt-current-user")),
        ),
        json_script(
            "/api/v1/auth/me",
            "HTTP/1.1 200 OK",
            envelope(current_user_data()),
        ),
    ])
    .await;
    let client = test_manager_client();
    client
        .login(Some(base), "13800000000", "secret")
        .await
        .unwrap();

    let current = client.get_current_user().await.unwrap();
    assert_eq!(current.id, "9");
    assert_eq!(current.account_id, "16");
    assert_eq!(current.organization_id, "9");
    assert_eq!(current.organization_name, "Client UAT Org");
    assert_eq!(current.permissions, vec!["robot:read"]);

    let requests = captured.lock().await;
    let me_request = requests
        .iter()
        .find(|request| request.request_line.contains("/api/v1/auth/me"))
        .unwrap();
    assert_eq!(
        me_request.headers.get("authorization").map(String::as_str),
        Some("Bearer jwt-current-user")
    );
}

#[tokio::test]
async fn context_login_persists_complete_server_identity_and_logout_clears_it() {
    let (base, _, _handle) = spawn_mock_manager(vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("jwt-context-login")),
        ),
        json_script(
            "/api/v1/auth/me",
            "HTTP/1.1 200 OK",
            envelope(current_user_data()),
        ),
    ])
    .await;
    let directory = tempfile::tempdir().unwrap();
    let settings = Arc::new(SettingsService::new(directory.path().to_path_buf()));
    let coordinator =
        test_context_coordinator(base, settings.clone(), InMemorySecretStore::shared());

    coordinator
        .login(ManagerEnvironment::Staging, "client@example.com", "secret")
        .await
        .unwrap();

    let snapshot = coordinator.snapshot().await;
    assert_eq!(snapshot.revision, 1);
    assert_eq!(snapshot.auth_state, ManagerAuthState::Authenticated);
    let key = snapshot.context_key.unwrap();
    assert_eq!(key.environment, ManagerEnvironment::Staging);
    assert_eq!(key.account_id, "16");
    assert_eq!(key.organization_id, "9");
    assert_eq!(snapshot.user.unwrap().id, "9");
    assert_eq!(snapshot.permissions, vec!["robot:read"]);

    let persisted = settings.load_global_manager_session().unwrap();
    let persisted = persisted.session.unwrap();
    assert_eq!(persisted.account_id, "16");
    assert_eq!(persisted.organization_id.as_deref(), Some("9"));
    let persisted_json = serde_json::to_string(&persisted)
        .unwrap()
        .to_ascii_lowercase();
    for forbidden in ["jwt", "password", "temptoken", "access_token"] {
        assert!(!persisted_json.contains(forbidden));
    }

    let lifecycle = Arc::new(RecordingLifecycle::default());
    coordinator.set_lifecycle_sink(lifecycle.clone()).await;
    coordinator.logout().await.unwrap();
    let signed_out = coordinator.snapshot().await;
    assert_eq!(signed_out.revision, 2);
    assert_eq!(signed_out.auth_state, ManagerAuthState::SignedOut);
    assert!(settings
        .load_global_manager_session()
        .unwrap()
        .session
        .is_none());
    assert_eq!(lifecycle.contexts.lock().unwrap().len(), 1);
    assert_eq!(
        lifecycle.contexts.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .account_id,
        "16"
    );
}

#[tokio::test]
async fn manual_login_recovers_from_incomplete_v3_session_metadata() {
    let (base, captured, _handle) = spawn_mock_manager(vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("jwt-recovered-login")),
        ),
        json_script(
            "/api/v1/auth/me",
            "HTTP/1.1 200 OK",
            envelope(current_user_data()),
        ),
    ])
    .await;
    let directory = tempfile::tempdir().unwrap();
    let settings = Arc::new(SettingsService::new(directory.path().to_path_buf()));
    std::fs::create_dir_all(settings.global_manager_session_path().parent().unwrap()).unwrap();
    std::fs::write(
        settings.global_manager_session_path(),
        r#"{
          "schema_version": 3,
          "session": {
            "environment": "staging",
            "userId": "9",
            "accountId": "16",
            "accountName": "client_uat",
            "organizationId": "9",
            "organizationName": "Client UAT Org",
            "organizationType": "enterprise"
          }
        }"#,
    )
    .unwrap();
    let coordinator =
        test_context_coordinator(base, settings.clone(), InMemorySecretStore::shared());

    coordinator
        .login(ManagerEnvironment::Staging, "client@example.com", "secret")
        .await
        .expect("manual login must replace invalid restore-only metadata");

    let requests = captured.lock().await;
    assert_eq!(requests.len(), 2);
    assert!(requests[0]
        .request_line
        .starts_with("POST /auth/login-by-password"));
    assert!(requests[1].request_line.starts_with("GET /api/v1/auth/me"));
    drop(requests);
    assert!(settings
        .load_global_manager_session()
        .unwrap()
        .session
        .unwrap()
        .has_complete_identity());
}

#[tokio::test]
async fn context_lists_accounts_with_current_bearer_without_exposing_credentials() {
    let (base, captured, _handle) = spawn_mock_manager(vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("jwt-account-list")),
        ),
        json_script(
            "/api/v1/auth/me",
            "HTTP/1.1 200 OK",
            envelope(current_user_data()),
        ),
        json_script(
            "/api/v1/accounts/my",
            "HTTP/1.1 200 OK",
            envelope(serde_json::json!({
                "accounts": [{
                    "accountId": 42,
                    "accountName": "client_switched",
                    "nickname": "Switched",
                    "organizationId": 84,
                    "organizationName": "Switched Organization",
                    "organizationType": "enterprise",
                    "role": "owner",
                    "avatar": "https://example.com/switched-avatar.png"
                }]
            })),
        ),
    ])
    .await;
    let directory = tempfile::tempdir().unwrap();
    let coordinator = test_context_coordinator(
        base,
        Arc::new(SettingsService::new(directory.path().to_path_buf())),
        InMemorySecretStore::shared(),
    );
    coordinator
        .login(ManagerEnvironment::Staging, "client@example.com", "secret")
        .await
        .unwrap();

    let accounts = coordinator.list_accounts().await.unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].account_id, "42");
    assert_eq!(accounts[0].organization_id, "84");
    let serialized = serde_json::to_string(&accounts)
        .unwrap()
        .to_ascii_lowercase();
    assert!(!serialized.contains("jwt"));
    assert!(!serialized.contains("token"));

    let requests = captured.lock().await;
    let request = requests
        .iter()
        .find(|request| request.request_line.contains("/api/v1/accounts/my"))
        .unwrap();
    assert_eq!(request.request_line, "GET /api/v1/accounts/my HTTP/1.1");
    assert_eq!(
        request.headers.get("authorization").map(String::as_str),
        Some("Bearer jwt-account-list")
    );
}

#[tokio::test]
async fn context_switch_account_cleans_departing_context_and_publishes_one_final_snapshot() {
    let (base, captured, _handle) = spawn_mock_manager(vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("jwt-before-switch")),
        ),
        json_script(
            "/api/v1/auth/me",
            "HTTP/1.1 200 OK",
            envelope(current_user_data()),
        ),
        json_script(
            "/api/v1/auth/switch-account",
            "HTTP/1.1 200 OK",
            envelope(serde_json::json!({
                "token": "jwt-after-switch",
                "userId": 9,
                "accountId": 42,
                "accountName": "client_switched",
                "organizationId": 84,
                "organizationName": "Switched Organization",
                "organizationType": "enterprise"
            })),
        ),
        json_script(
            "/api/v1/auth/me",
            "HTTP/1.1 200 OK",
            envelope(switched_current_user_data()),
        ),
    ])
    .await;
    let directory = tempfile::tempdir().unwrap();
    let settings = Arc::new(SettingsService::new(directory.path().to_path_buf()));
    let coordinator =
        test_context_coordinator(base, settings.clone(), InMemorySecretStore::shared());
    let events = Arc::new(RecordingContextEvents::default());
    coordinator.set_event_sink(events.clone()).await;
    coordinator
        .login(ManagerEnvironment::Staging, "client@example.com", "secret")
        .await
        .unwrap();
    let lifecycle = Arc::new(RecordingLifecycle::default());
    coordinator.set_lifecycle_sink(lifecycle.clone()).await;

    coordinator.switch_account("42").await.unwrap();

    let snapshot = coordinator.snapshot().await;
    assert_eq!(snapshot.revision, 2);
    assert_eq!(snapshot.auth_state, ManagerAuthState::Authenticated);
    assert_eq!(snapshot.context_key.as_ref().unwrap().account_id, "42");
    assert_eq!(snapshot.context_key.as_ref().unwrap().organization_id, "84");
    assert_eq!(snapshot.account.as_ref().unwrap().name, "client_switched");
    assert_eq!(snapshot.permissions, vec!["robot:read", "robot:connect"]);
    assert_eq!(events.snapshots.lock().unwrap().len(), 2);
    assert_eq!(
        lifecycle.contexts.lock().unwrap().as_slice(),
        &[Some(ManagerContextKey {
            environment: ManagerEnvironment::Staging,
            account_id: "16".to_string(),
            organization_id: "9".to_string(),
        })]
    );
    let persisted = settings
        .load_global_manager_session()
        .unwrap()
        .session
        .unwrap();
    assert_eq!(persisted.account_id, "42");
    assert_eq!(persisted.organization_id.as_deref(), Some("84"));

    let requests = captured.lock().await;
    let switch_request = requests
        .iter()
        .find(|request| request.request_line.contains("/api/v1/auth/switch-account"))
        .unwrap();
    assert_eq!(
        switch_request
            .headers
            .get("authorization")
            .map(String::as_str),
        Some("Bearer jwt-before-switch")
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&switch_request.body).unwrap(),
        serde_json::json!({"accountId": 42})
    );
    let switched_me = requests
        .iter()
        .filter(|request| request.request_line.contains("/api/v1/auth/me"))
        .nth(1)
        .unwrap();
    assert_eq!(
        switched_me.headers.get("authorization").map(String::as_str),
        Some("Bearer jwt-after-switch")
    );
}

#[tokio::test]
async fn context_account_selection_is_an_atomic_revisioned_transition() {
    let (base, _, _handle) = spawn_mock_manager(vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(serde_json::json!({
                "message": "请选择要登录的账户",
                "tempToken": "temporary-secret",
                "expiresIn": 300,
                "accounts": [{
                    "accountId": 16,
                    "accountName": "client_uat",
                    "nickname": "Client UAT",
                    "organizationId": 9,
                    "organizationName": "Client UAT Org",
                    "organizationType": "enterprise"
                }]
            })),
        ),
        json_script(
            "/auth/select-account",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("jwt-selected")),
        ),
        json_script(
            "/api/v1/auth/me",
            "HTTP/1.1 200 OK",
            envelope(current_user_data()),
        ),
    ])
    .await;
    let directory = tempfile::tempdir().unwrap();
    let settings = Arc::new(SettingsService::new(directory.path().to_path_buf()));
    let coordinator =
        test_context_coordinator(base, settings.clone(), InMemorySecretStore::shared());

    coordinator
        .login(ManagerEnvironment::Beta, "13800000000", "secret")
        .await
        .unwrap();
    let selecting = coordinator.snapshot().await;
    assert_eq!(selecting.revision, 1);
    assert_eq!(
        selecting.auth_state,
        ManagerAuthState::AccountSelectionRequired
    );
    assert!(selecting.context_key.is_none());
    assert!(settings
        .load_global_manager_session()
        .unwrap()
        .session
        .is_none());

    coordinator.select_account("16").await.unwrap();
    let authenticated = coordinator.snapshot().await;
    assert_eq!(authenticated.revision, 2);
    assert_eq!(authenticated.auth_state, ManagerAuthState::Authenticated);
    assert_eq!(
        authenticated.context_key.unwrap().environment,
        ManagerEnvironment::Beta
    );
}

#[tokio::test]
async fn context_onboarding_state_never_exposes_an_authenticated_scope() {
    let (base, _, _handle) = spawn_mock_manager(vec![json_script(
        "/auth/login-by-password",
        "HTTP/1.1 200 OK",
        envelope(serde_json::json!({
            "token": "onboarding-only-token",
            "userId": "user-pending",
            "needsOnboarding": true
        })),
    )])
    .await;
    let directory = tempfile::tempdir().unwrap();
    let settings = Arc::new(SettingsService::new(directory.path().to_path_buf()));
    let coordinator =
        test_context_coordinator(base, settings.clone(), InMemorySecretStore::shared());

    coordinator
        .login(ManagerEnvironment::Staging, "client@example.com", "secret")
        .await
        .unwrap();

    let snapshot = coordinator.snapshot().await;
    assert_eq!(snapshot.revision, 1);
    assert_eq!(snapshot.auth_state, ManagerAuthState::OnboardingRequired);
    assert_eq!(snapshot.user.unwrap().id, "user-pending");
    assert!(snapshot.context_key.is_none());
    assert!(snapshot.account.is_none());
    assert!(snapshot.organization.is_none());
    assert!(settings
        .load_global_manager_session()
        .unwrap()
        .session
        .is_none());
}

#[tokio::test]
async fn context_restore_revalidates_keychain_session_with_auth_me() {
    let (base, captured, _handle) = spawn_mock_manager(vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("jwt-restored")),
        ),
        json_script(
            "/api/v1/auth/me",
            "HTTP/1.1 200 OK",
            envelope(current_user_data()),
        ),
    ])
    .await;
    let directory = tempfile::tempdir().unwrap();
    let settings = Arc::new(SettingsService::new(directory.path().to_path_buf()));
    let secrets = InMemorySecretStore::shared();
    let first = test_context_coordinator(base.clone(), settings.clone(), secrets.clone());
    first
        .login(ManagerEnvironment::Prod, "client@example.com", "secret")
        .await
        .unwrap();

    // Simulate metadata written by the previous client release. The keychain JWT remains valid,
    // but organization identity must come from the live server before the context is authoritative.
    std::fs::write(
        settings.global_manager_session_path(),
        r#"{
          "schema_version": 2,
          "session": {
            "environment": "prod",
            "userId": 9,
            "accountId": "stale-account-hint",
            "accountName": "stale-name"
          }
        }"#,
    )
    .unwrap();

    let restored = test_context_coordinator(base, settings.clone(), secrets)
        .restore_session()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(restored.environment, ManagerEnvironment::Prod);
    assert_eq!(restored.user.user_id, "9");
    assert_eq!(restored.user.account_id, "16");
    let upgraded = settings.load_global_manager_session().unwrap();
    assert_eq!(upgraded.schema_version, 3);
    let upgraded = upgraded.session.unwrap();
    assert_eq!(upgraded.user_id, "9");
    assert_eq!(upgraded.user_nickname.as_deref(), Some("Client UAT"));
    assert_eq!(upgraded.user_email.as_deref(), Some("client@example.com"));
    assert_eq!(upgraded.user_phone.as_deref(), Some("13800000000"));
    assert_eq!(upgraded.account_id, "16");
    assert_eq!(upgraded.account_name, "client_uat");
    assert_eq!(upgraded.account_nickname.as_deref(), Some("Client UAT"));
    assert_eq!(
        upgraded.account_avatar.as_deref(),
        Some("https://example.com/avatar.png")
    );
    assert_eq!(upgraded.employee_no.as_deref(), Some("000016"));
    assert_eq!(upgraded.organization_id.as_deref(), Some("9"));
    assert_eq!(
        upgraded.organization_name.as_deref(),
        Some("Client UAT Org")
    );
    assert_eq!(upgraded.organization_type.as_deref(), Some("enterprise"));
    assert_eq!(
        upgraded.permissions.as_deref(),
        Some(["robot:read".to_string()].as_slice())
    );
    assert!(upgraded.has_complete_identity());

    let requests = captured.lock().await;
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.request_line.contains("/api/v1/auth/me"))
            .count(),
        2
    );
}

#[tokio::test]
async fn context_restore_revalidates_incomplete_v3_metadata_with_auth_me() {
    let (base, captured, _handle) = spawn_mock_manager(vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("jwt-incomplete-v3")),
        ),
        json_script(
            "/api/v1/auth/me",
            "HTTP/1.1 200 OK",
            envelope(current_user_data()),
        ),
        json_script(
            "/api/v1/auth/me",
            "HTTP/1.1 200 OK",
            envelope(current_user_data()),
        ),
    ])
    .await;
    let directory = tempfile::tempdir().unwrap();
    let settings = Arc::new(SettingsService::new(directory.path().to_path_buf()));
    let secrets = InMemorySecretStore::shared();
    test_context_coordinator(base.clone(), settings.clone(), secrets.clone())
        .login(ManagerEnvironment::Staging, "client@example.com", "secret")
        .await
        .unwrap();

    std::fs::write(
        settings.global_manager_session_path(),
        r#"{
          "schema_version": 3,
          "session": {
            "environment": "staging",
            "userId": "9",
            "accountId": "16",
            "accountName": "client_uat",
            "organizationId": "9",
            "organizationName": "Client UAT Org",
            "organizationType": "enterprise"
          }
        }"#,
    )
    .unwrap();

    let restored = test_context_coordinator(base, settings.clone(), secrets)
        .restore_session()
        .await
        .expect("incomplete v3 metadata must be revalidated instead of blocking restore")
        .expect("the live keychain session should restore");

    assert_eq!(restored.user.account_id, "16");
    let upgraded = settings.load_global_manager_session().unwrap();
    assert_eq!(upgraded.schema_version, 3);
    assert!(upgraded.session.unwrap().has_complete_identity());
    let requests = captured.lock().await;
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.request_line.contains("/api/v1/auth/me"))
            .count(),
        2
    );
}

#[tokio::test]
async fn transient_restore_validation_failure_preserves_retryable_credentials() {
    let (base, _, server) = spawn_mock_manager(vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("jwt-retryable-restore")),
        ),
        json_script(
            "/api/v1/auth/me",
            "HTTP/1.1 200 OK",
            envelope(current_user_data()),
        ),
    ])
    .await;
    let directory = tempfile::tempdir().unwrap();
    let settings = Arc::new(SettingsService::new(directory.path().to_path_buf()));
    let secrets = InMemorySecretStore::shared();
    test_context_coordinator(base.clone(), settings.clone(), secrets.clone())
        .login(ManagerEnvironment::Staging, "client@example.com", "secret")
        .await
        .unwrap();
    server.abort();

    let first_error = test_context_coordinator(base.clone(), settings.clone(), secrets.clone())
        .restore_session()
        .await
        .unwrap_err();
    assert!(matches!(first_error, ManagerError::NetworkError(_)));
    assert!(settings
        .load_global_manager_session()
        .unwrap()
        .session
        .is_some());

    let retry_error = test_context_coordinator(base, settings, secrets)
        .restore_session()
        .await
        .unwrap_err();
    assert!(matches!(retry_error, ManagerError::NetworkError(_)));
}

#[tokio::test]
async fn context_switch_account_preserves_opaque_public_id_on_the_wire() {
    let (base, captured, _handle) = spawn_mock_manager(vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("jwt-before-opaque-switch")),
        ),
        json_script(
            "/api/v1/auth/me",
            "HTTP/1.1 200 OK",
            envelope(current_user_data()),
        ),
        json_script(
            "/api/v1/auth/switch-account",
            "HTTP/1.1 200 OK",
            envelope(serde_json::json!({
                "token": "jwt-after-opaque-switch",
                "userId": 9,
                "accountId": "acct_public_42",
                "accountName": "client_public",
                "organizationId": "org_public_84",
                "organizationName": "Public Organization",
                "organizationType": "enterprise"
            })),
        ),
        json_script(
            "/api/v1/auth/me",
            "HTTP/1.1 200 OK",
            envelope(serde_json::json!({
                "id": 9,
                "nickname": "Client Public",
                "email": "client@example.com",
                "phone": "13800000000",
                "accountAvatar": "",
                "accountId": "acct_public_42",
                "accountName": "client_public",
                "employeeNo": "000042",
                "organizationId": "org_public_84",
                "organizationName": "Public Organization",
                "organizationType": "enterprise",
                "permissions": ["robot:read"]
            })),
        ),
    ])
    .await;
    let directory = tempfile::tempdir().unwrap();
    let coordinator = test_context_coordinator(
        base,
        Arc::new(SettingsService::new(directory.path().to_path_buf())),
        InMemorySecretStore::shared(),
    );
    coordinator
        .login(ManagerEnvironment::Staging, "client@example.com", "secret")
        .await
        .unwrap();

    coordinator
        .switch_account("acct_public_42")
        .await
        .expect("opaque public account IDs must remain strings on the wire");

    let snapshot = coordinator.snapshot().await;
    assert_eq!(snapshot.context_key.unwrap().account_id, "acct_public_42");
    let requests = captured.lock().await;
    let switch_request = requests
        .iter()
        .find(|request| request.request_line.contains("/api/v1/auth/switch-account"))
        .unwrap();
    let body: serde_json::Value = serde_json::from_str(&switch_request.body).unwrap();
    assert_eq!(body["accountId"], "acct_public_42");
}

#[tokio::test]
async fn context_unauthorized_clears_identity_and_emits_expiry_once() {
    let (base, _, _handle) = spawn_mock_manager(vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("jwt-expiring-context")),
        ),
        json_script(
            "/api/v1/auth/me",
            "HTTP/1.1 200 OK",
            envelope(current_user_data()),
        ),
        raw_script(
            "/api/v1/digital-employees",
            "HTTP/1.1 401 Unauthorized",
            r#"{"code":401,"message":"expired","data":null}"#,
            "application/json",
        ),
    ])
    .await;
    let directory = tempfile::tempdir().unwrap();
    let settings = Arc::new(SettingsService::new(directory.path().to_path_buf()));
    let coordinator =
        test_context_coordinator(base, settings.clone(), InMemorySecretStore::shared());
    let events = Arc::new(RecordingContextEvents::default());
    coordinator.set_event_sink(events.clone()).await;
    coordinator
        .login(ManagerEnvironment::Staging, "client@example.com", "secret")
        .await
        .unwrap();
    let lifecycle = Arc::new(RecordingLifecycle::default());
    coordinator.set_lifecycle_sink(lifecycle.clone()).await;

    let error = coordinator.list_digital_employees().await.unwrap_err();
    assert!(matches!(error, ManagerError::Unauthorized));
    let snapshot = coordinator.snapshot().await;
    assert_eq!(snapshot.revision, 2);
    assert_eq!(snapshot.auth_state, ManagerAuthState::SignedOut);
    assert!(snapshot.context_key.is_none());
    assert_eq!(events.auth_expired.load(Ordering::SeqCst), 1);
    let emitted = events.snapshots.lock().unwrap();
    assert_eq!(emitted.len(), 2);
    assert_eq!(emitted[1], snapshot);
    assert!(settings
        .load_global_manager_session()
        .unwrap()
        .session
        .is_none());
    assert_eq!(lifecycle.contexts.lock().unwrap().len(), 1);
    assert_eq!(
        lifecycle.contexts.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .account_id,
        "16"
    );
}

#[tokio::test]
async fn concurrent_switch_unauthorized_and_logout_serialize_cleanup_without_old_commits() {
    let (base, captured, _handle) = spawn_mock_manager(vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("jwt-concurrent-old")),
        ),
        json_script(
            "/api/v1/auth/me",
            "HTTP/1.1 200 OK",
            envelope(current_user_data()),
        ),
        delayed_raw_script(
            "/api/v1/digital-employees",
            "HTTP/1.1 401 Unauthorized",
            r#"{"code":401,"message":"expired old request","data":null}"#,
            Duration::from_millis(500),
        ),
        json_script(
            "/api/v1/auth/switch-account",
            "HTTP/1.1 200 OK",
            envelope(serde_json::json!({
                "token": "jwt-concurrent-new",
                "userId": 9,
                "accountId": 42,
                "accountName": "client_switched",
                "organizationId": 84,
                "organizationName": "Switched Organization",
                "organizationType": "enterprise"
            })),
        ),
        json_script(
            "/api/v1/auth/me",
            "HTTP/1.1 200 OK",
            envelope(switched_current_user_data()),
        ),
    ])
    .await;
    let directory = tempfile::tempdir().unwrap();
    let coordinator = Arc::new(test_context_coordinator(
        base,
        Arc::new(SettingsService::new(directory.path().to_path_buf())),
        InMemorySecretStore::shared(),
    ));
    let events = Arc::new(RecordingContextEvents::default());
    coordinator.set_event_sink(events.clone()).await;
    coordinator
        .login(ManagerEnvironment::Staging, "client@example.com", "secret")
        .await
        .unwrap();
    let lifecycle = Arc::new(BlockingFirstLifecycle::default());
    coordinator.set_lifecycle_sink(lifecycle.clone()).await;

    let old_unauthorized = tokio::spawn({
        let coordinator = coordinator.clone();
        async move { coordinator.list_digital_employees().await }
    });
    wait_for_captured_request(&captured, "/api/v1/digital-employees").await;

    let switch = tokio::spawn({
        let coordinator = coordinator.clone();
        async move { coordinator.switch_account("42").await }
    });
    tokio::time::timeout(Duration::from_secs(2), lifecycle.entered.notified())
        .await
        .expect("switch should enter the common lifecycle cleanup");

    // The transition owns the identity lock before any connection generation can be captured.
    assert!(tokio::time::timeout(
        Duration::from_millis(50),
        coordinator.capture_authenticated_generation(),
    )
    .await
    .is_err());

    let logout = tokio::spawn({
        let coordinator = coordinator.clone();
        async move { coordinator.logout().await }
    });
    lifecycle.release.notify_one();

    switch.await.unwrap().unwrap();
    logout.await.unwrap().unwrap();
    assert!(matches!(
        old_unauthorized.await.unwrap().unwrap_err(),
        ManagerError::ContextChanged
    ));

    let snapshot = coordinator.snapshot().await;
    assert_eq!(snapshot.auth_state, ManagerAuthState::SignedOut);
    assert_eq!(snapshot.revision, 3);
    assert_eq!(events.auth_expired.load(Ordering::SeqCst), 0);
    let contexts = lifecycle.contexts.lock().unwrap();
    assert_eq!(contexts.len(), 2);
    assert_eq!(contexts[0].as_ref().unwrap().account_id, "16");
    assert_eq!(contexts[1].as_ref().unwrap().account_id, "42");
}

#[tokio::test]
async fn stale_unauthorized_after_relogin_cannot_clear_the_new_context() {
    let (base, captured, _handle) = spawn_mock_manager(vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("jwt-new-generation")),
        ),
        json_script(
            "/api/v1/auth/me",
            "HTTP/1.1 200 OK",
            envelope(current_user_data()),
        ),
        delayed_raw_script(
            "/api/v1/digital-employees",
            "HTTP/1.1 401 Unauthorized",
            r#"{"code":401,"message":"old request expired","data":null}"#,
            Duration::from_millis(200),
        ),
    ])
    .await;
    let directory = tempfile::tempdir().unwrap();
    let settings = Arc::new(SettingsService::new(directory.path().to_path_buf()));
    let coordinator = Arc::new(test_context_coordinator(
        base,
        settings,
        InMemorySecretStore::shared(),
    ));
    let events = Arc::new(RecordingContextEvents::default());
    coordinator.set_event_sink(events.clone()).await;
    coordinator
        .login(ManagerEnvironment::Staging, "client@example.com", "secret")
        .await
        .unwrap();

    let stale_request = tokio::spawn({
        let coordinator = coordinator.clone();
        async move { coordinator.list_digital_employees().await }
    });
    wait_for_captured_request(&captured, "/api/v1/digital-employees").await;
    coordinator
        .login(ManagerEnvironment::Staging, "client@example.com", "secret")
        .await
        .unwrap();
    assert!(matches!(
        stale_request.await.unwrap().unwrap_err(),
        ManagerError::ContextChanged
    ));

    let snapshot = coordinator.snapshot().await;
    assert_eq!(snapshot.auth_state, ManagerAuthState::Authenticated);
    assert_eq!(snapshot.revision, 2);
    assert_eq!(events.auth_expired.load(Ordering::SeqCst), 0);
    let emitted = events.snapshots.lock().unwrap();
    assert_eq!(emitted.len(), 2);
    assert_eq!(emitted[1], snapshot);
}

#[tokio::test]
async fn stale_unauthorized_after_explicit_logout_does_not_emit_auth_expired() {
    let (base, captured, _handle) = spawn_mock_manager(vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("jwt-logged-out")),
        ),
        json_script(
            "/api/v1/auth/me",
            "HTTP/1.1 200 OK",
            envelope(current_user_data()),
        ),
        delayed_raw_script(
            "/api/v1/digital-employees",
            "HTTP/1.1 401 Unauthorized",
            r#"{"code":401,"message":"old request expired","data":null}"#,
            Duration::from_millis(200),
        ),
    ])
    .await;
    let directory = tempfile::tempdir().unwrap();
    let settings = Arc::new(SettingsService::new(directory.path().to_path_buf()));
    let coordinator = Arc::new(test_context_coordinator(
        base,
        settings,
        InMemorySecretStore::shared(),
    ));
    let events = Arc::new(RecordingContextEvents::default());
    coordinator.set_event_sink(events.clone()).await;
    coordinator
        .login(ManagerEnvironment::Staging, "client@example.com", "secret")
        .await
        .unwrap();

    let stale_request = tokio::spawn({
        let coordinator = coordinator.clone();
        async move { coordinator.list_digital_employees().await }
    });
    wait_for_captured_request(&captured, "/api/v1/digital-employees").await;
    coordinator.logout().await.unwrap();
    assert!(matches!(
        stale_request.await.unwrap().unwrap_err(),
        ManagerError::ContextChanged
    ));

    let snapshot = coordinator.snapshot().await;
    assert_eq!(snapshot.auth_state, ManagerAuthState::SignedOut);
    assert_eq!(snapshot.revision, 2);
    assert_eq!(events.auth_expired.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn stale_success_after_relogin_is_rejected_before_reaching_the_caller() {
    let (base, captured, _handle) = spawn_mock_manager(vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("jwt-current")),
        ),
        json_script(
            "/api/v1/auth/me",
            "HTTP/1.1 200 OK",
            envelope(current_user_data()),
        ),
        delayed_json_script(
            "/api/v1/digital-employees",
            "HTTP/1.1 200 OK",
            envelope(serde_json::json!({
                "total": 1,
                "page": 1,
                "pageSize": 20,
                "items": [{"id": 99, "name": "old-account-employee"}]
            })),
            Duration::from_millis(200),
        ),
    ])
    .await;
    let directory = tempfile::tempdir().unwrap();
    let settings = Arc::new(SettingsService::new(directory.path().to_path_buf()));
    let coordinator = Arc::new(test_context_coordinator(
        base,
        settings,
        InMemorySecretStore::shared(),
    ));
    coordinator
        .login(ManagerEnvironment::Staging, "client@example.com", "secret")
        .await
        .unwrap();

    let stale_request = tokio::spawn({
        let coordinator = coordinator.clone();
        async move { coordinator.list_digital_employees().await }
    });
    wait_for_captured_request(&captured, "/api/v1/digital-employees").await;
    coordinator
        .login(ManagerEnvironment::Staging, "client@example.com", "secret")
        .await
        .unwrap();

    assert!(matches!(
        stale_request.await.unwrap().unwrap_err(),
        ManagerError::ContextChanged
    ));
    assert_eq!(
        coordinator.snapshot().await.auth_state,
        ManagerAuthState::Authenticated
    );
}

#[tokio::test]
async fn pinned_composite_generation_rejects_an_account_change_between_steps() {
    let (base, _, _handle) = spawn_mock_manager(vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("jwt-composite")),
        ),
        json_script(
            "/api/v1/auth/me",
            "HTTP/1.1 200 OK",
            envelope(current_user_data()),
        ),
    ])
    .await;
    let directory = tempfile::tempdir().unwrap();
    let settings = Arc::new(SettingsService::new(directory.path().to_path_buf()));
    let coordinator = test_context_coordinator(base, settings, InMemorySecretStore::shared());
    coordinator
        .login(ManagerEnvironment::Staging, "client@example.com", "secret")
        .await
        .unwrap();
    let pinned_generation = coordinator
        .capture_authenticated_generation()
        .await
        .unwrap();

    coordinator
        .login(ManagerEnvironment::Staging, "client@example.com", "secret")
        .await
        .unwrap();

    assert!(matches!(
        coordinator
            .ensure_authenticated_generation(pinned_generation)
            .await
            .unwrap_err(),
        ManagerError::ContextChanged
    ));
    assert!(matches!(
        coordinator
            .list_digital_employees_for_generation(pinned_generation)
            .await
            .unwrap_err(),
        ManagerError::ContextChanged
    ));
}

#[tokio::test]
async fn generation_guarded_commit_excludes_logout_until_local_side_effect_finishes() {
    let (base, _, _handle) = spawn_mock_manager(vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("jwt-guarded-commit")),
        ),
        json_script(
            "/api/v1/auth/me",
            "HTTP/1.1 200 OK",
            envelope(current_user_data()),
        ),
    ])
    .await;
    let directory = tempfile::tempdir().unwrap();
    let settings = Arc::new(SettingsService::new(directory.path().to_path_buf()));
    let coordinator = Arc::new(test_context_coordinator(
        base,
        settings,
        InMemorySecretStore::shared(),
    ));
    coordinator
        .login(ManagerEnvironment::Staging, "client@example.com", "secret")
        .await
        .unwrap();
    let generation = coordinator
        .capture_authenticated_generation()
        .await
        .unwrap();

    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let commit = tokio::spawn({
        let coordinator = coordinator.clone();
        let entered = entered.clone();
        let release = release.clone();
        async move {
            coordinator
                .commit_for_authenticated_generation(generation, || async move {
                    entered.notify_one();
                    release.notified().await;
                    Ok(())
                })
                .await
        }
    });
    entered.notified().await;

    let logout = tokio::spawn({
        let coordinator = coordinator.clone();
        async move { coordinator.logout().await }
    });
    tokio::task::yield_now().await;
    assert!(!logout.is_finished());
    assert_eq!(
        coordinator.snapshot().await.auth_state,
        ManagerAuthState::Authenticated
    );

    release.notify_one();
    commit.await.unwrap().unwrap();
    logout.await.unwrap().unwrap();
    assert_eq!(
        coordinator.snapshot().await.auth_state,
        ManagerAuthState::SignedOut
    );
}

#[tokio::test]
async fn departing_context_cleanup_rejects_an_old_generation_commit_before_side_effects() {
    let (base, _, _handle) = spawn_mock_manager(vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("jwt-cleanup-guard")),
        ),
        json_script(
            "/api/v1/auth/me",
            "HTTP/1.1 200 OK",
            envelope(current_user_data()),
        ),
    ])
    .await;
    let directory = tempfile::tempdir().unwrap();
    let coordinator = Arc::new(test_context_coordinator(
        base,
        Arc::new(SettingsService::new(directory.path().to_path_buf())),
        InMemorySecretStore::shared(),
    ));
    coordinator
        .login(ManagerEnvironment::Staging, "client@example.com", "secret")
        .await
        .unwrap();
    let generation = coordinator
        .capture_authenticated_generation()
        .await
        .unwrap();
    let lifecycle = Arc::new(BlockingFirstLifecycle::default());
    coordinator.set_lifecycle_sink(lifecycle.clone()).await;

    let logout = tokio::spawn({
        let coordinator = coordinator.clone();
        async move { coordinator.logout().await }
    });
    tokio::time::timeout(Duration::from_secs(2), lifecycle.entered.notified())
        .await
        .expect("logout should enter departing-context cleanup");

    let side_effect_ran = Arc::new(AtomicBool::new(false));
    let commit = tokio::spawn({
        let coordinator = coordinator.clone();
        let side_effect_ran = side_effect_ran.clone();
        async move {
            coordinator
                .commit_for_authenticated_generation(generation, || async move {
                    side_effect_ran.store(true, Ordering::SeqCst);
                    Ok(())
                })
                .await
        }
    });
    tokio::task::yield_now().await;
    assert!(!commit.is_finished());
    assert!(!side_effect_ran.load(Ordering::SeqCst));

    lifecycle.release.notify_one();
    logout.await.unwrap().unwrap();
    assert!(matches!(
        commit.await.unwrap().unwrap_err(),
        ManagerError::NoSession | ManagerError::ContextChanged
    ));
    assert!(!side_effect_ran.load(Ordering::SeqCst));
}

#[tokio::test]
async fn login_uses_email_field_for_email_identifier() {
    let script = vec![json_script(
        "/auth/login-by-password",
        "HTTP/1.1 200 OK",
        envelope(single_account_login_data("jwt-email")),
    )];
    let (base, captured, _h) = spawn_mock_manager(script).await;

    let client = test_manager_client();
    client
        .login(Some(base), "user@example.com", "Test@123456")
        .await
        .expect("email login should succeed");

    let requests = captured.lock().await;
    let body: serde_json::Value = serde_json::from_str(&requests[0].body).unwrap();
    assert_eq!(body["email"], "user@example.com");
    assert!(body.get("phone").is_none());
}

#[tokio::test]
async fn invalid_login_credentials_do_not_expire_an_existing_session() {
    let (success_base, _, _success_server) = spawn_mock_manager(vec![json_script(
        "/auth/login-by-password",
        "HTTP/1.1 200 OK",
        envelope(single_account_login_data("jwt-existing")),
    )])
    .await;
    let (invalid_base, _, _invalid_server) = spawn_mock_manager(vec![json_script(
        "/auth/login-by-password",
        "HTTP/1.1 401 Unauthorized",
        serde_json::json!({"code": 401, "message": "invalid credentials", "data": null}),
    )])
    .await;

    let client = test_manager_client();
    client
        .login(Some(success_base), "13800138008", "correct")
        .await
        .unwrap();
    let error = client
        .login(Some(invalid_base), "13800138008", "wrong")
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        ManagerError::InvalidCredentials { ref message } if message == "invalid credentials"
    ));
    assert!(client.has_session().await);
}

#[tokio::test]
async fn login_without_an_account_returns_onboarding_required() {
    let script = vec![json_script(
        "/auth/login-by-password",
        "HTTP/1.1 200 OK",
        envelope(serde_json::json!({
            "token": "onboarding-token",
            "userId": 99,
            "needsOnboarding": true
        })),
    )];
    let (base, _, _h) = spawn_mock_manager(script).await;

    let client = test_manager_client();
    let result = client
        .login(Some(base), "13800138008", "Test@123456")
        .await
        .unwrap();

    assert!(matches!(
        result,
        LoginResult::OnboardingRequired { ref user_id } if user_id == "99"
    ));
    assert!(!client.has_session().await);
}

#[tokio::test]
async fn login_multi_account_returns_account_selection_required() {
    let script = vec![json_script(
        "/auth/login-by-password",
        "HTTP/1.1 200 OK",
        envelope(serde_json::json!({
            "message": "请选择要登录的账户",
            "tempToken": "temp-xyz",
            "expiresIn": 300,
            "accounts": [
                {"accountId": 2, "accountName": "testuser2_enterprise", "nickname": "测试用户2",
                 "organizationId": 2, "organizationName": "测试企业", "organizationType": "enterprise"},
                {"accountId": 3, "accountName": "testuser2_personal", "nickname": "测试用户2",
                 "organizationId": 1, "organizationName": "one-person-org-1", "organizationType": "personal"}
            ]
        })),
    )];
    let (base, _cap, _h) = spawn_mock_manager(script).await;

    let client = test_manager_client();
    let result = client
        .login(Some(base), "13900139000", "Test@123456")
        .await
        .expect("multi-account login should return selection-required");

    match result {
        LoginResult::AccountSelectionRequired { accounts } => {
            assert_eq!(accounts.len(), 2);
            assert_eq!(accounts[0].account_id, "2");
            assert_eq!(accounts[0].organization_id, "2");
            assert_eq!(accounts[0].account_name, "testuser2_enterprise");
            assert_eq!(accounts[0].organization_type, "enterprise");
            assert_eq!(accounts[1].organization_type, "personal");
        }
        _ => panic!("expected AccountSelectionRequired"),
    }
}

#[tokio::test]
async fn select_account_completes_session() {
    let script = vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(serde_json::json!({
                "message": "请选择要登录的账户",
                "tempToken": "temp-xyz",
                "expiresIn": 300,
                "accounts": [
                    {"accountId": 2, "accountName": "testuser2_enterprise", "nickname": "n",
                     "organizationId": 2, "organizationName": "ent", "organizationType": "enterprise"}
                ]
            })),
        ),
        json_script(
            "/auth/select-account",
            "HTTP/1.1 200 OK",
            envelope(serde_json::json!({
                "token": "final-jwt",
                "userId": 2,
                "accountId": 2,
                "accountName": "testuser2_enterprise"
            })),
        ),
    ];
    let (base, captured, _h) = spawn_mock_manager(script).await;

    let client = test_manager_client();
    client
        .login(Some(base), "13900139000", "Test@123456")
        .await
        .unwrap();
    let user = client.select_account("2").await.expect("select-account");
    assert_eq!(user.account_id, "2");
    assert_eq!(user.account_name, "testuser2_enterprise");

    // 第二个请求 body 遵循 Manager numeric transport contract；公开 DTO 仍使用 opaque string。
    let reqs = captured.lock().await;
    assert_eq!(reqs.len(), 2);
    let select_body: serde_json::Value = serde_json::from_str(&reqs[1].body).unwrap();
    assert_eq!(select_body["tempToken"], "temp-xyz");
    assert_eq!(select_body["accountId"], 2);
    // sessionToken 不应再出现
    assert!(select_body.get("sessionToken").is_none());
}

#[tokio::test]
async fn select_account_preserves_opaque_public_id_on_the_wire() {
    let script = vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(serde_json::json!({
                "message": "请选择要登录的账户",
                "tempToken": "temp-public",
                "expiresIn": 300,
                "accounts": [{
                    "accountId": "acct_public_02",
                    "accountName": "public_enterprise",
                    "organizationId": "org_public_02",
                    "organizationName": "Public Enterprise",
                    "organizationType": "enterprise"
                }]
            })),
        ),
        json_script(
            "/auth/select-account",
            "HTTP/1.1 200 OK",
            envelope(serde_json::json!({
                "token": "final-public-jwt",
                "userId": 2,
                "accountId": "acct_public_02",
                "accountName": "public_enterprise"
            })),
        ),
    ];
    let (base, captured, _handle) = spawn_mock_manager(script).await;
    let client = test_manager_client();
    client
        .login(Some(base), "13900139000", "Test@123456")
        .await
        .unwrap();

    let user = client
        .select_account("acct_public_02")
        .await
        .expect("opaque public account IDs must be supported during account selection");

    assert_eq!(user.account_id, "acct_public_02");
    let requests = captured.lock().await;
    let body: serde_json::Value = serde_json::from_str(&requests[1].body).unwrap();
    assert_eq!(body["tempToken"], "temp-public");
    assert_eq!(body["accountId"], "acct_public_02");
}

#[tokio::test]
async fn select_account_without_pending_session_errors() {
    let client = test_manager_client();
    let err = client.select_account("1").await.unwrap_err();
    assert!(matches!(err, ManagerError::NoSession));
}

#[tokio::test]
async fn list_digital_employees_sends_bearer_and_parses_paginated_envelope() {
    let script = vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("jwt-xyz")),
        ),
        json_script(
            "/api/v1/digital-employees",
            "HTTP/1.1 200 OK",
            envelope(serde_json::json!({
                "total": 2, "page": 1, "pageSize": 20,
                "items": [
                    {"id": 11, "name": "本地联调员工", "robotId": "r-1",
                     "status": "running", "templateType": "tfrserver", "templateDisplayName": "智能客服",
                     "namespace": "tfrobotserver", "clusterName": "local-tfrobotserver",
                     "robotAccountId": "org-legacy-18:account-24"},
                    {"id": 12, "name": "robot-2"}
                ]
            })),
        ),
    ];
    let (base, captured, _h) = spawn_mock_manager(script).await;

    let client = test_manager_client();
    client
        .login(Some(base), "13800138008", "Test@123456")
        .await
        .unwrap();
    let list = client.list_digital_employees().await.expect("list");
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].id, 11);
    assert_eq!(list[0].template_type.as_deref(), Some("tfrserver"));
    assert_eq!(list[0].status.as_deref(), Some("running"));
    assert_eq!(list[0].cluster_name.as_deref(), Some("local-tfrobotserver"));
    assert_eq!(
        list[0].robot_account_id.as_deref(),
        Some("org-legacy-18:account-24")
    );

    // Bearer token 应当在第二个请求（list）里
    let reqs = captured.lock().await;
    let list_req = reqs
        .iter()
        .find(|r| r.request_line.contains("/api/v1/digital-employees"))
        .unwrap();
    assert_eq!(
        list_req.headers.get("authorization").map(String::as_str),
        Some("Bearer jwt-xyz")
    );

    let _ = client.logout().await;
}

#[tokio::test]
async fn list_digital_employees_tolerates_empty_items() {
    let script = vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("jwt-empty")),
        ),
        json_script(
            "/api/v1/digital-employees",
            "HTTP/1.1 200 OK",
            envelope(serde_json::json!({
                "total": 0, "page": 1, "pageSize": 20, "items": []
            })),
        ),
    ];
    let (base, _cap, _h) = spawn_mock_manager(script).await;

    let client = test_manager_client();
    client
        .login(Some(base), "13800138009", "Test@123456")
        .await
        .unwrap();
    let list = client.list_digital_employees().await.expect("list");
    assert!(list.is_empty());
}

#[tokio::test]
async fn connection_info_returns_full_dto() {
    let script = vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("jwt-ok")),
        ),
        json_script(
            "/connection-info",
            "HTTP/1.1 200 OK",
            envelope(serde_json::json!({
                "socketBaseURL": "https://127.0.0.1:8443",
                "sioPath": "/socket.io/",
                "namespace": "tfrobotserver",
                "rid": "5f4b3b3b-3b3b-3b3b-3b3",
                "robotType": "tfrobot",
                "smcpNamespace": "/smcp",
                "computerName": "本地联调员工",
                "routingHeaders": {
                    "X-TF-Namespace": "tfrobotserver",
                    "X-TF-RobotId": "5f4b3b3b-3b3b-3b3b-3b3",
                    "X-TF-RobotType": "tfrobot"
                }
            })),
        ),
    ];
    let (base, _cap, _h) = spawn_mock_manager(script).await;

    let client = test_manager_client();
    client
        .login(Some(base), "13800138008", "Test@123456")
        .await
        .unwrap();
    let info = client.get_connection_info(11).await.expect("info");
    assert_eq!(info.socket_base_url, "https://127.0.0.1:8443");
    assert_eq!(info.rid.as_deref(), Some("5f4b3b3b-3b3b-3b3b-3b3"));
    assert_eq!(info.computer_name.as_deref(), Some("本地联调员工"));
    // M6（TFRM-161）后 connection-info 仅返纯路由头（X-TF-*），不再下发鉴权令牌（TFRC-20/auth-dict）。
    assert_eq!(info.routing_headers.len(), 3);
    assert!(!info.routing_headers.contains_key("access_token"));
    assert_eq!(
        info.routing_headers.get("X-TF-RobotId").map(String::as_str),
        Some("5f4b3b3b-3b3b-3b3b-3b3")
    );
    let _ = client.logout().await;
}

#[tokio::test]
async fn transport_unauthorized_does_not_mutate_session_before_coordinator_settlement() {
    let script = vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("jwt-will-expire")),
        ),
        raw_script(
            "/api/v1/digital-employees",
            "HTTP/1.1 401 Unauthorized",
            r#"{"code":401,"message":"token expired","data":null}"#,
            "application/json",
        ),
    ];
    let (base, _cap, _h) = spawn_mock_manager(script).await;

    let client = test_manager_client();
    client
        .login(Some(base), "13800138008", "Test@123456")
        .await
        .unwrap();
    assert!(client.has_session().await);

    let err = client.list_digital_employees().await.unwrap_err();
    assert!(matches!(err, ManagerError::Unauthorized));
    // Simulates cancellation after the response is classified but before coordinator settlement:
    // transport classification must not create a half-transition in session/keychain state.
    assert!(client.has_session().await);
}

#[tokio::test]
async fn payment_required_parses_redirect_url_from_envelope() {
    let script = vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("jwt-ok")),
        ),
        raw_script(
            "/connection-info",
            "HTTP/1.1 402 Payment Required",
            r#"{"code":402,"message":"账号已欠费","data":{"redirectUrl":"https://pay.example.com/renew"}}"#,
            "application/json",
        ),
    ];
    let (base, _cap, _h) = spawn_mock_manager(script).await;

    let client = test_manager_client();
    client
        .login(Some(base), "13800138008", "Test@123456")
        .await
        .unwrap();
    let err = client.get_connection_info(11).await.unwrap_err();
    match err {
        ManagerError::PaymentRequired {
            message,
            redirect_url,
        } => {
            assert_eq!(message, "账号已欠费");
            assert_eq!(
                redirect_url.as_deref(),
                Some("https://pay.example.com/renew")
            );
        }
        other => panic!("expected PaymentRequired, got {other:?}"),
    }
    // 402 不是鉴权问题 — session 应保留
    assert!(client.has_session().await);
}

#[tokio::test]
async fn payment_required_without_redirect_url_field_still_parses() {
    let script = vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("jwt-ok")),
        ),
        raw_script(
            "/connection-info",
            "HTTP/1.1 402 Payment Required",
            r#"{"code":402,"message":"欠费","data":null}"#,
            "application/json",
        ),
    ];
    let (base, _cap, _h) = spawn_mock_manager(script).await;

    let client = test_manager_client();
    client
        .login(Some(base), "13800138008", "Test@123456")
        .await
        .unwrap();
    let err = client.get_connection_info(11).await.unwrap_err();
    match err {
        ManagerError::PaymentRequired {
            message,
            redirect_url,
        } => {
            assert_eq!(message, "欠费");
            assert!(redirect_url.is_none());
        }
        other => panic!("expected PaymentRequired, got {other:?}"),
    }
}

#[tokio::test]
async fn not_found_returned_for_missing_robot() {
    let script = vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("jwt-ok")),
        ),
        raw_script(
            "/connection-info",
            "HTTP/1.1 404 Not Found",
            r#"{"code":404,"message":"not found","data":null}"#,
            "application/json",
        ),
    ];
    let (base, _cap, _h) = spawn_mock_manager(script).await;

    let client = test_manager_client();
    client
        .login(Some(base), "13800138008", "Test@123456")
        .await
        .unwrap();
    let err = client.get_connection_info(99999).await.unwrap_err();
    assert!(matches!(err, ManagerError::NotFound));
}

#[tokio::test]
async fn visibility_revoked_404_maps_to_not_found_or_no_permission() {
    // TFRM-167：不可见/权限回收的资源返回 404 + 顶层 errorCode；数字 code/message 不变。
    let script = vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("jwt-ok")),
        ),
        raw_script(
            "/connection-info",
            "HTTP/1.1 404 Not Found",
            r#"{"code":404,"message":"not found","errorCode":"ERR_NOT_FOUND_OR_NO_PERMISSION","data":null}"#,
            "application/json",
        ),
    ];
    let (base, _cap, _h) = spawn_mock_manager(script).await;

    let client = test_manager_client();
    client
        .login(Some(base), "13800138008", "Test@123456")
        .await
        .unwrap();
    let err = client.get_connection_info(11).await.unwrap_err();
    assert!(matches!(err, ManagerError::NotFoundOrNoPermission));
    // 不是鉴权问题 — session 应保留（前端只剔除该项 + refetch）
    assert!(client.has_session().await);
}

#[tokio::test]
async fn forbidden_mapped_from_403() {
    let script = vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("jwt-ok")),
        ),
        raw_script(
            "/api/v1/digital-employees",
            "HTTP/1.1 403 Forbidden",
            r#"{"code":403,"message":"no","data":null}"#,
            "application/json",
        ),
    ];
    let (base, _cap, _h) = spawn_mock_manager(script).await;

    let client = test_manager_client();
    client
        .login(Some(base), "13800138008", "Test@123456")
        .await
        .unwrap();
    let err = client.list_digital_employees().await.unwrap_err();
    assert!(matches!(err, ManagerError::Forbidden));
    // 403 不清 session
    assert!(client.has_session().await);
}

#[tokio::test]
async fn other_status_bucket_redacts_the_upstream_body() {
    let script = vec![raw_script(
        "/auth/login-by-password",
        "HTTP/1.1 500 Internal Server Error",
        "db down",
        "text/plain",
    )];
    let (base, _cap, _h) = spawn_mock_manager(script).await;

    let client = test_manager_client();
    let err = client.login(Some(base), "a", "b").await.unwrap_err();
    match err {
        ManagerError::Other { status, body } => {
            assert_eq!(status, 500);
            assert_eq!(body, "Unexpected Manager response");
        }
        other => panic!("expected Other, got {other:?}"),
    }
}

#[tokio::test]
async fn login_errors_when_internal_caller_does_not_resolve_an_environment() {
    let client = test_manager_client();
    let err = client.login(None, "a", "b").await.unwrap_err();
    assert!(matches!(err, ManagerError::MissingBaseUrl));
}

#[tokio::test]
async fn list_without_login_errors_no_session() {
    let client = test_manager_client();
    let err = client.list_digital_employees().await.unwrap_err();
    assert!(matches!(err, ManagerError::NoSession));
}

#[tokio::test]
async fn connection_info_without_login_errors_no_session() {
    let client = test_manager_client();
    let err = client.get_connection_info(1).await.unwrap_err();
    assert!(matches!(err, ManagerError::NoSession));
}

#[tokio::test]
async fn logout_without_session_is_noop() {
    let client = test_manager_client();
    client.logout().await.expect("logout should be idempotent");
    assert!(!client.has_session().await);
}

#[tokio::test]
async fn network_error_when_server_unreachable() {
    // 绑定一个随机端口，立刻 drop listener 让端口释放 / 拒绝连接
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let unreachable = format!("http://{}", addr);

    let client = test_manager_client();
    let err = client.login(Some(unreachable), "a", "b").await.unwrap_err();
    // reqwest 可能返回 NetworkError（connect refused）或 InvalidResponse（若端口碰巧被复用），
    // 但在本地 race 下绝大概率是 NetworkError。放宽断言，不接受非错误。
    match err {
        ManagerError::NetworkError(_) | ManagerError::Other { .. } => {}
        ManagerError::InvalidResponse(_) => {}
        other => panic!("unexpected error variant: {other:?}"),
    }
}

// ───────────────────── token-exchange (TFRC-11 / C1) ─────────────────────

#[tokio::test]
async fn exchange_token_posts_form_and_parses_oauth_response() {
    let script = vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("user-jwt-xyz")),
        ),
        json_script(
            "/api/v1/oauth/token",
            "HTTP/1.1 200 OK",
            serde_json::json!({
                "access_token": "short-robot-jwt",
                "issued_token_type": "urn:ietf:params:oauth:token-type:access_token",
                "token_type": "Bearer",
                "expires_in": 300,
                "scope": "smcp:connect"
            }),
        ),
    ];
    let (base, captured, _h) = spawn_mock_manager(script).await;

    let client = test_manager_client();
    client
        .login(Some(base), "13800138008", "Test@123456")
        .await
        .unwrap();

    let tok = client.exchange_token("robot-acct-1", None).await.unwrap();
    assert_eq!(tok.access_token, "short-robot-jwt");
    assert_eq!(tok.token_type, "Bearer");
    assert_eq!(tok.expires_in, 300);
    assert_eq!(tok.scope.as_deref(), Some("smcp:connect"));

    // 校验 wire 请求：POST /api/v1/oauth/token + form-urlencoded 字段（RFC 8693）。
    let reqs = captured.lock().await.clone();
    let xchg = reqs
        .iter()
        .find(|r| r.request_line.contains("/api/v1/oauth/token"))
        .expect("token-exchange request should be captured");
    assert!(
        xchg.request_line.starts_with("POST "),
        "should be POST: {}",
        xchg.request_line
    );
    assert_eq!(
        xchg.headers.get("content-type").map(String::as_str),
        Some("application/x-www-form-urlencoded")
    );
    assert!(
        xchg.body
            .contains("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Atoken-exchange"),
        "body: {}",
        xchg.body
    );
    assert!(
        xchg.body.contains("subject_token=user-jwt-xyz"),
        "subject_token must be the session User JWT. body: {}",
        xchg.body
    );
    assert!(
        xchg.body
            .contains("subject_token_type=urn%3Aietf%3Aparams%3Aoauth%3Atoken-type%3Ajwt"),
        "body: {}",
        xchg.body
    );
    assert!(
        xchg.body.contains("audience=robot%3Arobot-acct-1"),
        "audience must be robot:<id>. body: {}",
        xchg.body
    );
    assert!(
        !xchg.body.contains("scope="),
        "scope must be omitted when None. body: {}",
        xchg.body
    );
}

#[tokio::test]
async fn exchange_token_sends_scope_when_present() {
    let script = vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("user-jwt")),
        ),
        json_script(
            "/api/v1/oauth/token",
            "HTTP/1.1 200 OK",
            serde_json::json!({"access_token": "t", "token_type": "Bearer", "expires_in": 300}),
        ),
    ];
    let (base, captured, _h) = spawn_mock_manager(script).await;

    let client = test_manager_client();
    client.login(Some(base), "p", "w").await.unwrap();
    let _ = client
        .exchange_token("r1", Some("smcp:connect tools:call".to_string()))
        .await
        .unwrap();

    let reqs = captured.lock().await.clone();
    let xchg = reqs
        .iter()
        .find(|r| r.request_line.contains("/api/v1/oauth/token"))
        .unwrap();
    // 空格分隔的 scope 在 form-urlencoded 里编码为 `+`。
    assert!(
        xchg.body.contains("scope=smcp%3Aconnect+tools%3Acall"),
        "body: {}",
        xchg.body
    );
}

#[tokio::test]
async fn exchange_token_maps_400_to_token_exchange_error() {
    let script = vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("user-jwt")),
        ),
        json_script(
            "/api/v1/oauth/token",
            "HTTP/1.1 400 Bad Request",
            serde_json::json!({"error": "invalid_grant", "error_description": "subject token revoked"}),
        ),
    ];
    let (base, _cap, _h) = spawn_mock_manager(script).await;

    let client = test_manager_client();
    client.login(Some(base), "p", "w").await.unwrap();
    let err = client.exchange_token("r1", None).await.unwrap_err();
    match err {
        ManagerError::TokenExchange { error, description } => {
            assert_eq!(error, "invalid_grant");
            assert_eq!(description.as_deref(), Some("subject token revoked"));
        }
        other => panic!("expected TokenExchange, got {other:?}"),
    }
    // token-exchange 的 400 不是 session 鉴权失败 — session 应保留。
    assert!(client.has_session().await);
}

#[tokio::test]
async fn exchange_token_maps_503_to_signing_unavailable() {
    let script = vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            envelope(single_account_login_data("user-jwt")),
        ),
        json_script(
            "/api/v1/oauth/token",
            "HTTP/1.1 503 Service Unavailable",
            serde_json::json!({"error": "temporarily_unavailable", "error_description": "signing keys not provisioned"}),
        ),
    ];
    let (base, _cap, _h) = spawn_mock_manager(script).await;

    let client = test_manager_client();
    client.login(Some(base), "p", "w").await.unwrap();
    let err = client.exchange_token("r1", None).await.unwrap_err();
    match err {
        ManagerError::SigningUnavailable { message } => {
            assert_eq!(message.as_deref(), Some("signing keys not provisioned"));
        }
        other => panic!("expected SigningUnavailable, got {other:?}"),
    }
    assert!(client.has_session().await);
}

#[tokio::test]
async fn exchange_token_without_login_errors_no_session() {
    let client = test_manager_client();
    let err = client.exchange_token("r1", None).await.unwrap_err();
    assert!(matches!(err, ManagerError::NoSession));
}
