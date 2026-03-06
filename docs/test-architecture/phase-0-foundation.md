# Phase 0: 测试基础设施 + 单元测试

> **目标**: 搭建测试基础设施，完成 Rust Service 层单元测试、前端全组件渲染测试、CI Pipeline。
>
> **依赖**: 无
>
> **预期产出**: CI 绿色通过，覆盖率报告可见，前端 80% / Rust 70% 基线建立。

---

## 0.1 前端测试基础设施

### 0.1.1 安装依赖

```bash
pnpm add -D @vitest/coverage-v8
```

### 0.1.2 更新 vite.config.ts

在现有 `test` 配置中添加覆盖率：

```typescript
// vite.config.ts
export default defineConfig({
  test: {
    globals: true,
    environment: 'jsdom',
    setupFiles: 'src/test/setup.ts',
    coverage: {
      provider: 'v8',
      reporter: ['text', 'text-summary', 'lcov', 'html'],
      reportsDirectory: 'coverage',
      include: ['src/**/*.{ts,tsx}'],
      exclude: [
        'src/test/**',
        'src/vite-env.d.ts',
        'src/main.tsx',       // 入口文件不测
        'src/i18n.ts',        // 配置文件不测
      ],
      thresholds: {
        lines: 80,
        branches: 75,
        functions: 80,
        statements: 80,
      },
    },
  },
});
```

### 0.1.3 更新 package.json scripts

```json
{
  "scripts": {
    "test": "vitest run",
    "test:watch": "vitest",
    "test:coverage": "vitest run --coverage",
    "test:e2e": "playwright test --config e2e/playwright.config.ts",
    "test:e2e:ui": "playwright test --config e2e/playwright.config.ts --ui",
    "test:all": "pnpm test:coverage && pnpm test:e2e"
  }
}
```

### 0.1.4 扩展 test setup mock

```typescript
// src/test/setup.ts — 在已有 mock 基础上补充

// 已有:
// - vi.mock('@tauri-apps/api/core')   → invoke mock
// - vi.mock('@tauri-apps/plugin-dialog') → open/save mock
// - window.matchMedia mock

// 新增:
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),  // 返回 unlisten 函数
  emit: vi.fn(() => Promise.resolve()),
}));

// Ant Design 需要 ResizeObserver
global.ResizeObserver = class ResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
};

// 禁用 Ant Design 动画，加速测试
import { configure } from '@testing-library/react';
configure({ asyncUtilTimeout: 5000 });
```

### 0.1.5 创建组件测试辅助工具

```typescript
// src/test/helpers/render.tsx
import { render, RenderOptions } from '@testing-library/react';
import { ConfigProvider } from 'antd';
import { ReactElement } from 'react';

/**
 * 包装 Ant Design ConfigProvider 的自定义 render
 * 所有组件测试应使用此函数代替原生 render
 */
export function renderWithProviders(
  ui: ReactElement,
  options?: Omit<RenderOptions, 'wrapper'>
) {
  function Wrapper({ children }: { children: React.ReactNode }) {
    return <ConfigProvider>{children}</ConfigProvider>;
  }
  return render(ui, { wrapper: Wrapper, ...options });
}

export * from '@testing-library/react';
export { renderWithProviders as render };
```

```typescript
// src/test/helpers/store.ts
/**
 * 重置 Zustand store 到初始状态的工具
 * 用于测试之间的隔离
 */
export function resetAllStores() {
  // 各 store 的 getState().reset() 或手动 setState
  // 根据实际 store 实现补充
}
```

---

## 0.2 前端组件渲染测试

### 测试编写规范

每个组件测试文件遵循统一结构：

```typescript
// src/test/components/XxxComponent.test.tsx
import { render, screen, fireEvent, waitFor } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import XxxComponent from '@/components/XxxComponent';

// Mock 依赖的 store（如需要）
vi.mock('@/stores/xxxStore', () => ({
  useXxxStore: vi.fn(() => ({
    items: [],
    loading: false,
    error: null,
    fetchItems: vi.fn(),
  })),
}));

describe('XxxComponent', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // ── 渲染测试 ──
  it('renders correctly', () => {
    const { container } = render(<XxxComponent />);
    expect(container).toMatchSnapshot();
  });

  it('renders empty state', () => {
    render(<XxxComponent />);
    expect(screen.getByText(/no data/i)).toBeInTheDocument();
  });

  it('renders loading state', () => {
    // 修改 mock 返回 loading: true
    render(<XxxComponent />);
    // 断言 loading 指示器存在
  });

  it('renders error state', () => {
    // 修改 mock 返回 error: 'something went wrong'
    render(<XxxComponent />);
    expect(screen.getByText(/something went wrong/i)).toBeInTheDocument();
  });

  // ── 交互测试 ──
  it('handles user action', async () => {
    render(<XxxComponent />);
    fireEvent.click(screen.getByRole('button', { name: /add/i }));
    await waitFor(() => {
      // 断言预期结果
    });
  });
});
```

### 各组件测试清单

#### Dashboard (`src/test/components/Dashboard.test.tsx`)

```typescript
describe('Dashboard', () => {
  // 渲染
  it('renders connection status card');
  it('renders MCP server statistics');
  it('renders runtime detection results');
  it('renders recent logs section');

  // 数据加载
  it('calls fetchDashboard on mount');
  it('shows loading skeleton while fetching');
  it('shows error alert on fetch failure');

  // 状态展示
  it('shows "connected" badge when connected');
  it('shows "disconnected" badge when disconnected');
  it('shows correct server count (running/total)');
});
```

#### McpConfig 模块

```typescript
// src/test/components/McpConfig/McpServerList.test.tsx
describe('McpServerList', () => {
  it('renders list of servers with status badges');
  it('renders empty state when no servers configured');
  it('opens add form when add button clicked');
  it('calls startServer when start button clicked');
  it('calls stopServer when stop button clicked');
  it('shows confirm dialog before delete');
  it('calls removeServer after confirm');
  it('shows import/export buttons');
});

// src/test/components/McpConfig/McpServerForm.test.tsx
describe('McpServerForm', () => {
  // 已有: parseToolMetaJson 测试 — 保留

  // 新增渲染测试:
  it('renders form with all fields for stdio type');
  it('renders form with all fields for http type');
  it('renders form with all fields for sse type');
  it('switches form fields when type changes');

  // 表单验证:
  it('shows validation error when name is empty');
  it('shows validation error when command is empty (stdio)');
  it('shows validation error when url is empty (http/sse)');

  // 提交:
  it('calls addServer with correct payload on submit');
  it('calls updateServer when editing existing server');
  it('resets form after successful submit');
});

// src/test/components/McpConfig/ServerStatusBadge.test.tsx
describe('ServerStatusBadge', () => {
  it('renders green badge for running status');
  it('renders gray badge for stopped status');
  it('renders red badge for error status');
  it('renders yellow badge for starting status');
});
```

#### SmcpConnection 模块

```typescript
// src/test/components/SmcpConnection/SmcpConnection.test.tsx
describe('SmcpConnection', () => {
  it('renders profile list');
  it('renders empty state');
  it('renders connection status indicator');
  it('opens profile form on add click');
  it('calls connect with selected profile');
  it('calls disconnect when connected');
  it('shows confirm before delete profile');
});

// src/test/components/SmcpConnection/ProfileForm.test.tsx
describe('ProfileForm', () => {
  it('renders all form fields');
  it('validates required fields');
  it('masks API key input');
  it('calls saveProfile on submit');
  it('populates form when editing existing profile');
});
```

#### InputVariables 模块

```typescript
// src/test/components/InputVariables/InputVariables.test.tsx
describe('InputVariables', () => {
  it('renders input definitions list');
  it('renders empty state');
  it('opens add form on button click');
  it('opens value editor on edit click');
  it('calls removeInput on delete confirm');
  it('shows import button');
});

// src/test/components/InputVariables/InputForm.test.tsx
describe('InputForm', () => {
  it('renders PromptString fields');
  it('renders PickString fields with options');
  it('renders Command fields');
  it('switches fields when type changes');
  it('validates required fields');
  it('calls addOrUpdateInput on submit');
});

// src/test/components/InputVariables/InputValueEditor.test.tsx
describe('InputValueEditor', () => {
  it('renders current value');
  it('calls setValue on save');
  it('calls removeValue on clear');
});
```

#### DebugPanel 模块

```typescript
// src/test/components/DebugPanel/ToolBrowser.test.tsx
describe('ToolBrowser', () => {
  it('renders tool list');
  it('renders empty state when no tools');
  it('calls selectTool on item click');
  it('shows tool description and schema');
});

// src/test/components/DebugPanel/SchemaForm.test.tsx
describe('SchemaForm', () => {
  it('renders form from JSON schema with string field');
  it('renders form from JSON schema with number field');
  it('renders form from JSON schema with boolean field');
  it('renders nested object fields');
  it('handles required field validation');
});

// src/test/components/DebugPanel/ToolCallTest.test.tsx
describe('ToolCallTest', () => {
  it('renders selected tool info');
  it('renders parameter form');
  it('calls executeTool on submit');
  it('shows result after execution');
  it('shows error on execution failure');
  it('shows loading during execution');
});
```

#### LogViewer

```typescript
// src/test/components/LogViewer.test.tsx
describe('LogViewer', () => {
  it('renders log entries table');
  it('renders empty state');
  it('renders filter controls');
  it('calls fetchLogs on mount');
  it('calls setFilter and fetchLogs when filter changes');
  it('calls exportLogs on export button click');
  it('shows confirm before clearLogs');
  it('renders log level with correct color tag');
});
```

#### Settings 模块

```typescript
// src/test/components/Settings/AppearanceSettings.test.tsx
describe('AppearanceSettings', () => {
  it('renders theme selector with current value');
  it('renders language selector with current value');
  it('calls updateSettings on theme change');
  it('calls updateSettings on language change');
});

// src/test/components/Settings/DataSettings.test.tsx
describe('DataSettings', () => {
  it('renders log retention setting');
  it('calls updateSettings on retention change');
  it('shows confirm before clear data');
});

// src/test/components/Settings/RuntimeSettings.test.tsx
describe('RuntimeSettings', () => {
  it('renders detected runtimes');
  it('renders custom path inputs');
  it('calls detectRuntimes on refresh');
  it('calls updateSettings on path change');
});

// src/test/components/Settings/AboutSection.test.tsx
describe('AboutSection', () => {
  it('renders app version');
  it('renders smcp-computer version');
});
```

---

## 0.3 Rust 单元测试

### 0.3.1 添加测试依赖

```toml
# src-tauri/Cargo.toml [dev-dependencies] 追加
[dev-dependencies]
tempfile = "3"
tokio = { version = "1", features = ["full", "test-util"] }
serde_json = "1"
```

### 0.3.2 创建 Test Builder 模块

```rust
// src-tauri/src/test_helpers/mod.rs
// 仅在测试时编译
#![cfg(test)]

pub mod builders;
```

```rust
// src-tauri/src/test_helpers/builders.rs
#![cfg(test)]

use crate::commands::mcp::McpServerConfig;

/// MCP 服务器配置 Builder — 用于快速构造测试数据
pub struct StdioConfigBuilder {
    name: String,
    command: String,
    args: Vec<String>,
    env: std::collections::HashMap<String, String>,
}

impl StdioConfigBuilder {
    pub fn new() -> Self {
        Self {
            name: "test-stdio-server".into(),
            command: "node".into(),
            args: vec!["server.js".into()],
            env: Default::default(),
        }
    }

    pub fn name(mut self, name: &str) -> Self {
        self.name = name.into();
        self
    }

    pub fn command(mut self, cmd: &str) -> Self {
        self.command = cmd.into();
        self
    }

    pub fn args(mut self, args: Vec<&str>) -> Self {
        self.args = args.into_iter().map(String::from).collect();
        self
    }

    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    pub fn build(self) -> McpServerConfig {
        // 根据实际 McpServerConfig 枚举构造 Stdio 变体
        // McpServerConfig::Stdio { name, command, args, env, ... }
        todo!("根据实际类型实现")
    }
}

pub struct HttpConfigBuilder {
    name: String,
    url: String,
}

impl HttpConfigBuilder {
    pub fn new() -> Self {
        Self {
            name: "test-http-server".into(),
            url: "http://localhost:8080".into(),
        }
    }

    pub fn name(mut self, name: &str) -> Self { self.name = name.into(); self }
    pub fn url(mut self, url: &str) -> Self { self.url = url.into(); self }

    pub fn build(self) -> McpServerConfig {
        todo!("根据实际类型实现")
    }
}

pub struct SseConfigBuilder {
    name: String,
    url: String,
}

impl SseConfigBuilder {
    pub fn new() -> Self {
        Self {
            name: "test-sse-server".into(),
            url: "http://localhost:8081/sse".into(),
        }
    }

    pub fn name(mut self, name: &str) -> Self { self.name = name.into(); self }
    pub fn url(mut self, url: &str) -> Self { self.url = url.into(); self }

    pub fn build(self) -> McpServerConfig {
        todo!("根据实际类型实现")
    }
}
```

### 0.3.3 ConfigService 单元测试

```rust
// src-tauri/src/services/config.rs — 文件末尾添加
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn setup() -> (ConfigService, tempfile::TempDir) {
        let tmp = tempdir().unwrap();
        let svc = ConfigService::new(tmp.path());
        (svc, tmp)
    }

    // ── JSON 读写基础 ──

    #[test]
    fn test_load_empty_configs() {
        let (svc, _tmp) = setup();
        let configs = svc.load_configs().unwrap();
        assert!(configs.is_empty());
    }

    #[test]
    fn test_save_and_load_configs_roundtrip() {
        let (svc, _tmp) = setup();
        // 构造测试数据，保存，重新加载，断言一致
    }

    #[test]
    fn test_add_config() {
        let (svc, _tmp) = setup();
        // 添加一个配置，验证 load 能取回
    }

    #[test]
    fn test_add_duplicate_config_returns_error() {
        let (svc, _tmp) = setup();
        // 添加同名配置两次，验证第二次返回错误
    }

    #[test]
    fn test_remove_config() {
        let (svc, _tmp) = setup();
        // 添加 → 删除 → 验证不存在
    }

    #[test]
    fn test_remove_nonexistent_config_returns_error() {
        let (svc, _tmp) = setup();
        let result = svc.remove_config("nonexistent");
        assert!(result.is_err());
    }

    // ── 边界条件 ──

    #[test]
    fn test_load_corrupted_json_file() {
        let (svc, tmp) = setup();
        // 手动写入非法 JSON 到文件
        std::fs::write(tmp.path().join("mcp_servers.json"), "not json").unwrap();
        let result = svc.load_configs();
        assert!(result.is_err());
    }

    #[test]
    fn test_config_file_permissions_error() {
        // 仅 Unix: 设置文件只读后尝试写入
        #[cfg(unix)]
        {
            let (svc, tmp) = setup();
            let path = tmp.path().join("mcp_servers.json");
            std::fs::write(&path, "[]").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
            // 尝试保存应返回错误
        }
    }

    // ── Input 相关 ──

    #[test]
    fn test_load_empty_inputs() {
        let (svc, _tmp) = setup();
        let inputs = svc.load_inputs().unwrap();
        assert!(inputs.is_empty());
    }

    #[test]
    fn test_save_and_load_inputs_roundtrip() {
        let (svc, _tmp) = setup();
        // ...
    }

    // ── Input Values ──

    #[test]
    fn test_load_empty_input_values() {
        let (svc, _tmp) = setup();
        let values = svc.load_input_values().unwrap();
        assert!(values.is_empty());
    }

    #[test]
    fn test_save_and_load_input_values_roundtrip() {
        let (svc, _tmp) = setup();
        // ...
    }

    // ── Connection Profiles ──

    #[test]
    fn test_load_empty_profiles() {
        let (svc, _tmp) = setup();
        let profiles = svc.load_profiles().unwrap();
        assert!(profiles.is_empty());
    }

    #[test]
    fn test_save_and_load_profiles_roundtrip() {
        let (svc, _tmp) = setup();
        // ...
    }
}
```

### 0.3.4 LogService 单元测试

```rust
// src-tauri/src/services/logger.rs — 文件末尾添加
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn setup() -> (LogService, tempfile::TempDir) {
        let tmp = tempdir().unwrap();
        let svc = LogService::new(tmp.path().join("test_logs.db")).unwrap();
        (svc, tmp)
    }

    #[test]
    fn test_write_and_query_log() {
        let (svc, _tmp) = setup();
        svc.write("info", "test", "hello world", None).unwrap();

        let logs = svc.query(LogFilter::default()).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].message, "hello world");
        assert_eq!(logs[0].level, "info");
        assert_eq!(logs[0].category, "test");
    }

    #[test]
    fn test_query_with_level_filter() {
        let (svc, _tmp) = setup();
        svc.write("info", "cat", "msg1", None).unwrap();
        svc.write("error", "cat", "msg2", None).unwrap();

        let filter = LogFilter { level: Some("error".into()), ..Default::default() };
        let logs = svc.query(filter).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].message, "msg2");
    }

    #[test]
    fn test_query_with_category_filter() {
        let (svc, _tmp) = setup();
        svc.write("info", "mcp", "msg1", None).unwrap();
        svc.write("info", "smcp", "msg2", None).unwrap();

        let filter = LogFilter { category: Some("mcp".into()), ..Default::default() };
        let logs = svc.query(filter).unwrap();
        assert_eq!(logs.len(), 1);
    }

    #[test]
    fn test_query_with_keyword_filter() {
        let (svc, _tmp) = setup();
        svc.write("info", "cat", "hello world", None).unwrap();
        svc.write("info", "cat", "goodbye", None).unwrap();

        let filter = LogFilter { keyword: Some("hello".into()), ..Default::default() };
        let logs = svc.query(filter).unwrap();
        assert_eq!(logs.len(), 1);
    }

    #[test]
    fn test_query_with_limit_offset() {
        let (svc, _tmp) = setup();
        for i in 0..20 {
            svc.write("info", "cat", &format!("msg{i}"), None).unwrap();
        }

        let filter = LogFilter { limit: Some(5), offset: Some(10), ..Default::default() };
        let logs = svc.query(filter).unwrap();
        assert_eq!(logs.len(), 5);
    }

    #[test]
    fn test_cleanup_old_logs() {
        let (svc, _tmp) = setup();
        svc.write("info", "cat", "old msg", None).unwrap();
        // cleanup 0 days = 删除所有
        svc.cleanup(0).unwrap();
        let logs = svc.query(LogFilter::default()).unwrap();
        assert!(logs.is_empty());
    }

    #[test]
    fn test_clear_all() {
        let (svc, _tmp) = setup();
        svc.write("info", "cat", "msg", None).unwrap();
        svc.clear_all().unwrap();
        let logs = svc.query(LogFilter::default()).unwrap();
        assert!(logs.is_empty());
    }

    #[test]
    fn test_export_returns_json() {
        let (svc, _tmp) = setup();
        svc.write("info", "cat", "msg", None).unwrap();
        let json = svc.export(LogFilter::default()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_array());
    }

    #[test]
    fn test_write_with_details() {
        let (svc, _tmp) = setup();
        svc.write("error", "cat", "msg", Some("stack trace here")).unwrap();
        let logs = svc.query(LogFilter::default()).unwrap();
        assert_eq!(logs[0].details.as_deref(), Some("stack trace here"));
    }
}
```

### 0.3.5 SettingsService 单元测试

```rust
// src-tauri/src/services/settings.rs — 文件末尾添加
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn setup() -> (SettingsService, tempfile::TempDir) {
        let tmp = tempdir().unwrap();
        let svc = SettingsService::new(tmp.path());
        (svc, tmp)
    }

    #[test]
    fn test_load_default_settings() {
        let (svc, _tmp) = setup();
        let settings = svc.load().unwrap();
        // 验证默认值
        assert_eq!(settings.language, "en");
        assert_eq!(settings.log_retention_days, 30);
        // theme 默认 System
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let (svc, _tmp) = setup();
        let mut settings = svc.load().unwrap();
        settings.language = "zh".into();
        settings.log_retention_days = 7;
        svc.save(&settings).unwrap();

        let loaded = svc.load().unwrap();
        assert_eq!(loaded.language, "zh");
        assert_eq!(loaded.log_retention_days, 7);
    }

    #[test]
    fn test_load_missing_file_returns_defaults() {
        let (svc, _tmp) = setup();
        // 文件不存在时应返回默认值而非错误
        let settings = svc.load().unwrap();
        assert_eq!(settings.language, "en");
    }

    #[test]
    fn test_load_corrupted_file_returns_error_or_defaults() {
        let (svc, tmp) = setup();
        std::fs::write(tmp.path().join("settings.json"), "invalid json").unwrap();
        // 根据实现：可能返回错误或回退到默认值
        let result = svc.load();
        // 断言行为
    }
}
```

### 0.3.6 KeychainService 单元测试

```rust
// src-tauri/src/services/keychain.rs — 文件末尾添加
#[cfg(test)]
mod tests {
    use super::*;

    // 注意: Keychain 测试依赖系统凭证服务
    // CI 中 Linux 需要 gnome-keyring + dbus
    // 测试名包含 "keychain" 以便按需 skip

    #[test]
    fn test_keychain_save_and_get() {
        let test_key = "tfrobot-test-keychain-roundtrip";
        let test_value = "test-api-key-12345";

        // 清理可能的残留
        let _ = delete_credential(test_key);

        save_credential(test_key, test_value).unwrap();
        let retrieved = get_credential(test_key).unwrap();
        assert_eq!(retrieved, Some(test_value.to_string()));

        // 清理
        delete_credential(test_key).unwrap();
    }

    #[test]
    fn test_keychain_get_nonexistent() {
        let result = get_credential("tfrobot-test-nonexistent-key").unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_keychain_delete() {
        let test_key = "tfrobot-test-keychain-delete";
        save_credential(test_key, "value").unwrap();
        delete_credential(test_key).unwrap();
        let result = get_credential(test_key).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_keychain_overwrite() {
        let test_key = "tfrobot-test-keychain-overwrite";
        let _ = delete_credential(test_key);

        save_credential(test_key, "old-value").unwrap();
        save_credential(test_key, "new-value").unwrap();
        let result = get_credential(test_key).unwrap();
        assert_eq!(result, Some("new-value".to_string()));

        delete_credential(test_key).unwrap();
    }
}
```

### 0.3.7 RuntimePaths 单元测试

```rust
// src-tauri/src/services/runtime.rs — 文件末尾添加
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_paths_construction() {
        // 验证 from_app() 返回的路径结构合理
        // 不需要真实 Tauri AppHandle，测试纯路径逻辑
    }

    #[test]
    fn test_validate_with_nonexistent_paths() {
        // 验证 validate() 对不存在的路径返回 false/None
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_macos_paths_contain_expected_segments() {
        // macOS 特定路径格式验证
    }
}
```

### 0.3.8 扩展已有 Serde 测试 (commands/mcp.rs)

```rust
// src-tauri/src/commands/mcp.rs — 在已有 #[cfg(test)] 中扩展
#[cfg(test)]
mod tests {
    // ... 已有 deserialization 测试 ...

    // 新增: 序列化往返测试
    #[test]
    fn test_stdio_config_serialize_roundtrip() {
        // 构造 → 序列化 → 反序列化 → 断言等于原始值
    }

    #[test]
    fn test_http_config_serialize_roundtrip() { /* ... */ }

    #[test]
    fn test_sse_config_serialize_roundtrip() { /* ... */ }

    // 新增: 边界条件
    #[test]
    fn test_deserialize_unknown_type_returns_error() {
        let json = r#"{"type": "unknown", "url": "http://example.com"}"#;
        let result: Result<McpServerConfig, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_missing_required_fields() {
        let json = r#"{"type": "stdio"}"#;  // 缺少 command
        let result: Result<McpServerConfig, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }
}
```

---

## 0.4 CI Pipeline 搭建

### 0.4.1 GitHub Actions Workflow

```yaml
# .github/workflows/test.yml
name: Test Suite

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

env:
  CARGO_TERM_COLOR: always

jobs:
  # ══════════════════════════════════════════
  # 前端测试（Ubuntu，最快）
  # ══════════════════════════════════════════
  frontend-test:
    name: Frontend Tests
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
      - run: pnpm build   # tsc 类型检查 + vite build
      - run: pnpm test:coverage

      - uses: codecov/codecov-action@v4
        if: always()
        with:
          flags: frontend
          files: coverage/lcov.info
          token: ${{ secrets.CODECOV_TOKEN }}

  # ══════════════════════════════════════════
  # Rust 测试（三平台矩阵）
  # ══════════════════════════════════════════
  rust-test:
    name: Rust Tests (${{ matrix.os }})
    strategy:
      fail-fast: false
      matrix:
        include:
          - os: macos-latest
            rust-target: aarch64-apple-darwin
          - os: ubuntu-latest
            rust-target: x86_64-unknown-linux-gnu
          - os: windows-latest
            rust-target: x86_64-pc-windows-msvc
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.rust-target }}

      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: src-tauri

      # Linux: 系统依赖 + Keychain 模拟
      - name: Install Linux dependencies
        if: runner.os == 'Linux'
        run: |
          sudo apt-get update
          sudo apt-get install -y \
            libwebkit2gtk-4.1-dev \
            libappindicator3-dev \
            librsvg2-dev \
            patchelf \
            gnome-keyring \
            dbus-x11
          # 启动 D-Bus + gnome-keyring 以支持 Keychain 测试
          eval $(dbus-launch --sh-syntax)
          echo "DBUS_SESSION_BUS_ADDRESS=$DBUS_SESSION_BUS_ADDRESS" >> $GITHUB_ENV
          echo "test-password" | gnome-keyring-daemon --unlock --daemonize

      # Windows: 安装 WebView2（通常已预装）
      - name: Install Windows dependencies
        if: runner.os == 'Windows'
        run: |
          # WebView2 通常已在 GitHub Actions Windows runner 上预装
          echo "Windows dependencies ready"

      - name: Run tests
        working-directory: src-tauri
        run: cargo test

      # 覆盖率仅在 Linux 上生成（避免重复）
      - name: Generate coverage
        if: runner.os == 'Linux'
        working-directory: src-tauri
        run: |
          cargo install cargo-tarpaulin --locked
          cargo tarpaulin --out xml --output-dir ../coverage

      - uses: codecov/codecov-action@v4
        if: runner.os == 'Linux' && always()
        with:
          flags: rust
          files: coverage/cobertura.xml
          token: ${{ secrets.CODECOV_TOKEN }}

  # ══════════════════════════════════════════
  # 质量门禁汇总
  # ══════════════════════════════════════════
  quality-gate:
    name: Quality Gate
    needs: [frontend-test, rust-test]
    runs-on: ubuntu-latest
    if: always()
    steps:
      - name: Check all jobs passed
        run: |
          if [[ "${{ needs.frontend-test.result }}" != "success" ]] || \
             [[ "${{ needs.rust-test.result }}" != "success" ]]; then
            echo "Some test jobs failed"
            exit 1
          fi
          echo "All tests passed!"
```

### 0.4.2 Codecov 配置

```yaml
# .codecov.yml（项目根目录）
codecov:
  require_ci_to_pass: true

coverage:
  precision: 2
  round: down
  status:
    project:
      frontend:
        target: 80%
        flags: [frontend]
      rust:
        target: 70%
        flags: [rust]
    patch:
      default:
        target: 80%

flags:
  frontend:
    paths:
      - src/
    carryforward: true
  rust:
    paths:
      - src-tauri/src/
    carryforward: true

comment:
  layout: "reach, diff, flags, files"
  behavior: default
  require_changes: false
```

### 0.4.3 .gitignore 更新

```gitignore
# 追加到 .gitignore
coverage/
```

---

## 0.5 验收标准

- [ ] `pnpm test:coverage` 通过，前端覆盖率 ≥ 80%
- [ ] `cargo test` 在 macOS/Linux/Windows 全部通过
- [ ] `cargo tarpaulin` Rust 覆盖率 ≥ 70%
- [ ] GitHub Actions CI 流水线绿色
- [ ] Codecov PR 评论正常显示覆盖率变化
- [ ] 所有组件至少有 render snapshot 测试
- [ ] ConfigService、LogService、SettingsService 有完整单元测试
- [ ] KeychainService 有平台条件测试
