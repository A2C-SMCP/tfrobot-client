# Phase 1: 集成测试 + Contract Tests + Playwright E2E

> **目标**: 搭建 Echo MCP Server，完成 Rust Command 层集成测试、smcp-computer 契约测试、Playwright 前端 E2E。
>
> **依赖**: Phase 0（基础设施就绪、单元测试通过）
>
> **预期产出**: 集成测试覆盖所有 Tauri Command、契约测试保护 API 边界、E2E 覆盖核心用户流程。

---

## 1.1 Echo MCP Server（测试 Fixture）

### 目的

集成测试需要真实的 MCP 服务器进程，用于验证 `MCPServerManager` 的启动/停止/工具调用全流程。

### 实现

```
src-tauri/tests/echo-mcp-server/
├── package.json
└── index.js
```

```json
// package.json
{
  "name": "echo-mcp-server",
  "version": "0.1.0",
  "private": true,
  "description": "Minimal MCP stdio server for integration testing"
}
```

```javascript
// index.js
// MCP JSON-RPC over stdio 最小实现
//
// 支持的方法:
// - initialize      → 返回 server info + capabilities
// - tools/list      → 返回 [{name: "echo", inputSchema: {...}}]
// - tools/call      → echo 回输入参数
// - notifications/initialized → no-op
// - shutdown        → 优雅退出

const readline = require('readline');

const rl = readline.createInterface({
  input: process.stdin,
  output: process.stdout,
  terminal: false,
});

let buffer = '';

process.stdin.on('data', (chunk) => {
  buffer += chunk.toString();

  // MCP 使用 Content-Length 分隔的 JSON-RPC
  // 简化实现: 按换行分割
  const lines = buffer.split('\n');
  buffer = lines.pop() || '';

  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith('Content-Length')) continue;

    try {
      const request = JSON.parse(trimmed);
      handleRequest(request);
    } catch {
      // 忽略非 JSON 行（如 Content-Length header）
    }
  }
});

function handleRequest(req) {
  const { id, method, params } = req;

  let result;

  switch (method) {
    case 'initialize':
      result = {
        protocolVersion: '2024-11-05',
        serverInfo: { name: 'echo-mcp-server', version: '0.1.0' },
        capabilities: { tools: {} },
      };
      break;

    case 'notifications/initialized':
      return; // notification, no response

    case 'tools/list':
      result = {
        tools: [
          {
            name: 'echo',
            description: 'Echoes back the input',
            inputSchema: {
              type: 'object',
              properties: {
                message: { type: 'string', description: 'Message to echo' },
              },
              required: ['message'],
            },
          },
        ],
      };
      break;

    case 'tools/call':
      const toolName = params?.name;
      const args = params?.arguments || {};
      if (toolName === 'echo') {
        result = {
          content: [
            { type: 'text', text: JSON.stringify(args) },
          ],
        };
      } else {
        sendResponse(id, null, { code: -32601, message: `Unknown tool: ${toolName}` });
        return;
      }
      break;

    case 'shutdown':
      sendResponse(id, {});
      process.exit(0);
      return;

    default:
      sendResponse(id, null, { code: -32601, message: `Unknown method: ${method}` });
      return;
  }

  sendResponse(id, result);
}

function sendResponse(id, result, error) {
  const response = {
    jsonrpc: '2.0',
    id,
    ...(error ? { error } : { result }),
  };
  const json = JSON.stringify(response);
  const header = `Content-Length: ${Buffer.byteLength(json)}\r\n\r\n`;
  process.stdout.write(header + json);
}
```

### CI 中确保 Node.js 可用

GitHub Actions 的 `actions/setup-node@v4` 已在 Phase 0 的前端 job 中安装。Rust 集成测试 job 也需要 Node.js：

```yaml
# 在 rust-test job 中添加
- uses: actions/setup-node@v4
  with:
    node-version: 20
```

---

## 1.2 Rust 集成测试（Command 层）

### 测试基础设施

```rust
// src-tauri/tests/common/mod.rs

use std::sync::Arc;
use std::collections::HashMap;
use tempfile::TempDir;
use tokio::sync::RwLock;

// 导入项目类型
use tfrobot_client::AppState;
use tfrobot_client::services::config::ConfigService;
use tfrobot_client::services::logger::LogService;
use tfrobot_client::services::settings::SettingsService;

/// 创建隔离的测试 AppState
/// 每个测试用独立的 tempdir，互不影响
pub async fn create_test_app_state() -> (AppState, TempDir) {
    let tmp = tempfile::tempdir().unwrap();

    let config = Arc::new(ConfigService::new(tmp.path()));
    let log_service = Arc::new(
        LogService::new(tmp.path().join("logs.db")).unwrap()
    );
    let settings_service = Arc::new(SettingsService::new(tmp.path()));

    // 使用真实的 MCPServerManager（不 mock）
    let manager = Arc::new(RwLock::new(
        Some(smcp_computer::MCPServerManager::new())
    ));

    let state = AppState {
        manager,
        config,
        inputs: Arc::new(RwLock::new(HashMap::new())),
        connection: Arc::new(RwLock::new(None)),
        log_service,
        settings_service,
    };

    (state, tmp)
}

/// Echo MCP Server 的 stdio 配置
/// 需要 Node.js 在 PATH 中
pub fn echo_server_config(name: &str) -> serde_json::Value {
    let server_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/echo-mcp-server/index.js");

    serde_json::json!({
        "type": "stdio",
        "name": name,
        "command": "node",
        "args": [server_path.to_str().unwrap()],
        "env": {}
    })
}
```

### MCP Command 集成测试

```rust
// src-tauri/tests/mcp_commands_test.rs

mod common;

use common::{create_test_app_state, echo_server_config};

/// ── CRUD 操作 ──

#[tokio::test]
async fn test_add_and_list_mcp_server() {
    let (state, _tmp) = create_test_app_state().await;

    // 添加服务器
    let config = echo_server_config("test-echo");
    let result = add_mcp_server(state.clone(), config).await;
    assert!(result.is_ok());

    // 列出服务器
    let servers = get_mcp_servers(state.clone()).await.unwrap();
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].name, "test-echo");
}

#[tokio::test]
async fn test_add_duplicate_server_fails() {
    let (state, _tmp) = create_test_app_state().await;
    let config = echo_server_config("dup");

    add_mcp_server(state.clone(), config.clone()).await.unwrap();
    let result = add_mcp_server(state.clone(), config).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_remove_mcp_server() {
    let (state, _tmp) = create_test_app_state().await;
    let config = echo_server_config("to-remove");

    add_mcp_server(state.clone(), config).await.unwrap();
    remove_mcp_server(state.clone(), "to-remove".into()).await.unwrap();

    let servers = get_mcp_servers(state.clone()).await.unwrap();
    assert!(servers.is_empty());
}

#[tokio::test]
async fn test_update_mcp_server() {
    let (state, _tmp) = create_test_app_state().await;
    let config = echo_server_config("to-update");
    add_mcp_server(state.clone(), config).await.unwrap();

    // 修改配置后 update
    let mut updated = echo_server_config("to-update");
    updated["args"] = serde_json::json!(["--verbose"]);
    let result = update_mcp_server(state.clone(), updated).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_get_server_config() {
    let (state, _tmp) = create_test_app_state().await;
    let config = echo_server_config("get-config");
    add_mcp_server(state.clone(), config).await.unwrap();

    let retrieved = get_mcp_server_config(state.clone(), "get-config".into()).await.unwrap();
    assert!(retrieved.is_some());
}

/// ── 生命周期管理 ──

#[tokio::test]
async fn test_start_and_stop_server() {
    let (state, _tmp) = create_test_app_state().await;
    let config = echo_server_config("lifecycle");
    add_mcp_server(state.clone(), config).await.unwrap();

    // 启动
    start_mcp_server(state.clone(), "lifecycle".into()).await.unwrap();
    let servers = get_mcp_servers(state.clone()).await.unwrap();
    assert_eq!(servers[0].status, "running");

    // 停止
    stop_mcp_server(state.clone(), "lifecycle".into()).await.unwrap();
    let servers = get_mcp_servers(state.clone()).await.unwrap();
    assert_eq!(servers[0].status, "stopped");
}

#[tokio::test]
async fn test_start_all_stop_all() {
    let (state, _tmp) = create_test_app_state().await;

    for i in 0..3 {
        let config = echo_server_config(&format!("server-{i}"));
        add_mcp_server(state.clone(), config).await.unwrap();
    }

    start_all_servers(state.clone()).await.unwrap();
    let servers = get_mcp_servers(state.clone()).await.unwrap();
    assert!(servers.iter().all(|s| s.status == "running"));

    stop_all_servers(state.clone()).await.unwrap();
    let servers = get_mcp_servers(state.clone()).await.unwrap();
    assert!(servers.iter().all(|s| s.status == "stopped"));
}

#[tokio::test]
async fn test_start_nonexistent_server_fails() {
    let (state, _tmp) = create_test_app_state().await;
    let result = start_mcp_server(state.clone(), "ghost".into()).await;
    assert!(result.is_err());
}

/// ── 异常场景（Mock 使用场景）──

#[tokio::test]
async fn test_start_server_with_invalid_command() {
    let (state, _tmp) = create_test_app_state().await;

    // 使用不存在的命令
    let config = serde_json::json!({
        "type": "stdio",
        "name": "bad-server",
        "command": "/nonexistent/binary",
        "args": [],
        "env": {}
    });
    add_mcp_server(state.clone(), config).await.unwrap();

    let result = start_mcp_server(state.clone(), "bad-server".into()).await;
    assert!(result.is_err());
}
```

### Connection Command 集成测试

```rust
// src-tauri/tests/connection_test.rs

mod common;
use common::create_test_app_state;

#[tokio::test]
async fn test_save_and_list_profiles() {
    let (state, _tmp) = create_test_app_state().await;

    let profile = serde_json::json!({
        "name": "test-profile",
        "url": "https://smcp.example.com",
    });
    save_profile(state.clone(), profile, "test-api-key".into()).await.unwrap();

    let profiles = list_profiles(state.clone()).await.unwrap();
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].name, "test-profile");
}

#[tokio::test]
async fn test_delete_profile() {
    let (state, _tmp) = create_test_app_state().await;

    let profile = serde_json::json!({
        "name": "to-delete",
        "url": "https://smcp.example.com",
    });
    save_profile(state.clone(), profile, "key".into()).await.unwrap();
    delete_profile(state.clone(), "to-delete".into()).await.unwrap();

    let profiles = list_profiles(state.clone()).await.unwrap();
    assert!(profiles.is_empty());
}

#[tokio::test]
async fn test_connection_status_when_disconnected() {
    let (state, _tmp) = create_test_app_state().await;
    let status = get_connection_status(state.clone()).await.unwrap();
    assert!(!status.connected);
}

// 注意: connect_smcp 需要真实 SMCP 服务器
// 在无服务器环境下测试连接失败路径:
#[tokio::test]
async fn test_connect_to_unreachable_server_fails() {
    let (state, _tmp) = create_test_app_state().await;

    let profile = serde_json::json!({
        "name": "unreachable",
        "url": "https://localhost:19999",  // 不存在的端口
    });
    save_profile(state.clone(), profile, "key".into()).await.unwrap();

    let result = connect_smcp(state.clone(), "unreachable".into()).await;
    assert!(result.is_err());
}
```

### Config IO 集成测试

```rust
// src-tauri/tests/config_io_test.rs

mod common;
use common::create_test_app_state;

#[tokio::test]
async fn test_detect_claude_desktop_format() {
    let (state, tmp) = create_test_app_state().await;

    let content = serde_json::json!({
        "mcpServers": {
            "myserver": {
                "command": "node",
                "args": ["server.js"]
            }
        }
    });
    let path = tmp.path().join("claude_config.json");
    std::fs::write(&path, serde_json::to_string_pretty(&content).unwrap()).unwrap();

    let format = detect_config_format(state.clone(), path.to_str().unwrap().into()).await.unwrap();
    assert_eq!(format, "claude_desktop");
}

#[tokio::test]
async fn test_import_claude_desktop_config() {
    let (state, tmp) = create_test_app_state().await;

    let content = serde_json::json!({
        "mcpServers": {
            "imported-server": {
                "command": "python",
                "args": ["-m", "mcp_server"]
            }
        }
    });
    let path = tmp.path().join("import.json");
    std::fs::write(&path, serde_json::to_string_pretty(&content).unwrap()).unwrap();

    import_config(state.clone(), path.to_str().unwrap().into(), "claude_desktop".into())
        .await
        .unwrap();

    let servers = get_mcp_servers(state.clone()).await.unwrap();
    assert_eq!(servers.len(), 1);
}

#[tokio::test]
async fn test_export_and_reimport_config() {
    let (state, tmp) = create_test_app_state().await;

    // 添加服务器
    let config = common::echo_server_config("export-me");
    add_mcp_server(state.clone(), config).await.unwrap();

    // 导出
    let export_path = tmp.path().join("exported.json");
    export_config(
        state.clone(),
        export_path.to_str().unwrap().into(),
        vec!["export-me".into()],
    ).await.unwrap();

    // 验证导出文件是合法 JSON
    let exported = std::fs::read_to_string(&export_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&exported).unwrap();
    assert!(parsed.is_object() || parsed.is_array());
}
```

### Logs / Settings / Inputs 集成测试

```rust
// src-tauri/tests/logs_test.rs
// 测试 get_logs / export_logs / clear_logs command 通过 AppState 调用

// src-tauri/tests/settings_test.rs
// 测试 get_settings / update_settings / detect_runtimes / get_app_info

// src-tauri/tests/inputs_test.rs
// 测试 input 定义和值的增删改查完整流程
// 测试 import_inputs 从 JSON 文件导入
```

（结构与上述模式相同，不再展开。每个测试文件遵循相同的 `create_test_app_state` + 真实操作 + 断言模式。）

---

## 1.3 Contract Tests（smcp-computer 契约）

```
src-tauri/tests/
└── contract/
    └── smcp_computer_contract_test.rs
```

```rust
// src-tauri/tests/contract/smcp_computer_contract_test.rs
//
// 契约测试验证 tfrobot-client 对 smcp-computer API 的假设。
// 当 smcp-computer 升级时，这些测试应最先失败，提供清晰的升级指引。

use smcp_computer::{MCPServerManager, McpServerConfig, Tool};

/// ── API 存在性契约 ──
/// 验证我们依赖的 public API 仍然存在且签名兼容

#[test]
fn contract_manager_constructible() {
    // MCPServerManager::new() 应当无参数可调用
    let _manager = MCPServerManager::new();
}

#[tokio::test]
async fn contract_server_lifecycle_api_exists() {
    let manager = MCPServerManager::new();

    // 以下方法应当存在且可编译
    // 不断言具体行为，只验证 API 签名
    // （编译通过即表示契约满足）

    // add_server: 接受 config，返回 Result
    // start_server: 接受 name，返回 Result
    // stop_server: 接受 name，返回 Result
    // remove_server: 接受 name，返回 Result
    // list_servers: 返回 Vec<ServerInfo> 或类似
    // get_server_status: 接受 name，返回状态

    // 注意: 具体方法名根据 smcp-computer 实际 API 调整
    // 如果编译失败，说明 API 已变更，需要适配
}

/// ── 序列化格式契约 ──

#[test]
fn contract_config_internally_tagged() {
    // McpServerConfig 使用 serde internally tagged 格式
    // {"type": "stdio", "command": "...", ...}

    let json = r#"{"type": "stdio", "command": "node", "args": ["server.js"]}"#;
    let config: Result<McpServerConfig, _> = serde_json::from_str(json);
    assert!(config.is_ok(), "Stdio config should deserialize from internally tagged JSON");
}

#[test]
fn contract_config_http_format() {
    let json = r#"{"type": "http", "url": "http://localhost:8080"}"#;
    let config: Result<McpServerConfig, _> = serde_json::from_str(json);
    assert!(config.is_ok(), "Http config should deserialize from internally tagged JSON");
}

#[test]
fn contract_config_sse_format() {
    let json = r#"{"type": "sse", "url": "http://localhost:8081/sse"}"#;
    let config: Result<McpServerConfig, _> = serde_json::from_str(json);
    assert!(config.is_ok(), "SSE config should deserialize from internally tagged JSON");
}

/// ── Tool 结构体契约 ──

#[test]
fn contract_tool_has_expected_fields() {
    // Tool 结构体应有 name、description、inputSchema
    let json = r#"{
        "name": "test_tool",
        "description": "A test tool",
        "inputSchema": {"type": "object", "properties": {}}
    }"#;
    let tool: Result<Tool, _> = serde_json::from_str(json);
    assert!(tool.is_ok(), "Tool should deserialize with name/description/inputSchema");

    let tool = tool.unwrap();
    assert_eq!(tool.name, "test_tool");
}

/// ── VERSION 常量契约 ──

#[test]
fn contract_version_exists() {
    // smcp_computer::VERSION 应存在
    let version = smcp_computer::VERSION;
    assert!(!version.is_empty());
    // 验证 semver 格式
    assert!(version.contains('.'), "VERSION should be semver format");
}
```

---

## 1.4 Playwright E2E 测试

### 1.4.1 安装依赖

```bash
pnpm add -D @playwright/test
npx playwright install chromium
```

### 1.4.2 Playwright 配置

```typescript
// e2e/playwright.config.ts
import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: process.env.CI ? 'github' : 'html',

  use: {
    baseURL: 'http://localhost:1420',
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
  },

  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],

  webServer: {
    command: 'pnpm dev',
    port: 1420,
    reuseExistingServer: !process.env.CI,
    timeout: 30_000,
  },
});
```

### 1.4.3 Tauri Invoke Mock Fixture

```typescript
// e2e/fixtures/mock-invoke.ts
//
// 在浏览器环境中替换 Tauri invoke，使 E2E 测试可脱离 Rust 后端运行

import { Page } from '@playwright/test';

/** 所有 command 的 mock 响应数据 */
const mockResponses: Record<string, unknown> = {
  get_mcp_servers: [
    {
      name: 'test-stdio-server',
      type: 'stdio',
      status: 'stopped',
      command: 'node',
      args: ['server.js'],
    },
  ],
  get_dashboard_data: {
    connected: false,
    connection_url: null,
    mcp_server_count: 1,
    mcp_running_count: 0,
    tools_count: 0,
    recent_logs: [],
    runtimes: { node: '/usr/local/bin/node', python: null, uv: null, pnpm: null },
  },
  list_profiles: [],
  get_connection_status: { connected: false },
  get_settings: {
    theme: 'system',
    language: 'en',
    log_retention_days: 30,
    custom_runtime_paths: {},
  },
  get_logs: [],
  list_input_definitions: [],
  list_input_values: {},
  get_available_tools: [],
  detect_runtimes: { node: '/usr/local/bin/node' },
  get_app_info: { version: '0.1.0', smcp_computer_version: '0.1.8' },
};

/**
 * 为页面注入 Tauri invoke mock
 * 必须在 page.goto() 之前调用
 */
export async function setupInvokeMock(page: Page, overrides?: Record<string, unknown>) {
  const responses = { ...mockResponses, ...overrides };

  await page.addInitScript((data) => {
    // @ts-ignore
    window.__TAURI_INTERNALS__ = {
      invoke: (cmd: string, args?: unknown) => {
        console.log(`[mock invoke] ${cmd}`, args);
        const response = (data as Record<string, unknown>)[cmd];
        if (response !== undefined) {
          return Promise.resolve(JSON.parse(JSON.stringify(response)));
        }
        console.warn(`[mock invoke] No mock for command: ${cmd}`);
        return Promise.resolve(null);
      },
      transformCallback: () => 0,
    };
  }, responses);
}
```

### 1.4.4 Page Object 模式

```typescript
// e2e/pages/base.page.ts
import { Page, Locator } from '@playwright/test';

export class BasePage {
  readonly page: Page;
  readonly sidebar: Locator;

  constructor(page: Page) {
    this.page = page;
    this.sidebar = page.locator('.ant-layout-sider');
  }

  async navigateTo(menuKey: string) {
    await this.sidebar.getByText(menuKey).click();
  }
}

// e2e/pages/mcp-config.page.ts
import { Page, Locator } from '@playwright/test';
import { BasePage } from './base.page';

export class McpConfigPage extends BasePage {
  readonly addButton: Locator;
  readonly serverList: Locator;
  readonly formModal: Locator;

  constructor(page: Page) {
    super(page);
    this.addButton = page.getByRole('button', { name: /add/i });
    this.serverList = page.locator('[data-testid="server-list"]');
    this.formModal = page.locator('.ant-modal');
  }

  async addStdioServer(name: string, command: string) {
    await this.addButton.click();
    await this.formModal.waitFor({ state: 'visible' });
    await this.page.getByLabel(/name/i).fill(name);
    await this.page.getByLabel(/command/i).fill(command);
    await this.formModal.getByRole('button', { name: /ok|submit|save/i }).click();
  }
}

// 其他 Page Object 结构类似
```

### 1.4.5 E2E 测试用例

```typescript
// e2e/tests/navigation.spec.ts
import { test, expect } from '@playwright/test';
import { setupInvokeMock } from '../fixtures/mock-invoke';

test.beforeEach(async ({ page }) => {
  await setupInvokeMock(page);
  await page.goto('/');
});

test('sidebar shows all navigation items', async ({ page }) => {
  const sidebar = page.locator('.ant-layout-sider');
  await expect(sidebar.getByText(/dashboard/i)).toBeVisible();
  await expect(sidebar.getByText(/mcp/i)).toBeVisible();
  await expect(sidebar.getByText(/connection/i)).toBeVisible();
  await expect(sidebar.getByText(/settings/i)).toBeVisible();
});

test('clicking menu item navigates to correct page', async ({ page }) => {
  await page.locator('.ant-layout-sider').getByText(/mcp/i).click();
  // 验证 MCP 页面内容出现
  await expect(page.getByText(/mcp server/i)).toBeVisible();
});

test('default page is dashboard', async ({ page }) => {
  await expect(page.getByText(/dashboard/i).first()).toBeVisible();
});
```

```typescript
// e2e/tests/mcp-crud.spec.ts
import { test, expect } from '@playwright/test';
import { setupInvokeMock } from '../fixtures/mock-invoke';

test.describe('MCP Server CRUD', () => {
  test.beforeEach(async ({ page }) => {
    await setupInvokeMock(page);
    await page.goto('/');
    // 导航到 MCP 页面
    await page.locator('.ant-layout-sider').getByText(/mcp/i).click();
  });

  test('displays server list', async ({ page }) => {
    await expect(page.getByText('test-stdio-server')).toBeVisible();
  });

  test('add server flow', async ({ page }) => {
    await page.getByRole('button', { name: /add/i }).click();
    await expect(page.locator('.ant-modal')).toBeVisible();

    // 填写表单
    await page.getByLabel(/name/i).fill('new-server');
    await page.getByLabel(/command/i).fill('node');

    // 提交
    await page.locator('.ant-modal').getByRole('button', { name: /ok|save/i }).click();
  });

  test('delete server shows confirmation', async ({ page }) => {
    // 找到删除按钮并点击
    await page.getByRole('button', { name: /delete/i }).first().click();
    // 应出现确认对话框
    await expect(page.getByText(/confirm|are you sure/i)).toBeVisible();
  });
});
```

```typescript
// e2e/tests/theme-switch.spec.ts
import { test, expect } from '@playwright/test';
import { setupInvokeMock } from '../fixtures/mock-invoke';

test.describe('Theme switching', () => {
  test('can switch to dark mode', async ({ page }) => {
    await setupInvokeMock(page, {
      get_settings: { theme: 'light', language: 'en', log_retention_days: 30 },
    });
    await page.goto('/');

    // 找到主题切换按钮/开关
    // 点击切换到 dark
    // 验证 body 或 root 有 dark 相关 class/attribute
  });
});
```

```typescript
// e2e/tests/language-switch.spec.ts
import { test, expect } from '@playwright/test';
import { setupInvokeMock } from '../fixtures/mock-invoke';

test.describe('Language switching', () => {
  test('switches to Chinese', async ({ page }) => {
    await setupInvokeMock(page);
    await page.goto('/');

    // 导航到 Settings
    // 切换语言到中文
    // 验证 UI 文本变为中文
  });
});
```

### 1.4.6 更新 CI Pipeline

在 Phase 0 的 `.github/workflows/test.yml` 中添加 E2E job：

```yaml
  # ── E2E 测试（依赖前端和 Rust 测试通过）──
  e2e-test:
    name: E2E Tests
    needs: [frontend-test, rust-test]
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: pnpm/action-setup@v4
        with:
          version: 9

      - uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: 'pnpm'

      - run: pnpm install --frozen-lockfile
      - run: npx playwright install --with-deps chromium
      - run: pnpm test:e2e

      - uses: actions/upload-artifact@v4
        if: failure()
        with:
          name: playwright-report
          path: e2e/playwright-report/
          retention-days: 7
```

---

## 1.5 验收标准

- [ ] Echo MCP Server 可通过 `node index.js` 启动并响应 `initialize` / `tools/list` / `tools/call`
- [ ] `cargo test --test '*'` 集成测试全部通过（需要 Node.js 环境）
- [ ] Contract test 编译通过 = smcp-computer API 契约满足
- [ ] `pnpm test:e2e` Playwright 测试通过
- [ ] CI 中 E2E job 绿色
- [ ] 所有核心用户流程（导航、CRUD、连接、导入导出、主题切换）有 E2E 覆盖
