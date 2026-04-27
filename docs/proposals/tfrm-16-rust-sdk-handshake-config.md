# 技术实现建议书：smcp-computer 0.1.15 — Socket.IO 握手配置化

| 项目 | 值 |
|---|---|
| Jira 跟进 | [TFRM-16](https://turingfocus.atlassian.net/browse/TFRM-16) |
| 提案方 | tfrobot-client（消费方） |
| 实现方 | rust-sdk 维护者 |
| 验收方 | tfrobot-client 工程师（通过本文附录用例） |
| 目标版本 | `smcp-computer` v0.1.15 |
| 协议版本 | **无变更**（仍为 A2C-SMCP 0.1.2-rc1） |
| 日期 | 2026-04-23 |

> **状态：已实现 + 已验收**（更新于 2026-04-25）
> - rust-sdk @ workspace 0.1.15，`SmcpComputerClientBuilder` + 默认 `access_token` + Agent 对称修改全部到位
> - tfrobot-client 侧验收：7/7 acceptance tests pass（`cargo test --test smcp_handshake_config_test --features verify-smcp-0-1-15`）
> - TFRM-16 Jira 不关闭：仍待 UI 改造、Manager 集成、指数退避、跨 SDK 协议版本对齐 4 项子任务（见 §5 非目标）

---

## 1. 背景

TFRM-16 要求 tfrobot-client（Tauri 桌面端）对齐 TuringFocus 生态的 SMCP 握手契约。TF 生态的 Envoy 网关做 4-header 路由（`X-TF-Namespace` / `X-TF-RobotId` / `X-TF-RobotType` + `access_token`），并要求 Socket.IO 应用层命名空间可由部署方决定（尽管 TF 目前使用 `/smcp`）。

现状是：`smcp-computer` crate **将鉴权 header 名硬编码为 `x-api-key`**，将 Socket.IO namespace 硬编码为 `SMCP_NAMESPACE`。这两处硬编码与 Python 参考实现 `a2c_smcp` 不一致，也与 A2C-SMCP 协议"auth-agnostic"的中性立场不一致：

- **协议是 auth-agnostic**：A2C-SMCP 协议不规定鉴权 header 的字段名；部署方自行决定。Python SDK 不假设任何字段名，Rust SDK 单侧假设 `x-api-key`。
- **TF 生态实际使用 `access_token`（下划线）**：Envoy 已配置 `headers_with_underscores_action: ALLOW`（`envoy_filter_deployable.go:232-269`）。
- **服务端从未启用过 `x-api-key`**：没有生产调用依赖旧默认值，本次直接切换默认值是安全的。

本建议书解决两个根因（均为 crate bug，非协议问题），不涉及 `/connection-info` 集成、指数退避、tfrobot-client UI 改造（这些由其他工单承载）。

---

## 2. 问题定位

### 2.1 鉴权 header 名硬编码

文件：`rust-sdk/crates/smcp-computer/src/socketio_client.rs`

```rust
// line 72-76
// 如果提供了认证密钥，添加到请求头
if let Some(secret) = auth_secret {
    builder = builder.opening_header("x-api-key", secret.as_str());  // ← 硬编码
}
```

构造器 `SmcpComputerClient::new()`（socketio_client.rs:52-89）接受 `auth_secret: Option<String>` 但不接受 header 名参数。消费方只能通过 `headers: Option<HashMap<String, String>>` 旁路注入（绕过 `auth_secret`），UX 混乱。

### 2.2 Socket.IO namespace 硬编码

文件：`rust-sdk/crates/smcp-computer/src/socketio_client.rs`

```rust
// line 68-70
let mut builder = ClientBuilder::new(url)
    .namespace(SMCP_NAMESPACE)                                       // ← 硬编码
    .transport_type(TransportType::Websocket);
```

```rust
// line 783-789
pub fn get_namespace(&self) -> String {
    // 从 client 中获取 namespace，如果无法获取则返回默认值
    "/smcp".to_string()                                              // ← 撒谎：返回字面量
}
```

配套漏洞：
- `crates/smcp-computer/src/computer.rs` 的 `connect_socketio()` 接受 `namespace: &str` 参数但被标记 `#[allow(unused_variables)]`。
- `crates/smcp-computer/src/cli/mod.rs:678` 暴露 `--namespace` CLI 参数——实际无效。

### 2.3 Agent 侧的对称问题（可选修复）

文件：`rust-sdk/crates/smcp-agent/src/auth.rs:34`

```rust
// AuthProvider::get_connection_headers() 也硬编码 "x-api-key"
```

注：`smcp-agent/src/transport.rs` 的 `SocketIoTransport::connect()` 已支持 namespace 参数，仅 auth header 名需要修。为保持双端 API 对称，**建议同一 PR 顺带修复**。若团队倾向分 PR，可以拆开。

---

## 3. 建议 API 设计

### 3.1 引入 `SmcpComputerClientBuilder`

Rust 惯用 Builder 模式，可读性好、向后扩展容易。

```rust
// crates/smcp-computer/src/socketio_client.rs

pub struct SmcpComputerClientBuilder {
    url: String,
    manager: Arc<RwLock<Option<MCPServerManager>>>,
    computer_name: String,
    inputs: Arc<RwLock<HashMap<String, MCPServerInput>>>,
    auth_secret: Option<String>,
    auth_header_name: Option<String>,   // None → "access_token"
    namespace: Option<String>,           // None → SMCP_NAMESPACE ("/smcp")
    headers: Option<HashMap<String, String>>,
}

impl SmcpComputerClientBuilder {
    /// 创建新的 Builder（必填项）
    pub fn new(
        url: impl Into<String>,
        manager: Arc<RwLock<Option<MCPServerManager>>>,
        computer_name: impl Into<String>,
        inputs: Arc<RwLock<HashMap<String, MCPServerInput>>>,
    ) -> Self { /* ... */ }

    pub fn auth_secret(mut self, secret: impl Into<String>) -> Self { /* ... */ }

    /// 自定义鉴权 HTTP header 名。未设置时默认 "access_token"。
    pub fn auth_header_name(mut self, name: impl Into<String>) -> Self { /* ... */ }

    /// 自定义 Socket.IO 应用层 namespace。未设置时默认 "/smcp"。
    pub fn namespace(mut self, ns: impl Into<String>) -> Self { /* ... */ }

    /// 附加任意 HTTP upgrade header（如 X-TF-RobotId 等 TF 生态路由 header）。
    pub fn headers(mut self, headers: HashMap<String, String>) -> Self { /* ... */ }

    /// 建立 Socket.IO 连接
    pub async fn connect(self) -> ComputerResult<SmcpComputerClient> { /* ... */ }
}
```

#### 优先级契约（key 冲突时谁赢）

当 `.auth_secret()` 与 `.headers(map)` 都写到同一个 header key 上（典型场景：`map` 已经包含 `auth_header_name` 那条 entry），约定 **`.headers(map)` 后写者赢**——wire 上只出现 map 中的值，Builder 内部应保证不留下 auth_secret 的旧值，也不允许底层 socketio 库把两者合并成 `"v1, v2"`。

为什么这样定：

- **匹配 tfrobot-client 实际形态**：Manager `/connection-info` 下发的 `routingHeaders` 已经包含 `access_token`，客户端整体透传到 `.headers(map)`，根本不需要 `.auth_secret()`。一旦消费方混用，意图通常是"用 map 里的值"。
- **避免幽灵旧值**：如果 auth_secret 赢，消费方只是用 map 覆盖一个 token 这种常见操作就会变成"看起来更新了但其实没生效"——非常难调试。

附录用例 `test_headers_map_overrides_auth_secret_when_same_key` 锁住此契约。

### 3.2 保留 `SmcpComputerClient::new()` 作为向后兼容入口

```rust
impl SmcpComputerClient {
    /// 向后兼容入口：内部委托给 Builder，默认 auth_header_name = "access_token"、namespace = "/smcp"。
    pub async fn new(
        url: &str,
        manager: Arc<RwLock<Option<MCPServerManager>>>,
        computer_name: String,
        auth_secret: Option<String>,
        inputs: Arc<RwLock<HashMap<String, MCPServerInput>>>,
        headers: Option<HashMap<String, String>>,
    ) -> ComputerResult<Self> {
        let mut b = SmcpComputerClientBuilder::new(url, manager, computer_name, inputs);
        if let Some(s) = auth_secret { b = b.auth_secret(s); }
        if let Some(h) = headers { b = b.headers(h); }
        b.connect().await
    }
}
```

**关键点**：`tfrobot-client/src-tauri/src/commands/connection.rs:121-128` 的既有调用点**无需任何代码修改**，升级到 0.1.15 即可自动获得新默认值（`access_token`）。

### 3.3 `get_namespace()` 修复

```rust
pub struct SmcpComputerClient {
    // ... 既有字段
    namespace: String,  // 新增：保存握手时使用的 namespace
}

impl SmcpComputerClient {
    pub fn get_namespace(&self) -> String {
        self.namespace.clone()
    }
}
```

Builder 在 `connect()` 时将解析后的 namespace（用户配置值或默认值 `SMCP_NAMESPACE`）保存到 `SmcpComputerClient::namespace` 字段。

### 3.4 `Computer::connect_socketio()` 修复

```rust
// crates/smcp-computer/src/computer.rs
pub async fn connect_socketio(
    &self,
    url: &str,
    namespace: &str,    // 移除 #[allow(unused_variables)]
    // ... 其它参数
) -> ComputerResult<()> {
    let builder = SmcpComputerClientBuilder::new(url, /* ... */)
        .namespace(namespace);                   // 真实使用
    // ...
}
```

CLI `--namespace` 参数（`cli/mod.rs:678`）自然生效，无需改 CLI 代码。

### 3.5 Agent 侧对称（可选）

```rust
// crates/smcp-agent/src/config.rs
pub struct AgentConfig {
    // ... 既有字段
    pub auth_header_name: Option<String>,  // None → "access_token"
}

// crates/smcp-agent/src/auth.rs:34
impl AuthProvider {
    pub fn get_connection_headers(&self, header_name: &str) -> HashMap<String, String> {
        // ... 使用 header_name 代替硬编码 "x-api-key"
    }
}
```

---

## 4. 版本号与 CHANGELOG

### 4.1 Workspace `Cargo.toml`

```toml
# rust-sdk/Cargo.toml
[workspace.package]
version = "0.1.15"  # was 0.1.14
```

### 4.2 CHANGELOG（建议加入）

```markdown
## 0.1.15 — 2026-04-XX

### 破坏性行为变更（非 API 破坏）
- **默认鉴权 HTTP header key** 由 `x-api-key` 改为 `access_token`。
  - 依据：A2C-SMCP 协议 auth-agnostic；TF 生态使用 `access_token`；历史默认值 `x-api-key` 从未在任何生产服务端启用。
  - 迁移：如需保留旧默认值，显式调用 `.auth_header_name("x-api-key")`。

### 新增
- `SmcpComputerClientBuilder` —— Builder 模式的 Computer 客户端构造器。
  - `.auth_secret(secret)` / `.auth_header_name(name)` / `.namespace(ns)` / `.headers(map)` / `.connect().await`
- `SmcpComputerClient::get_namespace()` 现在返回实际配置的 namespace（而非总是字面量 `"/smcp"`）。
- Agent 端 `AgentConfig::auth_header_name`（可选字段，默认 `access_token`）。

### 修复
- `Computer::connect_socketio()` 现在真实使用 `namespace` 参数（此前被 `#[allow(unused_variables)]` 忽略）。
- CLI `--namespace` 参数生效。

### 保持兼容
- `SmcpComputerClient::new(..)` 签名不变，内部委托给 Builder。
```

---

## 5. 非目标（由其他工单承载）

- Manager `/connection-info` HTTP 客户端 —— 阻塞于 TFRM-18，tfrobot-client 侧独立工单。
- 指数退避重连策略（503 → 1s/2s/4s/8s/30s）—— 需先决定放在 crate 还是消费方，另立工单。
- tfrobot-client UI 一等公民字段（`X-TF-Namespace/RobotId/RobotType`）—— UX 改造，独立工单。
- 协议版本跨 SDK 对齐（Rust 0.1.2-rc1 vs Python `a2c_smcp` 版本）—— cross-ask 事项，独立工单。

---

## 6. 验收方式

tfrobot-client 侧已提交验收测试用例 `src-tauri/tests/smcp_handshake_config_test.rs`，由 Cargo feature `verify-smcp-0-1-15` 门控（默认关闭，不影响现有 CI）。

**验收流程**（rust-sdk 工程师完成实现后）：

1. rust-sdk 发布 `smcp-computer` v0.1.15 到 crates.io。
2. tfrobot-client 维护者在 `src-tauri/Cargo.toml` 将 `smcp-computer` 升级到 `0.1.15`。
3. 运行：
   ```bash
   cd /Users/jqq/RustroverProjects/tfrobot-client/src-tauri
   cargo test --test smcp_handshake_config_test --features verify-smcp-0-1-15
   ```
4. 7 个用例全绿 → 在 TFRM-16 评论区确认验收通过。
5. TFRM-16 **不关闭**（还有 UI、Manager 集成等未完成子项）。

## 附录：验收用例源码快照

rust-sdk 维护者可以在 `crates/smcp-computer/tests/` 下写等价的 crate 内部测试。完整实现见本仓库 `src-tauri/tests/smcp_handshake_config_test.rs`。测试清单：

| # | 测试 | 断言 |
|---|---|---|
| 1 | `test_default_auth_header_is_access_token` | 未调 `.auth_header_name()` 时，WS upgrade header 键应为 `access_token` |
| 2 | `test_custom_auth_header_name_override` | `.auth_header_name("x-legacy-key")` 后，header 键应为 `x-legacy-key` |
| 3 | `test_custom_namespace_does_not_regress_auth_header` | 同时配置 `.namespace("/custom_ns")` + auth header 时不互相干扰 |
| 4 | `test_get_namespace_reflects_configured_value` | `client.get_namespace()` 返回用户配置值，而非字面量 `"/smcp"` |
| 5 | `test_backward_compat_new_uses_access_token_default` | 老的 `SmcpComputerClient::new()` 入口，默认 header 键为 `access_token` |
| 6 | `test_routing_headers_propagate_with_auth` | `.headers(map)` 同时携带 `X-TF-Namespace/RobotId/RobotType` + `access_token` 时，4 条 header 全部到达 WS upgrade（守护 TF Envoy 4-header 路由契约） |
| 7 | `test_headers_map_overrides_auth_secret_when_same_key` | `.auth_secret()` + `.headers(map)` 都写同一 key 时，`.headers(map)` 后写者赢（锁住 §3.1 优先级契约） |

测试实现采用**轻量 TCP 截包**策略（仅依赖 `tokio::net::TcpListener`，不引入 `socketioxide` 等重量依赖）：服务器接受一次连接，读取到 HTTP headers 结束（`\r\n\r\n`），解析 header，然后回 `HTTP/1.1 503` 让客户端退出 handshake。客户端的 `connect()` 会返回 `Err`（预期），测试断言**捕获到的 header**是否符合预期。

## 附录 B：API 命名备选

Builder 方法命名可由 rust-sdk 维护者定夺。以下为备选方案，测试用例按"方案 A"书写，若最终选方案 B/C，请同步调整测试中的方法名。

| 概念 | 方案 A（推荐） | 方案 B | 方案 C |
|---|---|---|---|
| 鉴权 header 键 | `.auth_header_name(name)` | `.with_auth_header(name)` | `.api_key_header(name)` |
| 应用层 namespace | `.namespace(ns)` | `.with_namespace(ns)` | `.socket_namespace(ns)` |
| 终止方法 | `.connect().await` | `.build().await` | `.start().await` |

---

*本建议书写于 2026-04-23 by tfrobot-client 侧工程师。如有疑问，请在 TFRM-16 下评论或直接 PR 本文档修改意见。*
