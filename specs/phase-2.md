# Phase 2: 连接与变量 — 技术执行 Spec

> **状态**: 待开发
> **对应 PRD**: 3.3 输入变量管理, 3.4 SMCP 服务器连接, 3.2.3 配置导入/导出, 2.1 分组侧边栏
> **前置**: Phase 1 (已完成)

---

## 1. 目标

实现 SMCP 连接 Profile 管理、Input 变量系统、配置导入/导出、侧边栏重构。完成后应用具备与 CLI 的 `socket`、`inputs`、`notify update`、`server add @file` 等命令对等的能力。

---

## 2. 后端变更

### 2.1 AppState 扩展

**文件**: `src-tauri/src/lib.rs`

将 AppState 从仅包含 `MCPServerManager` 升级为包含完整 `Computer` 实例：

```rust
use smcp_computer::computer::{Computer, SilentSession};

pub struct AppState {
    pub computer: Arc<RwLock<Computer<SilentSession>>>,
    pub config: Arc<ConfigService>,
    pub profile_service: Arc<ProfileService>,
    pub settings_service: Arc<SettingsService>,
}
```

**关键变更**：
- 用 `Computer<SilentSession>` 替代直接使用 `MCPServerManager`
- Computer 内部已包含 MCPServerManager、InputManager、Socket.IO client
- `SilentSession` 用于无交互输入解析（GUI 通过 `set_input_value` 预设值）

**初始化** (`setup` 闭包中):
```rust
let session = SilentSession::new("tfrobot-client");
let saved_configs = config_service.load_configs().unwrap_or_default();
let saved_inputs = config_service.load_inputs().unwrap_or_default();

// 转换为 HashMap
let servers_map: HashMap<String, MCPServerConfig> = saved_configs
    .into_iter()
    .map(|c| (c.name().to_string(), c))
    .collect();
let inputs_map: HashMap<String, MCPServerInput> = saved_inputs
    .into_iter()
    .map(|i| (i.id().to_string(), i))
    .collect();

let computer = Computer::new(
    "tfrobot-client",
    session,
    Some(inputs_map),
    Some(servers_map),
    false,  // auto_connect = false (用户手动连接)
    true,   // auto_reconnect = true
);

// 后台初始化
let computer_clone = computer.clone(); // Computer 内部是 Arc
tauri::async_runtime::spawn(async move {
    if let Err(e) = computer_clone.boot_up().await {
        tracing::error!("Failed to boot computer: {}", e);
    }
});
```

**MCP 命令迁移**: 所有现有 MCP 命令从 `state.manager` 改为 `state.computer`:
```rust
// 之前
let manager = state.manager.read().await;
manager.start_client(&name).await?;

// 之后
let computer = state.computer.read().await;
computer.start_mcp_client(&name).await?;
```

### 2.2 新增 Tauri Commands

#### 2.2.1 连接管理 (`src-tauri/src/commands/connection.rs`)

替换现有 3 个 stub，实现完整连接管理：

```rust
use crate::AppState;
use crate::services::profile::ConnectionProfile;
use tauri::State;

/// 列出所有连接 Profile
#[tauri::command]
pub async fn list_profiles(
    state: State<'_, AppState>,
) -> Result<Vec<ConnectionProfile>, String> {
    state.profile_service
        .list_profiles()
        .map_err(|e| e.to_string())
}

/// 保存连接 Profile（新建或更新）
/// API Key 通过 keychain 单独存储，profile 中仅存引用标识
#[tauri::command]
pub async fn save_profile(
    state: State<'_, AppState>,
    profile: ConnectionProfile,
    api_key: Option<String>,
    sensitive_headers: Option<HashMap<String, String>>,
) -> Result<(), String> {
    // 存储敏感信息到 keychain
    if let Some(key) = api_key {
        crate::services::keychain::save_credential(
            &format!("profile:{}", profile.name),
            &key
        ).map_err(|e| e.to_string())?;
    }
    if let Some(headers) = sensitive_headers {
        let headers_json = serde_json::to_string(&headers)
            .map_err(|e| e.to_string())?;
        crate::services::keychain::save_credential(
            &format!("profile-headers:{}", profile.name),
            &headers_json
        ).map_err(|e| e.to_string())?;
    }

    state.profile_service
        .save_profile(profile)
        .map_err(|e| e.to_string())
}

/// 删除连接 Profile（同时清理 keychain）
#[tauri::command]
pub async fn delete_profile(
    state: State<'_, AppState>,
    name: String,
) -> Result<(), String> {
    let _ = crate::services::keychain::delete_credential(
        &format!("profile:{}", name)
    );
    let _ = crate::services::keychain::delete_credential(
        &format!("profile-headers:{}", name)
    );
    state.profile_service
        .delete_profile(&name)
        .map_err(|e| e.to_string())
}

/// 使用 Profile 连接 SMCP 服务器（connect + join_office）
#[tauri::command]
pub async fn connect_smcp(
    state: State<'_, AppState>,
    profile_name: String,
) -> Result<(), String> {
    let profile = state.profile_service
        .get_profile(&profile_name)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Profile not found: {}", profile_name))?;

    // 从 keychain 获取 API Key
    let api_key = crate::services::keychain::get_credential(
        &format!("profile:{}", profile.name)
    ).map_err(|e| e.to_string())?;

    let computer = state.computer.read().await;

    // Step 1: connect_socketio
    computer.connect_socketio(
        &profile.url,
        &profile.namespace,
        &api_key,            // Option<String>
        &None,               // headers 由 SmcpComputerClient 处理
    ).await.map_err(|e| e.to_string())?;

    // Step 2: join_office
    computer.join_office(
        &profile.office_id,
        &profile.computer_name,
    ).await.map_err(|e| e.to_string())?;

    Ok(())
}

/// 断开 SMCP 连接
#[tauri::command]
pub async fn disconnect_smcp(
    state: State<'_, AppState>,
) -> Result<(), String> {
    let computer = state.computer.read().await;
    computer.leave_office().await.map_err(|e| e.to_string())?;
    computer.disconnect_socketio().await.map_err(|e| e.to_string())?;
    Ok(())
}

/// 获取当前连接状态
#[tauri::command]
pub async fn get_connection_status(
    state: State<'_, AppState>,
) -> Result<ConnectionStatusResponse, String> {
    let computer = state.computer.read().await;
    let client = computer.get_socketio_client();
    let client_guard = client.read().await;

    match client_guard.as_ref() {
        Some(c) => {
            let office_id = c.get_office_id().await;
            Ok(ConnectionStatusResponse {
                connected: office_id.is_some(),
                url: Some(c.get_url()),
                namespace: Some(c.get_namespace()),
                office_id,
                computer_name: Some(computer.name().to_string()),
            })
        }
        None => Ok(ConnectionStatusResponse {
            connected: false,
            url: None,
            namespace: None,
            office_id: None,
            computer_name: None,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct ConnectionStatusResponse {
    pub connected: bool,
    pub url: Option<String>,
    pub namespace: Option<String>,
    pub office_id: Option<String>,
    pub computer_name: Option<String>,
}
```

#### 2.2.2 输入变量管理 (`src-tauri/src/commands/inputs.rs` — 新建)

```rust
use crate::AppState;
use smcp_computer::mcp_clients::MCPServerInput;
use tauri::State;

#[tauri::command]
pub async fn list_inputs(
    state: State<'_, AppState>,
) -> Result<Vec<MCPServerInput>, String> {
    let computer = state.computer.read().await;
    computer.list_inputs().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_input(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<MCPServerInput>, String> {
    let computer = state.computer.read().await;
    computer.get_input(&id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_or_update_input(
    state: State<'_, AppState>,
    input: MCPServerInput,
) -> Result<(), String> {
    let computer = state.computer.read().await;
    computer.add_or_update_input(input.clone()).await.map_err(|e| e.to_string())?;
    // 持久化
    let inputs = computer.list_inputs().await.map_err(|e| e.to_string())?;
    state.config.save_inputs(&inputs).map_err(|e| e.to_string())?;
    // 自动通知 SMCP 服务器配置变更
    let _ = computer.emit_update_config().await;
    Ok(())
}

#[tauri::command]
pub async fn remove_input(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let computer = state.computer.read().await;
    computer.remove_input(&id).await.map_err(|e| e.to_string())?;
    let inputs = computer.list_inputs().await.map_err(|e| e.to_string())?;
    state.config.save_inputs(&inputs).map_err(|e| e.to_string())?;
    let _ = computer.emit_update_config().await;
    Ok(())
}

#[tauri::command]
pub async fn list_input_values(
    state: State<'_, AppState>,
) -> Result<HashMap<String, serde_json::Value>, String> {
    let computer = state.computer.read().await;
    computer.list_input_values().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_input_value(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<serde_json::Value>, String> {
    let computer = state.computer.read().await;
    computer.get_input_value(&id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_input_value(
    state: State<'_, AppState>,
    id: String,
    value: serde_json::Value,
) -> Result<(), String> {
    let computer = state.computer.read().await;
    computer.set_input_value(&id, value).await.map_err(|e| e.to_string())?;
    let _ = computer.emit_update_config().await;
    Ok(())
}

#[tauri::command]
pub async fn remove_input_value(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let computer = state.computer.read().await;
    computer.remove_input_value(&id).await.map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn clear_input_values(
    state: State<'_, AppState>,
) -> Result<(), String> {
    let computer = state.computer.read().await;
    computer.clear_input_values(None).await.map_err(|e| e.to_string())
}
```

#### 2.2.3 配置导入/导出 (`src-tauri/src/commands/config_io.rs` — 新建)

```rust
use serde::{Deserialize, Serialize};
use smcp_computer::mcp_clients::{MCPServerConfig, StdioServerConfig, StdioServerParameters};
use std::collections::HashMap;

/// Claude Desktop 配置格式
#[derive(Deserialize)]
struct ClaudeDesktopConfig {
    #[serde(rename = "mcpServers")]
    mcp_servers: HashMap<String, ClaudeDesktopServer>,
}

#[derive(Deserialize)]
struct ClaudeDesktopServer {
    command: String,
    args: Option<Vec<String>>,
    env: Option<HashMap<String, String>>,
}

/// CLI 原生配置格式
#[derive(Serialize, Deserialize)]
struct CliConfig {
    servers: Vec<MCPServerConfig>,
    inputs: Option<Vec<MCPServerInput>>,
}

#[derive(Serialize)]
pub struct ImportResult {
    pub imported: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}

#[tauri::command]
pub async fn import_config(
    state: State<'_, AppState>,
    path: String,
) -> Result<ImportResult, String> {
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;

    // 自动检测格式：尝试 CLI 格式，失败则尝试 Claude Desktop 格式
    let configs = if let Ok(cli_config) = serde_json::from_str::<CliConfig>(&content) {
        cli_config.servers
    } else if let Ok(cd_config) = serde_json::from_str::<ClaudeDesktopConfig>(&content) {
        // 转换 Claude Desktop → MCPServerConfig
        cd_config.mcp_servers.into_iter().map(|(name, server)| {
            MCPServerConfig::Stdio(StdioServerConfig {
                name,
                disabled: false,
                forbidden_tools: vec![],
                tool_meta: HashMap::new(),
                default_tool_meta: None,
                vrl: None,
                server_parameters: StdioServerParameters {
                    command: server.command,
                    args: server.args.unwrap_or_default(),
                    env: server.env.unwrap_or_default(),
                    cwd: None,
                },
            })
        }).collect()
    } else {
        return Err("Unrecognized config format".to_string());
    };

    let computer = state.computer.read().await;
    let mut imported = 0;
    let mut errors = vec![];

    for config in configs {
        match computer.add_or_update_server(config.clone()).await {
            Ok(_) => {
                state.config.add_config(config).map_err(|e| e.to_string())?;
                imported += 1;
            }
            Err(e) => errors.push(e.to_string()),
        }
    }

    Ok(ImportResult { imported, skipped: 0, errors })
}

#[tauri::command]
pub async fn export_config(
    state: State<'_, AppState>,
    path: String,
    names: Option<Vec<String>>,
) -> Result<(), String> {
    let configs = state.config.load_configs().map_err(|e| e.to_string())?;
    let inputs = state.config.load_inputs().unwrap_or_default();

    let filtered = match names {
        Some(ref names) => configs.into_iter()
            .filter(|c| names.contains(&c.name().to_string()))
            .collect(),
        None => configs,
    };

    let export = CliConfig { servers: filtered, inputs: Some(inputs) };
    let json = serde_json::to_string_pretty(&export).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}
```

### 2.3 新增 Service

#### ProfileService (`src-tauri/src/services/profile.rs` — 新建)

```rust
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionProfile {
    pub name: String,
    pub url: String,
    pub namespace: String,         // 默认 "/smcp"
    pub office_id: String,
    pub computer_name: String,
    pub auto_connect: bool,
    pub auto_reconnect: bool,
    // headers 中非敏感部分明文存储
    pub headers: HashMap<String, String>,
    // api_key 和 敏感 headers 不在此存储，通过 keychain
}

pub struct ProfileService {
    profiles_file: PathBuf,
}

impl ProfileService {
    pub fn new(app_data_dir: PathBuf) -> Result<Self, std::io::Error> { ... }
    pub fn list_profiles(&self) -> Result<Vec<ConnectionProfile>, ConfigError> { ... }
    pub fn get_profile(&self, name: &str) -> Result<Option<ConnectionProfile>, ConfigError> { ... }
    pub fn save_profile(&self, profile: ConnectionProfile) -> Result<(), ConfigError> { ... }
    pub fn delete_profile(&self, name: &str) -> Result<(), ConfigError> { ... }
}
```

存储文件: `{app_data_dir}/profiles.json`

### 2.4 ConfigService 扩展

**文件**: `src-tauri/src/services/config.rs`

新增 inputs 持久化:
```rust
impl ConfigService {
    // 现有方法保持不变...

    pub fn load_inputs(&self) -> Result<Vec<MCPServerInput>, ConfigError> {
        let file = self.config_dir.join("inputs.json");
        if !file.exists() { return Ok(vec![]); }
        let content = fs::read_to_string(&file)?;
        Ok(serde_json::from_str(&content)?)
    }

    pub fn save_inputs(&self, inputs: &[MCPServerInput]) -> Result<(), ConfigError> {
        let file = self.config_dir.join("inputs.json");
        let content = serde_json::to_string_pretty(inputs)?;
        fs::write(&file, content)?;
        Ok(())
    }
}
```

### 2.5 自动状态同步

MCP 命令中，在启停操作成功后自动触发 SMCP 通知:

```rust
// 在 start_mcp_server / stop_mcp_server / start_all / stop_all 的末尾添加:
// 自动通知工具列表变更
let _ = computer.emit_update_config().await;
```

### 2.6 命令注册

`lib.rs` 中 `generate_handler![]` 新增:
```rust
commands::inputs::list_inputs,
commands::inputs::get_input,
commands::inputs::add_or_update_input,
commands::inputs::remove_input,
commands::inputs::list_input_values,
commands::inputs::get_input_value,
commands::inputs::set_input_value,
commands::inputs::remove_input_value,
commands::inputs::clear_input_values,
commands::connection::list_profiles,
commands::connection::save_profile,
commands::connection::delete_profile,
commands::connection::connect_smcp,      // 替换 stub
commands::connection::disconnect_smcp,   // 替换 stub
commands::connection::get_connection_status, // 替换 stub
commands::config_io::import_config,
commands::config_io::export_config,
```

---

## 3. 前端变更

### 3.1 侧边栏重构

**文件**: `src/App.tsx`

从扁平 Menu 改为分组 Menu（使用 Ant Design Menu 的 `type: 'group'`）:

```typescript
const menuItems: MenuProps['items'] = [
  {
    key: 'overview-group',
    type: 'group',
    label: t('nav.overview'),
    children: [
      { key: 'dashboard', icon: <DashboardOutlined />, label: t('nav.dashboard') },
    ],
  },
  {
    key: 'config-group',
    type: 'group',
    label: t('nav.config'),
    children: [
      { key: 'mcp', icon: <ApiOutlined />, label: t('mcp.servers') },
      { key: 'variables', icon: <CodeOutlined />, label: t('variables.title') },
    ],
  },
  {
    key: 'connection-group',
    type: 'group',
    label: t('nav.connection'),
    children: [
      { key: 'smcp', icon: <CloudServerOutlined />, label: t('connection.smcpServer') },
      { key: 'resources', icon: <DesktopOutlined />, label: t('resources.title') },
    ],
  },
  {
    key: 'dev-group',
    type: 'group',
    label: t('nav.dev'),
    children: [
      { key: 'debug', icon: <BugOutlined />, label: t('debug.title') },
      { key: 'logs', icon: <FileTextOutlined />, label: t('logs.title') },
    ],
  },
  {
    key: 'system-group',
    type: 'group',
    label: t('nav.system'),
    children: [
      { key: 'settings', icon: <SettingOutlined />, label: t('settings.title') },
    ],
  },
];
```

Dashboard 和未实现页面暂时显示 placeholder，但导航结构已就位。

### 3.2 新增 Store: connectionStore

**文件**: `src/stores/connectionStore.ts`

```typescript
import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';

export interface ConnectionProfile {
  name: string;
  url: string;
  namespace: string;
  office_id: string;
  computer_name: string;
  auto_connect: boolean;
  auto_reconnect: boolean;
  headers: Record<string, string>;
}

export interface ConnectionStatus {
  connected: boolean;
  url?: string;
  namespace?: string;
  office_id?: string;
  computer_name?: string;
}

interface ConnectionState {
  profiles: ConnectionProfile[];
  status: ConnectionStatus;
  loading: boolean;
  error: string | null;

  fetchProfiles: () => Promise<void>;
  saveProfile: (profile: ConnectionProfile, apiKey?: string, sensitiveHeaders?: Record<string, string>) => Promise<void>;
  deleteProfile: (name: string) => Promise<void>;
  connect: (profileName: string) => Promise<void>;
  disconnect: () => Promise<void>;
  fetchStatus: () => Promise<void>;
}
```

每个 action 通过 `invoke()` 调用对应 Tauri command。

### 3.3 新增 Store: inputStore

**文件**: `src/stores/inputStore.ts`

```typescript
export interface InputDefinition {
  // MCPServerInput 的 TS 映射
  type: 'PromptString' | 'PickString' | 'Command';
  id: string;
  description: string;
  // type-specific fields...
}

interface InputState {
  inputs: InputDefinition[];
  values: Record<string, unknown>;
  loading: boolean;
  error: string | null;

  fetchInputs: () => Promise<void>;
  addOrUpdateInput: (input: InputDefinition) => Promise<void>;
  removeInput: (id: string) => Promise<void>;
  fetchValues: () => Promise<void>;
  setValue: (id: string, value: unknown) => Promise<void>;
  removeValue: (id: string) => Promise<void>;
  clearValues: () => Promise<void>;
}
```

### 3.4 SMCP 连接页面

**文件**: `src/components/SmcpConnection/index.tsx`

布局：上方 Profile 列表 + 下方连接状态面板

**Profile 列表** — Ant Design Table:
- 列: 名称, URL, Office ID, 状态（当前连接标记）
- 操作: 连接/断开, 编辑, 删除

**新建向导** — Ant Design Steps + Form:
- Step 1: URL + Namespace + API Key + Headers
- Step 2: Office ID + Computer Name + Auto Connect/Reconnect
- Step 3: 预览 + Profile Name + 保存

**连接状态面板** — 当 `status.connected === true` 时显示:
- 连接指示器（Badge）
- 服务器地址、Office、Computer Name
- 断开按钮

子组件:
- `SmcpConnection/ProfileList.tsx`
- `SmcpConnection/ProfileWizard.tsx`
- `SmcpConnection/ConnectionStatus.tsx`

### 3.5 输入变量页面

**文件**: `src/components/InputVariables/index.tsx`

左右分栏布局:
- 左侧: 变量列表（Table 或 List）+ 添加按钮
- 右侧: 选中变量的详情/编辑表单

**变量表单** — 根据 type 动态渲染:
- PromptString: id, description, default, password(checkbox), 当前值 Input
- PickString: id, description, options(Form.List), default(Select), 当前值 Select
- Command: id, description, command, args(Form.List), 当前值(只读) + 执行按钮

**引用数统计**: 调用 `invoke('get_mcp_servers')` 获取所有配置的 JSON，正则扫描 `${input:id}` 出现次数。

子组件:
- `InputVariables/InputList.tsx`
- `InputVariables/InputForm.tsx`
- `InputVariables/InputValueEditor.tsx`

### 3.6 配置导入/导出 UI

在 McpConfig 组件的批量操作栏新增两个按钮:

```typescript
<Button icon={<ImportOutlined />} onClick={handleImport}>
  {t('mcp.import')}
</Button>
<Button icon={<ExportOutlined />} onClick={handleExport}>
  {t('mcp.export')}
</Button>
```

**导入流程**:
1. 使用 Tauri `dialog.open()` 选择文件
2. 调用 `invoke('import_config', { path })`
3. 显示 ImportResult（成功数、错误列表）
4. 刷新服务器列表

**导出流程**:
1. 使用 Tauri `dialog.save()` 选择保存路径
2. 调用 `invoke('export_config', { path, names: null })` 导出全部
3. 显示成功提示

**依赖**: 需要新增 `@tauri-apps/plugin-dialog`

### 3.7 MCP 表单 Placeholder 可视化

在 `McpServerForm.tsx` 的文本输入框中:
- 检测值是否包含 `${input:...}` 模式
- 如包含，在输入框下方显示 Tag：`变量名 = 当前值` 或 `变量名 (未定义)` 警告

实现方式: 自定义 Form.Item 的 `extra` 属性，根据当前值动态渲染。

---

## 4. i18n 新增 Key

在 `locales/{en,zh}/translation.json` 中新增:

```json
{
  "nav": {
    "overview": "Overview",
    "config": "Configuration",
    "connection": "Connection",
    "dev": "Development",
    "system": "System",
    "dashboard": "Dashboard"
  },
  "variables": {
    "title": "Input Variables",
    "add": "Add Variable",
    "edit": "Edit Variable",
    "id": "Variable ID",
    "type": "Type",
    "description": "Description",
    "defaultValue": "Default Value",
    "currentValue": "Current Value",
    "references": "References",
    "promptString": "Prompt String",
    "pickString": "Pick String",
    "command": "Command",
    "options": "Options",
    "execute": "Execute",
    "clearAll": "Clear All Values",
    "import": "Import",
    "export": "Export",
    "messages": { ... }
  },
  "connection": {
    "profiles": "Connection Profiles",
    "addProfile": "New Profile",
    "wizard": {
      "step1": "Server",
      "step2": "Office",
      "step3": "Save"
    },
    "profileName": "Profile Name",
    "url": "Server URL",
    "namespace": "Namespace",
    "officeId": "Office ID",
    "computerName": "Computer Name",
    "apiKey": "API Key",
    "autoConnect": "Auto Connect",
    "autoReconnect": "Auto Reconnect",
    "headers": "Custom Headers",
    "testConnection": "Test Connection",
    "status": {
      "connected": "Connected",
      "disconnected": "Disconnected",
      "reconnecting": "Reconnecting"
    },
    "messages": { ... }
  },
  "mcp": {
    "import": "Import Config",
    "export": "Export Config",
    "importResult": "Imported {{count}} servers",
    ...existing keys...
  }
}
```

---

## 5. 新增依赖

### 前端
```bash
pnpm add @tauri-apps/plugin-dialog
```

### 后端
```toml
# Cargo.toml — 无新增 crate（dialog 通过 JS 端处理）
```

---

## 6. 文件清单

### 新建文件
```
src-tauri/src/commands/inputs.rs
src-tauri/src/commands/config_io.rs
src-tauri/src/services/profile.rs
src/stores/connectionStore.ts
src/stores/inputStore.ts
src/components/SmcpConnection/index.tsx
src/components/SmcpConnection/ProfileList.tsx
src/components/SmcpConnection/ProfileWizard.tsx
src/components/SmcpConnection/ConnectionStatus.tsx
src/components/InputVariables/index.tsx
src/components/InputVariables/InputList.tsx
src/components/InputVariables/InputForm.tsx
src/components/InputVariables/InputValueEditor.tsx
```

### 修改文件
```
src-tauri/src/lib.rs           — AppState 重构 + 命令注册
src-tauri/src/commands/mod.rs  — 新增 inputs, config_io 模块
src-tauri/src/commands/connection.rs — 全面重写（替换 stub）
src-tauri/src/commands/mcp.rs  — 迁移到 computer API + 自动通知
src-tauri/src/services/mod.rs  — 新增 profile 模块
src-tauri/src/services/config.rs — 新增 inputs 持久化方法
src/App.tsx                    — 侧边栏重构 + 新页面路由
src/components/McpConfig/index.tsx — 导入/导出按钮
src/components/McpConfig/McpServerForm.tsx — placeholder 可视化
src/locales/en/translation.json — 新增 key
src/locales/zh/translation.json — 新增 key
```

---

## 7. 验收标准

1. 可以创建、编辑、删除连接 Profile
2. 选择 Profile 后能成功连接到 SMCP 服务器并加入 Office
3. 连接状态面板实时显示连接状态
4. 可以创建三种类型的 Input 变量（PromptString / PickString / Command）
5. 变量值修改后自动同步到 SMCP 服务器
6. MCP 服务器启停后自动发送 notify update
7. 能导入 CLI JSON 格式和 Claude Desktop 格式的配置文件
8. 能导出当前配置为 CLI 兼容 JSON
9. 侧边栏已重构为分组结构，所有 Phase 的菜单项占位就绪
10. API Key 存储在系统 Keychain，不出现在配置文件中
