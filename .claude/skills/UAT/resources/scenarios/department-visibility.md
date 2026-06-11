# UAT 场景：部门可见性适配（面包屑 + staleness/离线兜底 + 可见性失效）

**关联 Jira**：[TFRM-56](https://turingfocus.atlassian.net/browse/TFRM-56)（父 Epic TFRM-47；后端契约 TFRM-167 / TFRM-168）
**对应上游 guide**：tfrsmanager client-uat-guide §10.3
**用例编号前缀**：`DV-`（Department Visibility）
**Seed 依赖**：`../seed-requests/seed-request-department-visibility.md`（🟡 待实施 → 落地后把 BLOCKED 用例转 active）

## 测试目标

验证客户端：① 数字员工列表展示「所属部门」面包屑（含多部门 / 未分配兜底）；② 进入列表页
60s staleness 兜底刷新；③ 离线保留最近一次列表 + 恢复在线自动校准；④ 收到 404 +
`ERR_NOT_FOUND_OR_NO_PERMISSION` 时剔除不可见项 + refetch + toast。

> 范围说明：可见性**过滤**由后端完成，客户端只负责**展示与兜底**（lean MVP，不做人员通讯录，
> 那是 TFRM-55 FrontPortal 的范围）。

## 前置条件

- 进程：见 `../../environment-checks.md`（**1420 / 8090 必需**；8080/8443 本场景不要求端到端连）
- Seed：`seed-request-department-visibility` 已落地（🟡 待实施），见 `../seed-data.md §部门可见性 seed`
- 客户端已用含 `departments` DTO 的新版本重编译（改过 `manager_client.rs` 要重启 `pnpm tauri dev`）
- DevTools Console 已打开

## 页面入口

客户端左侧导航：**连接** → **Manager 账号**

## 测试用例

> 📌 **组织类型差异（TFRM-170 方案2）**：个人组织所有账户自动入根部门「全员」，机器人面包屑恒为
> `全员`（1 级）；多部门 / 未分配 / 兄弟不可见**只能在企业组织**。故 DV-01 用个人账号验「全员」，
> DV-02/03/08 用企业账号（`testuser2_enterprise`）验富场景。

### P0 — 个人组织面包屑（flag 无关；依赖 seed 类别 A）

#### DV-01: 个人组织「全员」面包屑 ⏸️(BLOCKED on seed A)

- **做什么**：用 client_uat（个人组织）登录，观察"本地联调员工"行。
- **复制用**：
  - Manager 地址：`http://localhost:8090`
  - 手机号：`13800138008`
  - 密码：`Test@123456`
- **应该看到**：
  - UI：该行"所属部门"显示 `全员`（个人组织 1 级面包屑，方案2 形态）
  - Console：`manager: fetched 1 digital employees`
- **失败时贴什么回来**：贴该行截图 + Console `manager:` 行；若面包屑空，把 `curl /api/v1/digital-employees` 的 `.data.items[].departments` 贴回（应为 `[{name:"全员",...}]`）。

### P0 — 企业组织富面包屑（flag-off；依赖 seed 类别 B）

> 前置：登出后用 **testuser2_enterprise** 登录 —— `13900139000` / `Test@123456` →「选择账号」页选 **enterprise（测试企业，accountId 2）**。

#### DV-02: 企业多部门两行（含 3 级链）⏸️(BLOCKED on seed B)

- **做什么**：在测试企业列表里，找挂了多部门的那台机器人。
- **应该看到**：UI 该机器人"所属部门"显示**两行**面包屑（如 `总公司 / 研发中心 / 平台组` + `总公司 / 销售部 / 华东区`）。← 顺带覆盖 3 级链渲染。
- **失败时贴什么回来**：截图 + `curl` 的 `departments` 数组（应有 2 个元素，各自 ancestors 根→叶有序）。

#### DV-03: 企业未分配部门兜底 ⏸️(BLOCKED on seed B)

- **做什么**：同测试企业列表里，找未归属任何部门的机器人。
- **应该看到**：UI "所属部门"显示 `未分配部门`（英文 `Unassigned`），不是空白、不报错。
- **失败时贴什么回来**：截图；若显示空白，贴该项 `departments` 字段（应为 `[]`）。

> 部门树命名 / 各机器人归属以 `seed-request-department-visibility.md §类别B` 后端最终确认为准（见该文件「待后端确认」5 问）。

### P0 — staleness 兜底刷新（无需 seed，用现有数据即可）

#### DV-04: 60s 内复用不重拉

- **做什么**：client_uat 登录进列表后，**立刻**切到别的侧栏页（如"SMCP 服务器"）再切回"Manager 账号"。
- **应该看到**：Console **不**出现新的 `manager: fetched ...` 行（60s 内复用缓存）；列表照常显示。
- **失败时贴什么回来**：把切换前后 Console 里所有 `manager:` 行贴回（Claude 据此判定是否多打了一次 fetched）。

#### DV-05: 超 60s 强制刷新

- **做什么**：在列表页停留 **>60s** 后，切走再切回（或等 >60s 再进列表页）。
- **应该看到**：Console 再次出现 `manager: fetched N digital employees`。
- **失败时贴什么回来**：贴 Console `manager:` 行 + 大致停留时长。

### P0 — 离线兜底 + 恢复校准（无需 seed）

> ⚠️ 两种"离线"要分清：
> - **停 Manager 进程**（DV-06）：网卡仍在线 → `navigator.onLine` 不翻转 → 走 fetch 的 `network_error` 分支置离线；恢复要**手动刷新**或重进页面。
> - **关网卡 / 断 Wi-Fi**（DV-07）：`navigator.onLine` 翻转 → `offline`/`online` window 事件触发 → 恢复时**自动**校准。

#### DV-06: 停 Manager → 离线横幅 + 保留列表

- **做什么**：在列表页，停掉 Manager（`Ctrl-C` 掉 :8090 进程），然后点页面右上角"刷新"。
- **应该看到**：
  - UI：顶部黄色横幅"离线模式——正在展示最近一次加载的列表，恢复网络后将自动刷新。"；**列表仍显示**（未清空）。
  - Console：`manager: list_employees failed, kind=network_error`
- **失败时贴什么回来**：截图（横幅 + 列表是否还在）+ Console `manager:` 行。
- **恢复**：重启 Manager（`make local-run-init-debug`）→ 点"刷新"→ 横幅消失、Console 出现 `manager: fetched N digital employees`。

#### DV-07: 关 Wi-Fi → 自动校准（网卡级离线）

- **做什么**：列表页停留时，关闭 Wi-Fi/网卡 → 稍候再打开。
- **应该看到**：
  - 关闭后：顶部出现离线横幅。
  - 打开后：横幅消失，且**无需手动刷新**，Console 依次出现 `manager: back online, recalibrating employee list` + `manager: fetched N digital employees`。
- **失败时贴什么回来**：贴关/开网卡前后 Console `manager:` 行。

### P1 — 可见性失效（企业账号 + flag-on；依赖 seed 类别 B）

#### DV-08: 翻 flag 后连接触发剔除 ⏸️(BLOCKED on seed B + 需 flag-on)

- **前置**：用 **testuser2_enterprise** 登录（viewer 归属平台组）。`DEPT_VISIBILITY_FILTER_ENABLED=off` 时列表含华东区的"兄弟不可见机器人 R"。运维把 flag 置 **on**（R 落在 viewer 平台组子树外 → 变不可见）。在 60s staleness 窗口内操作。
- **做什么**：在列表里点 R 的"连接"。
- **应该看到**：
  - UI：右上角 warning toast「`<R 的名字>` 已不可访问，已从列表中移除。」；R 从列表消失。
  - Console 依次：`manager: select_employee_and_connect failed, kind=not_found_or_no_permission` → `manager: employee <id> no longer visible, removing from local list` → `manager: fetched N digital employees`（校准）。
- **失败时贴什么回来**：贴 Console `manager:` 行；若 toast 没出/项没剔除，对 R 的 id 跑一次 `curl .../digital-employees/<id>/connection-info` 把响应贴回（确认后端是否真回 404 + `errorCode`）。

## 关键断言点（Claude 必须主动校验）

1. **面包屑形态**：`ancestors` 根→叶有序、join `" / "`。DV-01（个人组织 client_uat）= `全员`（1 级）；DV-02（企业多部门）每条链根→叶（如 `总公司 / 研发中心 / 平台组`）。顺序反了或分隔符错 → 前端 `formatDeptBreadcrumb` bug。
2. **空归属兜底**（DV-03）：`departments: []` → 显示"未分配部门"/`Unassigned`，不得空白或报错。
3. **errorCode 解析**（DV-08）：Console 必须是 `kind=not_found_or_no_permission`（**不是** `kind=not_found`）。若是 `not_found`，说明 Rust `extract_error_code` 没命中 **或** Manager 没回 `errorCode` —— curl 验证后定归属。
4. **离线保留**（DV-06）：网络错误后 `employees` 列表长度不变（未被清空）。

## 失败时的 Bug 分流映射

| 症状 | 归属 |
|------|------|
| 面包屑顺序错 / 格式错（分隔符、缺级） | tfrobot-client 前端 `formatDeptBreadcrumb` |
| `departments` 缺失 / 为 null（面包屑全空 or Console `invalid_response`） | Rust DTO 或 Manager 契约 —— curl `/digital-employees` 验 |
| DV-08 Console 是 `kind=not_found`（非 `not_found_or_no_permission`） | Rust `extract_error_code` 或 Manager 未回 `errorCode` |
| toast 未出 / 项未剔除，但 Console 有 `not_found_or_no_permission` | tfrobot-client 前端 `EmployeeList` / `managerStore` |
| curl `/digital-employees` 的 `departments` 本身就空 | TFRSManager seed 未落部门归属 → 催 seed-request |
| 关网卡恢复后不自动刷新（DV-07） | 前端 online/offline 监听 or `setOnline` 校准逻辑 |

## 清理

| # | 用例 | 做什么 | 说明 |
|---|------|--------|------|
| DV-99a | 回干净登录态 | 登出 | 下一轮 UAT 从 fresh 开始 |
| DV-99b | flag 复位 | 若 DV-08 翻过 flag，验收后运维把 `DEPT_VISIBILITY_FILTER_ENABLED` 复位 off | 避免影响其他场景 |

## 维护说明

本场景于 `2026-06-11` 对齐 TFRM-167 / TFRM-168 契约（`departments[]` 含祖先链 + 404 `errorCode`）。
依赖 `seed-request-department-visibility`（🟡 待实施）。seed 落地后把 DV-01/02/03/08 从
⏸️ BLOCKED 转 active；字段形态变动走 `/uat-scenario update department-visibility ...`。
