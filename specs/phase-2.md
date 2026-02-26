# Phase 2: 连接与变量 — 技术执行 Spec

> **状态**: ✅ 已完成
> **对应 PRD**: 3.3 输入变量管理, 3.4 SMCP 服务器连接, 3.2.3 配置导入/导出, 2.1 分组侧边栏

---

## 已实现清单

### 后端 (Rust)

**AppState 变更** (`src-tauri/src/lib.rs`):
```rust
pub struct AppState {
    pub manager: Arc<RwLock<Option<MCPServerManager>>>,  // Option 以兼容 SmcpComputerClient
    pub config: Arc<ConfigService>,
    pub inputs: Arc<RwLock<HashMap<String, MCPServerInput>>>,  // 输入变量共享状态
    pub connection: Arc<RwLock<Option<ConnectionState>>>,       // 活跃 SMCP 连接
}
```

**新增 Tauri Commands**:

输入变量管理 (10 个) — `commands/inputs.rs`:
- `list_inputs` / `get_input` / `add_or_update_input` / `remove_input`
- `list_input_values` / `get_input_value` / `set_input_value` / `remove_input_value`
- `clear_input_values` / `import_inputs`

SMCP 连接管理 (6 个) — `commands/connection.rs`:
- `list_profiles` / `save_profile` / `delete_profile`
- `connect_smcp` (通过 SmcpComputerClient + join_office)
- `disconnect_smcp` / `get_connection_status`

配置导入/导出 (3 个) — `commands/config_io.rs`:
- `detect_config_format` / `import_config` / `export_config`
- 支持 CLI 原生 JSON 和 Claude Desktop 格式自动检测

**ConfigService 扩展** (`services/config.rs`):
- `load_inputs()` / `save_inputs()` — `inputs.json`
- `load_input_values()` / `save_input_values()` — `input_values.json`
- `load_profiles()` / `save_profiles()` — `connection_profiles.json`

**新增依赖**: `tauri-plugin-dialog`

### 前端 (React)

**新增组件**:
- `InputVariables/index.tsx` — 变量列表 + CRUD + 值管理
- `InputVariables/InputForm.tsx` — 三种类型变量定义表单
- `InputVariables/InputValueEditor.tsx` — 根据类型渲染 Input 或 Select
- `SmcpConnection/index.tsx` — Profile 列表 + 连接状态卡片
- `SmcpConnection/ProfileForm.tsx` — Profile 创建/编辑表单 + Keychain 集成

**新增 Store**:
- `stores/inputStore.ts` — 9 个 async actions
- `stores/connectionStore.ts` — 6 个 async actions

**mcpStore 扩展**: `importConfig()` / `exportConfig()` + `ImportResult` 类型

**侧边栏重构** (`App.tsx`):
- 概览: Dashboard
- 配置: MCP 服务器, 输入变量
- 连接: SMCP 服务器, 桌面资源
- 开发: 调试面板, 日志
- 系统: 设置

**MCP 配置页导入/导出**: 文件对话框 + 格式自动检测

**i18n**: 新增 ~80 个翻译 key (inputs.*, connection.*, nav.*, dashboard.*, debug.*)

**新增前端依赖**: `@tauri-apps/plugin-dialog`

### 已知限制（后续 Phase 需解决）

1. 连接向导简化为普通表单（可在 Phase 5 增强为 Steps 组件）
2. MCP 配置中的 `${input:id}` placeholder 可视化引用尚未实现
3. 自动状态同步（MCP/变量变更时自动 emit_update）仅在连接时触发
4. Office 成员列表（list_room）尚未实现
5. Command 类型变量的手动执行按钮未实现
6. SMCP 事件未通过 Tauri Event System 推送到前端
