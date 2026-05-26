//! 验收测试：smcp-computer 0.1.15 握手配置化（TFRM-16 子项）
//!
//! 本文件验证 rust-sdk PR 的实现是否符合 `docs/proposals/tfrm-16-rust-sdk-handshake-config.md` 建议。
//!
//! # 运行前提
//!
//! 1. rust-sdk 已发布 `smcp-computer` v0.1.15（含 `SmcpComputerClientBuilder` 与默认 `access_token` header）。
//! 2. `src-tauri/Cargo.toml` 已将 `smcp-computer` 版本升级至 `0.1.15`。
//! 3. 通过 feature flag 显式启用本文件：
//!    ```bash
//!    cargo test --test smcp_handshake_config_test --features verify-smcp-0-1-15
//!    ```
//!
//! 在版本升级前，整个文件被 `#![cfg(feature = "verify-smcp-0-1-15")]` 门控，
//! 不参与编译，不会影响现有 CI。
//!
//! # 验收策略
//!
//! 用一个轻量 `tokio::net::TcpListener` 模拟"Socket.IO 服务器"：
//! 1. 监听 127.0.0.1 的 OS 分配端口；
//! 2. 接受第一个连接，读取 HTTP headers 到 `\r\n\r\n`；
//! 3. 将 headers 存入共享 `Mutex`；
//! 4. 回一个 `HTTP/1.1 503` 让客户端 handshake 失败退出。
//!
//! 客户端的 `connect()` 会返回 `Err`（预期）；我们断言**捕获到的 headers** 是否符合预期。
//! 这样无需引入 `socketioxide` 等重量依赖，也足以验证"上游 WS upgrade 携带什么 header / 连接到什么 namespace"。

#![cfg(feature = "verify-smcp-0-1-15")]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use smcp_computer::mcp_clients::manager::MCPServerManager;
use smcp_computer::mcp_clients::model::MCPServerInput;
use smcp_computer::socketio_client::{SmcpComputerClient, SmcpComputerClientBuilder};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::RwLock;

/// 捕获的 HTTP headers + 请求行（GET 路径）
#[derive(Clone, Debug, Default)]
#[allow(dead_code)] // request_line 仅用于 Debug 输出的辅助诊断，无显式读取
struct CapturedRequest {
    request_line: String,
    headers: HashMap<String, String>,
}

type CaptureHandle = Arc<tokio::sync::Mutex<Option<CapturedRequest>>>;

/// 启动一个 one-shot HTTP/TCP 监听器，捕获首个连接的请求行和 headers，
/// 回 HTTP 503 让客户端 handshake 失败。
///
/// 返回 `(base_url, capture_handle)`。调用方用 `base_url` 作为 `SmcpComputerClientBuilder::new` 的 `url`。
async fn spawn_capture_server() -> (String, CaptureHandle) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let base_url = format!("http://{}", addr);

    let capture: CaptureHandle = Arc::new(tokio::sync::Mutex::new(None));
    let capture_in_task = capture.clone();

    tokio::spawn(async move {
        // 只处理第一个连接；后续客户端重连将收到 ConnectionRefused（listener drop 后）。
        if let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = Vec::with_capacity(4096);
            let mut tmp = [0u8; 1024];

            // 读到 \r\n\r\n 即 HTTP 头结束
            loop {
                match tokio::time::timeout(Duration::from_secs(2), socket.read(&mut tmp)).await {
                    Ok(Ok(0)) => break, // EOF
                    Ok(Ok(n)) => {
                        buf.extend_from_slice(&tmp[..n]);
                        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    _ => break, // 超时或读错
                }
            }

            let raw = String::from_utf8_lossy(&buf).to_string();
            let mut lines = raw.split("\r\n");
            let request_line = lines.next().unwrap_or("").to_string();
            let mut headers = HashMap::new();
            for line in lines {
                if line.is_empty() {
                    break;
                }
                if let Some((k, v)) = line.split_once(':') {
                    // header 名大小写不敏感；统一 lowercase 便于断言
                    headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
                }
            }

            *capture_in_task.lock().await = Some(CapturedRequest {
                request_line,
                headers,
            });

            // 回 503 并关闭；让客户端 handshake 失败退出，不进入长连接
            let _ = socket
                .write_all(
                    b"HTTP/1.1 503 Service Unavailable\r\n\
                      Content-Length: 0\r\n\
                      Connection: close\r\n\r\n",
                )
                .await;
            let _ = socket.shutdown().await;
        }
        // listener 在此 drop；后续 TCP 连接会 ConnectionRefused
    });

    (base_url, capture)
}

/// 构造 Builder 必填的空管家 + 空输入映射
fn empty_manager_and_inputs() -> (
    Arc<RwLock<Option<MCPServerManager>>>,
    Arc<RwLock<HashMap<String, MCPServerInput>>>,
) {
    (
        Arc::new(RwLock::new(None)),
        Arc::new(RwLock::new(HashMap::new())),
    )
}

/// 等待 capture 被填充（最长 3s）
async fn wait_capture(capture: &CaptureHandle) -> CapturedRequest {
    for _ in 0..30 {
        if let Some(c) = capture.lock().await.clone() {
            return c;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("capture server never received a request within 3s");
}

// ──────────────────────── 用例 1 ────────────────────────

#[tokio::test]
async fn test_default_auth_header_is_access_token() {
    let (url, capture) = spawn_capture_server().await;
    let (manager, inputs) = empty_manager_and_inputs();

    // 期望：不调 .auth_header_name() 时，默认 header 为 `access_token`（而非旧的 `x-api-key`）
    let result = SmcpComputerClientBuilder::new(&url, manager, "verify-computer", inputs)
        .auth_secret("test-secret-123")
        .connect()
        .await;

    assert!(
        result.is_err(),
        "capture server returned 503; connect() 应当失败"
    );

    let captured = wait_capture(&capture).await;
    assert_eq!(
        captured.headers.get("access_token").map(String::as_str),
        Some("test-secret-123"),
        "默认鉴权 header 应为 access_token（0.1.15 新默认）。实际 headers: {:?}",
        captured.headers
    );
    assert!(
        !captured.headers.contains_key("x-api-key"),
        "不应再发送旧的 x-api-key header。实际 headers: {:?}",
        captured.headers
    );
}

// ──────────────────────── 用例 2 ────────────────────────

#[tokio::test]
async fn test_custom_auth_header_name_override() {
    let (url, capture) = spawn_capture_server().await;
    let (manager, inputs) = empty_manager_and_inputs();

    let result = SmcpComputerClientBuilder::new(&url, manager, "verify-computer", inputs)
        .auth_secret("legacy-secret-456")
        .auth_header_name("x-legacy-key")
        .connect()
        .await;

    assert!(result.is_err());

    let captured = wait_capture(&capture).await;
    assert_eq!(
        captured.headers.get("x-legacy-key").map(String::as_str),
        Some("legacy-secret-456"),
        "自定义 auth_header_name 应覆盖默认值。实际 headers: {:?}",
        captured.headers
    );
    assert!(!captured.headers.contains_key("access_token"));
    assert!(!captured.headers.contains_key("x-api-key"));
}

// ──────────────────────── 用例 3 ────────────────────────

#[tokio::test]
async fn test_custom_namespace_does_not_regress_auth_header() {
    // 关于 namespace 字节级传播验证的说明：
    //
    // Socket.IO 的应用层 namespace（如 `/custom_ns`）是在 WS upgrade **完成后**，
    // 通过 Engine.IO OPEN + Socket.IO CONNECT 包（如 `40/custom_ns,`）在 WebSocket
    // 帧中发送的——不体现在 HTTP upgrade 请求的 URL 或 headers 上。
    //
    // 本文件的轻量 mock server 在 HTTP 层回 503 即断开，无法观察到 WS 帧。
    // 字节级的 namespace 传播由 rust-sdk 内部测试（对实际 Socket.IO 服务器）覆盖。
    //
    // 本用例退而验证一个**消费方关心的行为契约**：同时配置 namespace 与 auth header
    // 时不互相干扰（无回归），auth header 仍然按配置写入 HTTP upgrade 请求。
    let (url, capture) = spawn_capture_server().await;
    let (manager, inputs) = empty_manager_and_inputs();

    let result = SmcpComputerClientBuilder::new(&url, manager, "verify-computer", inputs)
        .auth_secret("ns-secret")
        .namespace("/custom_ns")
        .connect()
        .await;

    assert!(result.is_err(), "capture server 返回 503，connect 应当失败");

    let captured = wait_capture(&capture).await;
    assert_eq!(
        captured.headers.get("access_token").map(String::as_str),
        Some("ns-secret"),
        "同时配置 namespace 时不应影响默认 auth header。实际 headers: {:?}",
        captured.headers
    );
}

// ──────────────────────── 用例 4 ────────────────────────

#[tokio::test]
async fn test_get_namespace_reflects_configured_value() {
    // 本测试无需真实网络通信——只要 Builder/Client 正确保存 namespace 字段即可。
    // 但 connect() 需要一个可连的 URL 才能构造 client。我们用 capture server 让 connect 失败，
    // 然后需要能在 connect 失败前拿到 client 实例——这在 Builder 终止方法 `.connect()` 返回
    // Err 时是不可能的（client 未创建）。
    //
    // 所以本用例验证的是：如果 connect 成功到真服务器时，namespace 被正确保存。
    // 无法在本文件中跑通——转而依赖 rust-sdk 的 crate 内部测试覆盖此断言。
    //
    // 这里保留一个最小化的烟测：确认 `SmcpComputerClient::get_namespace()` 返回 String 类型，
    // 且方法签名与建议一致（不再是硬编码字面量）。若 rust-sdk 改签名，本调用会编译失败。
    fn _compile_time_signature_check(c: &SmcpComputerClient) -> String {
        c.get_namespace()
    }
    // 真正的运行时断言请在 rust-sdk/crates/smcp-computer/tests/ 下补全。
}

// ──────────────────────── 用例 6 ────────────────────────

#[tokio::test]
async fn test_routing_headers_propagate_with_auth() {
    // TF 生态的真实使用形态：Manager `/connection-info` 下发的 `routingHeaders` 整体
    // 经 tfrobot-client 透传到 `.headers(map)`，包含 4 条 header：
    //   - X-TF-Namespace
    //   - X-TF-RobotId
    //   - X-TF-RobotType
    //   - access_token
    //
    // Envoy 网关靠这 4 条做路由（前 3 条）+ 鉴权（access_token）。任何一条丢失，
    // 路由命中就会变成 404 / 错误的 upstream。本测试守住 `.headers(map)` 的全量传播，
    // 防止 rust-sdk 后续重构 `opening_header` 调用顺序或合并方式时悄悄回归。
    //
    // 注意：tfrobot-client 当前不用 `.auth_secret()`，access_token 走 `.headers(map)`
    // 与其他 TF header 一起送达，与 Python SDK / Manager DTO 形态完全对齐。
    let (url, capture) = spawn_capture_server().await;
    let (manager, inputs) = empty_manager_and_inputs();

    let mut routing_headers = HashMap::new();
    routing_headers.insert("X-TF-Namespace".to_string(), "tfrobotserver".to_string());
    routing_headers.insert(
        "X-TF-RobotId".to_string(),
        "5f4b3b3b-3b3b-3b3b-3b3".to_string(),
    );
    routing_headers.insert("X-TF-RobotType".to_string(), "tfrobot".to_string());
    routing_headers.insert(
        "access_token".to_string(),
        "ac4a30ae756c-test-token".to_string(),
    );

    let result = SmcpComputerClientBuilder::new(&url, manager, "verify-computer", inputs)
        .headers(routing_headers.clone())
        .connect()
        .await;

    assert!(result.is_err(), "capture server 返回 503，connect 应当失败");

    let captured = wait_capture(&capture).await;

    // 4 条 header 一条都不能少
    for (k, v) in &routing_headers {
        assert_eq!(
            captured
                .headers
                .get(&k.to_ascii_lowercase())
                .map(String::as_str),
            Some(v.as_str()),
            "routing header `{}` 必须出现在 WS upgrade 上，否则 TF Envoy 路由会失败。\
             实际 headers: {:?}",
            k,
            captured.headers
        );
    }
}

// ──────────────────────── 用例 7 ────────────────────────

#[tokio::test]
async fn test_headers_map_overrides_auth_secret_when_same_key() {
    // 优先级契约：当 `.auth_secret()` 与 `.headers(map)` 都写同一个 header key 时，
    // **`.headers(map)` 后写者赢**——这与 Builder 内部"先 auth、再迭代 headers"的实现一致。
    //
    // 为什么需要这条契约：tfrobot-client 当前完全走 `.headers(map)` 路径（Manager
    // 下发的 routingHeaders 已包含 access_token），不依赖 `.auth_secret()`；但任何
    // 切换到混用模式的消费方都需要清楚预期。本测试把 contract 锁住，rust-sdk 重构
    // 调用顺序时会立刻被这条用例发现。
    //
    // 不测试"appends 重复 header"的形态——HTTP 标准下重复 header 行为不可预期，
    // Builder 既然选了"后写覆盖"语义，就应该保证 wire 上只有一条。
    let (url, capture) = spawn_capture_server().await;
    let (manager, inputs) = empty_manager_and_inputs();

    let mut headers_map = HashMap::new();
    headers_map.insert("access_token".to_string(), "from-headers-map".to_string());

    let result = SmcpComputerClientBuilder::new(&url, manager, "verify-computer", inputs)
        .auth_secret("from-auth-secret") // 先写
        .headers(headers_map) // 后写，应覆盖
        .connect()
        .await;

    assert!(result.is_err());

    let captured = wait_capture(&capture).await;
    let access_token = captured.headers.get("access_token").map(String::as_str);

    // 注意：HTTP 解析阶段重复 header 会按 RFC 7230 合并为 "v1, v2"。
    // 我们的 capture server 只保留最后一条（HashMap 覆盖语义）。
    // 所以这里通过"是否包含 from-auth-secret"来判断 Builder 实际行为：
    //   - 若是 "from-headers-map" → headers map 赢（预期）
    //   - 若是 "from-auth-secret" → auth_secret 赢（与契约不符，需要修 Builder）
    //   - 若是合并形如 "from-auth-secret, from-headers-map" → underlying socketio
    //     append 而非 overwrite，需要 Builder 显式去重
    assert_eq!(
        access_token,
        Some("from-headers-map"),
        "headers map 在 auth_secret 之后写入，应当赢。\
         若实际是 from-auth-secret 或合并值，说明 Builder 未保证 \
         `.headers(map)` 后写覆盖语义。实际 access_token: {:?}",
        access_token
    );
}

// ──────────────────────── 用例 5 ────────────────────────

#[tokio::test]
async fn test_backward_compat_new_uses_access_token_default() {
    let (url, capture) = spawn_capture_server().await;
    let (manager, inputs) = empty_manager_and_inputs();

    // 走老的 `SmcpComputerClient::new()` 入口（tfrobot-client 当前的调用形态）。
    // 升级到 0.1.15 后，此入口应内部委托给 Builder，默认 header 为 access_token。
    let result = SmcpComputerClient::new(
        &url,
        manager,
        "verify-computer".to_string(),
        Some("compat-secret".to_string()),
        inputs,
        None,
    )
    .await;

    assert!(result.is_err());

    let captured = wait_capture(&capture).await;
    assert_eq!(
        captured.headers.get("access_token").map(String::as_str),
        Some("compat-secret"),
        "老的 new() 入口在 0.1.15 后也应走新默认 access_token。实际 headers: {:?}",
        captured.headers
    );
    assert!(
        !captured.headers.contains_key("x-api-key"),
        "0.1.15 后不应再有 x-api-key fallback"
    );
}
