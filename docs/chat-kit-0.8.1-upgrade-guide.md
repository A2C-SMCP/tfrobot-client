# Chat Kit 0.8.1 升级指导

日期：2026-09-10。范围：对照 Changesets、发布 tag 源码、npm 发布包和本项目实际接入，提供升级方案及实施记录；用户确认后，工作区六包已升级到 0.8.1。

追踪更新：本次升级使用 [#79](https://github.com/A2C-SMCP/tfrobot-client/issues/79)，实施计划见 `plans/CHAT_KIT_081_UPGRADE.md`。经在线核对，#76 已按维护者要求以代码完成口径关闭，#78 也已关闭；#76 关闭说明明确保留真实 Beta/Tauri 验收待办，本文相关验收要求不意味着旧 Issue 仍开放。

## 结论

建议升级，采用「六包统一 0.8.1 + 默认内存缓存 + 资源错误适配 + 中英文文案」，本轮不接入磁盘持久化。现有 API 可以直接编译，但 0.8.1 改变了默认加载行为，不能只按普通补丁替换版本号后直接验收。

临时副本仅修改六包版本后，前端生产构建及现有五个 Chat 测试文件的 38 项测试通过。它们证明现有接入的基础兼容性，不覆盖本轮新增缓存、本地化和错误映射的全部验收场景，更不代表 Beta/Tauri 验收通过。

## 版本与证据基线

- 本项目 HEAD：`eb6a6295455755f0e03834366ade12649c11970e`；评估包含工作区已有的 Chat 生命周期日志及测试 mock 修改，不覆盖这些改动。
- 评估开始时，`package.json` 主包和五个 `pnpm.overrides` 均为 `0.8.0`；实施后的状态见文末。
- 上游 `v0.8.0`：`5abc4f8d49cd5422c4fb8cb3c7993c6507087565`。
- 上游 `v0.8.1`：`de2250cf1b5258b9d7a8693c17d3cc3915cf89ba`。
- npm 已逐一确认六个生产包均发布 `0.8.1`；主包内部依赖是 `^0.8.1`，必须同步更新客户端 overrides，避免形成 0.8.1 外壳配 0.8.0 实现的组合。
- React / React DOM peer 仍要求 `>=18.2.0 <19.0.0`，Ant Design 仍要求 `>=5.23.4 <6.0.0`；本项目现有 React 18.3.1、锁定 Ant Design 5.29.3 满足要求。
- 已核对发布前 `7a3f771` 的四份 `.changeset/*.md`、发布后各包 CHANGELOG 和完整 tag diff；changeset 在版本提交中已消费，不能只检查当前 `.changeset` 目录。

## Changeset 与源码对照

| 变更 | 源码确认 | 本项目影响 |
| --- | --- | --- |
| `conversation-cache-recovery.md` / `a0dd631` | Runtime 默认创建实例级有界缓存；Kit factory 透传可选 `cache`；React 新增 `useConversationCache`；Workspace 接收缓存快照后更新 selection，UI 展示同步状态 | 同一 Robot 的 A→B→A 可先展示已缓存内容，再与服务端同步；现有自定义 compact workspace 可沿用 |
| `quiet-resource-errors.md` / `e3ba706` | Protocol 白名单归一化错误；资源 hook 和 UI 消费错误码；`labels.resource` 传到附件、Markdown、工具资源和事件详情 | 应补 Rust→Kit 错误码映射和资源文案；原有 port 签名兼容 |
| `quiet-chat-notices.md` / `4f32743` | UI 引入可关闭 Alert，状态或错误变化后重新展示；不更改 Runtime 状态 | degraded、连接错误、run 失败提示可关闭；关闭不等于恢复正常，也不应改变发送/中断权限 |
| `remove-event-navigation-bar.md` / `a2d7ab0` | 删除 Previous/Next、滑块、计数和 Follow latest 工具栏 | 这是可见 UI 变化；时间线选事件、详情 split/modal 与跳到最新消息仍保留 |

`chat-gateway-tfrobot/src` 在两个 tag 之间没有变更：Socket、current-server 兼容策略、上传协议没有新增接入要求。无需据此升级 Rust SDK 或要求 Server/Front 改造。

## 实施顺序与文件

### 1. 原子更新六包

在 `package.json` 中将主包以及以下五个 overrides 全部设为精确版本 `0.8.1`，再执行 `pnpm install` 更新 `pnpm-lock.yaml`：

```text
@turingfocus/chat-kit
@turingfocus/chat-gateway-tfrobot
@turingfocus/chat-protocol
@turingfocus/chat-react
@turingfocus/chat-runtime
@turingfocus/chat-ui-antd
```

检查 lockfile 的六包解析结果；不要只运行主包升级命令而遗漏 overrides。保持其他依赖升级独立。

### 2. 保留默认内存缓存，明确生命周期

`src/components/Chat/chatBridge.ts` 中现有 `createTFRobotChatClientFactory` 不传 `cache` 即启用默认内存缓存；无需新增客户端 Map 或手工复原消息。`cache: false` 是遇到缓存回归时的兼容开关，恢复旧的无缓存加载路径。

- 默认 LRU：20 个会话、10,000 条缓存时间线项目、8,000,000 字节编码内容；新鲜期 5 分钟，硬过期 24 小时，访问时检查，无轮询。这些是缓存预算，不能当作整个 WebView 的内存上限。
- 每次命中仍同步服务端，不能承诺减少每次切换的 HTTP 请求，也不是后台订阅全部会话。
- 从缓存恢复的视图先以只读状态显示；网络同步失败可继续阅读；认证、授权、not-found 失败会使相关缓存视图失效。不能把缓存命中当作实时连接恢复，也不自动重发消息或恢复旧 Ask User 操作权。
- 本项目 `CompactChatWorkspace` 已依赖 `useConversationWorkspace`。上游 `#handleClientSnapshot` 会把命中缓存的 pending 会话切到 ready；现有 `contentStateFor()` 无需复制缓存逻辑。应补真实 Kit 的延迟响应 UI 回归验证这条链路。
- 缓存属于 `OwnedChatProvider` 创建的 client。现有菜单往返保留实例，缓存可继续使用；切 Robot/身份导致旧实例销毁，不能承诺跨 Robot 或重启后恢复。
- 保留 `CURRENT_SERVER_PROFILE`、bounded rebase 和现有 lifecycle 日志。缓存状态与 lifecycle 状态分开理解。

本轮不传 `cache.storage` 或 `cache.attachments`。如果后续要求重启恢复，需要另行实现原子存储 update、稳定且隔离的 scope（Manager 环境/账户/组织/Robot）、注销清理和并发失效协调，以及附件重新授权。`leaseId` 是临时标识，不适合作为跨重启持久化 scope。上游持久化只保存受限展示投影，不是完整快照备份，不保留原生文件对象、私有资源 URL 或完整工具原始数据。

### 3. 在资源 port 边界转换错误

本项目 `src-tauri/src/services/chat_session/resources.rs` 通过 snake_case 序列化错误；`src/components/Chat/chatResources.ts` 当前直接把 IPC 拒绝交给 Kit。0.8.1 不识别其中的 `permission`、`not_found` 等值，会降级为 `unknown`。

建议在 TypeScript port 的 resolve/open/download 公共错误边界捕获并抛出 `ChatResourceError`（从 `@turingfocus/chat-kit/headless` 导入），保持 Rust 错误用于客户端精细诊断：

| 客户端错误 | 建议交给 Kit | 说明 |
| --- | --- | --- |
| `permission` | `unauthorized` | Kit 不区分未登录与无权限，文案应覆盖两者 |
| `not_found` | `not-found` | Kit 不提供资源解析 Retry 按钮 |
| `network`、`timeout`、`busy` | `network` | Kit 提供手动重试；宿主保留超时/队列满细分说明 |
| `unsupported` | `unsupported` | 不能自动打开不代表不能原生下载，保留宿主原说明 |
| `cancelled`、`AbortError` | `cancelled` | 中性取消，不弹失败 Alert |
| `invalid`、`too_large`、`save`、未知值 | `unknown` | Kit 没有精确等价码；保留现有中文原因，不伪装成权限或格式问题 |

Kit 的 `unauthorized/network/expired/unknown` 允许资源解析手动 Retry，`not-found/unsupported/cancelled` 不提供该 Retry；原生 open/download 动作错误仍通过用户再次点击动作重试。对通用 fallback 不应额外自动重试。

保留现有 Rust loopback → Front `resource/s3` 字节代理、`ChatResourceProvider` 的 lease scope、原生保存/打开及释放句柄逻辑。0.8.1 的浏览器默认下载不能替代本项目代理，也没有解决内部 MinIO 地址不可达的问题。

还要区分两条错误通道：port 的 IPC 拒绝可由 Kit 分类；`<img>` 获取 loopback 字节失败时，DOM 不提供可靠 HTTP 原因，Kit 只能给 `unknown`，音视频也只能按 MediaError 粗分。现有 `chat-resource-error` 脱敏事件及宿主 Alert 仍有必要，不能因为 Kit 有分类就删除。重复提示应按错误来源协调，不用 DOM 补丁隐藏。

### 4. 补齐公开文案

修改 `src/components/Chat/chatBridge.ts` 的 `chatUiLabels()`，以及 `src/locales/zh/translation.json`、`src/locales/en/translation.json`：

- 新增 5 个顶层 key：`cacheSyncing`、`cacheStale`、`cacheSyncFailed`、`cacheAttachmentUnavailable`、`cacheStorageFailed`。
- 新增嵌套 `resource` 的 19 个 key：`screenshot`、`generatedFile`、`fileTitle`、`image`、`audio`、`video`、`loading`、`retry`、`open`、`download`、`opening`、`downloading`、`unauthorized`、`network`、`not-found`、`expired`、`unsupported`、`cancelled`、`unknown`。
- 继续把同一 labels 对象传给 `ChatUiShell` / `ChatConversationView`，上游负责传播到资源子组件。现有 `chat.resourceErrors` 保留客户端特有的保存、超限、并发和网络原因。

旧 labels 类型保持兼容，所以漏补不会编译报错，而会出现英文 fallback。0.8.1 只补齐资源等已公开文案入口，不应宣传所有代码/References 控件已全面中文化。

## 验证与验收

### 本次已执行

在 `/tmp/chatkit-081-review.iDJXXa/client` 复制当前前端代码与 lockfile，仅更新六包版本，安装真实 npm 0.8.1 发布包（`--ignore-scripts`），执行：

```bash
pnpm build
pnpm exec vitest run src/test/components/Chat.test.tsx src/test/components/ChatBridge070.test.ts src/test/components/ChatKitContract.test.ts src/test/components/ChatAttachments080.test.ts src/test/components/ChatResources.test.ts --maxWorkers=2
```

结果：生产构建通过；5 文件 / 38 项测试通过。包含真实 Kit 的 current-server、上传、Markdown 资源契约；`Chat.test.tsx` 本身 mock 了 Kit，不能用它证明新的缓存 UI 行为。构建有既有的大 chunk 和动态/静态 import 提示；测试有 jsdom `scrollTo` 未实现提示，不影响此次结果。没有执行 Rust、完整前端回归或真实 Beta/Tauri 联调。

### 升级实现后必须补验

1. 使用真实 Kit 和可延迟响应的 fixture 验证 A→B→A：同步未完成时已显示 A 的缓存，发送/中断/旧 Ask User 不被错误启用；同步成功替换为服务端结果，网络失败保留只读视图，401/403/404 使相关缓存失效。
2. 验证快速 A→B→C、迟到响应、草稿/附件切换、切 Robot、切组织/账户、退出登录，确保不串内容；菜单往返保留现有 client，`cache: false` 可回退。
3. 使用真实资源 UI 验证 permission/not_found/timeout/cancelled 映射、Retry 可用性、取消保存对话框不报失败；中文和英文覆盖消息附件、Markdown、工具资源及事件详情。
4. 验证提示关闭后底层状态不变，新错误/新连接状态/切会话按上游语义重新展示；移除导航栏后仍能选择时间线事件并打开 split/modal 详情，隐藏页面不留下浮层。
5. 执行前端全量测试、TypeScript/ESLint 和生产构建；若修改 Rust 再运行相应 Rust 测试与 Clippy。
6. 继承 #76 尚未完成的 Beta/Tauri 验收：真实 PNG/JPEG、原生保存与系统打开、音视频播放/拖动、多个附件、长时间后重读、身份与权限隔离。0.8.1 不改变部署 Front 的 Range 支持，不能据此关闭历史附件验收。

回退优先按原因处理：缓存专属问题可先设 `cache: false`；完整回滚时原子恢复六包版本和 lockfile，并同步撤销仅 0.8.1 导出的类型/API 使用，保留现有资源代理及独立日志改动。无需做持久化数据迁移，因为本方案未启用磁盘缓存。

## 固定版本来源

- [npm 0.8.1 元数据](https://registry.npmjs.org/@turingfocus/chat-kit/0.8.1)
- [完整 tag 对比](https://github.com/A2C-SMCP/tf-chat-kit/compare/v0.8.0...v0.8.1)
- [发布前 Changesets](https://github.com/A2C-SMCP/tf-chat-kit/tree/7a3f771/.changeset)
- [主包 CHANGELOG](https://github.com/A2C-SMCP/tf-chat-kit/blob/de2250cf1b5258b9d7a8693c17d3cc3915cf89ba/packages/chat-kit/CHANGELOG.md)
- [UI CHANGELOG](https://github.com/A2C-SMCP/tf-chat-kit/blob/de2250cf1b5258b9d7a8693c17d3cc3915cf89ba/packages/chat-ui-antd/CHANGELOG.md)
- [缓存实现](https://github.com/A2C-SMCP/tf-chat-kit/blob/de2250cf1b5258b9d7a8693c17d3cc3915cf89ba/packages/chat-runtime/src/chat-client.ts)
- [缓存宿主契约](https://github.com/A2C-SMCP/tf-chat-kit/blob/de2250cf1b5258b9d7a8693c17d3cc3915cf89ba/docs/baselines/issue-77/integration.md)
- [资源错误契约](https://github.com/A2C-SMCP/tf-chat-kit/blob/de2250cf1b5258b9d7a8693c17d3cc3915cf89ba/packages/chat-protocol/src/resources.ts)
- [公开文案定义](https://github.com/A2C-SMCP/tf-chat-kit/blob/de2250cf1b5258b9d7a8693c17d3cc3915cf89ba/packages/chat-ui-antd/src/labels.ts)

来源采用固定 tag 的源码及 npm 实际发布结果；上游 integration 文档尾部有“待提交”的历史阶段说明，不以该说明替代此次发布核验。

## 2026-09-10：用户确认后的实施记录

- 主包及五个 overrides 已统一为 0.8.1，lockfile 中无 Chat Kit 0.8.0 解析项；新增 `socket.io@4.8.3` 仅用于本地真实协议测试。
- 采用 Kit 默认实例级内存缓存；没有配置磁盘存储、跨 Robot 缓存或轮询。现有 compact workspace 仅增加命名导出供真实宿主测试挂载，运行路径保持。
- `chatResources.ts` 在 resolve/open/download 公共边界把原生错误转换为 `ChatResourceError`；取消为中性结果，诊断只传原生白名单错误码。现有资源字节代理、原生动作和句柄释放保留。
- 已补 5 个缓存提示及 19 个资源文案的中英文映射。
- 新增缓存测试真实运行宿主 factory → Tauri IPC 测试桥 → 本地 HTTP，以及真实 Socket.IO / Runtime / compact workspace / UI。IPC 测试桥模拟 Rust 边界，不能声称该测试运行了 Rust 或真实 Manager。覆盖缓存显示与同步、网络失败只读、401/403/404 失效、草稿附件、迟到响应、新实例隔离、提示关闭及真实事件详情。
- 扩展真实资源 UI 测试，验证原生错误映射、Retry 可用性、取消、中英文原生下载提示。Tauri 保存对话框由 IPC fixture 代替，真实对话框仍需桌面验收。
- UAT/Seed 评估：已增加 `.claude/skills/UAT/resources/scenarios/chat-kit-081.md`；沿用既有账号与数据，不修改 Seed。真实 Beta/macOS Tauri 尚未执行。

验证与审查的最终结果在本节后续补充。当前标准 `pnpm lint:eslint` 会扫描已有 `experiments/codex-chat-background-heartbeat/target/**` 生成的非源码 `.js` 文件，报 24 个解析错误；未改动实验产物或 ESLint 配置。执行 `pnpm exec eslint . --ignore-pattern '**/target/**'` 后为 0 error、6 个既有 warning。此排除仅针对生成目录，不屏蔽业务源码。

最终实现的质量验证：`pnpm test` 60 文件 / 591 项全部通过（173.50 秒）；`pnpm build` 成功，包含 TypeScript 编译；单独 `pnpm lint:ts` 通过；`git diff --check` 通过。构建保留动态/静态 import 与大 chunk 提示，jsdom 有未实现布局 API 的提示。新增缓存专项 8 项和资源专项 21 项均已包含在该全量结果中。尚未 commit/push/创建 PR。

最终隔离审查：`fork_turns="none"` 只读审查代理按 code-review rubric 检查完整暂存与工作区差异、生产调用链和上游源码，结论 **APPROVE，0 个阻塞项**。非阻塞测试建议：现有中断与旧 Ask User 命令拒绝断言没有先构造运行中任务/真实待答请求，不能单凭这两条断言证明全部旧操作权边界；缓存只读、发送禁用和上游源码另有证据。按默认 block 模式保留该增强建议，不把它描述为已覆盖的真实待答交互场景。另已纠正文中“当前 0.8.0”为评估开始时的基线。


## 2026-09-10：开发窗口白屏排查

用户提交前反馈 Tauri 白屏。对同一 localhost:1420 入口在浏览器复现：`@turingfocus_chat-kit_headless.js` 缺少 `ChatResourceError` 导出。磁盘安装包确为 0.8.1 且包含导出，但 `node_modules/.vite/deps/_metadata.json` 的主包及 headless 入口仍指向 0.8.0。

已执行一次 `pnpm exec vite optimize --force` 重建缓存（Vite 6 支持该命令但提示手动 optimize 已弃用），然后触发开发服务配置重新加载。重新检查 metadata 指向 0.8.1，预构建模块包含新导出，浏览器重新加载已出现应用界面。浏览器不具备 Tauri 原生 bridge，剩余原生 API 报错不作为真实桌面验收；Tauri 窗口恢复仍需用户确认。

后续依赖升级如遇同类缺失导出，应停止开发进程后用 `pnpm dev --force` 启动以刷新预构建依赖，再刷新 WebView；不需要回退 Kit、伪造导出或永久禁用缓存。开发缓存不由生产 build / Vitest 自动验证，本轮补充真实开发入口加载检查。缓存失配的具体形成过程尚未确定，不把其归因于已确认的 Vite 自动失效缺陷。参见 [Vite 依赖缓存说明](https://vite.dev/guide/dep-pre-bundling#caching)。
