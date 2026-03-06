# CLAUDE.md

本文件为 Claude Code (claude.ai/code) 在此代码仓库中工作时提供指导。

## 项目概述

TFRobot Client 是一个跨平台桌面应用程序，用于管理 A2C-SMCP（Agent To Computer SMCP）协议。它为基于 Rust 的 `smcp-computer` 库提供图形界面封装，使用户能够管理 MCP（Model Context Protocol）服务器并连接到 SMCP 服务器。

## 技术栈

- **前端**: React 18 + TypeScript + Ant Design 5 + Zustand（状态管理）+ i18next（国际化）
- **后端**: Tauri 2.x (Rust) + Tokio 异步运行时
- **构建工具**: Vite（前端）、Cargo（Rust 后端）
- **包管理器**: pnpm

## 常用命令

```bash
# 开发模式（运行完整 Tauri 应用，支持热重载）
pnpm tauri dev

# 构建生产版本
pnpm tauri build

# 仅前端开发（不启动 Rust 后端）
pnpm dev

# 前端类型检查
pnpm build  # 执行 tsc && vite build
```

## 架构

```
src/                    # React 前端 (TypeScript)
├── App.tsx            # 主布局，包含 Ant Design 侧边栏导航
├── i18n.ts            # i18next 配置（英文 + 中文）
├── locales/           # 翻译 JSON 文件 (en/, zh/)
├── stores/            # Zustand 状态存储
├── hooks/             # 自定义 React Hooks
└── components/        # UI 组件

src-tauri/             # Rust 后端 (Tauri)
├── src/
│   ├── lib.rs         # Tauri 命令注册和插件配置
│   ├── commands/      # Tauri 命令处理器（IPC 端点）
│   │   ├── mcp.rs         # MCP 服务器增删改查与生命周期管理
│   │   ├── connection.rs  # SMCP 服务器连接管理
│   │   └── logs.rs        # 日志获取与导出
│   └── services/      # 业务逻辑
│       ├── keychain.rs    # 系统凭证存储（keyring crate）
│       ├── logger.rs      # 双层日志系统（用户友好 + 详细日志）
│       └── runtime.rs     # 内置运行时检测（Node、Python、uv、pnpm）
├── resources/         # 打包的运行时二进制文件（已加入 gitignore）
└── tauri.conf.json    # Tauri 应用配置（窗口、插件、打包）
```

## 关键模式

### Tauri 命令 (Rust)
所有暴露给前端的后端函数都使用 `#[tauri::command]` 并返回 `Result<T, String>`：
```rust
#[tauri::command]
pub async fn command_name(args: Type) -> Result<ReturnType, String> {
    // 实现代码
}
```
命令在 `lib.rs` 中通过 `tauri::generate_handler![]` 注册。

### 前后端 IPC 通信
```typescript
import { invoke } from '@tauri-apps/api/core';
const result = await invoke('command_name', { arg1, arg2 });
```

### 国际化翻译
翻译文件采用嵌套 JSON 结构，位于 `src/locales/{lang}/translation.json`。使用方式：
```typescript
const { t } = useTranslation();
t('section.key')
```

### 路径别名
TypeScript 导入中 `@/` 解析为 `src/`。

## 测试

```bash
# 前端单元测试（Vitest + React Testing Library）
pnpm test              # 运行所有测试
pnpm test:watch        # 监听模式
pnpm test:coverage     # 运行测试并生成覆盖率报告

# Rust 后端测试
cd src-tauri && cargo test

# E2E 测试（Playwright，仅 Chromium）
pnpm test:e2e
pnpm test:e2e:ui       # 带 UI 的交互模式

# 全部测试
pnpm test:all          # 前端覆盖率 + E2E
```

### 前端测试约定
- 测试文件位于 `src/test/`，使用 Vitest + jsdom 环境
- 使用 `src/test/helpers/render.tsx` 中的 `render()` 替代原生 render，已集成 Ant Design ConfigProvider 和 I18nextProvider
- 使用 `src/test/helpers/store.ts` 中的 `resetAllStores()` 在 `beforeEach` 中重置 Zustand store 状态
- 所有 Zustand store 均实现 `reset()` 方法（基于 `initialState` 模式）
- 组件测试断言真实翻译文本（如 `'MCP Servers'`），而非 i18n key（如 `'mcp.servers'`）
- Tauri API（invoke、dialog、event、shell）在 `src/test/setup.ts` 中全局 mock

## 开发注意事项

- `smcp-computer` crate 依赖在 `Cargo.toml` 中已被注释 - 待发布到 crates.io 后取消注释
- 大部分命令实现中有 `// TODO: Integrate with smcp-computer` 占位符
- 开发服务器运行在 1420 端口（在 vite.config.ts 中配置）
- 应用窗口：默认 1200x800px，最小 800x600px

## 详细规格说明

完整的技术规格、里程碑规划和架构决策请参阅 `PLAN.md`。
