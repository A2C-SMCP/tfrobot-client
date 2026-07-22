# Phase 1: 核心基础 — 技术执行 Spec

> 历史说明：本文档记录早期 Phase 1 设计与实现状态，包含旧 `MCPServerManager`
> 主路径描述。SDK Computer 架构对齐后的当前实现以
> `plans/SDK_CLIENT_ARCHITECTURE_ALIGNMENT.md` 为准。

> **状态**: ✅ 已完成
> **对应 PRD**: 3.2 MCP 服务器管理（基础部分）

---

## 已实现清单

本 Phase 已在 commit `a98d729` 中完成，以下为已实现内容的技术记录，供后续 Phase 参考。

### 后端 (Rust)

**AppState** (`src-tauri/src/lib.rs`):
```rust
pub struct AppState {
    pub manager: Arc<RwLock<MCPServerManager>>,
    pub config: Arc<ConfigService>,
}
```

**已注册 Tauri Commands** (8 个 MCP + 3 个 connection stub + 2 个 logs stub):
- `get_mcp_servers` → `Vec<McpServerStatus>`
- `add_mcp_server(config: MCPServerConfig)`
- `update_mcp_server(config: MCPServerConfig)`
- `remove_mcp_server(name: String)`
- `start_mcp_server(name: String)`
- `stop_mcp_server(name: String)`
- `start_all_servers()`
- `stop_all_servers()`
- `connect_smcp` / `disconnect_smcp` / `get_connection_status` (stub)
- `get_logs` / `export_logs` (stub)

**Services**:
- `ConfigService`: JSON 文件持久化 (`mcp_servers.json`)
- `KeychainService`: keyring crate 封装 (`save/get/delete_credential`)
- `RuntimeService`: 平台特定运行时路径检测
- `LogManager`: 仅 tracing 输出，无持久化

**依赖** (`Cargo.toml`):
- `smcp-computer = "0.1"`
- `tauri = "2"` (features: tray-icon)
- `tauri-plugin-shell = "2"`, `tauri-plugin-updater = "2"`
- `keyring = "3"`, `chrono = "0.4"`, `thiserror = "2"`, `anyhow = "1"`

### 前端 (React)

**组件**:
- `App.tsx`: Layout + 扁平侧边栏 (mcp/connection/resources/logs/settings)
- `McpConfig/index.tsx`: CRUD 容器 + 批量操作
- `McpConfig/McpServerList.tsx`: Ant Design Table
- `McpConfig/McpServerForm.tsx`: 动态表单 (Stdio/HTTP/SSE)
- `McpConfig/ServerStatusBadge.tsx`: 状态徽标

**Store**: `stores/mcpStore.ts` — Zustand, 9 个 async actions

**i18n**: `locales/en/` + `locales/zh/` — 97 个翻译 key

**依赖** (`package.json`):
- `antd ^5.23.4`, `@ant-design/icons ^5.5.2`
- `zustand ^5.0.3`, `i18next ^24.2.2`, `react-i18next ^15.4.0`
- `@tauri-apps/api ^2.2.0`

### 已知限制（后续 Phase 需解决）

1. `McpServerStatus` 缺少 `type` 字段 — 后续 Phase 5 高级配置表单需要
2. `handleEdit` 未实现 — 需要 `get_server_config(name)` 命令
3. `McpServerConfig` TS 类型使用 `{ Stdio: ... }` tagged union — 与 Rust serde 对齐
4. AppState 仅包含 `MCPServerManager` + `ConfigService`，后续需扩展
5. 侧边栏结构为扁平列表，Phase 2 需改为分组结构
