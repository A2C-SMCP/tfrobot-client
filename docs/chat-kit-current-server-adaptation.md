# Chat Kit 适配当前 TFRobotServer 修改方案

- 结论日期：2026-08-11
- 目标宿主：`tfrobot-client`
- 目标环境：当前 Staging Robot 109
- 已发布 Chat Kit：`0.6.0`，tag commit `2cbba9c4`
- 当前 Server 源码参考：`develop@2a97c8f4`
- Staging Server 部署版本：未暴露，黑盒行为与当前源码存在差异

## 1. 结论

不能直接把 tfrobot-client 切到已发布的 Chat Kit 0.6.0 并启用真实聊天。
空 ACK 是确定的首个阻塞项，但不是唯一接入问题。完整实验还发现：

1. 初次 `join_conversation` 返回空 ACK，0.6.0 立即以 `validation`
   失败，订阅无法建立。
2. Server 对不存在的 conversation 也返回相同空 ACK，空 ACK 本身不能证明
   会话存在、调用方获准访问或确实订阅成功。
3. Staging 的发送响应没有被 0.6.0 识别为真实 `taskId`。三次发送均退化为
   同一个 synthetic runId，导致中断在 chat-kit 本地以 `conflict` 拒绝。
4. 重连的空 ACK 不能证明离线消息已回放。0.6.0 只补一次 REST status，生命周期
   永久停在 `recovering`，Runtime/UI 随之永久禁止发送与中断。
5. 当前 Server 没有 cursor/replay/outbox，也不持久化 `chat_error`。chat-kit 可以
   通过 REST 历史做尽力补偿，但不能宣称无损恢复。

因此应修改 chat-kit，而不是仅在 tfrobot-client 外包一层“空 ACK 等于成功”的
无条件 shim。推荐提供显式的 current-server 兼容 profile，严格模式继续保持默认，
并把降级恢复事实暴露给 Runtime、UI 和诊断系统。

这组修改涉及 Gateway、Protocol、Runtime、UI 和 Testing 的公共契约。建议作为
`0.7.0` 发布；若团队坚持发布 `0.6.1`，也必须原子升级整个 package family，不能只
替换 `@turingfocus/chat-gateway-tfrobot`。

### 1.1 已验证能成功的方案

本次不是只根据失败现象推演修改方向。我在 Chat Kit 0.6.0 tag commit
`2cbba9c4` 的隔离副本中实现了最小候选修改，并用一个确定性的 current Server
fixture 跑通了完整正向链路：

| 顺序 | current Server 行为 | Chat Kit 候选实现 | 实测结果 |
| --- | --- | --- | --- |
| 1 | Socket join 不校验会话，最终只回空 ACK | 先用相同 session 对目标会话做 REST snapshot preflight | 会话可读时继续；preflight 失败时失败关闭 |
| 2 | `join_conversation` callback 为 0 个参数 | 仅在显式 `current-server` profile 且 preflight 成功时接受 | 订阅成功，生命周期进入 `degraded/best-effort` |
| 3 | send 返回 `{task_id:"run-from-current-server"}` | DTO 接受 camel/snake alias，使用时统一取值 | 返回真实 runId，不再生成 synthetic runId |
| 4 | interrupt 请求仍要求 `{taskId}`，响应可为 `{task_id}` | 请求保持标准 camelCase，响应 alias 归一化 | 中断成功，cancellationId 正确 |
| 5 | reconnect 仍回空 ACK，且没有 replay cursor | 先进入 `recovering`，再用 REST snapshot rebase | 离线新增的 assistant 消息被恢复 |
| 6 | REST rebase 无法证明瞬时事件无丢失 | rebase 完成后进入 `degraded`，不标记 complete | 恢复可发送，同时保留风险提示 |

候选实现的正向断言明确验证了：

- `subscribe.ok === true`；
- lifecycle 为 `degraded`、`complete=false`、`assurance=best-effort`；
- send 得到 `run-from-current-server`；
- interrupt 实际请求体为 `{taskId:"run-from-current-server"}`，并成功返回；
- 断线期间写入 fixture 历史的 `message-missed-while-offline` 在重连后的
  `snapshot.replace` 中出现；
- 最终 lifecycle reason 为 `legacy-rest-rebase-complete`，命令重新可用。

验证结果：

- 候选成功链路 + Gateway 严格模式 + Runtime + Protocol：4 个文件，251/251 通过；
- Chat Kit 全仓单测：39 个文件，900/900 通过；
- 完整 TypeScript typecheck 通过；
- 候选变更 ESLint 通过，Prettier check 通过。

因此，结论不是“当前 Server 无法接入”，而是：**已发布的 0.6.0 不能直接接入；按本文
profile + preflight + task alias + REST rebase + degraded lifecycle 的组合修改，接入链路
可以成功。**

需要区分原型证明与生产完成度：本次候选为最小可判别实现，重连只抓取最新一页历史，
已经证明成功机制成立；生产合入仍必须实现第 6 节的 bounded pagination、checkpoint、
revision guard 和幂等合并，不能把单页 rebase 宣称为完整恢复。

## 2. 实测证据

### 2.1 Staging 黑盒结果

| 能力 | 实测结果 | 当前结论 |
| --- | --- | --- |
| 会话列表 | 连续两页均成功，cursor 可用 | 兼容 |
| 创建会话 | 创建成功；返回标题与请求标题不同 | 可用，但需保留兼容诊断 |
| 加载空会话 | timeline 为空、run 为 null | 兼容 |
| 加载不存在会话 | 返回非重试 `validation` | 可工作，错误分类不准确 |
| 有效 Socket 鉴权 | 成功连接 | 兼容 |
| 无效 Socket 鉴权 | 78 ms 内拒绝 | Server 拒绝有效；错误缺少稳定分类字段 |
| 有效会话 join | 13 ms 收到 0 参数 ACK | 不兼容严格 0.6 |
| 不存在会话 join | 13 ms 收到相同空 ACK | join 不校验存在性 |
| 兼容 shim 后订阅 | `connecting -> joining -> active` | 证明显式兼容策略可建立实时链路 |
| 文本发送 | HTTP 接受；收到 user/run/event 更新 | 部分兼容 |
| assistant 消息 | Robot 本轮快速失败，未产生 | 未证明 |
| taskId | 三次发送得到相同 synthetic ID | 不兼容旧响应字段 |
| 中断 | chat-kit 本地 `conflict` | 被 taskId 问题阻塞 |
| 重连 | `reconnecting -> joining -> recovering` | 行为诚实但无法恢复可用状态 |
| 离线更新自动恢复 | 未恢复 | 0.6 没有历史 rebase |
| 测试数据清理 | 删除成功，HTTP 200 | 无持久残留 |

完整脱敏结果位于
`experiments/codex-chat-kit-current-server/result.json`，实验报告位于
`experiments/codex-chat-kit-current-server/report.md`。

### 2.2 源码和自动化证据

- Server `on_join_conversation` 只调用 `enter_room`，不验证 conversation，也没有
  显式返回值。
- Server Git 历史显示：TFRS-164 之前发送/中断返回 plain dict
  `{task_id: ...}`；`c1e2eb53` 才改为能序列化 `{taskId: ...}` 的 DTO。Staging
  synthetic runId 现象与旧部署行为一致。
- 当前 Server 相关 DTO/Socket/error 单测 11/11 通过。
- Server 数据库集成测试因本机 PostgreSQL `127.0.0.1:30433` 未运行而在 setup
  阶段失败，不能计为功能失败或通过。
- Chat Kit v0.6.0 Gateway/Protocol/Runtime 重点测试 267/267 通过。这些测试验证了
  0.6 自身逻辑，但测试 fixture 固定使用新式 `taskId` 和显式 join ACK。
- tfrobot-client 的 exact-0.6 临时副本构建成功，相关前端测试 24/24 通过；当前
  Chat 测试 mock 了整个 chat-kit，不能发现线上协议不兼容。
- 在 exact 0.6.0 隔离副本中实现 current-server 候选后，新增正向 fixture 与既有
  严格模式全仓测试合计 900/900 通过，完整 typecheck、候选文件 lint/format 通过。

## 3. 设计原则

### 3.1 严格模式不能被削弱

不能全局把 `undefined` ACK 改成成功。默认 profile 必须继续要求 Server 的显式、
可验证 ACK。只有宿主显式选择 current-server profile 时，才启用兼容行为。

### 3.2 可用与无损恢复必须分开表达

REST rebase 能恢复已持久化的消息和工具事件，并能查询当前 run 状态；它不能恢复
未持久化的 `chat_error`，也不能证明抓取窗口之外没有事件。因此：

- `active` 只表示 Server 已显式确认订阅/恢复契约；
- `degraded` 表示聊天可继续使用，但恢复保证为 best effort；
- 不允许把 REST rebase 伪装成 `recovery.complete=true`。

### 3.3 兼容规则归 Gateway 所有

`task_id`、空 ACK、Server 路径、查询参数和 Socket 事件名都是 TFRobotServer
transport 细节，只能存在于 `@turingfocus/chat-gateway-tfrobot`。Protocol、Runtime
和 UI 只消费标准化的 lifecycle、run 和 timeline。

### 3.4 默认失败关闭，降级必须可观测

兼容 profile 的每一次降级都必须产生结构化 lifecycle/diagnostic，不能静默 fallback。
凭证、headers、原始 payload 和消息正文不得进入诊断。

## 4. 公共 API 修改

在 `TFRobotGatewayOptions` 增加一个显式 profile：

```ts
export type TFRobotServerProfile =
  | { readonly mode: "verified" }
  | {
      readonly mode: "current-server";
      readonly recovery?: {
        readonly pageSize?: number;       // default 100
        readonly maxPages?: number;       // default 10
        readonly maxItems?: number;       // default 1000
      };
    };

export interface TFRobotGatewayOptions {
  // existing fields...
  readonly serverProfile?: TFRobotServerProfile;
}
```

规则：

- 未传时等价于 `{mode: "verified"}`。
- 不提供独立的 `acceptEmptyAck: true` 布尔值，避免宿主只打开 ACK 放行，却忘记
  existence preflight、taskId alias 和重连补偿。
- `current-server` profile 是一组不可拆分的安全约束，而不是若干松散开关。

tfrobot-client 的最终接入应为：

```ts
createTFRobotChatClientFactory({
  // current descriptor/session/fetch/creator options...
  serverProfile: {
    mode: "current-server",
    recovery: { pageSize: 100, maxPages: 10, maxItems: 1000 },
  },
});
```

## 5. Gateway 必须修改的行为

### 5.1 初次 join：空 ACK + REST 预检

`current-server` 模式的订阅顺序：

1. 用同一 SessionProvider 获取 subscribe 会话。
2. 对 conversation 执行 deadline-bounded REST preflight。建议使用 status 与最新一页
   messages；任一 authentication/authorization 失败立即失败关闭。
3. 建立 Socket 并发送 `join_conversation`。
4. 显式拒绝 ACK 始终失败；显式成功 ACK 按 verified 路径处理。
5. 只有 ACK 为空且 preflight 已证明 conversation 可读时，才接受订阅。
6. 发布 `degraded`，原因 `legacy-empty-join-ack`，随后允许命令。

preflight 解决“任意不存在 ID 都收到空 ACK”的问题，但不能创造 Server 没有实现的
会话级授权；文档和诊断必须明确这一限制。

建议的脱敏诊断：

```ts
{
  kind: "socket.lifecycle",
  status: "degraded",
  phase: "initial-join",
  reason: "legacy-empty-join-ack",
  assurance: "rest-preflight"
}
```

### 5.2 taskId camel/snake 兼容

以下 DTO 同时接受 `taskId` 和 `task_id`：

- send response；
- interrupt response；
- status response（tolerant reader，防止旧部署漂移）。

归一化规则：

1. 只出现一个字段：转为 string 后采用。
2. 两个字段同时出现且归一化后相同：采用。
3. 两个字段同时出现但不同：返回非重试 `validation`，不得猜测。
4. 两者都缺失：保留现有 synthetic runId fallback，但发出
   `missing-transport-task-id` 诊断；该 run 不可中断。

当前 0.6 的 `sendTextDtoSchema.taskId` 是 optional，导致 `{task_id}` 被当作合法“没有
taskId”而静默降级。应改为先解析兼容 DTO，再统一映射，不能继续依赖 optional 字段
掩盖协议漂移。

### 5.3 创建标题不一致

创建成功响应应继续以 Server 返回的 conversation 为事实源，不得在 Gateway 中无条件
覆盖成请求标题。同时增加脱敏诊断：

```ts
{ kind: "http.compatibility", operation: "create", reason: "title-normalized" }
```

只有返回标题为空或字段缺失时，才可使用请求标题作 UI fallback，并把原始差异保留在
安全的 normalized raw metadata 中。必须补真实环境回归来确定当前 Server 是主动
规范化、使用默认标题，还是部署版本偏差。

### 5.4 错误分类

- missing conversation 的历史/status 组合请求应尽量映射为 `not-found`，而不是泛化
  `validation`。
- Socket connect_error 缺少稳定状态时可保持 retryable `network`，但诊断增加
  `phase: "socket-handshake"`；不得通过解析可能含凭证的任意错误正文猜测。
- 显式 401/403 和结构化 unauthorized/forbidden 仍按现有逻辑处理并触发
  `onSessionInvalid`。

## 6. 重连补偿算法

### 6.1 状态机

Protocol 增加：

```ts
export type ChatLifecycleStatus =
  | "connecting"
  | "joining"
  | "active"
  | "degraded"
  | "reconnecting"
  | "recovering"
  | "auth-required"
  | "offline"
  | "subscription-failed";

export interface ChatRecovery {
  readonly complete: boolean;
  readonly assurance?: "server-verified" | "best-effort";
  readonly source?: "server-replay" | "rest-rebase";
  readonly cursor?: string;
  readonly reason?: string;
}
```

约束：

- `active` 必须是 `complete=true`、`assurance=server-verified`。
- `degraded` 必须是 `complete=false`、`assurance=best-effort` 且有 reason。
- `recovering` 表示补偿仍在执行，命令不可用。
- `degraded` 表示补偿已结束且 transport 可用，命令可用，但 UI 必须提示。

### 6.2 current-server reconnect 流程

1. disconnect 后进入 `reconnecting`，暂停命令和 realtime 应用。
2. 重连并重新 join；空 ACK 只作为“旧 Server 已调用 callback”的弱信号。
3. 进入 `recovering`，并行读取最新 status 与最新历史页。
4. 从最新页向前分页，直到找到上次已持久化的 timeline checkpoint，或达到
   `maxPages/maxItems/deadlineAt`。
5. 使用消息 ID、event ID + transition ID 做幂等 upsert；不能简单 append。
6. 采用 revision guard：rebase 期间到达的更高 revision realtime 更新不得被旧 REST
   snapshot 覆盖。
7. history 合并后再应用最新 status，恢复 run；若 status 不含真实 taskId，则 run
   `canInterrupt=false`。
8. 最终进入 `degraded`：
   - 找到 checkpoint：`rest-rebased-best-effort`；
   - 未找到 checkpoint：`recovery-window-exceeded`；
   - 无历史但 status 成功：`no-durable-checkpoint`。
9. REST authentication/authorization 失败进入 `auth-required`；网络/解析失败进入
   `offline`，不允许发送。

即使找到 checkpoint，也不能进入 `active`，因为 `chat_error` 和瞬时 state change
不是完整持久化日志。只有未来 Server 明确返回 `{accepted:true,
recoveryComplete:true,cursor}` 时才进入 verified `active`。

### 6.3 ID 和去重

Server 的 `msgId` 在 DTO 中可空。当前 fallback
`message:<conversation>:<timestamp>` 可能碰撞。为保证 live 与 REST rebase 得到同一 ID：

- 优先 transport `msgId`；
- 其次使用 conversation、role、server timestamp、sequence 和规范化 payload 的稳定
  SHA-256 指纹；
- 指纹仅用于内存 ID，不进入日志；
- event transition 同样必须用稳定 event/transition identity。

## 7. 各 package 修改清单

### 7.1 可直接执行的实现顺序

为避免只修空 ACK 后才发现 run、中断或重连仍不可用，Chat Kit 应按以下顺序在同一个
版本中实现和提交：

1. 先在 Testing 中落地 `currentServerFixture` 和本文 1.1 的单条端到端成功用例；初始
   状态应失败在 join，而不是通过弱化断言得到假绿。
2. Gateway 增加判别联合 `serverProfile`，默认 `verified`，把 legacy 行为限制在
   `current-server` 分支。
3. Gateway 在 initial join 前执行 authenticated REST preflight；只允许
   “preflight 成功 + 空 ACK”进入 `degraded`。strict 模式的空 ACK 测试必须继续失败。
4. DTO schema 同时校验 `taskId/task_id`，但不要用会改变对象形状的 schema transform。
   HTTP status 在重连路径会被重复解析，schema 必须幂等；应在 mapper/use site 通过
   一个纯函数取 transport task ID：

   ```ts
   const transportTaskId = (value: {
     taskId?: string | number | null;
     task_id?: string | number | null;
   }) => value.taskId ?? value.task_id ?? undefined;
   ```

5. send、interrupt、status/mapRun 三条路径统一调用该函数；alias 冲突由 schema 拒绝，
   两者缺失才允许 synthetic fallback。
6. Protocol 增加受 refinement 约束的 `degraded`；Runtime/UI 仅在
   `complete=false && assurance=best-effort` 时允许 degraded 命令，不能泛化为所有未知
   lifecycle 都可用。
7. 先实现最新一页 REST rebase 跑通正向 fixture，再扩展为第 6 节的 bounded pagination
   与 checkpoint；增加 revision/generation guard 后才允许进入发布候选。
8. 最后依次执行 focused success、既有 strict recovery、全仓 test/typecheck/lint/format、
   pack/consumer 和 Staging conformance。任何一步失败都不能只给 tfrobot-client 加 shim
   绕过。

### `@turingfocus/chat-gateway-tfrobot`

- 增加 `serverProfile` 判别联合与默认 strict 行为。
- 拆出 `JoinAcknowledgementPolicy`，实现 preflight 后的 empty ACK 兼容。
- DTO 层兼容 `taskId/task_id` 并拒绝冲突。
- 增加 bounded history rebase、checkpoint、revision guard 和幂等合并。
- 在 socket lifecycle 中发布 `degraded` 与结构化 reason/assurance/source。
- 对 missing conversation、握手和 title normalization 增强诊断。
- 保持不导出 `chat_message/chat_event/chat_error/state_changed` producer emit API。

### `@turingfocus/chat-protocol`

- 增加 `degraded` lifecycle。
- 扩展 recovery 的 `assurance/source`。
- schema/refinement 保证 active 与 degraded 语义不混淆。
- 保持缺少 lifecycle 的旧 Gateway 兼容路径。

### `@turingfocus/chat-runtime`

- command readiness：仅 `active` 或 `degraded` 可发送；`interrupt` 还要求真实 runId。
- `recovering` 期间缓存 realtime，REST rebase 完成后按 revision 合并。
- `snapshot.replace` 不得误删仍未解决的 domain error occurrence。
- `degraded -> active`、重复重连和会话切换必须保持 generation 隔离。

### `@turingfocus/chat-ui-antd`

- degraded 状态显示非阻塞 warning，不伪装为正常连接。
- degraded 允许输入/发送；interrupt 仍由 run `canInterrupt` 决定。
- 增加可覆盖的 i18n labels：degraded title/description、recovery-window-exceeded。
- 离线、认证失败、订阅失败仍保持阻塞 UI。

### `@turingfocus/chat-testing`

- 增加 `currentServerFixture`，准确复现：空 initial/reconnect ACK、任意 room join、
  snake task ID、无 replay、REST history/status。
- 增加 remote conformance runner；输出必须脱敏且记录部署 version unknown。
- fixture 必须覆盖持久化与非持久化事件差异，不能只模拟 happy path。

### `@turingfocus/chat-kit`

- headless/UI facade 导出新增 profile 和 lifecycle 类型。
- README 增加 current-server 接入、风险、迁移和退出 legacy profile 的说明。

## 8. 必须新增的测试

### P0 单元/集成测试

1. strict profile 初次空 ACK 仍失败。
2. current-server profile：preflight 成功 + 空 ACK -> degraded subscription。
3. preflight not-found/401/403 时，即使收到空 ACK也失败。
4. 显式拒绝 ACK 永远不能被 legacy profile 覆盖。
5. `taskId`、`task_id`、相同双字段、冲突双字段和双缺失五组 DTO 用例。
6. snake task ID 能完成 run-pinned interrupt，interrupt body 仍发送标准 `taskId`。
7. reconnect empty ACK -> bounded REST rebase -> degraded。
8. 显式 verified replay ACK -> active，不走 legacy rebase。
9. rebase 期间 realtime 更新优先，旧 status/snapshot 不得回滚状态。
10. duplicate/out-of-order message/event/transition 幂等。
11. checkpoint 找不到、分页超限、deadline、dispose、会话切换。
12. `chat_error` 未持久化时不得产生 `complete=true`。
13. Runtime/UI 在 degraded 可发送，但明确展示 warning。
14. 凭证和消息正文不进入 diagnostic/result fixture。

### 真实环境 conformance 门禁

每个候选版本至少在 Staging 执行：

- list 两页；
- create 并验证 title roundtrip；
- load 既有长会话与 previousCursor；
- valid/invalid auth；
- valid/missing conversation join；
- send 并验证真实非 synthetic runId；
- assistant message、tool event、state、chat_error；
- run-pinned interrupt；
- 强制断线，在离线窗口完成一次持久化消息，然后 reconnect/rebase；
- 重复/丢失计数；
- 删除测试会话。

当前实验未证明 assistant/history/chat_error 的真实样本，所以这些必须是发布门禁，不能
因仓库 mock 测试通过而标绿。

## 9. tfrobot-client 接入修改

chat-kit 新版本完成后，tfrobot-client 只需要宿主级配置，不应复制 Gateway 逻辑：

1. `package.json` 和 pnpm overrides 中 facade + 五个 leaf package 原子升级同一版本。
2. `createClientChatFactory` 传 `serverProfile.mode="current-server"`。
3. `onLifecycleDiagnostic` 只记录 status/reason/assurance，不记录 token、URL query、消息。
4. 增加至少一个不 mock chat-kit 的 host contract test，使用真实
   `createTFRobotChatClientFactory` + deterministic current-server fixture。
5. Staging feature flag 灰度，监控 subscription failure、synthetic runId、degraded
   reconnect、rebase 超限和 auth invalidation。

现有 Tauri trust boundary、短 token、BFF cookie/routing 和 message creator 分层无需
下沉到 chat-kit，也不需要调整 Manager 主 JWT 边界。

## 10. 发布与回滚

### 发布顺序

1. chat-kit 合入 P0 实现和 fixture。
2. 全仓 check、pack、lower-bound consumer、真实 Staging conformance 通过。
3. 发布统一版本 package family。
4. tfrobot-client 临时副本安装发布 tarball，执行 build/unit/host contract。
5. feature flag 小流量启用 current-server profile。
6. 观测至少一个完整的发送、中断和断线恢复周期后扩大。

### 回滚

- 保留 tfrobot-client 的 chat feature flag；失败时关闭新 client，不在运行时同时维护
  两条 Socket 连接。
- package 回滚必须 facade + leaf 原子回滚。
- 回滚不删除用户会话数据；只 dispose 当前 Gateway/Runtime。

### 退出 legacy profile

未来 Server 提供 access-validated ACK 和 durable replay cursor 后：

1. conformance 验证 verified profile；
2. tfrobot-client 切换 `{mode:"verified"}`；
3. 观测 current-server profile 使用率归零；
4. 再删除 snake alias/empty ACK/rebase 兼容代码。

## 11. 风险与边界

- 当前 Server Socket 只在握手校验 `chat:read`，并暴露内部 producer inbound handlers。
  chat-kit 已不导出这些 emit，但无法从客户端修复 Server 端 producer 隔离；TFRS-297
  仍是安全债务。
- REST preflight 只能证明当前凭证可读取目标 conversation，不能补出 Server 未实现的
  conversation owner/tenant 授权。
- current-server profile 的目标是“在明确降级语义下可用”，不是提供与未来 replay
  Server 等价的无损保证。
- Staging 部署 commit 未知且与当前 develop 行为不同。remote conformance 必须记录
  version unknown，并以黑盒行为为准。

## 12. 验收结论

本次决策为：**当前 published 0.6.0 No-Go；修改技术路径已由正向原型验证为 Go；完成
生产级 P0 和 Staging conformance 后，发布并接入新版本 Go。**

P0 完成标准：

- 初次订阅能在 current-server profile 下经 REST preflight 接受空 ACK；
- 发送能从 `task_id` 或 `taskId` 得到真实 runId；
- run-pinned interrupt 可用；
- reconnect 能完成 bounded REST rebase 并进入可用但明确的 degraded 状态；
- strict/verified 默认没有被削弱；
- Staging conformance 覆盖 assistant/history/chat_error 和离线恢复；
- tfrobot-client 使用真实 chat-kit 的 host contract test 能发现协议回归。

在这些条件满足前，仅通过 TypeScript、构建和 mock 单测不足以确认升级安全。
