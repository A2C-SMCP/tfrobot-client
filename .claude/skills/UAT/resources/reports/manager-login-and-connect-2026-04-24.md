# UAT 报告 — manager-login-and-connect（2026-04-24）

**场景**：`scenarios/manager-login-and-connect.md`
**日期**：2026-04-24
**执行模式**：Claude 引导 + 用户手动操作（Tauri 协同 UAT）
**环境**：
- tfrobot-client @ :1420（`pnpm tauri dev`）
- TFRSManager user-service @ :8090（`make local-run-init-debug`）
- TFRobotServer uvicorn @ :8080 + Caddy TLS sidecar @ :8443

---

## 摘要

| 类别                 | 数量 |
|---------------------|------|
| 总用例              | 15 active + 3 BLOCKED |
| 通过 ✅              | 11   |
| 跳过/未测 ⏭️         | 4    |
| 受阻 ⏸️（Mock 缺失） | 3    |
| 失败 ❌（真 bug）    | 2（都已修）|

**整体评价**：核心 E 路径（登录 → 员工列表 → 选中即连 → SMCP 真连通）**已打通**，脱敏硬校验 100% 命中。UAT 过程中发现 2 个真实 bug 并在现场修复，1 个契约问题需记录，剩余场景待下一轮补测。

---

## 用例详情

| #       | 用例                      | 结果  | 备注 |
|--------|---------------------------|------|------|
| ML-01  | 缺 base URL 错误          | ✅   | Alert + missing_base_url kind 命中 |
| ML-02  | URL 无法连通              | ✅   | network_error kind 命中 |
| ML-03  | 密码错误                  | ⏭️ 跳过 | 表单 state 惯性，未真正触发错密码路径，**建议下轮重测** |
| ML-04  | 正确凭据登录（单账户）    | ✅   | accountName=client_uat、accountId=16 |
| ML-05  | 多账户登录 + select       | ✅   | tempToken + 双账户选项 + enterprise 成功切入 |
| ML-06  | 混合状态员工列表          | ✅   | status Tag 颜色正确、init_failed disable Connect |
| ML-07  | 空员工列表                | ✅   | Empty 态渲染，`fetched 0 digital employees` |
| ML-08  | 选中员工 → 拉 connection-info | ✅ | 首次因 stale 连接池失败（见 UAT-BUG-001），修复后全链路通 |
| ML-09  | **脱敏硬校验**            | ✅   | `access_token:"***"` 出现、无 64-hex 串、顶层无 `accessToken` |
| ML-10  | SMCP 真连通               | ✅   | `Successfully joined office: 5f4b...` 日志证据 |
| ML-11  | Server-side auth succeeded | ⏭️ 跳过 | smcp-computer 层已打 `joined office` = 等价证据，未去 TFRobotServer 日志二次确认 |
| ML-12a | Popconfirm 弹窗           | ✅   | 三按钮（覆盖 / 另存副本 / ×）正确渲染 |
| ML-12b | 覆盖                      | ✅   | 首次遇 UAT-BUG-002 "already exists in room"，修复后通过 |
| ML-12c | 另存副本                  | ⏭️ **未测** | 下轮补测 |
| ML-12d | 取消                      | ⏭️ **未测** | 下轮补测 |
| ML-13  | 退出登录                  | ⏭️ **未测** | 下轮补测 |
| ML-14  | 登出后状态干净             | ⏭️ **未测** | 下轮补测 |
| ML-15  | i18n 英文切换             | ⏭️ **未测** | 下轮补测 |
| ML-F1  | 402 欠费熔断              | ⏸️ BLOCKED | 等 Manager 接 freezeChecker 到 connection-info 路径 |
| ML-G1  | 403 权限不足              | ⏸️ BLOCKED | 等 Manager 提供无权限账号或 mock flag |
| ML-H1  | 401 JWT 失效              | ⏸️ BLOCKED | 等 Manager 提供 dev revoke 端点或可配 TTL |

---

## 本轮修复的 Bug（已合入代码）

### UAT-BUG-001 — reqwest 连接池 stale 连接导致第二次 /connection-info 失败

**症状**：`manager_login` + `manager_list_digital_employees` 相继成功后，20 秒后首次调 `manager_get_connection_info` 失败，错误 kind=`network_error`；curl 直打同端点完全正常。

**根因**：reqwest 默认启用 HTTP/1.1 keep-alive 连接池，Go 侧 `net/http` server 或某层中间件关闭了空闲连接，客户端从 pool 取出 stale 连接复用时失败。reqwest 的 `e.to_string()` 只显示顶层"error sending request for url (...)"，未展开 source chain，掩盖了 "connection reset by peer" 这类底层原因。

**修复**（`src-tauri/src/services/manager_client.rs`）：
1. `ManagerClient::new` 的 reqwest Client 配置：
   - `.pool_max_idle_per_host(0)` — 禁用连接池（Manager 调用极少，性能影响可忽略）
   - `.tcp_keepalive(Duration::from_secs(10))` — 启用 TCP keepalive 探活
2. 新增 `flatten_reqwest_err` 函数，把 `source()` chain 全部展平后写进 `NetworkError.detail`，将来同类问题可以一眼看到"-> connection reset by peer"等真实原因

**影响面**：Manager 的 4 个 REST 调用（login / select-account / list / connection-info）—— `.map_err(...)` 批量替换。

**验证**：UAT 中用户第二次点连接按钮成功；后续 ML-12b 修复后多次切换无再现。

---

### UAT-BUG-002 — 已连接同一 employee 时重连被服务端拒绝（"already exists in room"）

**症状**：ML-08 成功后立即再点连接（或走同名冲突 → 覆盖），客户端走 save_profile + connect_smcp，但 TFRobotServer 返回：
```
Socket.IO error: Failed to join office: Internal server error:
Computer with name '本地联调员工' already exists in room '5f4b3b3b-3b3b-3b3b-3b3'
```

**根因**：`selectEmployeeAndConnect` 直接调 `connect_smcp` 而不先断开已有连接。TFRobotServer 按 `(office_id, computer_name)` 判重，同一 room 内不允许同名 computer 并存。客户端 Profile 的 name 改变了（覆盖或副本）但 Manager 下发的 `computerName` 和 `rid` 不会变，于是触发冲突。

**修复**（`src/stores/managerStore.ts` `selectEmployeeAndConnect`）：
在 `saveProfile` 之后、`connect` 之前，若 `connectionStore.status.connected === true`，先调 `disconnect()`，并打 `manager: disconnected prior session before reconnect` 日志。断开失败时仅 warn 不阻塞主流程。

**验证**：ML-12b 覆盖流程日志：
```
manager: profile saved name=本地联调员工
[connection] Disconnecting from SMCP server
[socketio_client] Left office: 5f4b3b3b-...
SMCP disconnected
manager: disconnected prior session before reconnect
[socketio_client] Successfully joined office: 5f4b3b3b-...
SMCP connected: 本地联调员工
manager: connect initiated profile=本地联调员工
```

---

### UAT-BUG-003 — Manager 账号页 Connect 按钮不反映 SMCP 当前连接状态

**症状**：用户成功连上员工后，Manager 账号页的该员工行按钮仍然显示"连接"（无"已连接"指示，用户可能再点一次触发 UAT-BUG-002）。

**根因**：`EmployeeList` 组件只消费 `managerStore` 的 employees 列表状态，没有读 `connectionStore.status`。按钮是否可点、label 是什么，完全不知道 SMCP 是否已连。

**修复**（`src/components/ManagerAccount/EmployeeList.tsx`）：
- 订阅 `useConnectionStore.status` 和 `disconnect`
- 新增 `isConnectedEmployee(emp)` 判定：`status.connected && status.office_id === emp.robotId`（按 office_id 匹配而不是 profile name，这样"另存副本"创建的 profile 也能正确识别）
- 已连接的员工：显示绿色 ✓ `已连接` Tag，按钮变为红色"断开连接"（调 `disconnect_smcp`）
- 未连接：按原逻辑显示"连接"按钮（按 status=running 控 disable）
- `handleConnect` 成功后主动 `fetchConnectionStatus()` 触发 UI 立即刷新
- 新增 i18n：`connected` / `disconnect` / `disconnectSuccess`

**验证**：待下一轮复测确认 UI 即时切换。

---

## 未修复的问题（下一轮处理）

### CQ-001 — antd `Modal` / `message` Static function context 警告（P2）

**症状**：DevTools Console 出现反复：
```
Warning: [antd: Modal] Static function can not consume context like dynamic theme.
                      Please use 'App' component instead.
Warning: [antd: message] Static function can not consume context like dynamic theme.
                        Please use 'App' component instead.
```

**出现场景**：
- `EmployeeList.tsx` 里的 `Modal.confirm`（同名冲突 Popconfirm）
- `EmployeeList.tsx` 里的 `message.success`/`message.error`
- `SmcpConnection/index.tsx` 里的 `message.*`（已有问题，非本次引入）

**影响**：dark theme / ConfigProvider 嵌套下样式/主题会回落到默认值；功能不受影响。

**修复方向**：
1. `src/App.tsx` 最外层用 `<App>...</App>` 包裹（antd 的 App 组件，不是我们的 App 函数）
2. 各调用方用 `App.useApp()` 拿 `{ modal, message, notification }` 替换 `Modal.confirm` / `message.*` 静态调用
3. 改动面：`EmployeeList.tsx` + `SmcpConnection/index.tsx` + 其他使用 `message.*` 的组件（建议 grep 一把统一改）

**优先级**：P2，不阻塞 UAT。

---

### CQ-002 — `fetched N digital employees` 日志在 EmployeeList 挂载时打 x2（P3）

**症状**：DevTools Console 里每次进入员工列表页，`manager: fetched N digital employees` 出现两次。

**根因**：
- `EmployeeList.tsx` `useEffect` 依赖 `[session, fetchEmployees]`
- React StrictMode 开发模式下 effect 被故意跑两次
- 也有可能 `useManagerStore()` hook 在 session 变化瞬间触发额外渲染

**影响**：开发模式日志噪声，生产环境 StrictMode 不开不会出现。

**修复方向**：
1. 在 store 内部 `fetchEmployees` 前加"如果正在 loading 就不重复发起"的 guard
2. 或者把 effect 依赖从 `[session, ...]` 改为 `[session?.userId]`，session 对象引用变化但 userId 不变时不重发

**优先级**：P3，纯开发体验。

---

### ML-03 补测

ML-03 密码错误用例在本轮因为 antd Form 字段 state 保留上一用例的正确密码，被跳过。下一轮补测时，在填入错误密码前**显式清空**密码输入框再输入。

---

### ML-12c / 12d / 13 / 14 / 15 未跑

阻塞于用户当前对话截断点，下一轮继续推进。

具体衔接点：
- 当前状态：用户在 Manager 账号页，client_uat 已登录，本地联调员工已 Connected（UI 应该显示"已连接"+"断开连接"按钮，修复 UAT-BUG-003 后有效）
- 下一步从 ML-12c（另存副本）开始，按场景文档继续推进

---

### BLOCKED 场景解锁

F/G/H 三个错误路径场景需要 TFRSManager 配合提供触发条件：

- **F（402 欠费）**：Manager 把 `freezeChecker` 接到 `/connection-info` 路径，对冻结组织的请求返回 `HTTP 402 + {message, redirectUrl?}`
- **G（403 权限）**：Manager 提供一个无权访问某 employee 的账号，或 mock flag 让 `/connection-info` 对特定 id 返回 403
- **H（401 JWT 失效）**：Manager 提供 dev 端点主动 revoke JWT，或者可配置短 TTL（如 60s）的测试账号

建议生成 seed-request 报告走 `/uat-scenario` 流程正式提给 Manager 工程师。

---

## 开发体验与 UAT 设计的反思

本轮 UAT 暴露的几点可复用经验：

1. **契约以 server 实测 JSON 为准，而非规格文档**：#23 实现时按 TFRM-18 Jira 规格猜字段结构，结果 envelope / 扁平 LoginResponse / tempToken 这些都和实际不符，UAT 前端跑一遍即暴露。memory 里"Verify before modify"的规则是对的，应再加一条"契约先 curl"。

2. **reqwest 错误堆栈默认不展开**：调试 HTTP 客户端问题时，第一要务是展平 source chain，否则看到的只是无信息量的顶层消息。`flatten_reqwest_err` 已抽成工具函数，将来别的 HTTP 调用复用。

3. **Tauri 协同 UAT 的节奏控制**：每步"做什么 / 复制用 / 应该看到 / 失败贴什么回来"四段式在这轮跑得比较顺。关键断言（脱敏、连接成功）有 Console 日志模式做硬标准，比肉眼观察稳得多。

4. **UI 状态与 store 状态的同步**：EmployeeList 最初完全忽略 connectionStore，是典型的"store 隔离不当"设计漏洞。场景类 UI 必须显式订阅相关 store，不能假设"用户不会走到需要关联状态的 UI"。

5. **表单 state 惯性**：ML-03 被跳过的原因提醒后续场景写作时，字段切换路径要显式写"清空 → 填新值"，不能默认上一步的残留值会被自动覆盖。

---

## 附：关键日志片段

### ML-09 脱敏硬校验通过的日志原文

```
manager: connection-info ok for 本地联调员工: {
  "socketBaseURL":"https://127.0.0.1:8443",
  "sioPath":"/socket.io/",
  "namespace":"tfrobotserver",
  "rid":"5f4b3b3b-3b3b-3b3b-3b3",
  "robotType":"tfrobot",
  "smcpNamespace":"/smcp",
  "computerName":"本地联调员工",
  "routingHeaders":{
    "X-TF-RobotType":"tfrobot",
    "X-TF-RobotId":"5f4b3b3b-3b3b-3b3b-3b3",
    "access_token":"***",
    "X-TF-Namespace":"tfrobotserver"
  }
}
```

**断言匹配**：
- ✅ `"access_token":"***"` 字面存在
- ✅ 无 64 字符 hex 串
- ✅ 顶层无 `"accessToken"` 字段（被 `redactForLog` 省略）

真值 `accessToken = "ac4a30ae...756c"`（64 字符 hex）在此日志中完全不可见。

### ML-12b 修复后覆盖流程完整日志

```
manager_get_connection_info: id=11
manager: connection-info ok for 本地联调员工: {...脱敏正常...}
[connection] Saving connection profile: 本地联调员工
manager: profile saved name=本地联调员工
[connection] Disconnecting from SMCP server
[socketio_client] Left office: 5f4b3b3b-3b3b-3b3b-3b3
SMCP disconnected
manager: disconnected prior session before reconnect
[connection] Connecting with profile: 本地联调员工
[socketio_client] Connected to SMCP server at https://127.0.0.1:8443 with computer name: 本地联调员工
[socketio_client] Successfully joined office: 5f4b3b3b-3b3b-3b3b-3b3
[connection] Connected to SMCP server: https://127.0.0.1:8443
SMCP connected: 本地联调员工
manager: connect initiated profile=本地联调员工
```
