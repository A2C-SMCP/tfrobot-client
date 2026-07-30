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
use std::sync::Arc;
use std::time::Duration;

use tfrobot_client_lib::services::manager_client::{LoginResult, ManagerClient, ManagerError};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

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
}

type Captured = Arc<Mutex<Vec<CapturedRequest>>>;

fn test_manager_client() -> ManagerClient {
    ManagerClient::new_with_secret_store(
        tfrobot_client_lib::services::keychain::InMemorySecretStore::shared(),
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
                let matched = script
                    .iter()
                    .find(|r| request_line.contains(r.path_contains))
                    .cloned();
                let resp = match matched {
                    Some(r) => format!(
                        "{}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        r.status_line,
                        r.content_type,
                        r.body.len(),
                        r.body
                    ),
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
    }
}

/// 标准成功 envelope：`{code:200, message:"success", data}`。
fn envelope(data: serde_json::Value) -> serde_json::Value {
    serde_json::json!({"code": 200, "message": "success", "data": data})
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
            assert_eq!(user.user_id, 9);
            assert_eq!(user.account_id, 16);
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
            assert_eq!(accounts[0].account_id, 2);
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
    let user = client.select_account(2).await.expect("select-account");
    assert_eq!(user.account_id, 2);
    assert_eq!(user.account_name, "testuser2_enterprise");

    // 第二个请求 body 应当是 tempToken + accountId (number)
    let reqs = captured.lock().await;
    assert_eq!(reqs.len(), 2);
    let select_body: serde_json::Value = serde_json::from_str(&reqs[1].body).unwrap();
    assert_eq!(select_body["tempToken"], "temp-xyz");
    assert_eq!(select_body["accountId"], 2);
    // sessionToken 不应再出现
    assert!(select_body.get("sessionToken").is_none());
}

#[tokio::test]
async fn select_account_without_pending_session_errors() {
    let client = test_manager_client();
    let err = client.select_account(1).await.unwrap_err();
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
                     "robot_account_id": 4242},
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
    assert_eq!(list[0].robot_account_id, Some(4242));

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
async fn unauthorized_on_authed_request_clears_session_and_returns_unauthorized() {
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
    // 401 后内存 session 必须被清理
    assert!(!client.has_session().await);
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
async fn other_status_bucket_captures_body() {
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
            assert_eq!(body, "db down");
        }
        other => panic!("expected Other, got {other:?}"),
    }
}

#[tokio::test]
async fn login_errors_when_base_url_missing_and_env_unset() {
    std::env::remove_var("TFRS_MANAGER_BASE_URL");
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
