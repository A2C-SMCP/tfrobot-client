# SDK Client Best Practices

本文档说明如何基于当前 Rust SDK 封装一个业务侧 `Client`，由它统一管理多个 `Computer` 实例，并负责每个 `Computer` 的配置、运行时、连接状态和生命周期。

## 推荐分层

业务侧不要直接把所有逻辑堆在 `Computer` 上，建议分三层：

```text
YourClient
  - 全局配置
  - computer 注册表
  - 生命周期编排
  - 状态查询/健康检查
  - 统一 shutdown

ManagedComputer
  - Computer 实例
  - 当前配置快照
  - Socket.IO 连接信息
  - office 归属
  - 运行状态
  - 启停锁

smcp_computer::Computer
  - MCP server 管理
  - 工具调用
  - inputs 渲染
  - Socket.IO 接线
  - skills/blob 子系统
```

`Computer` 本身已经管理单个 computer 内部的多个 MCP server。业务侧 `Client` 应该管理“多个 Computer”，不要绕过 `Computer` 直接操作多个 `MCPServerManager`。

## 核心生命周期

单个 `Computer` 推荐按以下顺序启动：

```rust
let session = SilentSession::new("session-xxx");

let computer = Computer::new(
    "computer-a",
    session,
    Some(inputs),
    Some(mcp_servers),
    true,
    true,
)
.with_confirm_callback(|req_id, server, tool, params| {
    // 生产环境建议接入业务审批策略
    true
});

computer.boot_up().await?;

computer.connect_socketio(
    server_url,
    ConnectOptions {
        auth_payload: Some(serde_json::json!({ "token": token })),
        headers: None,
        ..Default::default()
    },
).await?;

computer.join_office(office_id, "computer-a").await?;
```

关闭时只走：

```rust
computer.shutdown().await?;
```

不要只调用 `disconnect_socketio()` 或 `stop_mcp_client()` 当作完整释放。`shutdown()` 才会统一释放 Socket.IO、MCP clients、skill watcher、blob 等资源。

## Client 数据结构建议

```rust
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{Mutex, RwLock};
use smcp_computer::computer::{Computer, SilentSession};

pub struct SmcpClient {
    server_url: String,
    token: Option<String>,
    computers: RwLock<HashMap<String, Arc<ManagedComputer>>>,
}

pub struct ManagedComputer {
    name: String,
    office_id: RwLock<Option<String>>,
    state: RwLock<ComputerRuntimeState>,
    computer: Arc<Computer<SilentSession>>,
    lifecycle_lock: Mutex<()>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputerRuntimeState {
    Created,
    Booting,
    Booted,
    Connecting,
    Connected,
    JoinedOffice,
    Stopping,
    Stopped,
    Error,
}
```

每个 `ManagedComputer` 建议放一个 `lifecycle_lock`。启动、重连、关闭、更新配置都应该串行化，避免同时执行 `boot_up()`、`connect_socketio()`、`shutdown()` 造成状态竞争。

## Computer 注册

注册时构造 `Computer`，但不要立刻连接，除非业务明确要求自动启动：

```rust
pub async fn register_computer(
    &self,
    name: String,
    inputs: HashMap<String, MCPServerInput>,
    servers: HashMap<String, MCPServerConfig>,
) -> anyhow::Result<()> {
    let session = SilentSession::new(format!("session-{name}"));

    let computer = Computer::new(
        name.clone(),
        session,
        Some(inputs),
        Some(servers),
        true,
        true,
    )
    .with_confirm_callback(|_, _, _, _| true);

    let managed = ManagedComputer {
        name: name.clone(),
        office_id: RwLock::new(None),
        state: RwLock::new(ComputerRuntimeState::Created),
        computer: Arc::new(computer),
        lifecycle_lock: Mutex::new(()),
    };

    self.computers.write().await.insert(name, Arc::new(managed));
    Ok(())
}
```

## 启动与入房

```rust
pub async fn start_computer(
    &self,
    name: &str,
    office_id: &str,
) -> anyhow::Result<()> {
    let managed = self.get_computer(name).await?;
    let _guard = managed.lifecycle_lock.lock().await;

    *managed.state.write().await = ComputerRuntimeState::Booting;
    managed.computer.boot_up().await?;

    *managed.state.write().await = ComputerRuntimeState::Connecting;
    managed.computer.connect_socketio(
        &self.server_url,
        ConnectOptions {
            auth_payload: self.token.as_ref().map(|t| serde_json::json!({ "token": t })),
            ..Default::default()
        },
    ).await?;

    *managed.state.write().await = ComputerRuntimeState::Connected;
    managed.computer.join_office(office_id, name).await?;

    *managed.office_id.write().await = Some(office_id.to_string());
    *managed.state.write().await = ComputerRuntimeState::JoinedOffice;

    Ok(())
}
```

## 配置管理

每个 `Computer` 的 MCP server 配置可以启动前整体传入，也可以运行时动态更新：

```rust
computer.add_or_update_server(server_config).await?;
computer.remove_server("server-name").await?;

computer.add_or_update_input(input).await?;
computer.remove_input("api_key").await?;
computer.set_input_value("api_key", serde_json::json!("xxx")).await?;
```

最佳实践：

- `MCPServerConfig.name` 在同一个 `Computer` 内必须唯一。
- 不同 MCP server 暴露同名 tool 时，必须用 `ToolMeta.alias` 解决冲突。
- 敏感值优先走 `MCPServerInput` 或 `envFile`，不要硬编码在 `server_parameters.env`。
- stdio server 推荐显式设置 `cwd`，避免业务进程工作目录变化导致找不到脚本。
- 更新运行中的 server 时，如果 `auto_reconnect=true`，SDK 会尝试重启对应 server；否则需要先 stop 再改。

## 工具调用

当前 `Computer::execute_tool()` 内部默认 `need_confirm = true`，如果没有设置 `with_confirm_callback`，会返回“未实现二次确认回调”。所以业务 client 必须二选一：

1. 注册 `with_confirm_callback`。
2. 对需要取消能力的路径使用 `execute_tool_cancellable()`。

推荐统一封装：

```rust
pub async fn call_tool(
    &self,
    computer_name: &str,
    req_id: &str,
    tool_name: &str,
    params: serde_json::Value,
    timeout_secs: Option<f64>,
) -> anyhow::Result<CallToolResult> {
    let managed = self.get_computer(computer_name).await?;
    let result = managed
        .computer
        .execute_tool_cancellable(req_id, tool_name, params, timeout_secs)
        .await?;

    Ok(result)
}
```

取消调用：

```rust
let cancelled = computer.acancel_tool(req_id).await;
```

取消是协作式的。SDK 会让当前调用尽快返回取消态，但远端 MCP server 是否真正停止，取决于它是否支持取消。

## 状态与健康检查

可以用这些 API 做状态面板：

```rust
let servers = computer.list_mcp_servers().await;
let status = computer.get_server_status().await;
let tools = computer.get_available_tools().await?;
let history = computer.get_tool_history().await?;
let initialized = computer.is_mcp_manager_initialized().await;
```

业务侧建议维护两类状态：

```text
Client 侧状态：
Created / Booting / Connected / JoinedOffice / Stopping / Error

SDK 侧状态：
get_server_status()
get_available_tools()
get_tool_history()
```

不要只依赖 SDK 内部状态来表达你的业务状态。比如 Socket.IO 已连接但还没 `join_office`，对业务来说通常还不能算 ready。

## 并发建议

- `Client.computers` 用 `RwLock<HashMap<String, Arc<ManagedComputer>>>`。
- 单个 computer 的启停、重连、配置更新用 `Mutex` 串行化。
- 工具调用可以并发执行，但每次调用必须有唯一 `req_id`。
- shutdown 时先阻止新调用，再逐个 `shutdown()`。
- 不要在持有全局 `computers.write()` 的时候 await 单个 computer 的启动/关闭，避免阻塞所有实例。

## 推荐 shutdown 流程

```rust
pub async fn shutdown_all(&self) {
    let computers: Vec<_> = self.computers
        .read()
        .await
        .values()
        .cloned()
        .collect();

    for managed in computers {
        let _guard = managed.lifecycle_lock.lock().await;
        *managed.state.write().await = ComputerRuntimeState::Stopping;

        if let Err(err) = managed.computer.shutdown().await {
            tracing::warn!(computer = %managed.name, error = %err, "computer shutdown failed");
            *managed.state.write().await = ComputerRuntimeState::Error;
        } else {
            *managed.state.write().await = ComputerRuntimeState::Stopped;
        }
    }
}
```

## 生产使用要点

- 每个 computer name 全局唯一，建议使用稳定 ID，不要用展示名。
- 每个 tool call req_id 全局唯一，方便取消和历史追踪。
- Socket.IO 鉴权放 `ConnectOptions.auth_payload`，不要放 headers。
- headers 只用于路由类信息。
- 所有工具调用都设置 timeout。
- stdio MCP server 必须考虑进程退出和重启。
- 对动态配置更新做审计日志。
- 对 `get_server_status()` 和 `get_tool_history()` 做周期采样，便于排障。
- `shutdown()` 必须在进程退出信号里调用，避免子进程和 watcher 泄漏。

## 总结

业务 `Client` 应该把 `Computer` 当成“一个可运行节点”，不要拆开管理它内部的 MCP 连接。每个 `Computer` 内部用 SDK 的 `add_or_update_server`、`start_mcp_client`、`execute_tool_cancellable`、`shutdown`；多个 `Computer` 的注册、状态、连接、入房、重连和批量关闭由业务 `Client` 统一编排。

## tfrobot-client 落地状态

本仓库的当前落地说明见 `plans/SDK_CLIENT_ARCHITECTURE_ALIGNMENT.md`。该说明记录了
TFRC-47 到 TFRC-53 后的最终边界：

- 生产代码中 `ComputerInstanceRuntime` 不再公开 legacy `MCPServerManager`。
- MCP server 管理、工具调用、resources/window 读面、Socket.IO 连接和 shutdown 均经由 SDK
  `Computer`。
- `ComputerRegistry` 只管理多个 runtime 的注册、查找和批量生命周期调度。
- 单个 runtime 使用 `ComputerRuntimeState` 和 per-computer `lifecycle_lock` 串行化生命周期操作。
