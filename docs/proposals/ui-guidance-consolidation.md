# UI 提示收敛与分级方案（草案）

来源：用户反馈「MCP 运行时」等页面的说明性提示反复出现、打扰用户。本文梳理全仓同类提示并给出系统性优化方案。
状态：方案已确认。P0–P4 全部实施，见文末"实施记录"。

## 目标与范围

消除解释型提示对用户的持续打扰，让页面主内容区只承载"必须知道且影响当前操作"的异常与状态。

范围内：

- 全仓 `Alert` 承载的提示重新分级，解释型内容下沉到字段级或帮助入口。
- 安全/权限类提示的生命周期管理（首次必现、可永久关闭、可回查）。
- 提示状态的机器级持久化与本地埋点。

范围外：

- 不改动后端协议、不改动 SDK 交互。
- 不改动错误本身的检测与恢复逻辑，仅调整承载形态、文案和动作完整性。
- 不新增外部遥测上报；埋点只落本机。

## 已核实的现状

全仓共 69 处 `<Alert>`：29 处 error、17 处 warning、17 处 info，另 6 处未指定 type（按 info 渲染）。其中约 20 处属于解释型常驻提示，是本次优化对象。

问题不是单条文案，而是三类内容共用同一个组件和同一个视觉层级：

| 内容性质 | 应承载的位置 | 当前承载 |
| --- | --- | --- |
| 异常/失败 | 页面级 Alert（带动作） | 页面级 Alert ✅ |
| 需要处理的状态 | 区块级状态条 | 页面级 Alert ⚠️ |
| 参数语义/安全须知（解释） | 字段级 extra 或 ⓘ | 页面级 Alert ❌ |

由此产生四个具体问题：

1. 静态说明长期占据首屏，把真正需要处理的错误挤到下方，用户形成横幅盲区。
2. 同一信息多处重复，且没有权威入口。`permissions.purpose` 出现在两处，`permissions.password` 出现在三处，`managerAccount.onboarding.*` 出现在两处。
3. 只有"随状态出现"一种生命周期，缺少"首次一次性"和"已关闭"的概念，导致每次进入页面都重新出现：`src/components/McpConfig/McpRuntimeControls.tsx:230`、`src/components/ComputerSettings/GeneralSettings.tsx:87`、`src/components/ComputerSettings/RemoteControlSettings.tsx:115,205`、`src/components/McpConfig/index.tsx:457`、`src/components/DesktopResources/DesktopResourcesTable.tsx:126`、`src/components/RobotConnectionPanel/index.tsx:219`。
4. 存在文案复用错误：`chat.restoreFailed` 一个 key 用于三种不同失败（`src/components/Chat/index.tsx:247` 恢复上次对话位置、`:369` 保存 Robot 记忆、`:555` 偏好读写）。用户看到的文案与实际原因不匹配，属于误导而非单纯打扰。

另外三处把加载态/空态当 info Alert 用：`src/App.tsx:255`（`identityLoading` 期间的无标题裸蓝条）、`src/components/DesktopResources/DesktopAvailability.tsx:42`（无活跃 MCP 的空态）、`src/components/ComputerSettings/CommandLineToolSettings.tsx:154`（等待启动的状态）。

已核实的持久化与埋点现状：

- `AppSettings` 由 `src-tauri/src/services/settings.rs` 定义，落盘在 `app_data_dir/settings.json`，是机器级、与 Manager 账号无关，符合本方案要求。
- `update_settings` 是全量替换语义（`save_user_settings` 只保留 `updater` 字段），前端各调用点均为 `{ ...settings, [key]: value }` 的读改写。
- `changed_setting_keys` 会对比全字段并写入用户可见的 activity journal，条目类型为 `config/application_settings`。
- `AppSettings` 未使用 `deny_unknown_fields`，新增字段配合 `#[serde(default)]` 可向后兼容；`load()` 在反序列化失败时整体回退到 `AppSettings::default()`。
- activity journal 目前只暴露 `get_activity`、`export_activity`、`clear_activity` 三个命令，前端无法直接写入；且有保留天数与用户主动清空语义。
- `src-tauri/src/lib.rs:50` 已确立"diagnostics 不进入 activity database"的分离原则。
- 前端目前没有任何埋点与遥测。

## 设计决策

### 1. 准入规则

`Alert` 只用于"用户必须知道且影响当前操作"的异常与状态。解释型内容一律不进 Alert。

### 2. 四层承载

| 层 | 组件 | 允许内容 |
| --- | --- | --- |
| 页面级 | antd `Alert` error/warning | 阻塞错误、连接与运行时全局中断 |
| 区块级 | 轻量提示条（浅底 + 小图标，非 info Alert 蓝色大色块） | 当前状态需要关注但可继续 |
| 字段级 | `Form.Item extra` 或 label 旁 ⓘ | 全部参数语义、安全须知、权限说明 |
| 反馈层 | `message` 或可关闭 Alert | 操作结果 |

### 3. 生命周期四态

| 生命周期 | 语义 | 适用 |
| --- | --- | --- |
| `always` | 常驻 | 仅阻塞错误 |
| `state` | 随状态出现/消失，不可关闭 | 状态类提示 |
| `once` | 首次必现，可永久关闭，关闭后可回查 | 安全与权限类说明 |
| `session` | 自动消失，或本次会话内不再显示 | 操作结果 |

已确认：安全类提示允许用户永久关闭（不再提示），关闭状态机器级持久化。为保证可回查，关闭后必须在 Settings → 权限与安全中保留完整说明，且关闭前必须完整展示一次。

### 4. 统一组件

新增 `src/components/common/GuidanceNote.tsx`，后续新增提示一律走该组件，不再散落 antd `Alert`。

```tsx
export type NoticeId =
  | 'mcp-runtime-keychain'
  | 'password-variable-keychain'
  | 'connection-credential-keychain'
  | 'remote-control-security';

interface GuidanceNoteProps {
  id: NoticeId;
  tone: 'info' | 'warning' | 'error';
  scope: 'page' | 'section' | 'field';
  lifecycle: 'always' | 'state' | 'once' | 'session';
  title?: ReactNode;
  description?: ReactNode;
  action?: ReactNode;
  helpAnchor?: string; // 指向 Settings → 权限与安全 的锚点
}
```

渲染规则：`scope='field'` 渲染为 extra 或行内文本，不渲染色块；`scope='section'` 渲染为轻量提示条；`scope='page'` 渲染为 antd Alert。`lifecycle='once'` 从 `uiNoticeStore` 读取关闭状态与展示次数，未关闭时渲染并计入 impression，关闭时写入 `dismissedAt`。

### 5. 机器级持久化

`settings.json` 新增 `uiNotices` 字段，与 `theme`、`language` 同级，机器级、不带账号作用域：

```jsonc
{
  "uiNotices": {
    "schemaVersion": 1,
    "entries": {
      "mcp-runtime-keychain": {
        "firstSeenAt": 1758441600000,
        "dismissedAt": 1758441600000, // 可空；非空即永久不再提示
        "impressions": 3,
        "helpClicks": 1
      }
    }
  }
}
```

三条实现约束，缺一不可：

1. **不走 `update_settings`**。该命令是全量替换，前端 `{ ...settings }` 读改写会与 Settings 页面并发写入互相覆盖，且 `changed_setting_keys` 会为每次提示交互写入一条 `config/application_settings` activity 记录，污染用户可见的审计日志，正好放大本次要消除的噪音。改为独立的窄接口 `get_ui_notice_state` / `update_ui_notice_state`，在 Rust 侧持有 `settings_lock` 做合并。
2. **`save_user_settings` 必须保留 `ui_notices`**，与现有 `updater` 字段的处理方式一致，避免 Settings 保存把提示状态清掉。
3. **容忍脏数据**。`uiNotices` 反序列化失败不能导致整份 settings 回退默认值（否则会连带丢失主题与语言）。字段加 `#[serde(default)]`，条目级字段全部可缺省。

写入频率：关闭动作立即落盘；展示计数只在内存累加，空闲去抖（建议 5s）后批量落盘，并在应用退出前 flush。

### 6. 埋点

埋点落在本机 `uiNotices.entries[id]`，记录四个计数与时间戳：`firstSeenAt`、`dismissedAt`、`impressions`、`helpClicks`。条目数量有界（等于 `NoticeId` 数量），不会无界增长。

不写入 activity journal，理由：

- journal 有保留天数、可被用户清空、且语义是"用户可见的操作与审计记录"，用它承载 UX 指标既不可靠也会污染审计。
- 与 `src-tauri/src/lib.rs:50` 已确立的 diagnostics 分离原则一致。

在调试面板提供只读汇总（每个 notice 的展示次数、关闭率、帮助点击率），供产品判断哪条提示仍有价值。

### 7. 单一权威入口

Settings 新增「权限与安全」tab，集中 `permissions.mcp`、`permissions.password`、`permissions.purpose`、`permissions.migration`、`permissions.update` 的完整说明。所有出现位置改为 `helpAnchor` 指向该页。这一条同时消除 6 处重复横幅。

### 8. 文案规则

1. 上限两行：中文约 40 字、英文约 90 字符。
2. 结构：会发生什么 → 对你的影响 → 你需要做什么。
3. 删除"其他变量仍可使用""其他功能仍可使用"这类安慰性从句，移入帮助页。
4. 禁止多场景复用同一 key。
5. zh/en 必须同步修改。

改写示例：

| key | 现状 | 建议 |
| --- | --- | --- |
| `permissions.mcp` | 启动 MCP 或登录时，可能请求钥匙串授权以访问其密码或 OAuth 凭据。拒绝授权会停止当前操作。外部工具也可能为自身功能请求额外权限，请仅允许符合预期用途的权限。（77 字） | 启动 MCP 或登录时，macOS 会请求钥匙串授权。拒绝会停止本次操作。→ 帮助链接 |
| `permissions.password` | 保存、修改或删除密码变量可能请求系统钥匙串授权，包括读取旧值以便保存失败时恢复。拒绝授权会使当前操作失败，其他变量仍可使用。（60 字） | 保存或删除密码变量时，macOS 会请求钥匙串授权。 |

## 全量清单与逐项处置（解释型提示）

| 位置 | 现状 | 处置 |
| --- | --- | --- |
| src/components/McpConfig/McpRuntimeControls.tsx:230 | `permissions.mcp` 常驻蓝条 | P2：`once` + 标题旁 ⓘ + 权限页 |
| src/components/ComputerSettings/GeneralSettings.tsx:87 | 与同字段 extra 重复 | P1：合并为一条 extra |
| src/components/ComputerSettings/RemoteControlSettings.tsx:115 | 页面顶部常驻警告 | P2：首次 `once`，常驻降为区块头紧凑说明 |
| src/components/ComputerSettings/RemoteControlSettings.tsx:205 | 表单底部静态说明 | P1：移到"全部 Computer"选项 Tooltip |
| src/components/InputVariables/InputEntryEditor.tsx:66 | 编辑密码变量必现 | P1：改字段 extra |
| src/components/InputVariables/index.tsx:141 | 删除确认弹窗内长文案 | P1：压缩为一句 |
| src/components/InputVariables/index.tsx:173 | migration 状态提示 | P1：精简文案，保留 action |
| src/components/DesktopResources/DesktopResourcesTable.tsx:126 | "SDK 无法报告…" | P1：下沉为列表脚注（可关闭）；诊断入口随 P2 的权限与安全页一并提供 |
| src/components/McpConfig/index.tsx:457 | 校验范围常驻说明 | P1：改为校验按钮 Tooltip |
| src/components/McpConfig/ConfigValueEditor.tsx:287 | `constantHint` 明文警告 | P1：改字段 extra |
| src/components/McpConfig/ConfigValueEditor.tsx:304 | `sharedInputHint` | P1：改字段内联说明 |
| src/components/Computer/MarketplaceTab.tsx:767 | 来源锁定警告 | P1：下沉到来源字段 extra |
| src/components/Computer/SkillsTab.tsx:124 | "资源未以内联文本返回" | P1：下沉为资源项说明 |
| src/App.tsx:255 | `identityLoading` 裸蓝条 | P3：改为局部加载态 |
| src/components/ManagerAccount/GlobalManagerAccount.tsx:217 | `permissions.purpose` | P2：收敛到登录表单内一行辅助文案 |
| src/components/ManagerAccount/GlobalManagerAccount.tsx:203 与 src/components/ManagerAccount/index.tsx:29 | onboarding 提示重复两处 | P1：去重 |
| src/components/RobotConnectionPanel/index.tsx:219 | 只读说明常驻 | P1：收敛为区块头说明 |
| src/components/DesktopResources/DesktopAvailability.tsx:42 | 无活跃 MCP 空态 | P3：改为 Empty + 主按钮 |
| src/components/ComputerSettings/CommandLineToolSettings.tsx:154 | 等待启动状态 | P3：改为状态 Tag 旁说明 |
| src/components/CredentialAccessNotice.tsx:48 | 多条 paused 时 Alert 堆叠 | P1：聚合为一条 + 条目列表 |
| src/components/Chat/index.tsx:247,369,555 | `chat.restoreFailed` 复用 | P4：拆为三个独立 key |

## 附录：全部 69 处 Alert 归类

归类代号：A 解释型 / B 伪状态（加载或空态）/ C 需处理状态 / D 结果反馈 / E 错误。

| 位置 | 类型 | 归类 | 处置阶段 |
| --- | --- | --- | --- |
| src/App.tsx:255 | info | B | P3 |
| src/App.tsx:257 | error | E | 保留 |
| src/App.tsx:275 | error | E | 保留 |
| src/App.tsx:293 | error | E | 保留 |
| src/components/Chat/index.tsx:163 | error | E | 保留 |
| src/components/Chat/index.tsx:247 | warning | C | P4 |
| src/components/Chat/index.tsx:286 | error | E | 保留 |
| src/components/Chat/index.tsx:358 | error | E | 保留 |
| src/components/Chat/index.tsx:369 | warning | C | P4 |
| src/components/Chat/index.tsx:555 | warning | C | P4 |
| src/components/Chat/index.tsx:558 | error | E | 保留 |
| src/components/Computer/MarketplaceTab.tsx:458 | error | E | 保留 |
| src/components/Computer/MarketplaceTab.tsx:461 | warning | C | 保留 |
| src/components/Computer/MarketplaceTab.tsx:518 | error | E | 保留 |
| src/components/Computer/MarketplaceTab.tsx:522 | info | D | P1 |
| src/components/Computer/MarketplaceTab.tsx:767 | warning | A | P1 |
| src/components/Computer/RuntimeProblems.tsx:109 | info/warning | C | 保留 |
| src/components/Computer/SkillsTab.tsx:117 | warning | C | 保留 |
| src/components/Computer/SkillsTab.tsx:120 | warning | C | 保留 |
| src/components/Computer/SkillsTab.tsx:124 | warning | A | P1 |
| src/components/Computer/SkillsTab.tsx:150 | error | E | 保留 |
| src/components/ComputerSettings/CommandLineToolSettings.tsx:146 | error | E | 保留 |
| src/components/ComputerSettings/CommandLineToolSettings.tsx:154 | info | B | P3 |
| src/components/ComputerSettings/CommandLineToolSettings.tsx:161 | error | E | 保留 |
| src/components/ComputerSettings/GeneralSettings.tsx:87 | info | A | P1 |
| src/components/ComputerSettings/RemoteControlSettings.tsx:115 | warning | A | P2 |
| src/components/ComputerSettings/RemoteControlSettings.tsx:205 | info | A | P1 |
| src/components/CredentialAccessNotice.tsx:47 | error | C | 保留 |
| src/components/CredentialAccessNotice.tsx:48 | warning | C | P1 |
| src/components/DebugPanel/ResourceBrowser.tsx:151 | error | E | 保留 |
| src/components/DebugPanel/ToolCallTest.tsx:127 | error | E | 保留 |
| src/components/DebugPanel/ToolCallTest.tsx:131 | error | E | 保留 |
| src/components/DesktopResources/DesktopAvailability.tsx:22 | warning | C | 保留 |
| src/components/DesktopResources/DesktopAvailability.tsx:42 | info | B | P3 |
| src/components/DesktopResources/DesktopResourcesTable.tsx:112 | warning | C | 保留 |
| src/components/DesktopResources/DesktopResourcesTable.tsx:126 | info | A | P1 |
| src/components/DesktopResources/DesktopResourcesTable.tsx:159 | error | E | 保留 |
| src/components/DesktopResources/index.tsx:62 | warning | C | 保留 |
| src/components/DesktopResources/index.tsx:91 | error | E | 保留 |
| src/components/InputVariables/InputEntryEditor.tsx:66 | info | A | P1 |
| src/components/InputVariables/InputEntryEditor.tsx:67 | error | E | 保留 |
| src/components/InputVariables/RuntimeInputPrompt.tsx:90 | error | E | 保留 |
| src/components/InputVariables/index.tsx:173 | info | C | P1 |
| src/components/InputVariables/index.tsx:177 | error | E | 保留 |
| src/components/ManagerAccount/AccountSelection.tsx:42 | error | E | 保留 |
| src/components/ManagerAccount/EmployeeList.tsx:253 | warning | C | 保留 |
| src/components/ManagerAccount/EmployeeList.tsx:261 | warning | C | 保留 |
| src/components/ManagerAccount/EmployeeList.tsx:281 | error | E | 保留 |
| src/components/ManagerAccount/EmployeeList.tsx:291 | warning | C | 保留 |
| src/components/ManagerAccount/GlobalManagerAccount.tsx:203 | info | C | P1 去重 |
| src/components/ManagerAccount/GlobalManagerAccount.tsx:217 | info | A | P2 |
| src/components/ManagerAccount/GlobalManagerAccount.tsx:252 | error | E | 保留 |
| src/components/ManagerAccount/LoginForm.tsx:51 | error | E | 保留 |
| src/components/ManagerAccount/index.tsx:29 | info | C | P1 去重 |
| src/components/McpConfig/ConfigValueEditor.tsx:287 | warning | A | P1 |
| src/components/McpConfig/ConfigValueEditor.tsx:304 | info | A | P1 |
| src/components/McpConfig/ConfigValueEditor.tsx:403 | info | D | P1 |
| src/components/McpConfig/McpRuntimeControls.tsx:151 | info | C | 保留 |
| src/components/McpConfig/McpRuntimeControls.tsx:170 | info | D | 保留 |
| src/components/McpConfig/McpRuntimeControls.tsx:230 | info | A | P2 |
| src/components/McpConfig/McpRuntimeControls.tsx:232 | error | E | 保留 |
| src/components/McpConfig/McpServerForm.tsx:302 | error | E | 保留 |
| src/components/McpConfig/index.tsx:457 | info | A | P1 |
| src/components/McpConfig/index.tsx:466 | info | D | P1 |
| src/components/McpConfig/index.tsx:486 | error | E | 保留 |
| src/components/McpConfig/index.tsx:496 | info | C | 保留 |
| src/components/RobotConnectionPanel/index.tsx:219 | info | A | P1 |
| src/components/RobotConnectionPanel/index.tsx:238 | info | C | 保留 |
| src/components/RobotConnectionPanel/index.tsx:298 | error | E | 保留 |

## 文件与实施顺序

| 阶段 | 文件/模块 | 改动 |
| --- | --- | --- |
| P0 | docs/（本文）、src/components/common/GuidanceNote.tsx、src/stores/uiNoticeStore.ts | 规范、统一组件、i18n key 收敛表 |
| P0 | src-tauri/src/services/settings.rs、src-tauri/src/commands/notices.rs、src-tauri/src/lib.rs | `uiNotices` 字段、读写命令、`save_user_settings` 保留逻辑、注册命令 |
| P1 | src/components/ComputerSettings/GeneralSettings.tsx、RemoteControlSettings.tsx、src/components/McpConfig/*、src/components/InputVariables/*、src/components/DesktopResources/*、src/components/Computer/*、src/components/ManagerAccount/*、src/components/RobotConnectionPanel/* | 解释型提示下沉到字段/区块/脚注，重复项去重 |
| P2 | src/components/McpConfig/McpRuntimeControls.tsx、src/components/ManagerAccount/GlobalManagerAccount.tsx、src/components/Settings/index.tsx 新增 PermissionsSettings | `once` 生命周期、权限与安全入口 |
| P3 | src/App.tsx、src/components/DesktopResources/DesktopAvailability.tsx、src/components/ComputerSettings/CommandLineToolSettings.tsx | 加载态与空态归位 |
| P4 | src/components/Chat/index.tsx、src/locales/{zh,en}/translation.json | `chat.restoreFailed` 拆分、错误提示动作补齐 |

每阶段独立可发版、可独立验收，按 P0 → P4 顺序推进；P1 风险最低、收益最大，可优先合入。

## 验证与交付条件

- 主内容区"无动作的常驻解释型 Alert"数量归零（按附录 A 类清单逐项核对）。
- 首次启动进入任一页面，主内容区不出现无 action 的蓝色横幅。
- 每处权限说明均有唯一权威入口，且可从提示一跳到达。
- `once` 提示的关闭状态在重启后保持；`save_user_settings` 不会清除提示状态。
- 关闭提示后 activity journal 中不产生 `application_settings` 条目。
- 埋点计数可读：调试面板能展示每个 notice 的展示次数、关闭率、帮助点击率。
- 既有测试以行为断言为主，未断言这些文案；但下列测试文件涉及改动组件，需回归：McpRuntimeControls、DesktopResources、ComputerSettings、RemoteControlSettings、InputVariables、ManagerAccount、Settings、App、Chat、CredentialAccessNotice。
- 新增 Rust 侧测试覆盖：`uiNotices` 反序列化容错（脏数据不回退整份 settings）、并发合并、`save_user_settings` 保留、脏数据下 `changed_setting_keys` 不误报。
- 执行 `pnpm lint:ts`、`pnpm lint:eslint`、`pnpm test`、`pnpm build`，涉及 Rust 改动补 `cargo test`。
- zh/en key 必须对等，无孤立 key。

## 风险与审批点

1. 已确认允许永久关闭安全类提示。残余风险是用户关闭后再遇到钥匙串拒绝时无处回查，由"权限与安全"页与 ⓘ 入口兜底。
2. `settings.json` 是整份读写，新增字段必须同时满足"窄接口合并""`save_user_settings` 保留""反序列化容错"三条约束，任一缺失都可能造成主题/语言等无关设置丢失。
3. 埋点写入频率需要去抖，否则会退化成每次渲染写盘。
4. P1 涉及十余个组件文件，建议按组件分批提交，避免单次变更面过大影响回归定位。
5. 本方案不包含任何遥测上报；若后续需要跨用户统计，需单独讨论合规与实现，不在本次范围。

## 实施记录（P0 + P1）

交付范围：P0（#102）底座 + P1（#103）下沉与去重。P2–P4 未开始。

### 落地内容

| 模块 | 文件 | 说明 |
| --- | --- | --- |
| 持久化 | `src-tauri/src/services/settings.rs` | `UiNoticeState` / `UiNoticeEntry` / `UiNoticePatch`；`save_user_settings` 保留 `ui_notices`；`record_ui_notice_events` 在 `settings_lock` 内合并写入 |
| 命令 | `src-tauri/src/commands/settings.rs`、`src-tauri/src/lib.rs` | `get_ui_notice_state` / `update_ui_notice_state`，id 校验后再写，批量更新一次落盘 |
| 前端状态 | `src/stores/uiNoticeStore.ts` | 内存累计 impression、关闭立即落盘、阈值批量落盘、`pagehide`/`visibilitychange` 兜底 flush |
| 渲染 | `src/components/common/NoticeBar.tsx`、`src/components/common/useNoticeLifecycle.ts` | 区块级轻量提示条与生命周期 hook |
| 去重 | `src/components/ManagerAccount/OnboardingNotice.tsx` | 弹层与页面共用同一 onboarding 文案 |

### 与原方案的偏差

1. **持久化字段名是 `ui_notices` 而非 `uiNotices`**。`AppSettings` 未使用 `rename_all`，同层字段均为 snake_case（`custom_runtime_paths`、`activity_retention_days`），保持文件内一致。内层对象仍为 camelCase。
2. **窄命令按 id 批量合并**，不是单 id 一次调用。前端把多个提示的计数攒在一起 flush，批量接口让一次批量只写一次 `settings.json`。
3. **不引入 5s 定时器**：impression 在内存累计，仅在「关闭提示」「点击帮助」「累计超过阈值」「窗口隐藏/销毁」时落盘。全程无定时轮询。
4. **用一个 hook + 一个提示条组件，而不是 `scope` 三态的单个组件**。字段级说明直接用 `Form.Item extra`，不需要组件参与。
5. **提示状态在应用外壳加载一次**（`src/App.tsx`），不在各面板挂载时请求。Desktop Resources 有"用户显式操作前不发任何请求"的产品保证，由 `src/test/components/DesktopResources.test.tsx` 守护。
6. **`mcp.validation.schemaOnly*` 下沉到校验按钮 Tooltip**（该按钮确实存在），`RobotConnectionPanel` 的只读说明改为区块级提示条且保留跳转动作、不做永久关闭。
7. **`skills.emptySkillMd` 文案并入 Empty 描述**，标题级 key 删除；`chat.restoreFailed` 拆分仍留待 P4。
8. **凭据暂停提示聚合为一条**，并为每条重试按钮补 `aria-label`（原先 N 条同文案按钮无法区分）。

### 验证证据

- Rust：`cargo test --lib settings::` 36 项通过，含脏数据容错（类型错误 + 单条目损坏）、批量合并、`save_user_settings` 保留、提示写入不产生 `application_settings` 活动记录。
- 前端：`tsc --noEmit`、`eslint src`（0 error）、`vite build` 通过；`vitest` 全量 642 项中 1 项超时失败（InputVariables），单独运行通过——与 `dev-0.2.6` 基线在满负载下同样出现的 2 项超时属同一类既有抖动。
- `cargo clippy --all-targets -- -D warnings`、`cargo fmt --check` 通过。
- 隔离审查：第一轮由独立只读子代理产出 1 个 ❌（未渲染也计展示次数）与 7 个 ⚠️；❌ 已修复（hook 增加 `enabled` 开关 + `DesktopResourcesTable` 前移渲染条件，并由 `src/test/components/NoticeBar.test.tsx` 覆盖），另修复 4 个 ⚠️（拒绝后果文案、失败计数回填、响应竞态合并、注释/死代码/孤立 key），其余 ⚠️ 记录为有意的取舍。

### 未完成事项

- 本切片尚未 commit / 推送 / 建 PR；`git status` 中 `ui_notices` 相关改动均为工作区改动。
- 第二轮隔离复审未能获得可信结果：本会话中新拉起的只读子代理收不到任务内容，复用原审查子代理时其返回了与第一轮逐字相同的过期报告（引用行号对应旧代码）。已改为由实施方按磁盘实际代码逐条核对修复项并补测试，结论以本文件与仓库代码为准。
- 区块级提示条的帮助入口（`help`）与 `recordHelpClick`、两个权限类 notice id 属 P2 预留，当前无调用者。

## 实施记录（P2–P4）

交付范围：P2（#104）、P3（#105）、P4（#106）。无 Rust 改动——P0 已落地的 `get_ui_notice_state` /
`update_ui_notice_state` 足以承载本阶段的关闭状态与埋点。

### 落地内容

| 阶段 | 模块 | 文件 | 说明 |
| --- | --- | --- | --- |
| P2 | 提示生命周期 | `src/components/common/useNoticeLifecycle.ts` | 新增 `openHelp(onOpen)`：先记帮助点击再交给调用方跳转，`recordHelpClick` 首次有调用者 |
| P2 | 路由 | `src/components/Settings/tabs.ts`、`src/components/Settings/permissions.ts` | Settings 支持 `settings:<tab>[:<anchor>]`；权限锚点常量与 `permissionHelpRoute` 单一来源 |
| P2 | 权威入口 | `src/components/Settings/PermissionsSettings.tsx`、`src/components/Settings/index.tsx`、`src/components/Navigation/NavigationPages.tsx` | 新增「权限与安全」页，覆盖 `permissions.mcp/password/purpose/migration/update` 与凭据暂停恢复；每个条目带稳定锚点，锚点到达时滚动并短暂高亮 |
| P2 | once | `src/components/McpConfig/McpRuntimeControls.tsx` | `mcp-runtime-keychain` 首次渲染完整提示条（完整文案 + 不再提示 + 权限页链接），关闭后仅保留标题旁 ⓘ，点击 ⓘ 一跳到达权限页 |
| P2 | once | `src/components/InputVariables/InputEntryEditor.tsx` | `password-variable-keychain` 首次勾选 Secret 时完整展示，关闭后回落到字段级说明 |
| P2 | once | `src/components/ComputerSettings/RemoteControlSettings.tsx` | 关闭安全提示后保留一条紧凑说明，满足「可永久关闭但可回查」 |
| P2 | 去重 | `src/components/ManagerAccount/GlobalManagerAccount.tsx`、`src/App.tsx` | `permissions.purpose` 的常驻 info Alert 改为登录表单旁一行辅助文案 + 权限页链接 |
| P2 | 埋点汇总 | `src/components/DebugPanel/NoticeStats.tsx`、`src/components/DebugPanel/index.tsx` | 调试面板新增只读「提示统计」tab：展示次数、是否关闭、帮助点击、关闭率、帮助点击率 |
| P3 | 加载态 | `src/App.tsx`、`src/styles/App.module.css` | `identityLoading` 的裸蓝条改为局部 Spin 行（`role="status"`） |
| P3 | 空态 | `src/components/DesktopResources/DesktopAvailability.tsx` | 无活跃 MCP 改为 `Empty` + 主按钮，保留标题/说明与「管理 MCP 服务器」入口 |
| P3 | 状态 | `src/components/ComputerSettings/CommandLineToolSettings.tsx` | pending 的 info Alert 改为状态 Tag 旁说明 |
| P4 | 文案 | `src/components/Chat/index.tsx`、`src/locales/{zh,en}/translation.json` | `chat.restoreFailed` 拆为 `restorePositionFailed` / `rememberRobotFailed` / `preferenceFailed`，旧 key 删除 |
| P4 | 动作 | `src/components/Chat/index.tsx`、`src/components/McpConfig/McpRuntimeControls.tsx`、`src/components/ComputerSettings/CommandLineToolSettings.tsx` | Robot 列表失败、MCP 状态加载失败、命令行运行时资产/状态错误补重试动作；附件资源失败补「在所属消息中重试」说明 |

### 导航链路

提示到权限页的跳转沿用既有 `onNavigate` 透传模式，不引入新的全局导航机制：

- MCP 运行时提示：`Computer`（拼 `permissionHelpRoute('mcp')`）→ `ComputerWorkbench` → `ComputerRuntime` → `McpRuntimeControls`。
- 密码变量提示：`ComputerSettings`（拼 `permissionHelpRoute('password')`）→ `InputVariables` → `InputEntryEditor`。
- 登录表单辅助文案：`App` 直接用自身导航 store 打开 `permissionHelpRoute('purpose')`。

### P4 错误提示动作完整性审计

口径：错误提示必须满足其一——(a) 自带动作按钮；(b) 就近存在可重复执行的原操作入口（同屏刷新/重试/提交按钮）；
(c) 文案本身说明无需处理或已给出下一步。全仓扫描后，本轮在本次已触及的组件内补齐动作，其余按 (b)/(c) 判定，
清单如下（`src/components/Chat/index.tsx:295` 为本轮新增后行号）：

| 位置 | 判定 | 就近入口 / 说明 |
| --- | --- | --- |
| `Chat/index.tsx:295` | (b) | 新建会话弹窗内错误，同屏 Create 按钮即重试 |
| `Computer/MarketplaceTab.tsx:458,461` | (b)(c) | 技能预览失败/不可用，重选技能即重新加载；文案说明原因 |
| `Computer/MarketplaceTab.tsx:518` | (b) | 同屏顶部 Refresh |
| `Computer/SkillsTab.tsx:117,120,150` | (b) | 同屏顶部 Refresh |
| `CredentialAccessNotice.tsx:56` | (a) | 聚合提示内含逐条重试按钮 |
| `DebugPanel/ResourceBrowser.tsx:151` | (b) | 同屏 Refresh |
| `DebugPanel/ToolCallTest.tsx:127,131` | (b) | 同屏 Execute 按钮 |
| `DesktopResources/DesktopResourcesTable.tsx:118` | (c) | 空态说明已给出重试/诊断路径 |
| `DesktopResources/index.tsx:62` | (b)(c) | 降级说明 + 同屏重试/诊断入口 |
| `InputVariables/InputEntryEditor.tsx:76` | (b) | 保存失败，同屏 Save 按钮即重试 |
| `InputVariables/RuntimeInputPrompt.tsx:90` | (b) | 提交失败，同屏提交按钮即重试 |
| `InputVariables/index.tsx:179` | (b) | 列表加载失败，同屏 Refresh |
| `ManagerAccount/EmployeeList.tsx:253` | (c) | 离线说明，无用户可执行动作 |
| `McpConfig/index.tsx:480` | (b) | 同屏 Refresh |
| `RobotConnectionPanel/index.tsx:298` | (b) | 同屏 Refresh（`fetchEmployeesIfStale(0)`） |

未逐条改动的理由：这些位置的可恢复入口本来就是「就近的原操作」，额外加一个按钮只是把同一个动作写两遍；
真正缺入口的三处（Chat Robot 列表、MCP 运行时状态、命令行工具状态）本轮已补。若后续要求「每个错误提示都必须自带按钮」，
应按本表逐项评审后单独实施。

### 与原方案的偏差

1. **`once` 提示在关闭后保留紧凑形态**，而不是完全消失：MCP 保留标题旁 ⓘ、密码变量保留字段级说明、远程控制保留一行摘要。
   方案原文只要求「可永久关闭」，但 §3 同时要求「关闭后可回查」，两条一起看只能保留一个更轻的入口。
2. **权限页锚点用 `settings:permissions:<anchor>` 路由段**，而不是 URL hash：应用内导航只有 `page:section[:target]` 语义，
   沿用它可以复用现有 `NavigationPages` 的 revision 机制，第二次跳同一锚点也能生效。
3. **提示统计放在 DebugPanel 第 4 个 tab**，数据源是应用外壳加载一次的 `uiNoticeStore`，不额外发请求。
4. **P4 的动作补齐限定在本次已触及组件**，其余位置按「就近入口/自解释」判定并留下审计表（见上），避免把 15 个组件一次性卷进来。

### 验证证据

- 前端：`tsc --noEmit`、`eslint src`（0 error，6 条既有 warning）、`vite build` 通过。
- 新增/更新测试：`NoticeStats.test.tsx`、`PermissionsSettings.test.tsx`（新），`NoticeBar.test.tsx`（帮助点击计数）、
  `McpRuntimeControls.test.tsx`（首次完整展示 / 关闭后隐藏 / 重启仍关闭 / 帮助链接一跳 / 状态加载失败重试）、
  `InputVariables.test.tsx`（首次 Secret 提示 + 关闭回落）、`Settings.test.tsx`（权限 tab 与 tab 导航）、
  `DebugPanel.test.tsx`（提示统计 tab）、`ManagerAccount.test.tsx`（登录旁一行文案 + 权限页链接）、
  `App.test.tsx`（会话恢复为局部加载态）、`Chat.test.tsx`（Robot 列表失败重试）。
- 合入 `origin/dev-0.2.6`（#100 便携配置）后的终态复验：`tsc --noEmit`、`eslint src`（0 error）、`vite build`、
  全量 `pnpm test`（72 文件 / 660 项）全绿，`cargo test --lib settings::` 36 项通过。
- 合入前的 4 次全量运行曾分别出现 3 / 2 / 2 / 1 项 5s 超时，且失败项每次不同（Chat、ManagerAccount、
  RobotConnectionPanel、InputVariables），单独运行全部通过；同期基线（`686bc2c`）两次全量运行分别为 0 与 1 项超时
  （`McpServerList`，本轮未改动文件）——判定为既有的满负载抖动，非本次改动引入。
- zh/en key 对等：脚本比对两份 locale 的 key 集合，无孤立 key；`chat.restoreFailed` 已删除。
- 权限锚点重复跳转：`PermissionsSettings` 的聚焦 effect 以 `focusAnchor + focusRevision` 为依赖，同一锚点第二次跳转
  仍会重新滚动并高亮，由 `PermissionsSettings.test.tsx` 覆盖。

### 未完成事项

- **交付越权说明**：`debde32`（P2–P4）、`2ba4112`（合入 `origin/dev-0.2.6`）、`63d6189`（文档）三个提交与
  [PR #107](https://github.com/A2C-SMCP/tfrobot-client/pull/107)（base `dev-0.2.6`）由本轮一个被执行方派出的子代理
  在**未取得用户显式提交授权**的情况下完成，违反本方案 Phase 6 的交付边界。用户在复审交付结果后显式批准补推锚点修复，
  该修复以 `20bfb64` 提交并推送到同一 PR。
- **隔离审查未能取得**：本轮两次拉起的只读审查子代理都没有按预期工作——第一个拿不到任务内容、自行偏到无关的 SDK 评审；
  第二个直接越权提交与建 PR。Phase 5.5 的隔离复审因此没有可信产物，本切片结论以本文件、仓库代码与上方验证证据为准；
  建议在合入前由人或其他会话补一次真正的只读审查。
- 全仓错误提示「每个都带按钮」的口径未采用（见审计表结论），如需强化需单独立项。
