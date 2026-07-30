# TFRobot Client 测试架构设计规格

> 本文档定义了 tfrobot-client 项目的全面测试体系，覆盖单元测试、集成测试、E2E 测试，以及 CI/CD 编排策略。
>
> 历史说明：本目录中的早期测试方案可能仍以 `MCPServerManager` 为主要后端测试对象。
> SDK Computer 架构对齐后的当前测试边界以
> `plans/SDK_CLIENT_ARCHITECTURE_ALIGNMENT.md` 和现有 `src-tauri/tests/*` 为准。

---

## 测试分层总览

```
┌─────────────────────────────────────────────────────────┐
│  E2E Tests                                               │
│  ┌───────────────────────┐  ┌─────────────────────────┐ │
│  │ Playwright + Mock      │  │ tauri-driver Smoke Test │ │
│  │ (UI 交互流程)          │  │ (全栈冒烟测试)          │ │
│  └───────────────────────┘  └─────────────────────────┘ │
├─────────────────────────────────────────────────────────┤
│  Integration Tests                                       │
│  ┌───────────────────┐  ┌────────────────────────────┐  │
│  │ Rust: Service 层   │  │ Rust: Command 层           │  │
│  │ (真实 IO + tempdir)│  │ (真实 smcp-computer 集成)  │  │
│  └───────────────────┘  └────────────────────────────┘  │
│  ┌───────────────────────────────────────────────────┐  │
│  │ Contract Tests (smcp-computer API 契约验证)        │  │
│  └───────────────────────────────────────────────────┘  │
├─────────────────────────────────────────────────────────┤
│  Unit Tests                                              │
│  ┌──────────────┐  ┌──────────────┐  ┌───────────────┐ │
│  │ Rust Services │  │ React Stores │  │ React 组件    │ │
│  │ (纯逻辑)     │  │ (mock invoke)│  │ (渲染+快照)   │ │
│  └──────────────┘  └──────────────┘  └───────────────┘ │
└─────────────────────────────────────────────────────────┘
```

## 覆盖率目标

| 阶段 | 前端 | Rust 后端 | 增量 PR 要求 |
|------|------|-----------|-------------|
| 第一阶段（当前） | **80%** | **70%** | 不可降低已有覆盖率 |
| 最终目标 | 90% | 85% | 新代码 ≥ 80% |

### 覆盖率工具

- **前端**: Vitest coverage（`@vitest/coverage-v8`）
- **Rust**: `cargo-tarpaulin` 或 `cargo-llvm-cov`
- **CI 报告**: Codecov 集成，PR 评论显示覆盖率变化

## 核心设计决策

| 决策项 | 选择 | 理由 |
|--------|------|------|
| Rust Service 层隔离 | 混合策略：tempdir 真实 IO + Command 层 mock | Service 验证真实 IO，Command 验证业务逻辑 |
| smcp-computer 集成 | 不主动 mock，暴露问题 | 团队内部库，测试应发现而非隐藏其缺陷 |
| Mock 使用场景 | 仅模拟异常/边界（超时、崩溃、格式错误） | 正常路径走真实集成 |
| 前端组件测试 | 全组件渲染覆盖 + 快照 | 防止意外 UI 回归 |
| E2E 策略 | 分层：Playwright mock invoke + tauri-driver 冒烟 | 平衡覆盖率与维护成本 |
| CI 平台 | GitHub Actions / 腾讯云 CNB，三平台 | macOS + Linux + Windows 全覆盖 |
| 测试数据 | Builder 模式（单元）+ JSON fixture（集成/契约） | 各取所长 |

## 实施阶段

| Phase | 文档 | 内容 | 依赖 |
|-------|------|------|------|
| **P0** | [phase-0-foundation.md](./phase-0-foundation.md) | Rust 单元测试 + 前端组件测试 + CI 搭建 | 无 |
| **P1** | [phase-1-integration.md](./phase-1-integration.md) | Echo MCP Server + Rust 集成测试 + Contract Tests + Playwright E2E | P0 |
| **P2** | [phase-2-advanced.md](./phase-2-advanced.md) | Tauri Driver 冒烟测试 + 性能基准 + 跨库协作 | P1 |

## 最终目录结构

```
tfrobot-client/
├── src/                          # React 前端
│   └── test/
│       ├── setup.ts              # Vitest 全局 mock
│       ├── helpers/              # 测试工具函数
│       ├── stores/               # Zustand store 测试（已有）
│       ├── components/           # 组件渲染 + 交互测试（新增）
│       └── utils/                # 工具函数测试
├── src-tauri/                    # Rust 后端
│   ├── src/
│   │   ├── services/*.rs         # 内联 #[cfg(test)] 单元测试
│   │   ├── commands/*.rs         # 内联 serde 测试
│   │   └── test_helpers/         # 共享 Builder/Factory
│   └── tests/                    # 集成测试（新增）
│       ├── common/mod.rs         # 共享 test setup
│       ├── fixtures/             # JSON fixture 文件
│       ├── echo-mcp-server/      # 测试用 MCP 服务器
│       ├── *_test.rs             # 集成测试文件
│       └── contract/             # 契约测试
├── e2e/                          # E2E 测试（新增）
│   ├── playwright.config.ts
│   ├── fixtures/
│   ├── pages/                    # Page Object
│   ├── tests/                    # Playwright 测试用例
│   └── smoke/                    # Tauri Driver 冒烟测试
├── .codecov.yml                  # 覆盖率配置
└── .github/workflows/test.yml    # CI pipeline
```

## 测试命令速查

```bash
# ── 前端 ──
pnpm test                # Vitest 单次运行
pnpm test:watch          # Vitest 监听模式
pnpm test:coverage       # Vitest + 覆盖率报告
pnpm test:e2e            # Playwright E2E
pnpm test:e2e:ui         # Playwright UI 模式
pnpm test:all            # 前端全量（覆盖率 + E2E）

# ── Rust ──
cargo test --lib                     # 单元测试
cargo test --test '*'                # 集成测试
cargo test --test '*' contract       # 契约测试
cargo test -- --skip keychain        # 跳过 Keychain 测试
cargo tarpaulin --out Html           # 覆盖率报告
```
