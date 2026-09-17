# Issue #90 本地升级验收记录

日期：2026-09-17（Asia/Shanghai）。需求：[GitHub #90](https://github.com/A2C-SMCP/tfrobot-client/issues/90)。
实施分支：`feat/90-chatkit-tfbash-upgrade`，基线 `dev-0.2.5@15ffcbe`。
用户已确认实施方案及 Enter 发送策略，并授权提交、推送及向 `develop` 创建 PR。Issue 保持 `in-progress`；完整验收尚未完成。

## 已实施范围

- Chat Kit 主包及五个 overrides 精确锁定 0.8.2，更新 pnpm lock。冻结安装通过。
- 生产 `ChatConversationView` 显式 `sendShortcut="enter"`；Enter/Ctrl+Enter 发送，Shift+Enter 换行，Cmd+Enter 不发送。
- 补齐两种快捷键、诊断、复制及恢复提示的 12 项中英文文案；通过 formatChatError 补齐安全错误摘要，标准操作/原因和未知发送结果提醒均双语化，并补齐 8 项连接/恢复生命周期文案。
- 保留 current-server/bounded rebase、OwnedChatProvider 生命周期、Rust lease、上传/资源代理、身份与缓存隔离。未新增 RemoteTool、实时 Ask User 或轮询。
- tfbash requirements、版本常量、四平台锁更新到 0.2.1；cutoff 固定为 2026-09-17T00:00:00Z。使用 uv 0.8.17，所有传递依赖版本/哈希保持原值，只有 tfbash 的版本及哈希变化。
- Python 3.12.14 / 20260825、Rust SDK 0.4.1、stdio 参数、七工具契约保持。
- 四平台离线目录实际重建，逐个读取 `tfbash_mcp-0.2.1.dist-info/METADATA` 核实版本；没有用 refresh-manifest 替代升级。

## 验证及边界

| 验证 | 结果 / 边界 |
| --- | --- |
| `pnpm build`、`pnpm lint:ts`、冻结安装 | 通过；Vite 有已有的大 chunk 和混合导入提示 |
| Chat 八个相关测试文件 | 最终串行合跑 8 文件 / 77 项全部通过（67.37 秒）。首次并行合跑一个 5 秒测试超时，单独复验及最终合跑均通过 |
| 快捷键测试 | 使用真实发布包，覆盖 Enter/Ctrl/Shift/Alt/Cmd、组合态/keyCode 229、长按、防重复发送、附件保留及双语文案；DOM 合成事件，不等于真实 macOS 输入法操作；另通过真实发布包 UI + HTTP 503 覆盖双语错误摘要和 clipboard adapter 成功/拒绝，复制文本不含凭据/原始响应 |
| 缓存回归调整 | 0.8.2 进度提示延迟两秒且改为非关闭型 status；测试等待真实提示，并继续断言缓存同步、错误与发送权限 |
| Rust built_in_tools 单测 | 4/4 通过 |
| Rust fmt / Clippy all-targets | 通过 |
| ESLint | 修改的 TS 文件无异常；源码检查 `eslint . --ignore-pattern 'experiments/**'` 0 errors / 6 既有 warnings。原始 `pnpm lint:eslint` 失败：实验目录编译产物产生 24 条解析错误，不宣称全仓 lint 全绿 |
| ARM macOS stdio | 制品内 Python 实际运行；七工具、list/read/subscribe/unsubscribe、通知、两个工作目录隔离、关闭 cleanup_complete、会话退出通过 |
| x86_64 macOS stdio | 同上，在 Apple Silicon 的 Rosetta 下通过；不是 Intel 实机证据 |
| Linux / Windows | 载荷已重建、metadata 已确认；本机无法原生运行，原生 stdio/进程回收仍待对应平台验收 |
| WKWebView 长历史 | 原生 Tauri 窗口，生产客户端 factory/HTTP bridge、真实 HTTP/Socket.IO、发布包；使用上述真实 Shell 产生的约 200k 输出。预热后 A→B→A 三次加载，完整输出与脱敏结果逐字比较、snapshot 无凭据；最终三次耗时 229 / 216 / 218 ms，脱敏后正文长 200124 字符 |
| 连接 / 恢复 | 原生探针三次 join 同一 socket，dispose 后 socket 释放；生产 Chat 页面切 Robot/会话，原生 SettingsService 保存，退出进程后恢复 Robot 43 / Conversation 99，通过 |

MCP 验收使用载荷的完整临时副本，防止 tfbash 内部 isolated Python bootstrap 生成字节码改变打包目录。副本外的两个独立工作目录在 finally 中释放。初次直接运行导致 macOS 原件 hash 变化，已重新构建原件并改用副本验证；不得仅刷新该污染目录的 manifest。

本地 MCP 客户端直连 stdio 验证包协议，不能代替生产 Rust SDK 的所有调用/关闭路径。原生 WKWebView 的 Manager 身份、凭据/lease 获取由本地 fixture 提供；HTTP、Socket.IO、Kit、客户端前端适配和偏好持久化实际运行，不代表生产账号联调通过。Shell 输出经合成服务端历史返回，未调用真实 Agent 将 Shell 结果写入生产历史。

## 复现命令

```sh
# 每个平台分别 prepare / verify，产物目录不入 Git
pnpm runtime:prepare:tfbash -- aarch64-apple-darwin
pnpm runtime:verify:tfbash -- aarch64-apple-darwin

# 使用对应平台的制品内 Python；Windows 使用 python/python.exe
src-tauri/resources/tfbash/aarch64-apple-darwin/python/bin/python3 -B \
  e2e/tfbash-upgrade.py /tmp/issue90-tfbash-native.json

pnpm exec tsc --noEmit --project e2e/chat-restoration/tsconfig.json
pnpm exec vite build --config e2e/chat-restoration/vite.config.mjs
cargo build --manifest-path src-tauri/Cargo.toml \
  --features chat-restoration-acceptance --example chat_restoration_acceptance
CHAT_UPGRADE_SHELL_REPORT=/tmp/issue90-tfbash-native.json \
  node e2e/chat-restoration/run.mjs
```

验收进程运行时保持 GUI 会话可用。JSON 只有合成 Shell 输出和测试目录，不含用户历史或真实凭据。未传 `CHAT_UPGRADE_SHELL_REPORT` 时仍执行原重启验收。

## 尚未满足的完整验收条件

- Intel 实机、Linux、Windows 原生 stdio 与生产客户端生命周期；四平台安装包构建。
- macOS 签名/公证、签名后 manifest 校验及安装包启动。
- 真实 OS 中文 IME、附件上传阻塞时的快捷键与原生剪贴板成功/拒绝提示。
- 生产账号的 token/账户/组织切换、注销释放、迟到/无归属事件隔离、断线与诊断中文展示/复制。现有自动化契约和 fixture 不能替代这些真机联调。
- 真实 Agent 执行 Shell 长输出进入历史、切会话和重启恢复的全系统闭环。

以上完成前，不关闭 #90、不声称 v0.2.5 可发布。隔离审查 0 项阻塞；两项非阻塞建议见下方审查记录。

## 提交与回退组织

按已获授权的两个独立提交组织：① Chat Kit 包/锁/宿主 props/双语/Chat 与 WKWebView 测试/文档；② tfbash requirements/脚本/四锁/MCP 验收脚本及验证记录。组合 WKWebView 模式依赖第二提交生成的 Shell 报告；不传报告的原测试仍独立运行。二进制载荷不提交。

Chat Kit 回退须成组恢复六包、锁及新增宿主 props/labels 到 0.8.1，并重装重建；旧版长历史性能问题会随之恢复。tfbash 回退须成组恢复 0.2.0、旧 cutoff 和四锁，重新构建载荷及 manifest，关闭并重启 MCP 后核实实际版本。两项可独立回退，不能只改 manifest。

## 本地证据

- Chat 回归：`/tmp/issue90-chat-final.log`。
- Rust 单测 / Clippy：`/tmp/issue90-rust-tests.log`、`/tmp/issue90-clippy.log`。
- 原始 / 排除实验产物的 ESLint：`/tmp/issue90-eslint.log`、`/tmp/issue90-eslint-source.log`。
- MCP ARM / Rosetta：`/tmp/issue90-tfbash-native.json`、`/tmp/issue90-tfbash-rosetta.json`。
- WKWebView：`/var/folders/v7/6wb5d0ks3rx3v0wg682l2m6c0000gn/T/chat-restoration-QjFaDi/result.json`。
- 基线完整 SHA：`15ffcbe57cf6f6f84460c9f349a2558e1f44260d`。

四平台重新构建并经脚本完整 verify 的 payload SHA-256（签名会改变此值，届时按既有签名流程重新生成 manifest）：

| Target | payload SHA-256 |
| --- | --- |
| aarch64-apple-darwin | `2df9dfe2745ff2264a60f5cc210692fd0e34841d1280533fdf0736bb6ad20c8e` |
| x86_64-apple-darwin | `aea2f90182694d6c89d7fc571f53ede64dfc1fe5ee103eaa04ad855fd7845170` |
| x86_64-unknown-linux-gnu | `17932d57e760eedb3c3dcf072fe052b6b2699b103a1d38ea601f524b2c4690d1` |
| x86_64-pc-windows-msvc | `12e44deadb0da64b3d00a7ed9708dc1e35a0d849cbc8cc481e476468bea3fde2` |

## 隔离审查过程

第一轮只读隔离审查发现 B1：中文错误摘要沿用上游英文默认实现。已验真并按 `fix-review block` 修复，使用 Kit 提供的 formatChatError 扩展点，避免显示原始 message/details/任意 operation；新增真实发布包 + 本地 HTTP 503 + UI 的双语诊断与复制成功/拒绝测试，以及未知发送结果和敏感字段断言。第二轮在独立上下文重新审查完整差异，结论 APPROVE：0 项阻塞，保留下述两项建议。

默认 block 模式下保留两项非阻塞建议：生产 Chat 重启尚未断言长工具正文恢复（现有逐字断言发生在独立 probe）；服务端 socket map 的立即释放断言可能受 disconnect 与 IPC 事件竞态影响而误报失败。两项均未用来声称完整生产闭环通过。

## PR 目标与基线范围

PR 目标为用户指定的 `develop`。准备 PR 时远端 develop 为 `67f16a434c713015f6a1c973e304f2a3d5e97959`，它是本次验证基线 `15ffcbe57cf6f6f84460c9f349a2558e1f44260d` 的祖先，落后 10 个既有提交。PR 因此同时携带 dev-0.2.5 的既有基线改动和本次两个升级提交。本记录的实现、77 项 Chat 回归与隔离审查范围是 #90 的升级差异，不宣称对其余 10 个提交做了全量复验或新的隔离审查。完整验收缺口保留在 Draft PR 中。
