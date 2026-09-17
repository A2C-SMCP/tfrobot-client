# dev-0.2.5：Chat Kit 0.8.2 与 tfbash MCP 0.2.1 升级指导

检查日期：2026-09-17，Asia/Shanghai。下文保留升级前调研依据；Issue #90 已按确认方案完成本地依赖升级与部分真实验收，当前结果与剩余门槛见 [实施验收记录](issue-90-upgrade-validation.md)。调研中的“源码确认”不等于运行验证通过。

## 1. 结论与目标版本

建议在 dev-0.2.5 升级至以下已发布稳定版本，分成两个可独立验证、回退的提交。优先 Chat Kit，因为它包含当前 macOS 长历史加载问题的对应修复。

| 组件 | 当前锁定 | 建议目标 | 一手发布依据 | 本次判断 |
| --- | --- | --- | --- | --- |
| Chat Kit 主包及五个内部包 | 0.8.1 | **全部精确锁定 0.8.2** | npm latest 六包均为 0.8.2；GitHub v0.8.2 于 2026-09-17 04:02:35 UTC 发布 | 应升级；必须处理快捷键变化、连接复用及诊断本地化 |
| tfbash-mcp | 0.2.0 | **0.2.1** | PyPI 0.2.1 wheel 于 2026-09-02 07:50:54 UTC 上传，未 yanked；GitHub v0.2.1 | 可升级；重点是四平台离线制品与 stdio 回归 |
| 内置 Python | 3.12.14 / 20260825 | 本轮保持 | tfbash 0.2.1 仍要求 Python >=3.10,<3.13 | 无本次升级驱动的 Python 换代需求 |
| A2C-SMCP Rust SDK | 0.4.1 | 本轮保持 | Client 使用既有 stdio MCP 配置；新功能未要求新增 SDK 调用 | 静态判断无强制联动；以真实 MCP 验收确认 |

“最新版”在本文指发布渠道的稳定版本，排除 main 未发布提交与预发布包。实施前重新查询版本；若发布了更新版本，先补差异评审，不把本文的结论直接套用到新版本。

## 2. 基线、范围及研究边界

- Client：`dev-0.2.5@a16427eb4642c1bd150e85b1b27fdd0d09ce6cb9`，远端同名分支一致。当前 package.json / Cargo.toml / Tauri 配置的产品版本仍为 **0.2.4**；分支名不代表安装包已是 0.2.5。产品版本应在统一发版步骤中处理。
- 实施基线已前移至 `dev-0.2.5@15ffcbe`；调研时的 Chat 诊断日志及 Promise mock 已随 `0869a39` 提交。升级保留这些已提交改动。诊断目录及 Rust 制品清理脚本仍为既有未跟踪内容，不纳入 #90。
- 当前 `docs/diagnostics/` 还有未跟踪的历史超时诊断记录；本文只引用其结论，不复制私人历史数据或凭据。
- 研究问题：已发布最新版是什么；是否保持现有宿主契约；哪些默认行为改变；离线包应如何更新；哪些真实验收能否决升级。
- 候选仅限用户指定的两项依赖。对照方案是保留旧版、只升级已发布包、进一步启用上游可选能力；替换聊天框架或 Shell 实现不在范围内。
- 结束条件：注册表与发布 tag 一致；两组 tag diff、关键入口、测试及客户端调用链已核对；未知项转成明确验收条件。

| 上游 | 旧 tag SHA | 新 tag SHA |
| --- | --- | --- |
| tf-chat-kit | `de2250cf1b5258b9d7a8693c17d3cc3915cf89ba` | `e6e23913363fa1c66130e7ab9e72d5ca9c81fbd7` |
| tfbash-mcp | `2813c0590666084e5dd80eb141bc1ec727f7f131` | `88042abc946fc940bf48ee8a58b2ea12fe36a326` |

已通过 npm/PyPI API、GitHub Release/API、隔离 clone 和完整 tag 文件差异核对。查询时两个上游的公开 open Issue 列表均为空；这只表示无当前公开 open 项，不能替代兼容性验证。Chat Kit 仓库标记 MIT；tfbash 的 GitHub license 字段为空，本次 tag diff 未改变授权声明，继续沿用已有依赖引入记录，不作新增授权推断。

## 3. Chat Kit 0.8.2：收益与宿主适配

| 变化 | 源码核对 | 对本项目的实际影响与建议 |
| --- | --- | --- |
| 长文本脱敏性能修复 | `chat-protocol/src/raw.ts` 改为扫描嵌入凭据参数；新增 `chat-redaction-performance.test.ts`、`long-history-webkit.spec.ts` | 与本地 0.8.1 JavaScriptCore 卡顿诊断对应。优先升级；不得以增加 15 秒请求 deadline、关脱敏或截断全部历史代替修复 |
| 默认快捷键变化 | `chat-composer.tsx` 默认 `sendShortcut="ctrl-enter"`；CHANGELOG 明确这次 patch 包含交互变化 | 现有 `ChatConversationView` 未传该参数，升级后 Enter 换行、Ctrl+Enter 发送，macOS 的 Cmd+Enter 不发送。必须明确选择，而不是当成无行为变化的补丁 |
| 同实例连接复用 | `socket-transport.ts`、`socket.ts` 跨会话保留健康 Socket.IO 连接；有独立连接复用集成测试 | 同 Robot A→B→A 可复用连接；每次仍执行目标订阅与服务端同步。继续由 OwnedChatProvider / client dispose 管理生命周期，不能全局共享多个账户/Robot 的 Gateway |
| 提示降噪与诊断记录 | Runtime 有界诊断存储，View 内置 notices 与复制诊断 UI | 主动补中文文案；不能删除真正阻塞错误、把恢复提示当可发送权限。现有日志回调仍独立工作 |
| 可选 RemoteTool / Ask User | 新增 `createTFRobotRemoteToolClient`、typed handlers 与路由接口 | **不是版本升级自动启用的能力**。本轮不注册远程工具或启用实时回答；需要可信请求→会话路由、鉴权与 Server 联调后单独接入 |

### 3.1 统一六包，防止混装

修改 `package.json`：

- `dependencies.@turingfocus/chat-kit` → `0.8.2`。
- 五个 `pnpm.overrides`：`chat-gateway-tfrobot`、`chat-protocol`、`chat-react`、`chat-runtime`、`chat-ui-antd` → `0.8.2`。
- `pnpm install` 更新 `pnpm-lock.yaml`；不要只执行主包 add 而留下旧 overrides。npm 主包依赖是 `^0.8.2`，因此仍需统一锁定内部实现。

React / React DOM peer 为 `>=18.2.0 <19.0.0`，Ant Design 为 `>=5.23.4 <6.0.0`，现有依赖约束匹配，无需为本次升级切换 React 19 或 Ant Design 6。

安装后用 `pnpm list --depth 10` / `pnpm why @turingfocus/chat-protocol` 核查解析结果，确认六包均为 0.8.2。若开发 WebView 报缺少导出，重启 Vite 并强制重建预打包（例如 Vite `--force`）；本项目 0.8.1 升级已有旧 Vite 缓存导致白屏的记录，不能据此误判新版导出缺失。

### 3.2 明确发送快捷键

用户已确认此次保留 Enter 发送习惯：在 `src/components/Chat/index.tsx` 的生产 `<ChatConversationView>` 显式传 `sendShortcut="enter"`；Ctrl+Enter 同样发送，Shift+Enter 换行。此项已在 #90 实施计划中获得确认。

若希望统一使用上游新默认，则显式传 `sendShortcut="ctrl-enter"` 并采用“Enter 换行，Ctrl+Enter 发送”文案。macOS 也使用 Ctrl，不能提示 Cmd。两种模式均要验证输入法组合态、组合结束后的 Enter、长按重复、附件上传期间与发送中的防重复提交。

### 3.3 补文案与诊断集成

修改 `chatUiLabels()` 与 `src/locales/{zh,en}/translation.json`，至少补以下可选标签，避免类型检查通过但界面退回英文：

- 快捷键：`composerEnterHint`、`composerCtrlEnterHint`。
- 诊断：`diagnostics`、`copyDiagnostic`、`diagnosticCopied`、`diagnosticCopyFailed`、`diagnosticDetails`、`noDiagnostics`、`activeFaults`。
- 恢复/认证：`signInAgain`、`recoveredComplete`、`recoveredBestEffort`。

可选 `formatChatError` 只处理 Kit 的安全标准错误，不重新暴露原始异常。`diagnostics: { appVersion, onRecord }` 可在 factory 侧接入；建议带真实应用版本，是否落入宿主日志按已有日志边界决定，不重复存储消息正文。不要把 Gateway 的 `onDiagnostic/onLifecycleDiagnostic` 与新的 Runtime `onRecord` 当成互相替代。

诊断预算为每 client 最多 50 条、单条 8 KiB、合计 128 KiB UTF-8 JSON，dispose 清空；不是跨重启日志。内置复制动作要在 WKWebView 下实测剪贴板成功与拒绝提示。`onRequestAuthentication` 是可选宿主入口；需要时连接现有 Manager 登录流程，不能让 Kit 自行持有 Manager 主令牌。

notice 默认进度延迟 2 秒、断线延迟 5 秒、恢复完整提示 3 秒、best-effort 提示 5 秒。本轮沿用默认，不以缩短提示时间改变实际重连/缓存权限。这些是一次性展示计时，不需要新增轮询。

### 3.4 保留现有宿主边界

- 保留 `CURRENT_SERVER_PROFILE` 的 current-server 与 bounded rebase，保留 Rust lease、Tauri fetch/upload/resource port、原生资源下载/打开和按身份销毁。
- 上游没有可靠 leave：连接存活期间可能保留访问过的房间，Gateway 在切换后过滤其他会话及无归属应用事件。不能宣传服务端已退订、所有历史事件可靠回放或所有消息自动补齐。
- 0.8.2 已含 Ask User 组件，但发布说明仍限制实时回答。客户端不能通过删掉原有禁用提示来“启用”它。
- 0.8.1 的实例缓存、错误分类与中文资源标签继续保留；本轮不增加磁盘会话缓存。

## 4. tfbash MCP 0.2.1：离线制品升级

### 4.1 源码变化和边界

Client 的 `services/built_in_tools.rs` 使用打包 Python 执行：

```text
-m tfbash_mcp --transport stdio --runtime-profile auto --host-profile ide --workspace-root <path>
```

tfbash 0.2.1 的主要新增是 `EmbeddedShellRuntime.list_resources/read_resource/subscribe_resource_updates`，提取共享 `ShellResourceAdapter` 并让 stdio server 复用。Shell Overview 资源在 0.2.0 的 stdio server 已存在；**不能把本次升级描述成 Client 首次获得 Shell Overview 或必须改为 Python 嵌入方式**。

资源 URI 仍为 `window://io.github.a2c-smcp.tfbash/shell-overview`，MIME 仍为 `text/markdown`，更新仍走事件订阅。七工具定义所在的 `mcp_adapter.py` 除导入该 URI 外无工具实现变更，CLI/runtime/domain 未发生本次修改。Python 范围与全部第三方依赖声明也未改变。

因此，本轮预计无需改动 Client 的命令参数、bundle ID `tfrobot_tfbash_mcp`、工作目录隔离或启用策略；仍需真实 MCP 证明该判断。新 Python 嵌入回调在线程中同步执行的规则与 Client 当前 stdio 接入无直接适配关系。

### 4.2 必须成组修改的文件

| 文件 | 改动 |
| --- | --- |
| `scripts/tfbash-requirements.in` | `tfbash-mcp==0.2.1` |
| `scripts/prepare-tfbash-runtime.mjs` | `TFBASH_VERSION='0.2.1'`；更新 `RESOLUTION_CUTOFF` |
| `scripts/tfbash-locks/aarch64-apple-darwin.txt` | 重新解析并生成哈希 |
| `scripts/tfbash-locks/x86_64-apple-darwin.txt` | 同上 |
| `scripts/tfbash-locks/x86_64-unknown-linux-gnu.txt` | 同上 |
| `scripts/tfbash-locks/x86_64-pc-windows-msvc.txt` | 同上，保留 Windows marker / pywinpty 原生依赖 |
| `src-tauri/resources/tfbash/<target>/` | 重新构建实际离线载荷与 manifest；该目录被 gitignore，不提交二进制到 Git |

旧 cutoff 是 `2026-09-01T00:00:00Z`，早于 0.2.1 上传时间，继续使用会使解析无法选择新包。建议本次固定为 `2026-09-17T00:00:00Z`，脚本与四份锁文件生成命令保持一致；后续不能使用浮动“今天”。

使用与 CI 一致的 uv **0.8.17**。修改 requirements 后，按当前文件头模式生成，优先保留旧锁中兼容的第三方版本，仅指定升级 tfbash；审查实际 diff，不能把新 cutoff 解释为批准全面升级所有传递依赖：

```sh
for target in aarch64-apple-darwin x86_64-apple-darwin x86_64-unknown-linux-gnu x86_64-pc-windows-msvc; do
  uv pip compile scripts/tfbash-requirements.in \
    --output-file "scripts/tfbash-locks/$target.txt" \
    --python-version 3.12 --python-platform "$target" \
    --only-binary :all: --generate-hashes \
    --exclude-newer 2026-09-17T00:00:00Z \
    --upgrade-package tfbash-mcp --no-emit-index-url
done
```

这是待执行命令，本次未解析四平台锁。若有额外依赖变化，记录原因与对应平台 wheel 可用性。PyPI 新版 wheel SHA-256 为 `807615f18fd09257a9b589fe251b901864dbbf3398ef4eb877f2394c5ecda273`；用于核对，不代替 uv 生成完整依赖哈希。

每个平台准备和验证：

```sh
pnpm runtime:prepare:tfbash -- aarch64-apple-darwin
pnpm runtime:verify:tfbash -- aarch64-apple-darwin
```

其他三个 target 分别运行同样命令；交叉准备不代表可以在 macOS 执行 Windows/Linux Python。运行验证放到对应 OS/CPU 的机器或 CI。

manifest 校验覆盖版本声明、锁文件 SHA、整个 payload SHA。另用对应原生平台打包 Python 的 `importlib.metadata.version('tfbash-mcp')` 确认实际安装为 0.2.1。**不能只用 `--refresh-manifest` 给旧载荷换新版声明**：该选项用于 macOS 签名改变二进制后的哈希刷新，并不会执行依赖升级。

现有 release workflow 已覆盖四 target，macOS 会签名所有嵌套 Mach-O 再 refresh manifest，应保留该顺序。本轮没有依据要求改动 Python 版本、下载校验和或签名权限。

## 5. 实施顺序与文件范围

1. 在 dev-0.2.5 最新基线建立升级工作分支；将既有未提交日志改动与升级差异明确分开。复核注册表版本并保存 SHA/元数据。
2. Chat Kit 六包与 lockfile一起升级；明确快捷键，补 labels；按需接入安全诊断 appVersion。保留已有业务适配。
3. 执行 Chat Kit 宿主契约和真实 WebView 验收，通过后形成独立提交。
4. tfbash requirements、版本常量、cutoff、四份锁一起更新；重建各平台离线载荷并验证，形成独立提交。
5. 在同一客户端中联合验证：内置 Shell 长输出进入聊天历史、切会话、重连、重启后读取；再完成隔离代码审查与发版检查。

前端预计文件：`package.json`、`pnpm-lock.yaml`、`src/components/Chat/index.tsx`、`chatBridge.ts`、两种 locale、相关 Chat 测试；tfbash 文件见上表。仅在真实发现新契约缺口时修改 Rust built_in_tools，不预先扩大 SDK/Server 范围。

本指南不创建/关闭升级工单、不提交/推送代码。前次 #88 授权仅适用于 #88，不延伸到本次升级实施。

## 6. 验证矩阵与放行条件

| 层级 | 必须验证 | 通过标准 |
| --- | --- | --- |
| 版本/构建 | 六个 npm 包精确版本，四份 Python 锁，实际 runtime metadata | 不混装、不遗留 0.8.1/0.2.0 实现，冻结 lock 安装通过 |
| Chat 基础 | 既有 Chat、Contract、Attachments、Cache、Resources、Restoration 测试 | 构建/类型/契约通过；mock Kit 的测试不能独立证明升级成功 |
| macOS 长历史 | 真实 WKWebView，长约 200k 工具输出、分隔符密集输入、合成凭据、预热及重复加载 | 正常完成、UI 可响应、不再因同步脱敏耗尽既有 deadline；正文保留且凭据被移除；不能只测 Node/V8 |
| 快捷键 | 选定模式、Ctrl/Shift/Cmd、中文 IME、上传与重复提交 | 行为与文案一致，无误发送、双发、草稿/附件丢失 |
| 连接/身份 | 同 Robot A→B→A，快速 A→B→C、断线重连、token 变更、切账户/组织/Robot、注销 | 健康连接按实例复用；目标历史仍同步；迟到/无归属事件不串会话；身份变化不沿用旧连接；dispose 释放 |
| notices/诊断 | 短/长断线、缓存同步失败、401/403、未知结果发送超时、复制诊断 | 阻塞错误可见，恢复等级准确，不自动重发，不导出正文/令牌，中文和剪贴板错误提示正确 |
| tfbash 原生 stdio | 使用制品内 Python 启动，initialize、tools/list、七工具核心路径 | 版本 0.2.1、工具集合/输入输出契约与旧版一致，命令执行和关闭正常 |
| tfbash resources | resources/list/read/subscribe/unsubscribe，创建与执行后更新，关闭后不再通知 | URI/MIME 一致，通知及注销正常，无关闭挂起；用真实 MCP 客户端，不仅 import Python |
| 跨平台/离线 | 四 target wheel 安装、对应平台启动、禁用→启用、Client重启/退出、多 Computer 工作目录 | 不依赖运行时在线下载；原生 Shell 可发现，工作目录隔离，进程正确回收 |
| 安装包 | 四平台包构建；macOS签名/公证/安装启动 | 下载/载荷哈希一致、签名后 manifest 有效、打包后仍使用新版 |

可先运行的现有本地命令（升级实施后执行）：

```sh
pnpm build
pnpm test -- src/test/components/Chat.test.tsx src/test/components/ChatKitContract.test.ts src/test/components/ChatBridge070.test.ts src/test/components/ChatAttachments080.test.ts src/test/components/ChatCache081.test.tsx src/test/components/ChatResources.test.ts src/test/components/ChatRestoration.test.tsx
cargo test --manifest-path src-tauri/Cargo.toml --lib services::built_in_tools::tests
pnpm fmt:check
pnpm lint:ts
pnpm lint:rust
```

ESLint 注意本工作区 `experiments/**` 下可能有既有编译产物解析错误；应区分源码失败和扫描范围污染，不把全仓失败报告成通过。原生新增场景可复用 `e2e/chat-restoration/`、`e2e/chat-background/` 的事件驱动验收模式；具体新增测试路径在实施时建立，本次未声称已有全覆盖用例。

上游的 `tauriMock/officeMock` 发布证据是包级模拟消费者；0.8.2 release 明确 `tfrobotfrontReal` 与 `secondHostReal` 缺失。不能用上游 release 通过代替本项目 WKWebView、真实账号/会话及安装包验收。真实账号验收应使用授权环境，脱敏记录结果，不保存私人历史响应。

## 7. 回退与发布条件

- Chat Kit 回退：反向提交该组件升级（六包+lock+宿主新 props/labels），回到 0.8.1；不要只改主包版本留下新版 props 或内部包。注意旧版长历史性能问题会随之恢复，回退是故障处置，不是该问题的解决方案。
- tfbash 回退：同时恢复 0.2.0 requirements、版本常量、旧 cutoff、四锁并重新构建载荷/manifest；已有升级产物不能直接复用。终止/重启运行中的旧 MCP 进程后核实实际版本。
- 两项不互相强依赖，可独立回退。远程 npm/PyPI 版本或上游 tag 不移动、不覆盖。
- 本次没有新增持久化格式迁移的源码证据，但仍要验证 workspace、会话恢复、缓存隔离与打包。未完成真实验收前只能报告“依赖升级完成”，不能报告“v0.2.5 可发布”。

## 8. 主张—证据与未知项

| 主张 | 类型/置信度 | 证据 | 限制/反证门槛 |
| --- | --- | --- | --- |
| 稳定目标为 0.8.2 / 0.2.1 | 事实，高 | 注册表、release 与固定 tag | 后续发布会使“最新”失效，实施前复查 |
| 0.8.2 包含对应 JSC 脱敏修复 | 事实，高 | raw.ts diff、Protocol CHANGELOG、WebKit 测试 | Client 真机复验尚未运行，不能承诺所有历史性能问题已消失 |
| Client 升级后默认 Enter 行为会变化 | 事实，高 | 当前 View 未传 sendShortcut；新版 composer 默认 ctrl-enter | 显式选择 enter 可保留习惯 |
| tfbash 不需改变当前 stdio 参数 | 推断，中高 | pyproject/CLI与七工具契约diff、Client入口 | 四平台 MCP运行失败即否决直接放行 |
| Shell Overview 不是 stdio 新增能力 | 事实，高 | 旧 server.py 已注册同URI；新共享resource_adapter | Python嵌入API是新增，不混同两条入口 |
| 本轮无需启用 RemoteTool/升级SDK | 范围建议，中高 | 可选独立factory、可信路由前提、现有stdio契约 | 若新增产品目标要求实时Ask User，必须另行设计并联调 |

本次未运行性能 benchmark、候选 npm 包构建、四平台锁解析或真实 MCP/WebView 实验；不提供虚构 PASS。发布说明与本地故障使真机实验成为实施放行条件，方案已经列于上方矩阵。若要在编码前量化收益，再以独立隔离实验比较相同合成输入下旧/新包，而不是在当前用户环境直接替换 node_modules。

## 9. 一手来源与调研产物

- [Chat Kit 0.8.2 Release（含消费者边界）](https://github.com/A2C-SMCP/tf-chat-kit/releases/tag/v0.8.2)
- [npm 主包 0.8.2 元数据](https://registry.npmjs.org/@turingfocus/chat-kit/0.8.2)
- [Chat Kit 完整版本差异](https://github.com/A2C-SMCP/tf-chat-kit/compare/v0.8.1...v0.8.2)
- [固定版本主包 CHANGELOG](https://github.com/A2C-SMCP/tf-chat-kit/blob/e6e23913363fa1c66130e7ab9e72d5ca9c81fbd7/packages/chat-kit/CHANGELOG.md)
- [固定版本脱敏实现](https://github.com/A2C-SMCP/tf-chat-kit/blob/e6e23913363fa1c66130e7ab9e72d5ca9c81fbd7/packages/chat-protocol/src/raw.ts)
- [固定版本 WebKit 长历史测试](https://github.com/A2C-SMCP/tf-chat-kit/blob/e6e23913363fa1c66130e7ab9e72d5ca9c81fbd7/tests/e2e/long-history-webkit.spec.ts)
- [固定版本宿主接入与诊断文档](https://github.com/A2C-SMCP/tf-chat-kit/blob/e6e23913363fa1c66130e7ab9e72d5ca9c81fbd7/docs/host-app-integration.md)
- [tfbash 0.2.1 Release](https://github.com/A2C-SMCP/tfbash-mcp/releases/tag/v0.2.1)
- [PyPI 0.2.1 元数据](https://pypi.org/pypi/tfbash-mcp/0.2.1/json)
- [tfbash 完整版本差异](https://github.com/A2C-SMCP/tfbash-mcp/compare/v0.2.0...v0.2.1)
- [固定版本 tfbash 资源适配](https://github.com/A2C-SMCP/tfbash-mcp/blob/88042abc946fc940bf48ee8a58b2ea12fe36a326/src/tfbash_mcp/resource_adapter.py)
- [固定版本 tfbash 依赖/入口](https://github.com/A2C-SMCP/tfbash-mcp/blob/88042abc946fc940bf48ee8a58b2ea12fe36a326/pyproject.toml)

本地隔离只读源码与元数据：`/tmp/tfrc-upgrade-20260917/tf-chat-kit`、`/tmp/tfrc-upgrade-20260917/tfbash-mcp`、`npm-082-metadata.json`、`pypi-021-metadata.json`。未运行其安装脚本或业务代码，未删除这些调研产物。产品基线见本仓 `package.json`、`scripts/prepare-tfbash-runtime.mjs`、`scripts/tfbash-requirements.in`、`src-tauri/src/services/built_in_tools.rs`、`.github/workflows/release.yml`。
