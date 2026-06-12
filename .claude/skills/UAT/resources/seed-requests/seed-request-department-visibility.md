# Seed 数据需求 — 部门可见性适配（TFRM-56）· v2（方案2 适配）

> 关联场景：`scenarios/department-visibility.md`
> 关联 Jira：TFRM-56（父 Epic TFRM-47）；后端契约 TFRM-167 / TFRM-168 / **TFRM-170**（均已交付 develop）
> 申请日期：2026-06-11（v2：因 TFRM-170「个人组织全员」策略调整账号映射）
> 状态：🟢 **已实施**（commit `137de75` @ develop，`make seed-local` + SQL 核对通过）。类别 A+B 全部就位。
> 归属：TFRSManager `cmd/seed/main.go`（`seedClientPortalUAT` + `seedDepartmentVisibilityData`）
> 最终账号映射：DV-01=client_uat(13800138008,面包屑「全员」)；DV-02/03/08=testuser2_enterprise(13900139000→accountId 2,平台组)。viewer/机器人/部门树详见 `../seed-data.md §部门可见性 seed`。

## 需求背景

TFRM-56 让客户端展示「所属部门」面包屑 + 处理可见性失效与离线兜底。**TFRM-170（方案2）**
规定：**个人组织所有账户自动入系统根部门「全员」**，富部门场景（多部门 / 未分配 / 兄弟不可见）
**只能在企业组织**。

因此本仓 UAT 的 seed 账号映射调整为：
- **个人组织**（client_uat / testuser）→ 机器人 `departments[]` 恒为 `[{name:"全员"}]`（1 级）→ 只验「全员」面包屑。
- **企业组织**（测试企业，viewer = `testuser2_enterprise` / accountId 2）→ 建富树，验多部门 / 未分配 / 兄弟不可见。

全部加性、向后兼容；本仓改动不影响既有场景 A–E（可见性现由 TFRM-174 恒生效，不再有 flag）。

## 需求清单

### 类别 A：个人组织「全员」（验 1 级面包屑）— 后端已写待提交

| 机器人 | 账号 | 部门归属 | 期望面包屑 | 备注 |
|--------|------|---------|-----------|------|
| 本地联调员工（id 11 / robotId `5f4b…`） | client_uat（13800138008，**个人组织**） | 系统根「全员」（方案2 自动入） | `全员` | 保持场景 E 端到端连通不变；服务于 DV-01 |

- 幂等：robot Account 入「全员」按 `(accountId, 全员deptId)` 去重。

### 类别 B：企业组织富数据（验多部门 / 未分配 / 兄弟不可见）— 🟢 已 seed（`137de75`）

**viewer = `testuser2_enterprise`**（`13900139000` 多账户登录 → 选 accountId 2 / org 测试企业）

**B-1 部门树**（测试企业内，深度 ≤5）：

| 部门 | 父 | 用途 |
|------|----|----|
| 总公司 | (root) | 面包屑根 |
| 研发中心 | 总公司 | 中间层 |
| 平台组 | 研发中心 | **viewer 归属部门** + 机器人归属（叶） |
| 销售部 | 总公司 | 兄弟子树 |
| 华东区 | 销售部 | 兄弟子树叶（放兄弟不可见机器人） |

**B-2 viewer 归属**：`testuser2_enterprise` 账号归属 **平台组** → 可见范围 = 平台组 + 子孙（可见性恒生效，无 flag）。

**B-3 机器人**（均 list-only，不配真实 `accessToken` / Cluster.Domain）：

| 机器人 | 部门归属 | 服务用例 | 期望 / 状态 |
|--------|---------|---------|------------|
| 企业机器人-多部门 | 平台组 **+** 华东区 | DV-02 | ✅ 两行面包屑（其一为 3 级 `总公司/研发中心/平台组`）；在平台组→对 viewer 可见 |
| 企业机器人-未分配 | （无任何部门） | DV-03 | ⚠️ 恒生效下对非 admin viewer 不可见 + TFRM-53 NOT NULL 后无未分配账户 → 无法用 seed 在 UI 验，降级前端单测 |
| 兄弟不可见 R（销售部机器人） | 华东区 | DV-08 | ⚠️ 静态在华东区 → 恒生效下永不进 viewer 列表，不能直接点；DV-08 需改用「起始在平台组的机器人 + 一次调岗」 |

- 幂等：机器人按 `(orgId, name)` 去重；归属按 `(robotAccountId, departmentId)` 去重。

## 后端确认 & seed 状态（🟢 已实施 · `137de75`）

部门树命名（总公司/研发中心/平台组/销售部/华东区）、viewer@平台组、三台企业机器人（list-only）均已按上表 seed。**两点遗留**（TFRM-53 NOT NULL + TFRM-174 删 flag 暴露）：

1. **DV-03 未分配**：无未分配账户可造、且对非 admin 不可见 → 降级为前端单测兜底，不在协同 UAT 跑。
2. **DV-08 调岗**：现有 R 静态在华东区不可用；需一台**起始在平台组**的机器人 + AdminPortal/Manager 调岗路径方可触发 404。待此二者就位转 active。

## 可见性生效方式（⚠️ flag 已删除 · TFRM-174）

`DEPT_VISIBILITY_FILTER_ENABLED` 已被 TFRM-174 删除——**可见性恒生效**，行为由账号 `data_scope`
决定（admin org-wide / 成员本子树）。原「翻 flag」式 UAT 手法不再适用，对各用例的影响：

| 验收场景 | 账号 / 视角 | seed | 状态 |
|---------|------------|------|------|
| DV-01 个人「全员」面包屑 | client_uat（个人组织） | 类别 A | ✅ active |
| DV-02 企业多部门面包屑 | testuser2_enterprise @ 平台组（机器人在平台组→可见） | 类别 B | ✅ active |
| DV-03 未分配兜底 | —— | —— | ⚠️ 降级代码级：TFRM-53 NOT NULL 后无未分配账户、且恒生效下未分配对非 admin 不可见，无法用 seed 在 UI 构造 |
| DV-04~07 staleness/离线 | 任意 | 无 | ✅ active |
| DV-08 调岗→404 | testuser2_enterprise @ 平台组 | 需 seed 调整 | ⏸️ 需「机器人起始可见 + 一次调岗」能力（见验收方式） |

## 验收方式

- **DV-01**：client_uat 登录 → 本地联调员工"所属部门" = `全员`
- **DV-02**：testuser2_enterprise 登录 → 多部门机器人显两行面包屑（其一为 3 级链）
- **DV-03**：⚠️ 无法用 seed 构造（见上表）；客户端 `[]→「未分配部门」`兜底由前端单测守护
- **DV-08**：viewer 列表含一台**起始在平台组**的机器人 R' → 管理员把 R' 调岗到华东区（viewer 子树外）→ 60s 内点 R' 连接 → connection-info 回 404+errorCode → toast + 剔除。⏸️ 待「起始可见机器人 + AdminPortal/Manager 调岗路径」就位

Manager 工程师 `make seed-local` 后 curl 自检（企业账号需先多账户登录拿 accountId 2 的 jwt）：
```bash
# 企业机器人列表应含多部门/未分配条目
curl -s http://localhost:8090/api/v1/digital-employees -H "Authorization: Bearer <jwt-acct2>" \
  | jq '.data.items[] | {name, departments}'
```

---

## 交接话术

```
📋 Seed 需求 v2 已写到 seed-requests/seed-request-department-visibility.md（方案2 适配）

类别 A（个人组织 client_uat 全员）：后端已写待提交，落地后解锁 DV-01。
类别 B（企业组织 测试企业富数据）：请确认 §「待后端确认」5 问，确认后 seed，解锁 DV-02/03/08。

依赖该 seed 的 DV 用例当前标 ⏸️ BLOCKED；落地 + 我把状态改 🟢 后转 active。
```
