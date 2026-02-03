# Phase 1: MCP Server 管理

## 目标

实现 MCP Server 的完整 CRUD 和生命周期管理，这是整个应用的核心功能。

## 前置条件

- [x] 项目脚手架已搭建
- [ ] `smcp-computer` crate 可从 crates.io 安装

## 任务清单

### 1.1 后端：集成 smcp-computer

- [ ] 取消注释 `Cargo.toml` 中的 `smcp-computer` 依赖
- [ ] 验证依赖可正常编译
- [ ] 创建 `AppState` 结构，持有 `MCPServerManager`
- [ ] 在 `lib.rs` 中初始化并注入状态

**参考代码** (来自 PLAN.md):
```rust
use smcp_computer::mcp_clients::{MCPServerManager, MCPServerConfig};
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct AppState {
    pub manager: Arc<RwLock<MCPServerManager>>,
}
```

### 1.2 后端：实现 MCP 命令

更新 `src-tauri/src/commands/mcp.rs`：

- [ ] `get_mcp_servers()` - 获取所有服务器状态
- [ ] `add_mcp_server(config)` - 添加新服务器
- [ ] `remove_mcp_server(name)` - 移除服务器
- [ ] `start_mcp_server(name)` - 启动单个服务器
- [ ] `stop_mcp_server(name)` - 停止单个服务器
- [ ] `start_all_servers()` - 启动所有服务器
- [ ] `stop_all_servers()` - 停止所有服务器
- [ ] `update_mcp_server(name, config)` - 更新服务器配置

### 1.3 后端：配置持久化

- [ ] 创建 `src-tauri/src/services/config.rs`
- [ ] 使用 Tauri 的 `app_data_dir` 存储配置
- [ ] 配置文件格式：JSON
- [ ] 应用启动时自动加载配置
- [ ] 配置变更时自动保存

**配置文件路径**:
- macOS: `~/Library/Application Support/com.tfrobot.client/mcp_servers.json`
- Windows: `%APPDATA%\com.tfrobot.client\mcp_servers.json`

### 1.4 前端：Zustand Store

创建 `src/stores/mcpStore.ts`：

- [ ] 状态定义：`servers`, `loading`, `error`
- [ ] Actions：`fetchServers`, `addServer`, `removeServer`, `startServer`, `stopServer`
- [ ] 调用 Tauri IPC

### 1.5 前端：MCP 配置组件

创建 `src/components/McpConfig/` 目录：

- [ ] `McpServerList.tsx` - 服务器列表（表格形式）
- [ ] `McpServerForm.tsx` - 添加/编辑表单（Modal）
- [ ] `ServerStatusBadge.tsx` - 状态徽章组件
- [ ] `index.tsx` - 主组件整合

**UI 功能**:
- [ ] 显示所有服务器及其状态（运行中/已停止/错误）
- [ ] 添加服务器按钮 → 弹出配置表单
- [ ] 表单根据类型（stdio/http/sse）动态切换字段
- [ ] 环境变量支持键值对编辑器
- [ ] 每行操作：启动/停止/编辑/删除
- [ ] 批量操作：全部启动/全部停止

### 1.6 前端：更新 App.tsx

- [ ] 替换 MCP 占位内容为 `<McpConfig />` 组件

## 验收标准

1. 可以通过 UI 添加一个 stdio 类型的 MCP Server
2. 添加的服务器配置在重启后仍然存在
3. 可以启动/停止服务器，状态正确显示
4. 可以编辑和删除服务器

## 技术注意事项

- 所有 Tauri 命令返回 `Result<T, String>`
- 使用 `tracing` 记录关键操作日志
- 前端使用 Ant Design 的 `Table`, `Modal`, `Form` 组件

## 预计文件变更

```
src-tauri/
├── Cargo.toml                    # 取消注释 smcp-computer
├── src/
│   ├── lib.rs                    # 添加 AppState
│   ├── commands/
│   │   └── mcp.rs                # 实现所有命令
│   └── services/
│       └── config.rs             # 新增：配置持久化

src/
├── stores/
│   └── mcpStore.ts               # 新增
├── components/
│   └── McpConfig/
│       ├── index.tsx             # 新增
│       ├── McpServerList.tsx     # 新增
│       ├── McpServerForm.tsx     # 新增
│       └── ServerStatusBadge.tsx # 新增
├── App.tsx                       # 修改
└── locales/
    ├── en/translation.json       # 补充翻译
    └── zh/translation.json       # 补充翻译
```
