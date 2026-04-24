//! Integration tests for `services::manager_client::ManagerClient`.
//!
//! 用轻量 `tokio::net::TcpListener` 模拟 Manager HTTP 接口：
//! - 每次请求读一个 HTTP 请求（到 `\r\n\r\n` + 可选 body），捕获请求行/headers/body；
//! - 按脚本回固定状态码 + body；
//! - 关闭连接让客户端推进。
//!
//! 不依赖 `hyper`/`warp` 等重量级 server，和已有的 `smcp_handshake_config_test.rs` 风格一致。

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
                            if let Some(pos) = buf
                                .windows(4)
                                .position(|w| w == b"\r\n\r\n")
                            {
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
                    match tokio::time::timeout(
                        Duration::from_secs(2),
                        socket.read(&mut chunk),
                    )
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

fn json_script(path: &'static str, status: &'static str, body: serde_json::Value) -> ScriptedResponse {
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

// ───────────────────── 用例 ─────────────────────

#[tokio::test]
async fn login_success_writes_session_and_returns_authenticated() {
    let script = vec![json_script(
        "/auth/login-by-password",
        "HTTP/1.1 200 OK",
        serde_json::json!({
            "token": "jwt-happy-path",
            "user": {"id": "u1", "username": "alice", "displayName": "Alice"}
        }),
    )];
    let (base, captured, _h) = spawn_mock_manager(script).await;

    let client = ManagerClient::new();
    let result = client
        .login(Some(base.clone()), "alice", "hunter2")
        .await
        .expect("login should succeed");

    match result {
        LoginResult::Authenticated { user } => {
            assert_eq!(user.username, "alice");
            assert_eq!(user.display_name.as_deref(), Some("Alice"));
        }
        _ => panic!("expected Authenticated variant"),
    }
    assert!(client.has_session().await);

    // 验证请求特征
    let reqs = captured.lock().await;
    assert_eq!(reqs.len(), 1);
    assert!(reqs[0].request_line.starts_with("POST /auth/login-by-password"));
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
    let parsed: serde_json::Value =
        serde_json::from_str(&reqs[0].body).expect("login body should be JSON");
    assert_eq!(parsed["username"], "alice");
    assert_eq!(parsed["password"], "hunter2");

    // 清理
    let _ = client.logout().await;
}

#[tokio::test]
async fn login_multi_account_returns_account_selection_required() {
    let script = vec![json_script(
        "/auth/login-by-password",
        "HTTP/1.1 200 OK",
        serde_json::json!({
            "sessionToken": "pending-xyz",
            "accounts": [
                {"id": "a1", "name": "Primary"},
                {"id": "a2", "name": "Tenant-B", "role": "admin"}
            ]
        }),
    )];
    let (base, _cap, _h) = spawn_mock_manager(script).await;

    let client = ManagerClient::new();
    let result = client
        .login(Some(base), "bob", "pw")
        .await
        .expect("multi-account login should return selection-required");

    match result {
        LoginResult::AccountSelectionRequired { accounts } => {
            assert_eq!(accounts.len(), 2);
            assert_eq!(accounts[0].id, "a1");
            assert_eq!(accounts[1].role.as_deref(), Some("admin"));
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
            serde_json::json!({
                "sessionToken": "pending-xyz",
                "accounts": [{"id": "a1", "name": "Primary"}]
            }),
        ),
        json_script(
            "/auth/select-account",
            "HTTP/1.1 200 OK",
            serde_json::json!({
                "token": "final-jwt",
                "user": {"id": "u1", "username": "bob"}
            }),
        ),
    ];
    let (base, captured, _h) = spawn_mock_manager(script).await;

    let client = ManagerClient::new();
    client.login(Some(base), "bob", "pw").await.unwrap();
    let user = client.select_account("a1").await.expect("select-account");
    assert_eq!(user.username, "bob");

    // 第二个请求 body 应当是 sessionToken + accountId
    let reqs = captured.lock().await;
    assert_eq!(reqs.len(), 2);
    let select_body: serde_json::Value = serde_json::from_str(&reqs[1].body).unwrap();
    assert_eq!(select_body["sessionToken"], "pending-xyz");
    assert_eq!(select_body["accountId"], "a1");
}

#[tokio::test]
async fn select_account_without_pending_session_errors() {
    let client = ManagerClient::new();
    let err = client.select_account("a1").await.unwrap_err();
    assert!(matches!(err, ManagerError::NoSession));
}

#[tokio::test]
async fn list_digital_employees_sends_bearer_and_parses_response() {
    let script = vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            serde_json::json!({
                "token": "jwt-xyz",
                "user": {"id": "u1", "username": "alice"}
            }),
        ),
        json_script(
            "/api/v1/digital-employees",
            "HTTP/1.1 200 OK",
            serde_json::json!([
                {"id": "r1", "name": "Robot-1", "robotId": "r1", "templateType": "tfrobot", "status": "running"},
                {"id": "r2", "name": "Robot-2"}
            ]),
        ),
    ];
    let (base, captured, _h) = spawn_mock_manager(script).await;

    let client = ManagerClient::new();
    client.login(Some(base), "alice", "pw").await.unwrap();
    let list = client.list_digital_employees().await.expect("list");
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].template_type.as_deref(), Some("tfrobot"));
    assert_eq!(list[0].status.as_deref(), Some("running"));

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
async fn connection_info_returns_full_dto() {
    let script = vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            serde_json::json!({
                "token": "jwt-ok",
                "user": {"id": "u1", "username": "alice"}
            }),
        ),
        json_script(
            "/connection-info",
            "HTTP/1.1 200 OK",
            serde_json::json!({
                "socketBaseURL": "https://tfr.example.com",
                "sioPath": "/socket.io/",
                "namespace": "tenant-a",
                "rid": "robot-42",
                "robotType": "tfrobot",
                "smcpNamespace": "/smcp",
                "accessToken": "admin-secret",
                "computerName": "computer-x",
                "routingHeaders": {
                    "X-TF-Namespace": "tenant-a",
                    "X-TF-RobotId": "robot-42",
                    "X-TF-RobotType": "tfrobot",
                    "access_token": "admin-secret"
                },
                "expiresAt": "2026-04-23T12:00:00Z"
            }),
        ),
    ];
    let (base, _cap, _h) = spawn_mock_manager(script).await;

    let client = ManagerClient::new();
    client.login(Some(base), "alice", "pw").await.unwrap();
    let info = client.get_connection_info("robot-42").await.expect("info");
    assert_eq!(info.socket_base_url, "https://tfr.example.com");
    assert_eq!(info.rid.as_deref(), Some("robot-42"));
    assert_eq!(info.access_token, "admin-secret");
    assert_eq!(info.routing_headers.len(), 4);
    assert_eq!(
        info.routing_headers.get("access_token").map(String::as_str),
        Some("admin-secret")
    );
    let _ = client.logout().await;
}

#[tokio::test]
async fn unauthorized_on_authed_request_clears_session_and_returns_unauthorized() {
    let script = vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            serde_json::json!({
                "token": "jwt-will-expire",
                "user": {"id": "u1", "username": "alice"}
            }),
        ),
        raw_script(
            "/api/v1/digital-employees",
            "HTTP/1.1 401 Unauthorized",
            r#"{"message":"token expired"}"#,
            "application/json",
        ),
    ];
    let (base, _cap, _h) = spawn_mock_manager(script).await;

    let client = ManagerClient::new();
    client.login(Some(base), "alice", "pw").await.unwrap();
    assert!(client.has_session().await);

    let err = client.list_digital_employees().await.unwrap_err();
    assert!(matches!(err, ManagerError::Unauthorized));
    // 401 后内存 session 必须被清理
    assert!(!client.has_session().await);
}

#[tokio::test]
async fn payment_required_parses_redirect_url() {
    let script = vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            serde_json::json!({
                "token": "jwt-ok",
                "user": {"id": "u1", "username": "alice"}
            }),
        ),
        raw_script(
            "/connection-info",
            "HTTP/1.1 402 Payment Required",
            r#"{"code":"ARREARS","message":"账号已欠费","redirectUrl":"https://pay.example.com/renew"}"#,
            "application/json",
        ),
    ];
    let (base, _cap, _h) = spawn_mock_manager(script).await;

    let client = ManagerClient::new();
    client.login(Some(base), "alice", "pw").await.unwrap();
    let err = client.get_connection_info("r1").await.unwrap_err();
    match err {
        ManagerError::PaymentRequired { message, redirect_url } => {
            assert_eq!(message, "账号已欠费");
            assert_eq!(redirect_url.as_deref(), Some("https://pay.example.com/renew"));
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
            serde_json::json!({
                "token": "jwt-ok",
                "user": {"id": "u1", "username": "alice"}
            }),
        ),
        raw_script(
            "/connection-info",
            "HTTP/1.1 402 Payment Required",
            r#"{"message":"欠费"}"#,
            "application/json",
        ),
    ];
    let (base, _cap, _h) = spawn_mock_manager(script).await;

    let client = ManagerClient::new();
    client.login(Some(base), "alice", "pw").await.unwrap();
    let err = client.get_connection_info("r1").await.unwrap_err();
    match err {
        ManagerError::PaymentRequired { redirect_url, .. } => assert!(redirect_url.is_none()),
        other => panic!("expected PaymentRequired, got {other:?}"),
    }
}

#[tokio::test]
async fn not_found_returned_for_missing_robot() {
    let script = vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            serde_json::json!({
                "token": "jwt-ok",
                "user": {"id": "u1", "username": "alice"}
            }),
        ),
        raw_script(
            "/connection-info",
            "HTTP/1.1 404 Not Found",
            "",
            "application/json",
        ),
    ];
    let (base, _cap, _h) = spawn_mock_manager(script).await;

    let client = ManagerClient::new();
    client.login(Some(base), "alice", "pw").await.unwrap();
    let err = client.get_connection_info("nope").await.unwrap_err();
    assert!(matches!(err, ManagerError::NotFound));
}

#[tokio::test]
async fn forbidden_mapped_from_403() {
    let script = vec![
        json_script(
            "/auth/login-by-password",
            "HTTP/1.1 200 OK",
            serde_json::json!({
                "token": "jwt-ok",
                "user": {"id": "u1", "username": "alice"}
            }),
        ),
        raw_script(
            "/api/v1/digital-employees",
            "HTTP/1.1 403 Forbidden",
            r#"{"message":"no"}"#,
            "application/json",
        ),
    ];
    let (base, _cap, _h) = spawn_mock_manager(script).await;

    let client = ManagerClient::new();
    client.login(Some(base), "alice", "pw").await.unwrap();
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

    let client = ManagerClient::new();
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
    let client = ManagerClient::new();
    let err = client.login(None, "a", "b").await.unwrap_err();
    assert!(matches!(err, ManagerError::MissingBaseUrl));
}

#[tokio::test]
async fn list_without_login_errors_no_session() {
    let client = ManagerClient::new();
    let err = client.list_digital_employees().await.unwrap_err();
    assert!(matches!(err, ManagerError::NoSession));
}

#[tokio::test]
async fn connection_info_without_login_errors_no_session() {
    let client = ManagerClient::new();
    let err = client.get_connection_info("r1").await.unwrap_err();
    assert!(matches!(err, ManagerError::NoSession));
}

#[tokio::test]
async fn logout_without_session_is_noop() {
    let client = ManagerClient::new();
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

    let client = ManagerClient::new();
    let err = client.login(Some(unreachable), "a", "b").await.unwrap_err();
    // reqwest 可能返回 NetworkError（connect refused）或 InvalidResponse（若端口碰巧被复用），
    // 但在本地 race 下绝大概率是 NetworkError。放宽断言，不接受非错误。
    match err {
        ManagerError::NetworkError(_) | ManagerError::Other { .. } => {}
        ManagerError::InvalidResponse(_) => {}
        other => panic!("unexpected error variant: {other:?}"),
    }
}
