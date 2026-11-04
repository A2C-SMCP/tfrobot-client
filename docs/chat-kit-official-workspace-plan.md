# 官方聊天工作区接入计划

日期：2026-11-04。Client 追踪：[#119](https://github.com/A2C-SMCP/tfrobot-client/issues/119)。
上游阻塞：[Chat Kit #166](https://github.com/A2C-SMCP/tf-chat-kit/issues/166)。
前置版本升级：[#118](https://github.com/A2C-SMCP/tfrobot-client/pull/118)，检查时仍为 open。

## 已确认目标与边界

用户要求聊天卡片内部与 Chat Kit Playground 使用相同的官方组件，随正式 Kit 版本升级同步内部样式。client 负责外部侧栏、机器人/平台选择、原生凭据和网络边界；Kit 负责内部导航、消息、事件详情、输入框和默认展示。

保留现有平台隔离、会话位置持久化、页内保留和后台 Socket 语义。继续支持 Enter 发送和 IME 保护。默认产品模式无开发诊断。颜色可以继承宿主主题；视觉对比必须固定相同主题、版本、尺寸和数据，不以 Playground 整页背景或账号头作为卡片内部的一致性标准。

## 运行版本核查

磁盘安装 chat-kit 0.11.0，但 1420 开发服务器实际提供的旧 Vite 预构建代码中，ChatConversationView 默认 `presentation = "standard"`。这是截图中诊断按钮、旧事件箭头与完整时间戳等差异的直接证据。

已将旧 deps 缓存备份到 `/tmp/tfrc-workspace-119/vite-deps-before`，通过配置文件的 mtime 变更触发原 Vite 进程重启。未改配置内容。重启期间原服务短暂不可用，随后监听 localhost 的 IPv6 地址；额外的 `pnpm dev --force` 因端口占用退出，没有替换原服务。最终从 `http://localhost:1420` 重新获取实际前端模块，确认默认 `presentation = "compact"` 且诊断由 `debug` 控制。

前后代码证据位于 `/tmp/tfrc-workspace-119/chat-kit-before.js` 与 `chat-kit-after.js`。这是开发服务器响应级核验，不等价于桌面 WebView 截图或远端会话 UAT。缓存刷新解决已发布版本未生效的问题，但不等价于完成官方工作区迁移。

## 上游缺口与归属

核对发布版 0.11.0 和远端 develop `027f30187167c59f1727928b74e7f5de39525ab2`：官方 `ChatCompactWorkspace` 内部创建 workspace。它的 `restoredDraft` 是一次性恢复，不返回选择成功结果、不协调手动选择与晚响应、没有宿主恢复错误/重试边界。`onActivityChange` 中的会话 ID 不能证明选择已经成功提交。单独的 `getPopupContainer` 也不能关闭 Modal 的焦点锁和 body 滚动锁。

需要上游提供通用宿主接入能力，具体 API 由维护者评审，不在 client 中 fork 组件、创建第二套竞争控制器或依赖私有 DOM：

| 能力 | Kit 责任 | client 责任 |
| --- | --- | --- |
| 初始恢复与显式导航 | 同一控制器的恢复/导航协调，公开成功、失败和过期语义 | 从原生 IPC 读取偏好，执行现有回退策略 |
| 已提交选择观察 | 只通知最终成功选择，不混入 pending/失败结果 | 带租约、平台和 revision 写入偏好，失败可重试 |
| retained page | 隐藏菜单/弹层、释放全局焦点与滚动锁，保留聊天连接和草稿 | 注入 PageHost 活动状态及容器 |
| 输入体验 | 官方 composer 支持宿主快捷键策略和 IME 保护 | 选择 Enter 策略，不复制输入框 |

新建入口将采用 Kit 的首条消息建会话流程，替代 client 当前要求输入标题的窗口；这是用户已确认采用官方内部交互的迁移影响。重命名/删除等官方入口须检查服务端 capability 与原生路由允许范围，不能为了显示按钮放开任意原生写请求。

## 实施顺序与文件清单

1. 上游维护者确认 #166，提供公共接口、文档、兼容说明、测试及可安装正式版本。当前阻塞于此；没有正式接口前不删现有实现。
2. client 从 #118 已验证版本衔接：`package.json`、`pnpm-lock.yaml` 固定交付版本，不混用临时本地包作为最终交付。
3. `src/components/Chat/index.tsx`：将本地自组 CompactChatWorkspace 内部替换成官方 ChatCompactWorkspace。删除重复的会话导航、新建标题弹窗与详情展示控制；保留资源 Provider、原生 SessionProvider/HTTP bridge 和外层选择器。
4. `src/components/Chat/useChatRestoration.ts`：接入同一个官方 workspace 的公开恢复/选择接口，保留 out-of-page 恢复、403/404 回退、临时失败不改偏好、用户手动选择胜过晚恢复、revision 写盘保护。具体签名以交付接口为准。
5. `src/components/Chat/Chat.module.css`：移除事件触发器等卡片内部覆盖，只保留外部布局、尺寸和承载容器样式。
6. 更新既有 `src/test/components/Chat*.test.*`、`e2e/chat-restoration/` 夹具的官方导航操作，避免按旧 DOM 结构断言；新增同版本官方工作区视觉对照证据。测试文件拆分以实际接口复杂度决定，不预建替代组件。
7. 测试通过后按 add-feature 执行隔离上下文完整审查，阻塞项清零后供用户 Review。未经本次显式提交授权不暂存已有修改、不 commit/push/建 PR。

## 验收矩阵

- 同 Kit 版本、主题、viewport 和 fixture 数据下，与官方工作区对照：会话导航、事件卡片、消息宽度、长消息折叠、详情模式、附件/发送入口均由官方组件决定。
- 默认无开发诊断；业务失败、重试和草稿恢复仍可用。Enter/IME 行为保留。
- 首启、出第一页 ID、403/404 回退、临时失败重试、偏好读写失败、快速切换和迟到响应不串会话。
- 新建采用首消息建会话，不重复发送；历史和平台切换不串草稿；新增官方 mutation 入口受 capability 和原生边界约束。
- PageHost 隐藏/显示期间，菜单、确认框、事件详情不泄漏，无全局锁残留；客户端/Socket 和草稿不因隐藏销毁。
- 真实 Kit + 本地 HTTP/Socket fixture 回归、TypeScript、相关 ESLint、生产构建、macOS IPC 写盘/进程重启恢复通过。
- UAT/seed：本任务不改变远端 seed；新建首消息流程与历史入口变化需更新现有原生 fixture。真实 Manager 登录、远端服务及签名安装包验收单独记录，不把 fixture 结果当成线上通过。

## 当前结果

已建立 client 追踪、提交上游 P0 需求并刷新旧开发预构建缓存。再次核实时 #166 暂无回复，远端 develop 仍为上述 SHA。

client 非依赖部分已实施：删除 Chat.module.css 对事件行边框、详情触发按钮边框/最小宽度/颜色的覆盖，并移除 ChatConversationView 的专用样式类，使这些内部样式由已安装 Kit 决定。外层容器、原生接入和恢复逻辑未迁移。

用户后续截图仍有差异，进一步核实了配置来源：官方工作区默认 modal，client 先前默认 auto（宽屏自动分栏）；已对齐 modal 默认并移除宿主指定的 0.64 分栏比例。Kit 官方主题在 ManagerChatApp 内部（绿色、14px），client 使用桌面蓝色、13px；官方 compactChatZhCN 未经 chat-kit facade 转导出，client 的旧 labels 缺少新增折叠文案。这两项已补充到上游 #166，要求公开共享主题/locale/组件的接入方式，避免复制常量后再次漂移。上传按钮由 capability/uploader 决定，不通过隐藏上传功能来匹配截图。

详情行为回归改为经过生产 client CompactChatWorkspace（不再直接给 ChatConversationView 传 modal），验证真实事件通过默认工作区打开对话框。新的默认值用于新挂载工作区；已有保留页或 HMR 保留的用户模式不会被强制重置。

完整官方工作区迁移尚未实施，依赖上游 #166 接口及可安装版本交付；本次样式准备不等价于完成本功能。

样式准备验证：TypeScript、`eslint src/components/Chat/index.tsx`、`git diff --check` 均通过；ChatCache081 与 ChatUpgrade0101 两文件 12/12 通过，包含真实已发布 Kit + 本地 HTTP/Socket fixture 的缓存、错误、事件详情与隐藏页面确认框回归。原始日志 `/tmp/tfrc-119-style-tests.log`。从运行中的 Vite 服务重新获取 CSS module，已确认不含 client 的 `data-chat-event-trigger` 覆盖。没有新增官方工作区路径、视觉截图或远端 UAT 通过的声明。

默认详情模式对齐后，TypeScript、变更 TSX 的 ESLint、diff check 再次通过，ChatCache081 + ChatUpgrade0101 12/12 通过（`/tmp/tfrc-119-modal-tests.log`）。其中事件详情测试经过 client 工作区的默认配置，而非测试中强制传 modal。颜色/字号/中文文案/官方导航仍有差异，整体视觉验收未通过；不因本次定向行为回归通过而标记整体完成。

隔离上下文审查结论：APPROVE，仅限本次独立样式准备，0 个阻塞项。非阻塞建议：完整迁移时补同版本、主题、viewport 与数据的实际视觉对照。当前分支 `feat/official-chat-workspace`，改动尚未暂存、提交或推送。上游 #166 的跨仓实施归属正在向用户确认；本地准备审查通过不代表整体功能交付。

默认 modal 与生产工作区测试变更的第二轮隔离审查同样 APPROVE、0 阻塞项，仅覆盖准备改动。非阻塞建议：补事件详情弹窗“打开 → 隐藏页 → 恢复页”的实际浏览器生命周期回归；现有草稿确认框回归不代表所有弹层均已验收。

## 继续修改：可见主题与文案对齐

用户再次要求继续修改后，完成以下 client 内可独立交付的视觉调整：

- 显式依赖公开包 `@turingfocus/chat-ui-antd@0.11.0`，与 Kit 依赖组固定同版；不依赖 node_modules 路径或私有导出。生产 ActiveChat 传入当前语言，使用官方 `compactChatZhCN` 作为中文产品文案，保留原生诊断安全摘要和资源错误文案。后续该包正式更新时，新增中文键自然跟随，不复制 locale 常量。
- 在 `src/styles/theme.ts` 定义宿主拥有的卡片主题：绿色主色、14px、独立 ConfigProvider 与 Ant App 根。`inherit: false` 隔离桌面 Button/字体等覆盖；保留父级弹层容器上下文。暗色模式通过 themeStore 和 Ant Design darkAlgorithm 适配，外围桌面主题不改。
- 当前 Kit 未公开主题 preset，因此这些颜色 token 目前仍是 client 的显式产品配置，不宣称会随上游主题 token 自动变化。#166 正式交付共享 preset 后应替换该宿主配置。此处是用户要求先修正可见差异后的过渡实现，完整官方工作区迁移仍保留。
- 增强既有桥接回归，验证中文折叠/详情/输入框文案来自产品 copy，同时原生错误不泄露原始私密文本。
- 新增 `e2e/chat-appearance/` 浏览器对照夹具：实际 client 工作区与公开官方 ChatCompactWorkspace、同版本/同显式主题/相同本地 HTTP + Socket.IO 数据。两者使用独立客户端，测试不访问 Manager、Keychain 或远端数据。

运行方式（需已有 `pnpm dev` 在 localhost:1420，18768 空闲）：

```sh
node e2e/chat-appearance/run.mjs
```

该夹具验证明暗两种主题、14px、中文输入框、默认无诊断、真实事件打开/关闭 modal、无 pageerror 并生成四张截图。它使用相同显式宿主主题做组件对照，不是对上游私有默认主题的独立验证，也不是全卡片像素一致测试。

结果：生产构建、TypeScript、变更 TS/TSX 与夹具 ESLint、frozen offline install 和 diff check 通过。聊天回归 10 文件共 84 项，首轮 83 通过、1 项 ChatPlatformLifecycle 的 30 秒预算超时，单独复跑 1/1 通过（耗时约 9.6 秒）；没有声称一次全绿全量回归。原始日志 `/tmp/tfrc-119-appearance-{build,tests}.log`、`/tmp/tfrc-119-platform-retry.log`、`/tmp/tfrc-119-visual.log`。

截图已人工查看：`/var/folders/v7/6wb5d0ks3rx3v0wg682l2m6c0000gn/T/tfrc-chat-appearance-nWSfrh/{client,official}-{light,dark}.png`。client 消息/事件/输入框已经使用同主题和 locale；顶部仍为旧自组导航（高度、图标顺序、缺少更多菜单），底部仍缺官方 AI 内容说明，Enter 策略按已确认宿主要求保留。附件入口由能力决定，未隐藏生产上传功能。

仍未完成的验收：完整官方工作区接入、共享上游主题 preset、已保留页事件 Modal 生命周期定向测试、桌面实际截图、真实 Manager/Server 和签名安装包 UAT。当前视觉对齐改动不等价于完成整个 #119。

本轮独立隔离审查：APPROVE，0 阻塞项，仅覆盖上述外观调整。非阻塞建议：补事件弹窗打开状态的明暗截图及 retained-page 焦点/滚动锁验证；现有四张截图均在关闭详情弹窗后生成，不作为弹窗主题视觉证据。后续用上游公开 preset 替换宿主主题常量。新建夹具文件已按技能精确暂存供审查，既有修改和本计划未暂存，尚未 commit/push。
