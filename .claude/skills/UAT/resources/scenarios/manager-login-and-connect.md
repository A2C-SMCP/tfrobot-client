# UAT 场景：Manager 登录 → 选中员工即连

**关联 Issue**：[#24](https://github.com/A2C-SMCP/tfrobot-client/issues/24)（父 [#20](https://github.com/A2C-SMCP/tfrobot-client/issues/20)）
**对应上游 guide**：`$HOME/GolandProjects/tfrsmanager/docs/local-dev/client-uat-guide.md`
**用例编号前缀**：`ML-`（Manager Login）

## 测试目标

验证 tfrobot-client 桌面端从"登录 TFRSManager → 拉数字员工 → 选中一个 → 立即发起
SMCP 连接并连通 TFRobotServer → 登出"的端到端用户闭环，包括：

- 登录态分支（缺 baseURL / 错 URL / 错凭据 / 单账户 / 多账户）
- 员工列表态（混合状态 / 空列表）
- 选中即连（profile 自动注入 + Popconfirm 冲突解决 + 立即建立 SMCP socket）
- **敏感信息脱敏**（access_token 不出现在任何日志里）
- 登出（keychain + SMCP socket + store 全清）

## 前置条件

- `pnpm tauri dev` 已起（端口 1420 监听）
- TFRobotServer + Caddy sidecar 起在 :8080 / :8443
- TFRSManager user-service 起在 :8090（`make local-run-init-debug`）
- 客户端 Rust 端已用新 DTO 重编译（`envelope + 扁平登录 + u64 id`；如果跑 UAT 前改过
  `manager_client.rs`，要重启 `pnpm tauri dev`）
- DevTools Console 已打开（用户可以贴日志）

Claude 在开场先跑 `resources/environment-checks.md` 全套探针，全绿才启动。

## 页面入口

客户端左侧导航：**连接** → **Manager 账号**

## 测试用例

### P0 — 登录分支

| #     | 用例                | 做什么                                                                | 复制用                                                              | 应该看到（UI + Console）                                                                                   |
|------|---------------------|----------------------------------------------------------------------|--------------------------------------------------------------------|---------------------------------------------------------------------------------------------------------|
| ML-01 | 缺 base URL         | Manager 地址**留空**，手机号密码随便填，点登录                         | (空)                                                                | UI 顶部红色 Alert："未配置 Manager 地址，请设置环境变量 TFRS_MANAGER_BASE_URL 或在登录表单中填写。"            |
| ML-02 | URL 无法连通         | Manager 地址填假地址，其他正常                                         | `http://127.0.0.1:9`<br>`13800138008` / `Test@123456`               | UI 红色 Alert："无法连接到 Manager 服务器..."；Console: `manager: login failed, kind=network_error`          |
| ML-03 | 密码错误            | Manager 地址对、手机号对、密码错                                       | `http://localhost:8090`<br>`13800138008` / `wrong`                  | UI 红色 Alert（`unauthorized` / `other`）；Console: `manager: login failed, kind=unauthorized`（或 other）|
| ML-04 | 正确凭据登录（单账户）| 三项都正确                                                           | `http://localhost:8090`<br>`13800138008` / `Test@123456`            | UI 切到"数字员工"页，顶部"当前登录：`client_uat`"；Console: `manager: login ok, accountId=16` + `fetched N digital employees` |
| ML-05 | 多账户登录 + 选择   | 前置：若已登录先 ML-12 登出。用 testuser2 手机号登录                   | `http://localhost:8090`<br>`13900139000` / `Test@123456`            | UI 先进"选择账号"页，列出 2 条（enterprise / personal）；点 enterprise 进入员工列表；Console: `manager: login requires account selection (2 options)` → `manager: account selected, accountId=2` |

### P0 — 员工列表态

| #     | 用例                  | 前置                 | 做什么                           | 应该看到                                                                                              |
|------|----------------------|---------------------|---------------------------------|------------------------------------------------------------------------------------------------------|
| ML-06 | 混合状态员工列表       | 用 testuser 登录     | 观察员工列表                     | 列表至少 4 行；`running` 行绿色 Tag；`init_failed` 行红色 Tag 且 Connect 按钮 disabled                 |
| ML-07 | 空员工列表            | 用 clientUATEmpty 登录 | 观察列表                         | 显示 `Empty` 空态："当前账号下没有可用的数字员工。"                                                    |

复制用（前置账号切换）：
- ML-06: `13800138000` / `Test@123456`
- ML-07: `13800138009` / `Test@123456`

### P0 — 选中即连（场景 E，核心验收）

前置：先 ML-12 登出，再用 `client_uat` 账号登录（`13800138008` / `Test@123456`）。

| #     | 用例                          | 做什么                                             | 应该看到                                                                                                                                                                     |
|------|-------------------------------|---------------------------------------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| ML-08 | 选中员工 → 拉 connection-info  | 点"本地联调员工"行右侧的"连接"按钮                  | Console 依次出现：<br>① `manager: connection-info ok for 本地联调员工: {...}` — **JSON 里 `routingHeaders.access_token` 必须是 `"***"`，顶层无 `accessToken` 字段，不含 `ac4a30ae` 片段** <br>② `manager: profile saved name=本地联调员工`<br>③ `manager: connect initiated profile=本地联调员工`<br>右上角 toast："正在连接 本地联调员工…" |
| ML-09 | 脱敏硬校验                    | 把 ML-08 Console 里 `connection-info ok` 那行完整贴给 Claude | Claude grep 该日志，断言：❶ 不含 64 字符 hex token 片段 ❷ `access_token":"***"` 出现一次 ❸ 顶层不出现 `"accessToken"` 键                                                           |
| ML-10 | SMCP 连接页反映已连            | 切到侧栏"**SMCP 服务器**"                           | Connection Status 显示 `Connected`；URL = `https://127.0.0.1:8443`；Office = `5f4b3b3b-3b3b-3b3b-3b3`；Computer = `本地联调员工`                                                 |
| ML-11 | Server 侧成功印证（可选）       | 在另一个终端跑 `tail -n 50 ~/.tfrobotserver/logs/tfrobot_api.log \| grep -E "auth succeeded\|ns=/smcp.*connected"` | 至少看到：`Socket.IO auth succeeded for 5f4b3b3b-3b3b-3b3b-3b3` + `[sid=XXX] [ns=/smcp] Client connected successfully` |

### P1 — 同名 Profile 冲突

前置：ML-08 后当前已经存在一个名为"本地联调员工"的 Profile。不登出，直接再点一次"连接"
按钮（或退出后重新用同账号进来再点）。

| #     | 用例                        | 做什么                                 | 应该看到                                                                                                |
|------|----------------------------|---------------------------------------|--------------------------------------------------------------------------------------------------------|
| ML-12a | 冲突弹窗出现                 | 点"连接"                               | Modal："同名连接配置已存在" + "\"本地联调员工\" 已存在，是覆盖还是另存副本？" 三按钮：`覆盖` / `另存副本` / 右上角 ×  |
| ML-12b | 选择"覆盖"                   | 点"覆盖"                               | Profile 原地更新；continue connect；toast 成功；SMCP 连接页仍显示同 profile 名已连                         |
| ML-12c | 选择"另存副本"                | 再触发一次，点"另存副本"                 | SMCP 服务器页 Profile 列表新增 `本地联调员工 (2)`；当前连接指向副本                                         |
| ML-12d | 点 × 取消                   | 再触发一次，点 Modal 右上角 ×            | 不建不连；无新 Profile；Console: 无 `manager: profile saved` 行                                            |

### P1 — 登出清理

| #     | 用例              | 做什么                                | 应该看到                                                                                                                          |
|------|-------------------|--------------------------------------|---------------------------------------------------------------------------------------------------------------------------------|
| ML-13 | 退出登录          | 员工列表页右上角点"退出登录"           | UI 回登录页；若 ML-08 连过，Console 先 `manager: logout disconnect ...` 再 `manager: logout ok`；SMCP 服务器页连接状态变为 Disconnected |
| ML-14 | 登出后状态干净     | 重新打开客户端（或重登录前）           | 登录表单字段为空；无残留员工数据                                                                                                    |

### P1 — i18n 双语切换

| #     | 用例       | 做什么                        | 应该看到                                                                             |
|------|-----------|------------------------------|-------------------------------------------------------------------------------------|
| ML-15 | 英文切换    | 右上角点 `EN`                 | Manager 账号模块所有文案切英文：Sign in to TFRSManager / Phone / Password / Digital Employees / Connect / Overwrite / Save a copy / Renew subscription。无漏翻（Console 无 `missingKey` warning） |

### 【BLOCKED】等待 TFRSManager 提供 Mock 能力

| #     | 用例        | 状态    | 解锁条件                                                          |
|------|------------|--------|-----------------------------------------------------------------|
| ML-F1 | 402 欠费熔断 | ⏸️ BLOCKED | Manager 把 freezeChecker 接入 connection-info 返回 `HTTP 402 + {message, redirectUrl?}` |
| ML-G1 | 403 权限不足 | ⏸️ BLOCKED | 需要 Manager 提供无权限账号或 mock flag                              |
| ML-H1 | 401 JWT 失效 | ⏸️ BLOCKED | 需要 Manager 提供 dev 端点 revoke JWT 或可配 TTL                      |

## 清理

| #     | 用例          | 做什么                                                                | 说明                                                |
|------|--------------|----------------------------------------------------------------------|---------------------------------------------------|
| ML-99a | 回到干净登录态  | 执行 ML-13 登出                                                       | 保证下一轮 UAT 从 fresh 状态开始                     |
| ML-99b | 清理测试期 Profile | 侧栏"SMCP 服务器" → 删除 ML-12c 产生的 `本地联调员工 (2)`（可选）     | 覆盖型副本名不影响下一轮，可选                        |
| ML-99c | Manager keychain 残留清理（仅诊断时需要） | `security delete-generic-password -s tfrobot-client -a "manager_jwt:<hash>"` | 正常流程不需要，ML-13 已走 `manager_logout` 会清掉   |

## 关键断言点（Claude 必须主动校验）

1. **脱敏硬校验**（ML-09）：用户贴回 Console 日志后，Claude 必须用 regex 验证不含 64
   字符 hex 串 `[a-f0-9]{64}`，且 `access_token` 值是 `"***"`。这是用户在本项目开发过程中
   特别点明的安全要求。
2. **templateType 枚举**（ML-06）：列表里看到的必须是 `tfrserver` / `tfropenclaw`，不是旧
   的 `tfrobot`/`openclaw`。如果看到旧字符串，说明 Manager 端回退了，要 cross-ask-tf。
3. **id 类型**（ML-08）：客户端传给 `manager_get_connection_info` 的 `id` 必须是数字。
   如果 Rust 日志里看到 `id="11"` 带引号，说明 Tauri command 签名没更新。

## 截图建议（附到 UAT 报告）

- ML-04 登录成功后的员工列表页（作为金标准快照）
- ML-08 SMCP 连接成功后的 SMCP 服务器页 Descriptions（office/url/computer）
- ML-12a Popconfirm Modal 截图

## 失败时的 Bug 分流映射

| 症状                                              | 最可能归属                  |
|--------------------------------------------------|----------------------------|
| ML-01/02/03 文案错或红色 Alert 不出现               | tfrobot-client 前端 i18n / LoginForm |
| ML-04 登录通过但跳到空白页、或 Console 报 `invalid_response` | tfrobot-client Rust DTO 和 Manager 不对齐 |
| ML-08 Console 看不到 `connection-info ok`          | Manager /connection-info 契约异常，curl 验证后报 cross-ask |
| ML-09 日志里有明文 token                           | tfrobot-client 前端 `redactForLog` 实现 bug — 立刻修 |
| ML-11 server 看不到 `auth succeeded`               | TFRobotServer 侧（access_token 校验 / 路由），贴 `tfrobot_api.log` |
| ML-12 Popconfirm 行为异常                          | tfrobot-client 前端 `EmployeeList.resolveNameConflict` |

## 维护说明

本场景用例编排在 `2026-04-24` 对齐 UAT guide v1 版本。若 Manager 响应字段形态发生变化
（如 `DigitalEmployeeResponse` 加了新字段），走 `/uat-scenario update
manager-login-and-connect ...` 更新本文档。F/G/H 变成非 BLOCKED 后也走 update 流程。
