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

全部加性、向后兼容；`DEPT_VISIBILITY_FILTER_ENABLED` 三环境默认 off 时场景 A–E 字节级不变。

## 需求清单

### 类别 A：个人组织「全员」（验 1 级面包屑）— 后端已写待提交

| 机器人 | 账号 | 部门归属 | 期望面包屑 | 备注 |
|--------|------|---------|-----------|------|
| 本地联调员工（id 11 / robotId `5f4b…`） | client_uat（13800138008，**个人组织**） | 系统根「全员」（方案2 自动入） | `全员` | 保持场景 E 端到端连通不变；服务于 DV-01 |

- 幂等：robot Account 入「全员」按 `(accountId, 全员deptId)` 去重。

### 类别 B：企业组织富数据（验多部门 / 未分配 / 兄弟不可见）— 待后端确认后建

**viewer = `testuser2_enterprise`**（`13900139000` 多账户登录 → 选 accountId 2 / org 测试企业）

**B-1 部门树**（测试企业内，深度 ≤5）：

| 部门 | 父 | 用途 |
|------|----|----|
| 总公司 | (root) | 面包屑根 |
| 研发中心 | 总公司 | 中间层 |
| 平台组 | 研发中心 | **viewer 归属部门** + 机器人归属（叶） |
| 销售部 | 总公司 | 兄弟子树 |
| 华东区 | 销售部 | 兄弟子树叶（放兄弟不可见机器人） |

**B-2 viewer 归属**：`testuser2_enterprise` 账号归属 **平台组**（决定 flag-on 可见范围 = 平台组 + 子孙）。

**B-3 机器人**（按 DV 用例所需，均可 list-only）：

| 机器人 | 部门归属 | 服务用例 | 期望 | flag |
|--------|---------|---------|------|------|
| 企业机器人-多部门 | 平台组 **+** 华东区 | DV-02 | 两行面包屑（其一为 3 级 `总公司/研发中心/平台组`） | off |
| 企业机器人-未分配 | （无任何部门） | DV-03 | 「未分配部门」（`departments:[]`） | off |
| 企业机器人-兄弟不可见 R | 华东区 | DV-08 | flag-on 时对 viewer(平台组) 不可见 → connection-info 回 404+errorCode | **on** |

- 均 **list-only**：不需要真实 `accessToken` / Cluster.Domain（DV-02/03 只读面包屑；DV-08 只需 connection-info 在不可见时回 **404 + 顶层 `errorCode:"ERR_NOT_FOUND_OR_NO_PERMISSION"`**）。
- 幂等：机器人按 `(orgId, name)` 去重；归属按 `(robotAccountId, departmentId)` 去重。

## ⚠️ 待后端确认（这些值用来写准 DV 步骤，确认前 DV-02/03/08 标 BLOCKED）

1. **部门树命名**：是否就用 `总公司 / 研发中心 / 平台组 / 销售部 / 华东区`？（客户端按此写期望面包屑文本；命名不同请给最终名）
2. **viewer 归属**：`testuser2_enterprise` 是否归属 `平台组`？（决定 flag-on 可见范围与 DV-08 不可见判定）
3. **企业机器人清单**：B-3 三台（多部门 / 未分配 / 兄弟不可见 R）是否都会 seed？各自部门归属确认？
4. **连通性**：企业机器人是否可全部 list-only（不配 cluster/accessToken）？DV-02/03 只读面包屑、DV-08 只需 404+errorCode，均不要求端到端连通。
5. **登录便利（可选）**：用 `testuser2_enterprise`（经多账户登录 13900139000 → 选 accountId 2）即可，还是你们更想 seed 一个**独立单账户企业 UAT 手机号**？（纯便利，二者皆可）

## flag 矩阵

| 验收场景 | `DEPT_VISIBILITY_FILTER_ENABLED` | 账号 | seed |
|---------|----------------------------------|------|------|
| DV-01 个人「全员」面包屑 | off（默认） | client_uat | 类别 A |
| DV-02/03 企业富面包屑 | **off**（让多部门/未分配机器人都在列表可见，便于读面包屑） | testuser2_enterprise | 类别 B |
| DV-04~07 staleness/离线 | 无关 | 任意 | 无 |
| DV-08 兄弟不可见 404 | **on**（建议 staging 先开） | testuser2_enterprise | 类别 B |

## 验收方式

- **DV-01**：client_uat 登录 → 本地联调员工"所属部门" = `全员`
- **DV-02/03**：testuser2_enterprise 登录（flag-off）→ 多部门机器人两行 / 未分配机器人显「未分配部门」
- **DV-08**：testuser2_enterprise 登录（flag-off 时列表含 R）→ 翻 flag on → 点 R 连接 → toast「…已不可访问，已从列表中移除」+ 剔除

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
