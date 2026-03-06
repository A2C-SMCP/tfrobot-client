# Phase 2: 全栈冒烟测试 + 性能基准 + 跨库协作

> **目标**: 补齐 Tauri Driver 全栈 E2E、建立性能基准、推动 smcp-computer 测试基础设施共建。
>
> **依赖**: Phase 1（集成测试 + E2E 就绪）
>
> **预期产出**: 发布前有冒烟测试把关、性能退化可量化检测、跨库测试协作机制建立。

---

## 2.1 Tauri Driver 冒烟测试

### 目的

Playwright E2E 通过 mock invoke 测试前端，但无法验证 Rust 后端是否真正工作。Tauri Driver 启动完整的 Tauri 应用（含 Rust 后端），通过 WebDriver 协议操控 WebView，验证前后端真实集成。

### 适用场景

- **Release 前冒烟验证**: 确保打包后应用可正常启动和基本使用
- **关键路径回归**: MCP 服务器启停、设置持久化等涉及真实文件 IO 的流程
- **不适合**: 覆盖所有 UI 交互（用 Playwright 已足够），频繁的开发迭代测试

### 2.1.1 安装 tauri-driver

```bash
cargo install tauri-driver
```

### 2.1.2 测试框架选择

使用 WebdriverIO（Node.js WebDriver 客户端）+ Vitest 作为测试运行器：

```bash
pnpm add -D webdriverio @wdio/cli
```

### 2.1.3 目录结构

```
e2e/
└── smoke/
    ├── wdio.config.ts           # WebdriverIO 配置
    ├── smoke.test.ts            # 冒烟测试用例
    └── helpers/
        └── tauri-app.ts         # 应用启停辅助
```

### 2.1.4 WebdriverIO 配置

```typescript
// e2e/smoke/wdio.config.ts
import { type Options } from '@wdio/types';
import path from 'path';

// 构建产物路径（macOS）
const appPath = path.resolve(
  __dirname,
  '../../src-tauri/target/release/bundle/macos/TFRobot.app/Contents/MacOS/TFRobot'
);

export const config: Options.Testrunner = {
  runner: 'local',
  specs: ['./smoke.test.ts'],
  maxInstances: 1,  // 桌面应用一次只能一个实例

  capabilities: [{
    browserName: 'wry',            // Tauri WebView
    'tauri:options': {
      application: appPath,
    },
  }],

  services: ['tauri'],             // @tauri-apps/webdriver

  framework: 'mocha',
  reporters: ['spec'],
  mochaOpts: {
    timeout: 60000,                // 桌面应用启动较慢
  },
};
```

### 2.1.5 冒烟测试用例

```typescript
// e2e/smoke/smoke.test.ts
//
// 冒烟测试只覆盖关键路径，确认应用可正常工作
// 不追求全面覆盖——那是 Playwright E2E 的职责

describe('TFRobot Smoke Test', () => {

  // ── 应用启动 ──

  it('应用窗口正常打开', async () => {
    // 验证窗口存在且有标题
    const title = await browser.getTitle();
    expect(title).toBeTruthy();
  });

  it('窗口尺寸符合配置 (≥800x600)', async () => {
    const { width, height } = await browser.getWindowRect();
    expect(width).toBeGreaterThanOrEqual(800);
    expect(height).toBeGreaterThanOrEqual(600);
  });

  // ── Dashboard ──

  it('Dashboard 页面正常渲染', async () => {
    // 等待 Dashboard 内容出现
    const dashboard = await $('[data-testid="dashboard"]');
    await dashboard.waitForDisplayed({ timeout: 10000 });
  });

  it('Dashboard 显示运行时检测结果', async () => {
    // 验证至少有一个 runtime 被检测到
    const runtimeSection = await $('[data-testid="runtimes"]');
    await runtimeSection.waitForDisplayed();
    const text = await runtimeSection.getText();
    expect(text).toMatch(/node|python|uv|pnpm/i);
  });

  // ── 导航 ──

  it('侧边栏导航可用', async () => {
    // 点击 Settings
    const settingsMenu = await $('li*=Settings');
    await settingsMenu.click();

    // 验证 Settings 页面内容出现
    const settingsContent = await $('[data-testid="settings"]');
    await settingsContent.waitForDisplayed({ timeout: 5000 });
  });

  // ── 设置持久化 ──

  it('设置修改后持久化', async () => {
    // 导航到 Settings
    const settingsMenu = await $('li*=Settings');
    await settingsMenu.click();

    // 修改语言设置
    // ... 根据实际 UI 操作

    // 验证: 刷新页面后设置仍保持
    await browser.refresh();
    // ... 断言设置值
  });

  // ── MCP 服务器管理（核心功能）──

  it('MCP 页面可正常加载', async () => {
    const mcpMenu = await $('li*=MCP');
    await mcpMenu.click();

    // 等待 MCP 页面渲染
    const mcpContent = await $('[data-testid="mcp-config"]');
    await mcpContent.waitForDisplayed({ timeout: 5000 });
  });
});
```

### 2.1.6 CI 配置

冒烟测试仅在 **macOS runner** 上运行（原生 GUI 支持），且仅在 **release 分支/tag** 时触发：

```yaml
# .github/workflows/test.yml 追加
  smoke-test:
    name: Smoke Test (macOS)
    needs: [frontend-test, rust-test, e2e-test]
    runs-on: macos-latest
    if: github.ref == 'refs/heads/main' || startsWith(github.ref, 'refs/tags/')
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: src-tauri

      - uses: pnpm/action-setup@v4
        with:
          version: 9
      - uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: 'pnpm'

      - run: pnpm install --frozen-lockfile

      # 构建 release 版本
      - run: pnpm tauri build

      # 安装 tauri-driver
      - run: cargo install tauri-driver

      # 运行冒烟测试
      - run: pnpm test:smoke
        timeout-minutes: 5

      - uses: actions/upload-artifact@v4
        if: failure()
        with:
          name: smoke-test-screenshots
          path: e2e/smoke/screenshots/
          retention-days: 7
```

### 2.1.7 package.json 脚本

```json
{
  "scripts": {
    "test:smoke": "cd e2e/smoke && npx wdio run wdio.config.ts"
  }
}
```

---

## 2.2 性能基准测试

### 目的

建立关键操作的性能基线，在 CI 中检测性能退化。

### 2.2.1 Rust 基准测试（Criterion）

```toml
# src-tauri/Cargo.toml 追加
[dev-dependencies]
criterion = { version = "0.5", features = ["async_tokio"] }

[[bench]]
name = "benchmarks"
harness = false
```

```rust
// src-tauri/benches/benchmarks.rs

use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use tempfile::tempdir;

// ── LogService 性能 ──

fn bench_log_write(c: &mut Criterion) {
    let tmp = tempdir().unwrap();
    let svc = LogService::new(tmp.path().join("bench.db")).unwrap();

    c.bench_function("log_write_single", |b| {
        b.iter(|| {
            svc.write("info", "bench", "benchmark message", None).unwrap();
        });
    });
}

fn bench_log_query(c: &mut Criterion) {
    let tmp = tempdir().unwrap();
    let svc = LogService::new(tmp.path().join("bench.db")).unwrap();

    // 预填充数据
    for i in 0..10_000 {
        svc.write("info", "bench", &format!("msg {i}"), None).unwrap();
    }

    let mut group = c.benchmark_group("log_query");

    for limit in [10, 100, 1000] {
        group.bench_with_input(
            BenchmarkId::new("with_limit", limit),
            &limit,
            |b, &limit| {
                let filter = LogFilter { limit: Some(limit), ..Default::default() };
                b.iter(|| svc.query(filter.clone()).unwrap());
            },
        );
    }

    group.bench_function("with_keyword_filter", |b| {
        let filter = LogFilter {
            keyword: Some("msg 5000".into()),
            limit: Some(100),
            ..Default::default()
        };
        b.iter(|| svc.query(filter.clone()).unwrap());
    });

    group.finish();
}

// ── ConfigService 性能 ──

fn bench_config_load_save(c: &mut Criterion) {
    let tmp = tempdir().unwrap();
    let svc = ConfigService::new(tmp.path());

    // 预填充 50 个服务器配置
    let mut configs = Vec::new();
    for i in 0..50 {
        configs.push(/* 构造 McpServerConfig */);
    }
    svc.save_configs(&configs).unwrap();

    let mut group = c.benchmark_group("config");

    group.bench_function("load_50_configs", |b| {
        b.iter(|| svc.load_configs().unwrap());
    });

    group.bench_function("save_50_configs", |b| {
        b.iter(|| svc.save_configs(&configs).unwrap());
    });

    group.finish();
}

// ── MCP Server 批量操作 ──

fn bench_server_bulk_operations(c: &mut Criterion) {
    // 注意: 这些基准测试需要 Node.js 环境
    // 使用 echo-mcp-server

    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("start_5_servers", |b| {
        b.iter(|| {
            rt.block_on(async {
                // 创建 manager, 添加 5 个 echo server, 全部启动, 全部停止
                // 测量总时间
            });
        });
    });
}

criterion_group!(
    benches,
    bench_log_write,
    bench_log_query,
    bench_config_load_save,
    // bench_server_bulk_operations,  // 可选，需要 Node.js
);
criterion_main!(benches);
```

### 2.2.2 运行与比较

```bash
# 运行基准测试
cargo bench --manifest-path src-tauri/Cargo.toml

# 输出 HTML 报告在 src-tauri/target/criterion/
```

### 2.2.3 CI 中的性能检测（可选）

使用 `criterion-compare` 或 GitHub Action 比较基准：

```yaml
# .github/workflows/bench.yml
name: Benchmarks

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]
    paths:
      - 'src-tauri/src/services/**'
      - 'src-tauri/benches/**'

jobs:
  benchmark:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2

      # Linux 系统依赖
      - run: |
          sudo apt-get update
          sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev

      - name: Run benchmarks
        working-directory: src-tauri
        run: cargo bench -- --output-format bencher | tee output.txt

      # 可选: 使用 benchmark-action 追踪趋势
      - uses: benchmark-action/github-action-benchmark@v1
        with:
          tool: 'cargo'
          output-file-path: src-tauri/output.txt
          github-token: ${{ secrets.GITHUB_TOKEN }}
          alert-threshold: '150%'      # 性能下降 50% 触发警告
          comment-on-alert: true
          fail-on-alert: false          # 不阻塞 PR，仅警告
          auto-push: true               # 自动推送基准数据到 gh-pages
```

---

## 2.3 跨库协作规格（smcp-computer）

### 背景

smcp-computer 是团队内部维护的核心依赖库。当前 tfrobot-client 通过自建 echo-mcp-server 和 contract test 进行测试。本节定义对 smcp-computer 的测试基础设施期望，作为后续协作的输入。

### 2.3.1 期望 smcp-computer 提供的测试支持

#### (A) 测试用 MCP Server

```rust
// 期望 smcp-computer 导出:
// smcp_computer::test_utils::EchoMcpServer

pub struct EchoMcpServer {
    // 可配置行为的测试 MCP 服务器
}

impl EchoMcpServer {
    /// 创建默认 echo server（stdio 传输）
    pub fn stdio() -> Self;

    /// 创建 SSE 传输的 echo server
    pub fn sse(port: u16) -> Self;

    /// 注入延迟（模拟慢服务器）
    pub fn with_latency(self, duration: Duration) -> Self;

    /// 注入错误（模拟服务器故障）
    pub fn with_error_on(self, method: &str, error: JsonRpcError) -> Self;

    /// 添加自定义 tool
    pub fn with_tool(self, tool: Tool) -> Self;

    /// 获取 McpServerConfig 用于注册到 Manager
    pub fn config(&self) -> McpServerConfig;

    /// 启动服务器（后台运行）
    pub async fn start(&mut self) -> Result<()>;

    /// 停止服务器
    pub async fn stop(&mut self) -> Result<()>;
}
```

#### (B) Test Builder / Factory

```rust
// 期望 smcp-computer 导出:
// smcp_computer::test_utils::builders

impl MCPServerManager {
    /// 测试用构造函数，使用临时目录
    pub fn test_new() -> Self;
}

impl McpServerConfig {
    /// 快速构造 stdio 测试配置
    pub fn test_stdio(name: &str, command: &str) -> Self;

    /// 快速构造 http 测试配置
    pub fn test_http(name: &str, url: &str) -> Self;

    /// 快速构造 sse 测试配置
    pub fn test_sse(name: &str, url: &str) -> Self;
}
```

#### (C) API 稳定性标记

在 smcp-computer 的 public API 文档中标注：

```rust
/// 添加 MCP 服务器配置
///
/// # Stability: Stable
/// 此 API 自 v0.1.0 起稳定，破坏性变更将遵循 semver。
pub async fn add_server(&mut self, config: McpServerConfig) -> Result<()>;

/// 获取服务器内部状态（调试用）
///
/// # Stability: Unstable
/// 此 API 可能在 minor 版本中变更。
pub fn get_internal_state(&self, name: &str) -> Option<InternalState>;
```

#### (D) CHANGELOG 规范

```markdown
# Changelog

## [0.2.0] - 2025-xx-xx

### ⚠ BREAKING CHANGES
- `MCPServerManager::new()` 现在需要 `Config` 参数 (#123)
- `Tool.input_schema` 字段类型从 `Value` 改为 `JsonSchema` (#456)

### Added
- `test_utils` 模块导出 `EchoMcpServer` (#789)
```

### 2.3.2 过渡阶段行动计划

| 步骤 | 行动 | 负责方 | 时间 |
|------|------|--------|------|
| 1 | 将本文档中的期望规格分享给 smcp-computer 团队 | tfrobot-client | Phase 2 启动时 |
| 2 | smcp-computer 评审可行性，给出排期 | smcp-computer | 2 周内 |
| 3 | smcp-computer 发布带 `test_utils` 的版本 | smcp-computer | 按排期 |
| 4 | tfrobot-client 迁移到 smcp-computer test_utils | tfrobot-client | 发布后 1 周 |
| 5 | 废弃自建 echo-mcp-server | tfrobot-client | 迁移完成后 |

### 2.3.3 迁移后测试架构变化

```diff
  src-tauri/tests/
  ├── common/
- │   └── mod.rs              # 使用自建 echo server
+ │   └── mod.rs              # 使用 smcp_computer::test_utils
- ├── echo-mcp-server/        # 自建 echo server（废弃）
  ├── mcp_commands_test.rs
  └── contract/               # 仍保留，验证 API 不回归
```

---

## 2.4 前端性能监控（可选）

### Lighthouse CI

对前端页面进行 Lighthouse 审计，追踪性能分数变化：

```bash
pnpm add -D @lhci/cli
```

```json
// lighthouserc.json
{
  "ci": {
    "collect": {
      "url": ["http://localhost:1420"],
      "startServerCommand": "pnpm dev",
      "numberOfRuns": 3
    },
    "assert": {
      "assertions": {
        "categories:performance": ["warn", { "minScore": 0.8 }],
        "categories:accessibility": ["error", { "minScore": 0.9 }]
      }
    }
  }
}
```

> 注意: Lighthouse 在 Tauri 应用中仅能衡量前端页面性能，不包含 Rust 后端。作为参考指标使用。

---

## 2.5 测试维护策略

### 定期审查

| 频率 | 审查内容 |
|------|---------|
| 每周 | 修复 flaky test（不稳定测试） |
| 每月 | 审查覆盖率趋势，更新快照 |
| 每季度 | 评审覆盖率目标是否需要提升 |
| 每次 smcp-computer 升级 | 运行 contract test，适配 API 变更 |

### Flaky Test 处理流程

1. CI 中标记 flaky test（retry 2 次后仍失败才报错）
2. 创建 issue 追踪 flaky test
3. 1 周内修复或临时 skip（必须有 issue 追踪）
4. 常见 flaky 原因：
   - 异步操作未正确等待
   - 测试间共享状态未清理
   - 时间依赖（如 `cleanup(days)` 在日期边界时行为不一致）

### 新功能测试要求

所有新功能 PR 必须包含：

| 层级 | 要求 |
|------|------|
| Rust 新 Command | 对应集成测试 |
| Rust 新 Service 方法 | 对应单元测试 |
| 前端新组件 | render 测试 + 快照 |
| 前端新 Store action | mock invoke 测试 |
| 核心用户流程变更 | 更新 E2E 测试 |
| smcp-computer API 变更 | 更新 contract test |

---

## 2.6 验收标准

- [ ] Tauri Driver 冒烟测试在 macOS CI 上通过
- [ ] `cargo bench` 基准测试可运行并产生 HTML 报告
- [ ] 基准数据自动追踪（可选：GitHub Pages 可视化）
- [ ] 跨库协作规格文档已分享给 smcp-computer 团队
- [ ] 测试维护流程文档化并被团队认可
- [ ] 新功能 PR 测试要求已在 PR template 或 CONTRIBUTING.md 中记录
