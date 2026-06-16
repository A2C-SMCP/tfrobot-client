# UAT Seed 数据参考

> 所有 Seed 数据由 TFRSManager 后端幂等初始化。本文档只映射**tfrobot-client UAT 会消费的子集**。
> 权威来源：`$HOME/GolandProjects/tfrsmanager/docs/local-dev/client-uat-guide.md` §3。
> 如需追加/修改 seed 数据，走 `/uat-scenario create|update` 并生成 seed-request 报告。
>
> **Seed 入口**：tfrsmanager `cmd/seed/main.go`，通过 `make local-run-init-debug` 自动执行（仅 local 模式，user-service 在 :8090）。

---

## 登录凭据汇总

| 手机号        | accountName            | 密码         | 特性                     | 对应 UAT 场景 |
|--------------|------------------------|--------------|--------------------------|--------------|
| 13800138000  | testuser（主账号）      | Test@123456  | 3 个 running + 1 init_failed | 场景 A/C     |
| 13800138008  | `client_uat`            | Test@123456  | 1 个 running（唯一真连通实例）| 场景 A/C/**E** |
| 13800138009  | clientUATEmpty 相关     | Test@123456  | 0 个数字员工              | 场景 D（空态）|
| 13900139000  | testuser2（多账户）     | Test@123456  | 返回 `AccountSelectionResponse`（企业+个人双账户）| 场景 B（多账户）|

> **关键约束**：只有 `13800138008` 的数字员工配了真实 `accessToken + Cluster.Domain = 127.0.0.1:8443`，
> 可以端到端连通 TFRobotServer。其他账号的 connection-info 也会返回，但 SMCP 连接不会成功。

---

## 13800138008 / client_uat 的数字员工（场景 E 的金标准）

| 字段                       | 值                                           |
|---------------------------|---------------------------------------------|
| id（Manager 数字主键）     | 11（可能随 seed 执行时序不同，以实际为准）     |
| name                       | `本地联调员工`                                |
| robotId                    | `5f4b3b3b-3b3b-3b3b-3b3`                     |
| namespace                  | `tfrobotserver`                              |
| templateType               | `tfrserver`                                  |
| templateDisplayName        | `智能客服机器人`                              |
| status                     | `running`                                    |
| clusterName                | `local-tfrobotserver`                        |

connection-info 返回（见 UAT guide §5.5）：

```jsonc
{
  "code": 200, "message": "success",
  "data": {
    "socketBaseURL": "https://127.0.0.1:8443",
    "sioPath": "/socket.io/",
    "namespace": "tfrobotserver",
    "rid": "5f4b3b3b-3b3b-3b3b-3b3",
    "robotType": "tfrobot",
    "smcpNamespace": "/smcp",
    "accessToken": "ac4a30aed11de510a388dd05a0ea26cc41b3ac87c6c1f6ff7c267d823611756c",
    "computerName": "本地联调员工",
    "routingHeaders": {
      "X-TF-Namespace": "tfrobotserver",
      "X-TF-RobotId": "5f4b3b3b-3b3b-3b3b-3b3",
      "X-TF-RobotType": "tfrobot",
      "access_token": "ac4a30aed11de510a388dd05a0ea26cc41b3ac87c6c1f6ff7c267d823611756c"
    }
  }
}
```

**脱敏断言的金标准**：真值 `accessToken` 长度 64 字符，以 `ac4a30ae` 开头。UAT 中
DevTools Console 的 `manager: connection-info ok for ...` 日志里必须**不含**该字符串片段，
`routingHeaders.access_token` 必须显示为 `"***"`。

---

## 13900139000 / testuser2 的两个账户（场景 B）

多账户登录响应 `.data.accounts`：

| accountId | accountName             | organizationType | organizationName     |
|----------|-------------------------|------------------|----------------------|
| 2        | testuser2_enterprise    | enterprise       | 测试企业              |
| 3        | testuser2_personal      | personal         | one-person-org-1     |

`tempToken` TTL 300s；一次 select-account 后 tempToken 作废。

---

## 空列表账号（场景 D）

13800138009：登录 token 正常返回；`list_digital_employees` 返回
`{total:0, page:1, pageSize:20, items:[]}`（注意 `items` 是空数组**不是 null**）。

---

## 部门可见性 seed（TFRM-56）

> 状态：🟢 **已实施** —— 后端 commit `137de75` @ develop，`make seed-local` + SQL 核对全部通过（部门树/多部门/未分配/兄弟不可见/client_uat 全员）。
> `scenarios/department-visibility.md` 的 DV-01/02/03/08 可转 active（账号映射见下表 + 注）。

为 TFRM-56「部门展示 + staleness/离线兜底 + 可见性失效」准备，**加性**（不影响场景 A–E）。
**TFRM-170 方案2**：个人组织账户自动入根部门「全员」，富部门场景只能落企业组织——故 seed 分两块：

| 类别 | 账号（组织类型） | 内容 | flag 依赖 |
|------|----------------|------|----------|
| A 个人「全员」 | client_uat 13800138008（个人） | 本地联调员工(id 11)→根部门「全员」→ 面包屑 `全员`（1 级） | 无关 |
| B 企业富树 | testuser2_enterprise（13900139000→选 accountId 2 / 测试企业） | 部门树 总公司→{研发中心→平台组, 销售部→华东区}；viewer 归属平台组；机器人：多部门(平台组+华东区)、未分配、兄弟不可见 R(华东区) | DV-02/03 off；DV-08 **on** |

- **类别 A + B 均已 seed 验证**（commit `137de75`）。企业富树 viewer = `testuser2_enterprise`（**accountId 2**，登录 13900139000→多账户选企业账户）@ **平台组**。3 机器人实例（list-only，无需端到端连通）：
  - `DV多部门机器人`（CRName `seed-dv-multi`）→ 平台组 **+** 华东区 → **DV-02** 两行面包屑
  - `DV未分配机器人`（CRName `seed-dv-unassigned`）→ 无部门 → **DV-03** `departments[]=[]`（⚠️ 恒生效下对非 admin viewer 不可见；DV-03 已降级代码级，见 scenario）
  - `销售部机器人`（CRName `seed-dv-sales`）→ 华东区 → **DV-08** ⚠️ 恒生效下对平台组 viewer 永不可见（不进列表），需改用「起始平台组机器人 + 调岗」触发，见 scenario
- **DV-01** 用 client_uat（13800138008）验 `本地联调员工` 面包屑 = `全员`（个人组织 1 级）。

- `departments[]` 元素形态：`{id, name, path, ancestors:[{id,name}...]}`，`ancestors` 含自身、根→叶有序，空=`[]` 非 null（契约见 TFRM-167/168 评论）。
- 面包屑 = `ancestors.map(name).join(' / ')`。客户端不依赖部门 id，只读 `ancestors[].name`。
- ⚠️ **`DEPT_VISIBILITY_FILTER_ENABLED` 已被 TFRM-174 删除**——可见性恒生效，行为由账号 `data_scope` 决定。无「翻 flag」手法：DV-08 改靠真实**调岗**构造不可见（管理员把机器人移出 viewer 子树）；DV-03「未分配」在 TFRM-53 NOT NULL 后无法用 seed 构造，降级为前端单测兜底。详见 `scenarios/department-visibility.md`。

## 未覆盖场景（blocked）

| UAT 场景 | 缺什么                                  | 解锁条件                    |
|---------|----------------------------------------|---------------------------|
| F 402 欠费 | connection-info 返回 HTTP 402 + `{message, redirectUrl?}` | TFRSManager 把 freezeChecker 接入 connection-info 路径 |
| G 403 权限 | 无权限账号访问某员工的 connection-info | 需要角色/RBAC 专用账号       |
| H 401 失效 | dev 端点手动 revoke JWT 或缩短 TTL       | Manager 提供 dev-only token revoke 端点 |

三者都不是 tfrobot-client 侧能自己构造的，需要 Manager 后端配合。排期用
`/uat-scenario update manager-login-and-connect 增加 F/G/H` 发起新一轮 seed-request。
