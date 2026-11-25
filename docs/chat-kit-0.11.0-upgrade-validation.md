# Chat Kit 0.11.0 升级验收

日期：2026-11-02。

## 变更

- `@turingfocus/chat-kit` 从 0.10.1 升至 0.11.0；现有核心模块 overrides 同步至 0.11.0，新增 chat-auth 0.12.1 override，防止独立版本漂移。
- 锁文件实际安装六个核心包 0.11.0 和 auth 0.12.1，integrity 与[发布记录](https://github.com/A2C-SMCP/tf-chat-kit/releases/tag/v0.11.0)一致。client 不依赖 chat-testing，无需为升级额外安装。
- 上游默认隐藏开发诊断。诊断复制测试显式传入 `debug`，新增默认模式仍显示业务错误、隐藏诊断入口的回归。生产页面沿用默认产品模式。
- 保留现有 Manager 登录、原生凭据/HTTP 边界、json-v2、平台隔离及 current-server best-effort 恢复配置。

## 实测

| 检查 | 结果 |
| --- | --- |
| `pnpm install --frozen-lockfile --offline` | 通过 |
| `pnpm build` | TypeScript、生产构建通过；保留大 chunk 提示 |
| `pnpm exec tsc --noEmit` | 测试适配后通过 |
| `pnpm exec eslint src/components/Chat src/test/components/Chat*.test.*` | 通过 |
| `pnpm exec vitest run --maxWorkers=1 --testTimeout=30000 src/test/components/Chat*.test.*` | 10 文件、83 测试通过，包括 HTTP/Socket、发送、恢复、附件、平台生命周期、缓存与诊断 |
| 全量 Vitest，2 workers、15000ms 预算 | 首轮 74 文件、681 测试：676 通过，5 失败；两项旧诊断断言已修复并纳入上述 83 项通过结果 |
| 非聊天失败项单独复验，1 worker、60000ms 预算 | Computer workbench、MCP 值类型和 Command 预览共 3 项通过，61 项未选中；未修改这些业务代码 |
| 浏览器 E2E，2 workers | 首轮 32/33 通过；1280px Computer 弹窗关闭后点击超时，单独复跑 1/1 通过 |
| macOS 原生重启夹具 | 重新构建前端及 Rust example，通过：Robot 43 / Conversation 99 写盘后新进程直接恢复 99 |
| 全仓 `pnpm lint:eslint` | 45 errors / 6 warnings，涉及现有 experiments 生成产物和实验源码；未改无关文件 |

首轮全量测试并非全绿；以上复验分别验证已修复断言和超时项，不代表重新执行了一次全绿的全量测试。

原生复验命令：

```sh
pnpm exec vite build --config e2e/chat-restoration/vite.config.mjs
cargo build --manifest-path src-tauri/Cargo.toml --features chat-restoration-acceptance --example chat_restoration_acceptance
node e2e/chat-restoration/run.mjs
```

本机原始日志：`/tmp/tfrc-0110-{build,tests,chat,retry,e2e,e2e-retry,native,eslint,chat-eslint,lock}.log`。
原生结果：`/var/folders/v7/6wb5d0ks3rx3v0wg682l2m6c0000gn/T/chat-restoration-zPOx8k/result.json`。这些本机临时路径不作为可移植仓库依赖。

## 验收边界

本次通过客户端本地集成验收；原生夹具使用生产 Chat、Kit、IPC 和 SettingsService，Manager 身份和服务端数据为本地 fixture。未执行远端 Manager 登录、真实 Server 收发/私有资源下载、签名安装包 UAT 或 33 分钟后台持续性验收，不将其标记为通过。未部署服务、升级 Rust SDK 或修改用户工作区既有未跟踪文件。
