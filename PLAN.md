# TFRobot Client 技术方案

> 历史说明：本文档是项目早期技术方案，部分关于 `MCPServerManager`、`SmcpComputerClient`
> 主路径和 `AppState` 架构的描述已经过时。SDK Computer 架构对齐后的当前实现以
> `plans/SDK_CLIENT_ARCHITECTURE_ALIGNMENT.md` 为准；本文保留作为历史阶段参考。

## 项目概述

### 背景

**A2C-SMCP**（Agent To Computer SMCP）是一种远程工具调用协议，定义了 AI 智能体（Agent）与计算机（Computer）通过 Socket.IO 进行通信的规范。该协议运行在 MCP（Model Context Protocol）之上，解决了企业级 AI 应用中多 MCP 服务管理、权限隔离和网络穿透的核心问题。

当前 A2C-SMCP 协议的 Rust 实现（`rust-sdk`）已完成，其中 `smcp-computer` crate 可将电脑转换为协议中的 "Computer" 角色，但目前仅提供命令行界面（CLI），对普通用户不友好。

### 目标

开发一个面向无技术背景普通用户的跨平台桌面客户端，降低 A2C-SMCP 协议的使用门槛。

### 产品名称

**TFRobot Client**

---

## 术语表

在阅读本文档之前，请先了解以下核心术语：

| 术语 | 说明 |
|------|------|
| **MCP** | Model Context Protocol，Anthropic 提出的标准化协议，用于 AI 模型与外部工具、数据源之间的交互 |
| **MCP Server** | 实现 MCP 协议的服务器进程，提供工具（tools）、资源（resources）、提示词（prompts）等能力 |
| **A2C-SMCP** | Agent To Computer SMCP，在 MCP 之上的远程调用协议，解决多 MCP 服务管理和网络穿透问题 |
| **Agent** | AI 智能体客户端，发起工具调用的一方（通常是 AI 助手） |
| **Computer** | MCP 服务器的宿主，管理多个 MCP Server，统一暴露工具能力 |
| **Server** | SMCP 信令服务器，负责 Agent 和 Computer 之间的连接管理、消息路由 |
| **Office/Room** | 逻辑隔离单元，Agent 和 Computer 必须在同一房间内才能通信 |
| **Socket.IO** | 基于 WebSocket 的实时通信协议，支持自动重连、命名空间、房间等特性 |

---

## A2C-SMCP 协议简介

### 三角色模型

```
┌─────────┐         ┌─────────┐         ┌─────────┐
│  Agent  │◄────────│ Server  │────────►│Computer │
└─────────┘         └─────────┘         └─────────┘
  │                    │                    │
  │ 工具调用发起方     │ 信令中枢           │ MCP 服务管理
  │ (AI 助手)          │ 连接管理           │ 工具执行
  │                    │ 消息路由           │
```

| 角色 | 职责 | 说明 |
|------|------|------|
| **Agent** | 发起工具调用 | 通常是 AI 智能体，如 Claude、GPT 等 |
| **Server** | 信令中枢 | 管理连接、路由消息、广播通知 |
| **Computer** | 执行工具 | 管理多个 MCP Server，执行实际的工具调用 |

### 房间隔离模型

- 每个房间（Office）通过 `office_id` 唯一标识
- 一个房间最多 1 个 Agent，可以有多个 Computer
- 跨房间通信被严格禁止
- Agent 进入房间后会收到 Computer 的工具列表

### 事件命名约定

| 前缀 | 方向 | 说明 |
|------|------|------|
| `client:` | Agent → Server → Computer | 工具操作类事件 |
| `server:` | 客户端 → Server | 房间管理、配置更新 |
| `notify:` | Server → 广播 | 状态变更通知 |

### 核心安全原则

**零凭证传播**：敏感 Token 仅存在本地 Computer，不在网络上流转。API Key 等敏感信息存储在用户本地，通过 MCP Server 的环境变量传递，永远不会发送到远程服务器。

---

## 现有 rust-sdk 项目结构

TFRobot Client 将复用 `rust-sdk` 中的 `smcp-computer` crate。以下是 rust-sdk 的项目结构：

```
rust-sdk/                              # A2C-SMCP Rust SDK
├── Cargo.toml                         # Workspace 配置
├── src/                               # 主包入口（基于 feature 重导出）
├── crates/
│   ├── smcp/                          # 核心协议类型定义
│   │   └── src/
│   │       ├── events.rs              # 事件常量定义
│   │       ├── types.rs               # 数据结构（ToolCallReq, SMCPTool 等）
│   │       └── error_codes.rs         # 标准错误码
│   │
│   ├── smcp-agent/                    # Agent 客户端实现
│   │   └── src/
│   │       ├── async_agent.rs         # 异步 Agent
│   │       ├── sync_agent.rs          # 同步 Agent
│   │       └── transport.rs           # Socket.IO 传输层
│   │
│   ├── smcp-computer/                 # ⭐ Computer 实现（TFRobot Client 核心依赖）
│   │   └── src/
│   │       ├── computer.rs            # SmcpComputer 主类
│   │       ├── mcp_clients/           # MCP 客户端管理
│   │       │   ├── manager.rs         # MCPServerManager
│   │       │   ├── model.rs           # 配置模型
│   │       │   ├── stdio_client.rs    # STDIO 传输
│   │       │   ├── sse_client.rs      # SSE 传输
│   │       │   └── http_client.rs     # HTTP 传输
│   │       ├── desktop/               # 桌面资源管理
│   │       ├── inputs/                # 输入处理系统
│   │       ├── socketio_client.rs     # Socket.IO 客户端
│   │       └── cli/                   # CLI 工具（可选）
│   │
│   ├── smcp-server-core/              # Server 核心逻辑
│   │   └── src/
│   │       ├── server.rs              # SmcpServerBuilder
│   │       ├── handler.rs             # 事件处理
│   │       └── session.rs             # 会话管理
│   │
│   └── smcp-server-hyper/             # Hyper HTTP 适配器
│
├── tests/                             # 集成测试
└── docs/                              # 文档
```

### smcp-computer 公开 API

```rust
// 核心模块导出
pub mod computer;        // SmcpComputer - Computer 主类
pub mod desktop;         // DesktopInfo, WindowURI - 桌面资源
pub mod errors;          // ComputerError, ComputerResult
pub mod inputs;          // InputHandler - 输入处理
pub mod mcp_clients;     // MCPServerManager - MCP 服务器管理
pub mod socketio_client; // SmcpComputerClient - Socket.IO 客户端

// 关键类型
pub use mcp_clients::{
    MCPServerManager,           // MCP 服务器管理器
    MCPServerConfig,            // 配置枚举 (Stdio/Http/Sse)
    StdioServerConfig,          // STDIO 服务器配置
    HttpServerConfig,           // HTTP 服务器配置
    SseServerConfig,            // SSE 服务器配置
    ToolMeta,                   // 工具元数据
    BaseMCPClient,              // MCP 客户端 trait
};
```

### 关键依赖说明

| 依赖 | 版本 | 用途 |
|------|------|------|
| `tf-rust-socketio` | 0.7.0 | Socket.IO 客户端（自研，基于 rust_socketio 增加了 ACK 响应支持） |
| `socketioxide` | 0.16.3 | Socket.IO 服务器（仅 Server 使用） |
| `rmcp` | 0.11.0 | MCP SDK，支持 stdio/SSE/HTTP 传输 |
| `tokio` | 1.x | 异步运行时 |
| `serde` / `serde_json` | 1.x | JSON 序列化 |

---

## 技术架构

### 整体架构

```
┌─────────────────────────────────────────────────────────────────┐
│                      TFRobot Client                              │
├─────────────────────────────────────────────────────────────────┤
│  ┌─────────────────────────────────────────────────────────┐    │
│  │                    React 前端 (Ant Design)               │    │
│  │  ┌─────────────┐ ┌─────────────┐ ┌─────────────────────┐│    │
│  │  │  MCP 配置   │ │ 资源浏览器  │ │    日志面板         ││    │
│  │  │    管理     │ │  (Desktop)  │ │ (时间线 + 详细)     ││    │
│  │  └─────────────┘ └─────────────┘ └─────────────────────┘│    │
│  │  ┌─────────────┐ ┌─────────────┐ ┌─────────────────────┐│    │
│  │  │ Server 状态 │ │ 连接管理    │ │      设置           ││    │
│  │  │   监控      │ │ (SMCP)      │ │                     ││    │
│  │  └─────────────┘ └─────────────┘ └─────────────────────┘│    │
│  └─────────────────────────────────────────────────────────┘    │
│                              │ Tauri IPC                         │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │                    Rust 后端 (Tauri 2.x)                 │    │
│  │  ┌─────────────┐ ┌─────────────┐ ┌─────────────────────┐│    │
│  │  │smcp-computer│ │   日志管理  │ │    进程管理         ││    │
│  │  │  (crates.io)│ │             │ │  (MCP Servers)      ││    │
│  │  └─────────────┘ └─────────────┘ └─────────────────────┘│    │
│  │  ┌─────────────┐ ┌─────────────┐ ┌─────────────────────┐│    │
│  │  │ 钥匙串集成  │ │ 错误上报    │ │   自动更新          ││    │
│  │  │ (Keychain)  │ │ (Sentry)    │ │   (Tauri Updater)   ││    │
│  │  └─────────────┘ └─────────────┘ └─────────────────────┘│    │
│  └─────────────────────────────────────────────────────────┘    │
├─────────────────────────────────────────────────────────────────┤
│                    内置运行时环境                                 │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐  │
│  │  Node.js 20 LTS │  │  Python 3.11    │  │  uv + pnpm      │  │
│  └─────────────────┘  └─────────────────┘  └─────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

### 核心技术栈

| 层级 | 技术选型 | 说明 |
|------|----------|------|
| 桌面框架 | Tauri 2.x | 跨平台桌面应用框架，基于 Rust 后端 + WebView 前端 |
| 前端框架 | React 18+ | 组件化 UI 开发 |
| UI 组件库 | Ant Design 5.x | 成熟的企业级组件库 |
| 状态管理 | Zustand | 轻量级状态管理 |
| CSS 方案 | CSS Modules | 作用域隔离，与 Ant Design 兼容 |
| 后端核心 | smcp-computer (crates.io) | 复用现有 A2C-SMCP Computer 实现 |
| 内置 Node | Node.js 20 LTS | MCP Server 运行时（Node.js 编写的 MCP Server） |
| 内置 Python | Python 3.11 | MCP Server 运行时（Python 编写的 MCP Server） |
| Python 包管理 | uv | 项目级虚拟环境，共享缓存，速度极快 |
| Node 包管理 | pnpm | 快速，节省空间 |
| 错误上报 | Sentry (自建) | Glitchtip 或自建 Sentry |
| 国际化 | i18next | 中英文双语支持 |

---

## 功能模块详细设计

### 1. MCP Server 配置管理

#### 1.1 可视化配置编辑器

MCP Server 配置在 rust-sdk 中对应 `MCPServerConfig` 枚举：

```rust
// 来自 smcp-computer/src/mcp_clients/model.rs
pub enum MCPServerConfig {
    Stdio(StdioServerConfig),   // 子进程通信
    Http(HttpServerConfig),     // HTTP 直接调用
    Sse(SseServerConfig),       // Server-Sent Events
}

pub struct StdioServerConfig {
    pub name: String,           // 服务器名称
    pub command: String,        // 启动命令 (如 "npx", "python")
    pub args: Vec<String>,      // 命令参数 (如 ["-m", "mcp_server"])
    pub env: HashMap<String, String>,  // 环境变量
    pub cwd: Option<PathBuf>,   // 工作目录
    pub disabled: bool,         // 是否禁用
    pub forbidden_tools: Vec<String>,  // 禁用的工具列表
    pub tool_meta: HashMap<String, ToolMeta>,  // 工具元数据
    pub inputs: Vec<MCPServerInput>,   // 输入定义（用于动态占位符）
}
```

**UI 设计要点**：
- 表单界面替代 JSON 手动编辑
- 根据传输类型（stdio / SSE / HTTP）动态切换表单字段
- 环境变量支持键值对列表编辑
- 验证方式：静态格式校验（必填字段、路径格式等），不进行实时健康检查

#### 1.2 运行时环境选择

MCP Server 通常用 Node.js 或 Python 编写，需要对应的运行时环境：

- **默认行为**：优先使用内置环境（Node 20 / Python 3.11）
- **环境切换**：用户可选择使用系统自带环境（路径自动检测或手动指定）
- **版本不兼容处理**：当内置版本不满足 Server 要求时，引导用户切换到自己的环境

#### 1.3 依赖管理策略

- **混合策略**：
  - 核心常用依赖预装在安装包内
  - 非核心依赖在配置 MCP Server 时动态安装
- **Python 依赖**：使用 uv 为每个 Server 创建独立虚拟环境，共享包缓存
- **Node 依赖**：在 Server 目录本地执行 pnpm install

### 2. MCP Server 进程管理

#### 2.1 启动策略

在 rust-sdk 中，`MCPServerManager` 负责管理所有 MCP Server 的生命周期：

```rust
// 初始化并启动所有服务器
let manager = MCPServerManager::new();
manager.initialize(configs).await?;
manager.start_all().await?;

// 单个服务器操作
manager.start_client("server_name").await?;
manager.stop_client("server_name").await?;
```

**UI 设计要点**：
- 客户端启动时启动所有已配置的 MCP Server
- 显示每个 Server 的运行状态（运行中 / 已停止 / 错误）
- 提供启动 / 停止 / 重启操作按钮

#### 2.2 系统托盘常驻

- **最小化到托盘**：关闭窗口时最小化到系统托盘，MCP Server 群继续运行
- **托盘菜单**：快捷操作（显示窗口 / 全部停止 / 退出）
- **状态指示**：托盘图标反映整体运行状态

### 3. SMCP Server 连接管理

#### 3.1 连接模式

Computer 通过 Socket.IO 连接到 SMCP Server，在 rust-sdk 中使用 `SmcpComputerClient`：

```rust
// 连接到 SMCP Server
let client = SmcpComputerClient::connect(
    "https://smcp-server.example.com",
    "my-computer-name",
    api_key,
).await?;

// 加入房间
client.join_office("office_id").await?;
```

**连接模式**：
- **远程 Server 模式**：Computer 连接到云端或自建的 SMCP Server
- **单 Server 连接**：每个客户端实例只连接一个 SMCP Server

#### 3.2 认证管理

- **集中管理 UI**：类似浏览器密码管理器的统一管理界面
- **安全存储**：使用系统钥匙串（macOS Keychain / Windows Credential Manager）
- **多 Server 支持**：虽然同时只连一个，但可保存多个 Server 的凭证便于切换

#### 3.3 断线处理

- **自动重连**：依赖 Socket.IO 内置的重连机制（`tf-rust-socketio` 支持）
- **失败处理**：Socket.IO 重连失败后提示用户检查网络和 Server 状态

### 4. Desktop 资源浏览器

#### 4.1 资源类型

Desktop 资源在 rust-sdk 中通过 `desktop` 模块管理：

```rust
// 获取桌面资源
let desktop_info = computer.get_desktop().await?;

// 资源类型
// - window://  窗口资源（各 MCP Server 暴露的窗口）
// - file://    文件资源
// - clipboard:// 剪贴板内容
```

**资源类型说明**：
- **窗口资源** (window://): 系统窗口列表，由 MCP Server 暴露
- **文件资源** (file://): 文件系统浏览
- **剪贴板资源** (clipboard://): 剪贴板内容
- **其他 MCP 资源**: 各 MCP Server 暴露的资源

#### 4.2 访问权限

- **跟随系统**：不做额外权限限制，依赖 macOS / Windows 系统级权限管理

#### 4.3 刷新策略

- **手动刷新**：用户点击刷新按钮时更新资源列表，避免高频变化导致 UI 闪烁

### 5. 日志管理系统

#### 5.1 双层日志架构

```
┌─────────────────────────────────────────────┐
│          用户友好层 (时间线视图)             │
│  - 工具调用记录                              │
│  - 简化的事件描述                            │
│  - 成功/失败状态                             │
└─────────────────────────────────────────────┘
                      │
┌─────────────────────────────────────────────┐
│          开发者详细层 (技术日志)             │
│  - 完整通信日志                              │
│  - 堆栈跟踪                                  │
│  - 请求/响应详情                             │
└─────────────────────────────────────────────┘
```

#### 5.2 用户友好日志

- **展示形式**：时间线形式，类似聊天记录
- **事件范围**：仅显示工具调用记录（不包括系统事件）
- **信息展示**：时间戳、工具名、调用参数摘要、结果状态

#### 5.3 开发者详细日志

- **用途**：技术支持时导出分析
- **导出功能**：一键导出日志文件
- **保留策略**：30 天自动清理

### 6. 客户端自动更新

#### 6.1 更新机制

- **检测方式**：使用 Tauri 内置 updater
- **提示方式**：弹窗提示用户确认更新
- **更新流程**：后台下载 → 提示用户 → 用户确认 → 重启应用更新

### 7. 错误上报

#### 7.1 上报机制

- **服务选型**：自建 Sentry 或 Glitchtip
- **用户授权**：首次启动时征求用户同意
- **上报内容**：崩溃信息、未捕获异常、关键错误

---

## 平台支持

### 目标平台

| 平台 | 优先级 | 特殊处理 |
|------|--------|----------|
| macOS | P0 | Keychain 集成、DMG 分发 |
| Windows | P0 | Credential Manager 集成、MSI/NSIS 分发 |
| Linux | P2 | 暂不专门支持，可能可用 |

### 分发方式

- **仅官网下载**：通过 GitHub Releases 或官网分发
- **安装包内容**：包含完整内置运行时（Node.js + Python + uv + pnpm）

---

## 国际化 (i18n)

### 语言支持

- **初版语言**：中文 + 英文
- **技术方案**：i18next + react-i18next
- **切换方式**：设置中手动切换，跟随系统语言作为默认值

---

## 工程结构

### 仓库结构

```
tfrobot-client/                    # 独立新仓库
├── src-tauri/                     # Rust 后端
│   ├── src/
│   │   ├── main.rs
│   │   ├── commands/              # Tauri commands
│   │   │   ├── mcp.rs             # MCP Server 管理
│   │   │   ├── connection.rs      # SMCP 连接管理
│   │   │   ├── resources.rs       # Desktop 资源
│   │   │   └── logs.rs            # 日志管理
│   │   ├── services/              # 业务逻辑
│   │   │   ├── mcp_manager.rs     # MCP Server 进程管理
│   │   │   ├── keychain.rs        # 钥匙串集成
│   │   │   └── logger.rs          # 日志服务
│   │   └── lib.rs
│   ├── Cargo.toml                 # 依赖 smcp-computer (crates.io)
│   └── tauri.conf.json
├── src/                           # React 前端
│   ├── components/                # UI 组件
│   │   ├── McpConfig/             # MCP 配置管理
│   │   ├── ResourceBrowser/       # 资源浏览器
│   │   ├── LogPanel/              # 日志面板
│   │   ├── ServerStatus/          # Server 状态监控
│   │   └── Settings/              # 设置页面
│   ├── stores/                    # Zustand stores
│   │   ├── mcpStore.ts
│   │   ├── connectionStore.ts
│   │   └── logStore.ts
│   ├── hooks/                     # 自定义 hooks
│   ├── locales/                   # i18n 翻译文件
│   │   ├── en/
│   │   └── zh/
│   ├── styles/                    # CSS Modules
│   ├── App.tsx
│   └── main.tsx
├── package.json
├── pnpm-lock.yaml
├── tsconfig.json
└── vite.config.ts
```

### Cargo.toml 依赖配置

```toml
[package]
name = "tfrobot-client"
version = "0.1.0"
edition = "2021"

[dependencies]
# Tauri 核心
tauri = { version = "2", features = ["tray-icon", "updater"] }
tauri-plugin-shell = "2"

# A2C-SMCP Computer 实现（已发布到 crates.io）
smcp-computer = "0.1"
# 或使用主包（包含所有组件）：
# a2c-smcp = { version = "0.1", features = ["computer"] }

# 序列化
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# 异步运行时
tokio = { version = "1", features = ["full"] }

# 日志
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# 跨平台钥匙串
keyring = "3"

# 错误处理
thiserror = "2"
anyhow = "1"
```

---

## 关键技术实现要点

### 1. 内置运行时打包

需要在安装包中嵌入以下运行时：

| 运行时 | 来源 | 说明 |
|--------|------|------|
| Node.js 20 LTS | [nodejs.org/dist](https://nodejs.org/dist/) | 预编译二进制，分平台打包 |
| Python 3.11 | [python-build-standalone](https://github.com/indygreg/python-build-standalone) | Standalone build，无需系统依赖 |
| uv | [astral.sh/uv](https://github.com/astral-sh/uv) | Rust 编写的 Python 包管理器 |
| pnpm | 通过内置 Node 运行 | `node_modules/.bin/pnpm` |

**Tauri 资源配置** (`tauri.conf.json`)：
```json
{
  "bundle": {
    "resources": [
      "resources/node/**",
      "resources/python/**",
      "resources/uv/**"
    ]
  }
}
```

### 2. 系统钥匙串集成

使用 `keyring` crate 实现跨平台凭证存储：

```rust
use keyring::Entry;

const SERVICE_NAME: &str = "tfrobot-client";

pub fn save_credential(server_url: &str, api_key: &str) -> Result<()> {
    let entry = Entry::new(SERVICE_NAME, server_url)?;
    entry.set_password(api_key)?;
    Ok(())
}

pub fn get_credential(server_url: &str) -> Result<Option<String>> {
    let entry = Entry::new(SERVICE_NAME, server_url)?;
    match entry.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn delete_credential(server_url: &str) -> Result<()> {
    let entry = Entry::new(SERVICE_NAME, server_url)?;
    entry.delete_credential()?;
    Ok(())
}
```

### 3. Tauri 与 smcp-computer 集成

通过 Tauri commands 暴露 smcp-computer 功能：

```rust
use smcp_computer::mcp_clients::{MCPServerManager, MCPServerConfig};
use tauri::State;
use std::sync::Arc;
use tokio::sync::RwLock;

// 应用状态
pub struct AppState {
    pub manager: Arc<RwLock<MCPServerManager>>,
}

#[tauri::command]
async fn start_mcp_server(
    state: State<'_, AppState>,
    config: MCPServerConfig,
) -> Result<(), String> {
    let mut manager = state.manager.write().await;
    manager.add_server(config).await.map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn stop_mcp_server(
    state: State<'_, AppState>,
    server_name: String,
) -> Result<(), String> {
    let mut manager = state.manager.write().await;
    manager.stop_client(&server_name).await.map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn get_available_tools(
    state: State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    let manager = state.manager.read().await;
    let tools: Vec<_> = manager.available_tools()
        .await
        .map_err(|e| e.to_string())?;
    Ok(tools)
}

#[tauri::command]
async fn call_tool(
    state: State<'_, AppState>,
    tool_name: String,
    params: serde_json::Value,
    timeout_secs: Option<u64>,
) -> Result<serde_json::Value, String> {
    let manager = state.manager.read().await;
    let result = manager
        .call_tool(&tool_name, params, timeout_secs.map(std::time::Duration::from_secs))
        .await
        .map_err(|e| e.to_string())?;
    Ok(result)
}
```

### 4. 双层日志实现

```rust
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};

/// 用户友好层：过滤并简化
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserFriendlyLog {
    pub timestamp: DateTime<Utc>,
    pub tool_name: String,
    pub status: CallStatus,
    pub summary: String,  // 参数摘要
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CallStatus {
    Pending,
    Success,
    Failed { error: String },
}

/// 开发者层：完整信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetailedLog {
    pub timestamp: DateTime<Utc>,
    pub level: LogLevel,
    pub component: String,
    pub message: String,
    pub context: serde_json::Value,
    pub stack_trace: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}
```

### 5. 前端 Tauri IPC 调用示例

```typescript
// src/hooks/useMcpServers.ts
import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';

interface McpServerState {
  servers: McpServerStatus[];
  loading: boolean;
  startServer: (config: McpServerConfig) => Promise<void>;
  stopServer: (name: string) => Promise<void>;
  refreshServers: () => Promise<void>;
}

export const useMcpServerStore = create<McpServerState>((set, get) => ({
  servers: [],
  loading: false,

  startServer: async (config) => {
    set({ loading: true });
    try {
      await invoke('start_mcp_server', { config });
      await get().refreshServers();
    } finally {
      set({ loading: false });
    }
  },

  stopServer: async (name) => {
    set({ loading: true });
    try {
      await invoke('stop_mcp_server', { serverName: name });
      await get().refreshServers();
    } finally {
      set({ loading: false });
    }
  },

  refreshServers: async () => {
    const servers = await invoke<McpServerStatus[]>('get_server_status');
    set({ servers });
  },
}));
```

---

## 初始化步骤（详细）

### 前置条件

确保开发环境已安装：
- Node.js 20+ (推荐使用 nvm 管理)
- pnpm 9+ (`npm install -g pnpm`)
- Rust 1.75+ (`rustup update stable`)
- Tauri CLI (`cargo install tauri-cli`)

macOS 额外需要：
- Xcode Command Line Tools (`xcode-select --install`)

Windows 额外需要：
- Visual Studio Build Tools 2022
- WebView2 Runtime

### 1. 创建项目

```bash
# 创建新仓库
mkdir tfrobot-client && cd tfrobot-client
git init

# 初始化 Tauri 2.x + React + TypeScript 项目
pnpm create tauri-app --template react-ts

# 安装前端依赖
pnpm install
pnpm add antd @ant-design/icons zustand i18next react-i18next
pnpm add -D @types/node
```

### 2. 配置 Tauri

编辑 `src-tauri/tauri.conf.json`：

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "TFRobot Client",
  "version": "0.1.0",
  "identifier": "com.tfrobot.client",
  "build": {
    "beforeBuildCommand": "pnpm build",
    "beforeDevCommand": "pnpm dev",
    "devUrl": "http://localhost:1420",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [
      {
        "title": "TFRobot Client",
        "width": 1200,
        "height": 800,
        "minWidth": 800,
        "minHeight": 600,
        "resizable": true
      }
    ],
    "trayIcon": {
      "iconPath": "icons/icon.png",
      "iconAsTemplate": true
    }
  },
  "bundle": {
    "active": true,
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ],
    "resources": [
      "resources/**/*"
    ],
    "macOS": {
      "minimumSystemVersion": "10.15"
    },
    "windows": {
      "certificateThumbprint": null,
      "digestAlgorithm": "sha256",
      "timestampUrl": ""
    }
  },
  "plugins": {
    "updater": {
      "pubkey": "",
      "endpoints": []
    },
    "shell": {
      "all": false,
      "execute": true,
      "sidecar": true,
      "scope": []
    }
  }
}
```

### 3. 添加 Rust 依赖

编辑 `src-tauri/Cargo.toml`：

```toml
[package]
name = "tfrobot-client"
version = "0.1.0"
edition = "2021"
description = "TFRobot Client - A2C-SMCP Computer GUI"
authors = ["Your Name"]

[lib]
name = "tfrobot_client_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
# Tauri 核心
tauri = { version = "2", features = ["tray-icon"] }
tauri-plugin-shell = "2"
tauri-plugin-updater = "2"

# A2C-SMCP Computer 实现（已发布到 crates.io）
smcp-computer = "0.1"

# 序列化
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# 异步运行时
tokio = { version = "1", features = ["full"] }

# 日志
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }

# 跨平台钥匙串
keyring = "3"

# 错误处理
thiserror = "2"
anyhow = "1"

# 时间处理
chrono = { version = "0.4", features = ["serde"] }
```

### 4. 设置内置运行时

创建资源目录结构：

```bash
mkdir -p src-tauri/resources/{node,python,uv}
```

下载并解压各平台的运行时二进制到对应目录：
- macOS arm64: `resources/node/node-v20.x.x-darwin-arm64/`
- macOS x64: `resources/node/node-v20.x.x-darwin-x64/`
- Windows x64: `resources/node/node-v20.x.x-win-x64/`

创建运行时检测模块 `src-tauri/src/services/runtime.rs`：

```rust
use std::path::PathBuf;
use tauri::AppHandle;

pub struct RuntimePaths {
    pub node: PathBuf,
    pub python: PathBuf,
    pub uv: PathBuf,
    pub pnpm: PathBuf,
}

impl RuntimePaths {
    pub fn from_app(app: &AppHandle) -> Result<Self, anyhow::Error> {
        let resource_dir = app.path().resource_dir()?;

        #[cfg(target_os = "macos")]
        let (node_dir, python_dir) = {
            #[cfg(target_arch = "aarch64")]
            let arch = "arm64";
            #[cfg(target_arch = "x86_64")]
            let arch = "x64";

            (
                resource_dir.join(format!("node/node-v20-darwin-{}/bin/node", arch)),
                resource_dir.join(format!("python/python-3.11-darwin-{}/bin/python3", arch)),
            )
        };

        #[cfg(target_os = "windows")]
        let (node_dir, python_dir) = (
            resource_dir.join("node/node-v20-win-x64/node.exe"),
            resource_dir.join("python/python-3.11-win-x64/python.exe"),
        );

        Ok(Self {
            node: node_dir,
            python: python_dir,
            uv: resource_dir.join("uv/uv"),
            pnpm: resource_dir.join("node/pnpm/bin/pnpm.cjs"),
        })
    }
}
```

### 5. 配置 i18n

创建翻译文件：

`src/locales/en/translation.json`:
```json
{
  "app": {
    "name": "TFRobot Client"
  },
  "mcp": {
    "servers": "MCP Servers",
    "addServer": "Add Server",
    "startAll": "Start All",
    "stopAll": "Stop All",
    "status": {
      "running": "Running",
      "stopped": "Stopped",
      "error": "Error"
    }
  },
  "connection": {
    "smcpServer": "SMCP Server",
    "connect": "Connect",
    "disconnect": "Disconnect",
    "connected": "Connected",
    "disconnected": "Disconnected"
  },
  "settings": {
    "title": "Settings",
    "language": "Language",
    "theme": "Theme"
  }
}
```

`src/locales/zh/translation.json`:
```json
{
  "app": {
    "name": "TFRobot 客户端"
  },
  "mcp": {
    "servers": "MCP 服务器",
    "addServer": "添加服务器",
    "startAll": "全部启动",
    "stopAll": "全部停止",
    "status": {
      "running": "运行中",
      "stopped": "已停止",
      "error": "错误"
    }
  },
  "connection": {
    "smcpServer": "SMCP 服务器",
    "connect": "连接",
    "disconnect": "断开",
    "connected": "已连接",
    "disconnected": "未连接"
  },
  "settings": {
    "title": "设置",
    "language": "语言",
    "theme": "主题"
  }
}
```

`src/i18n.ts`:
```typescript
import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import en from './locales/en/translation.json';
import zh from './locales/zh/translation.json';

// 检测系统语言
const getDefaultLanguage = () => {
  const lang = navigator.language.toLowerCase();
  return lang.startsWith('zh') ? 'zh' : 'en';
};

i18n.use(initReactI18next).init({
  resources: {
    en: { translation: en },
    zh: { translation: zh },
  },
  lng: getDefaultLanguage(),
  fallbackLng: 'en',
  interpolation: {
    escapeValue: false,
  },
});

export default i18n;
```

### 6. 启动开发

```bash
# 启动开发服务器
pnpm tauri dev
```

---

## 待定事项

1. ~~**smcp-computer 发布**：需要先将 rust-sdk 中的 smcp-* crates 发布到 crates.io~~ ✅ 已完成（2026-02-03）
2. **错误上报服务部署**：需要部署自建的 Sentry/Glitchtip 实例
3. **自动更新服务器**：需要配置 Tauri updater 使用的更新服务器地址
4. **内置运行时版本锁定**：确定打包的具体 Node/Python 版本号
5. **核心预装依赖列表**：确定需要预装的 Python/Node 包列表

---

## 里程碑建议

| 阶段 | 内容 |
|------|------|
| M1 | 项目脚手架搭建、基础 UI 框架、Tauri 配置 |
| M2 | MCP Server 配置管理（CRUD）、进程启动/停止 |
| M3 | SMCP Server 连接、钥匙串集成 |
| M4 | Desktop 资源浏览器 |
| M5 | 日志系统（双层架构） |
| M6 | 系统托盘、自动更新 |
| M7 | 错误上报、i18n |
| M8 | 内置运行时打包、分发准备 |

---

## 参考资料

- **A2C-SMCP 协议规范**: https://github.com/A2C-SMCP/a2c-smcp-protocol
- **Rust SDK 仓库**: https://github.com/a2c-smcp/rust-sdk
- **Python SDK 参考实现**: https://github.com/a2c-smcp/python-sdk
- **Tauri 2.0 文档**: https://v2.tauri.app/
- **MCP 协议规范**: https://modelcontextprotocol.io/
