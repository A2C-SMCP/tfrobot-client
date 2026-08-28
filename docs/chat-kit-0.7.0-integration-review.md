# Chat Kit 0.7.0 接入与修改说明

- 日期：2026-08-12
- 目标项目：`tfrobot-client`
- 原版本：`@turingfocus/chat-kit` 0.5.0
- 接入版本：`@turingfocus/chat-kit` 0.7.0
- 发布 tag：`v0.7.0@cc446555ea942cb08587921a3c7e41169a04588d`
- 跟踪任务：A2C-SMCP/tfrobot-client#48

## 结论

Chat Kit 0.7.0 已完成客户端代码接入，代码层结论为 **Go**。当前
TFRobotServer 兼容档位的确定性宿主协议测试已经跑通，证明以下路径能够成功：

1. current-server profile 先完成同 session REST preflight；
2. Server 的 `join_conversation` 回调没有 ACK 参数时，订阅仍成功；
3. lifecycle 进入可操作的 `degraded`，恢复语义明确为
   `complete=false`、`best-effort`、`rest-rebase`；
4. Server 返回 snake-case `task_id` 时，Chat Kit 保留真实 runId；
5. interrupt 使用该真实 runId，HTTP 请求体为 `{ "taskId": "..." }`；
6. 断线重连后执行有界 REST rebase，并恢复离线期间已持久化的消息。

默认 verified profile 的安全语义没有放宽：相同的空 ACK 在未显式启用
current-server profile 时仍以 `validation` 失败。

真实 Staging 灰度仍为 **Defer**。前置验证期间 Staging Manager 的 `/health` 和
`/api/v1/digital-employees` 持续返回 HTTP 503，尚未完成真实环境闭环；在该闭环通过前
不应启用聊天 feature flag 或扩大灰度。

## Server 现状与成功条件

当前 Server 的 `join_conversation` 行为是进入房间后调用无参数 Socket.IO callback，
没有返回可验证 ACK，也没有 durable replay cursor。0.7.0 不把这种行为伪装成 verified：

- `serverProfile.kind="verified"`：空 ACK 被拒绝；
- `serverProfile.kind="current-server"`：只有 REST preflight 成功后才接受空 ACK；
- 初次连接和重连均以 degraded/best-effort 表达能力边界；
- 重连通过有界 REST 历史回补恢复已持久化内容，不承诺恢复瞬时事件。

因此，成功接入不能只把依赖版本改为 0.7.0；宿主必须显式传入 current-server profile，
并继续保留可用的 REST BFF、短期 session provider 和 Socket 鉴权刷新链路。

## 已实施修改

### 1. 原子升级 package family

`package.json` 和 `pnpm-lock.yaml` 已将 facade 与五个 leaf package 固定为 0.7.0：

```json
{
  "dependencies": {
    "@turingfocus/chat-kit": "0.7.0"
  },
  "pnpm": {
    "overrides": {
      "@turingfocus/chat-gateway-tfrobot": "0.7.0",
      "@turingfocus/chat-protocol": "0.7.0",
      "@turingfocus/chat-react": "0.7.0",
      "@turingfocus/chat-runtime": "0.7.0",
      "@turingfocus/chat-ui-antd": "0.7.0"
    }
  }
}
```

继续使用精确版本和 overrides，避免 facade 的 semver 范围在未来安装时产生 leaf package
混装。

### 2. 显式启用 current-server profile

`src/components/Chat/chatBridge.ts` 新增客户端拥有的固定配置，并传给
`createTFRobotChatClientFactory`：

```ts
export const CURRENT_SERVER_PROFILE = Object.freeze({
  kind: 'current-server' as const,
  rebase: Object.freeze({
    deadlineMs: 10_000,
    maxItems: 500,
    maxPages: 10,
    pageSize: 50,
  }),
});
```

有界参数的含义：单次 rebase 最长 10 秒、最多 10 页、500 项，每页 50 项。配置不做
运行时自动探测；未来 Server 提供可验证 ACK 和 durable replay 证明后，再移除该配置，
回到默认 verified profile。

现有安全边界保持不变：

- Tauri lease 和短期 token 获取方式不变；
- 浏览器 header 不跨 IPC，Rust BFF 仍负责注入凭据和路由信息；
- Robot/employee 选择及 session 生命周期不下沉到 Chat Kit；
- client 不实现 ACK shim、task ID 兼容或历史 rebase。

### 3. 补齐本地化映射

`src/components/Chat/chatBridge.ts`、`src/locales/zh/translation.json` 和
`src/locales/en/translation.json` 已补充：

- degraded lifecycle 文案，明确“尽力回补，可能缺失瞬时更新”；
- rename 标题和确认文案；
- delete 标题、取消、确认和不可撤销提示。

注意：tfrobot-client 当前使用 Chat Kit 的 compact navigation。0.7.0 的 compact history
菜单只支持选择会话，不消费 rename/delete 回调；rename/delete 操作仅在 sidebar 会话列表
中暴露。本次没有擅自把现有紧凑布局改为侧边栏，也没有加入不可触达的宿主弹窗死代码。
文案已准备完毕，但若产品要求在 compact UI 中直接操作 rename/delete，需要后续选择：

1. 上游为 `ChatCompactNavigationConfig` 增加管理动作；或
2. 客户端明确改用 0.7.0 的 sidebar/managed workspace。

这不影响 current-server 的订阅、发送、中断与恢复主链路。

### 4. 增加真实发布物宿主协议测试

`src/test/components/ChatKitContract.test.ts` 不 mock `@turingfocus/chat-kit`，直接加载 npm
发布物，并复用真实 `createTauriChatFetch`。测试只替换两个确定性边界：

- Socket transport fixture：复现 Server 的零参数 join callback；
- Tauri `chat_http_request` fixture：复现 status、messages、send 和 interrupt 响应。

覆盖断言：

- verified + empty ACK 必须失败；
- current-server REST preflight + empty ACK 必须成功；
- degraded lifecycle 仍被 `isChatLifecycleOperable` 识别为可操作；
- `task_id` 转换为真实 runId；
- interrupt 请求使用 `taskId`；
- reconnect 恢复离线持久化消息；
- rebase 结果保持 `complete=false/best-effort/rest-rebase`。

`src/test/components/ChatBridge070.test.ts` 另外保护 factory 的 profile 转发和所有新增
label key。

## 验证结果

| 验证项 | 结果 |
| --- | --- |
| `pnpm install` | 通过 |
| 0.7.0 package family 唯一解析 | 通过；facade 与五个 leaf package 均为 0.7.0 |
| Chat 重点测试 | 16/16 通过 |
| `src/test` 维护范围前端测试 | 46 文件、459/459 通过 |
| `pnpm build` | 通过 |
| ESLint | 0 error；8 个仓库既有 warning |
| strict empty ACK | 正确拒绝，`validation` |
| current-server empty ACK | preflight 后订阅成功，degraded 且可操作 |
| snake-case task ID | 得到真实 runId |
| run-pinned interrupt | 成功；请求字段为 `taskId` |
| reconnect REST rebase | 恢复离线持久化 assistant 消息 |
| 恢复语义 | `complete=false`、`best-effort`、`rest-rebase` |
| Staging | Manager 503，未进入 Chat Kit，未产生写入 |

仓库根 `pnpm test` 还会收集遗留 `experiments/`：

- `experiments/codex-chat-kit-current-server/candidate-policy.test.mjs` 不是 Vitest suite；
- `experiments/codex-rust-sdk-capability-oauth-runtime-state/client-ui-projection.test.tsx`
  有一条与 Chat Kit 无关的 OAuth/MCP 投影失败。

因此根命令结果为 2 个实验文件失败、46 个维护范围文件通过；本次以明确限定的
`src/test` 459/459 作为前端回归结论，没有修改用户的实验文件。

## Staging 启用门禁

Manager 恢复后必须在同一真实会话完成以下闭环，全部通过才允许打开聊天灰度：

1. 获取数字员工并打开 Tauri chat lease；
2. create conversation；
3. rename conversation（可通过 API/side bar 验证；compact UI 限制见上文）；
4. send text，并确认返回非 synthetic 的真实 runId；
5. interrupt 该 runId，并确认 Server 接受；
6. 主动断开 Socket，在离线期间产生一条可持久化消息；
7. 重连，确认 REST rebase 恢复该消息；
8. 确认 lifecycle 显示 degraded，而不是 active/无损恢复；
9. delete conversation，并确认列表刷新后不可见；
10. 关闭 lease，确认没有残留远端测试会话或本地凭据。

失败时应记录 sanitized lifecycle 的 status、reason、assurance、source 以及 HTTP/Socket
阶段；不得记录 token、Authorization header、原始 session 或敏感消息内容。

## 回滚方案与剩余风险

代码回滚必须原子恢复 facade、五个 overrides 和 lockfile，不能只降 facade。回滚至
0.5.0 后，当前 Server 的空 ACK 仍会导致订阅失败，因此回滚只用于处理 0.7.0 自身回归，
不能作为当前协议的可用降级路径。

剩余风险：

- REST rebase 无法恢复未持久化的 `chat_error` 等瞬时事件；
- Staging 部署版本未能在本轮黑盒验证，真实行为仍可能与源码/fixture 不一致；
- recovery 上限触发时只保证有界尽力恢复，不能将 degraded 展示为完整恢复；
- compact navigation 尚不直接暴露 rename/delete 管理动作。
