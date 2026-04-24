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

## 未覆盖场景（blocked）

| UAT 场景 | 缺什么                                  | 解锁条件                    |
|---------|----------------------------------------|---------------------------|
| F 402 欠费 | connection-info 返回 HTTP 402 + `{message, redirectUrl?}` | TFRSManager 把 freezeChecker 接入 connection-info 路径 |
| G 403 权限 | 无权限账号访问某员工的 connection-info | 需要角色/RBAC 专用账号       |
| H 401 失效 | dev 端点手动 revoke JWT 或缩短 TTL       | Manager 提供 dev-only token revoke 端点 |

三者都不是 tfrobot-client 侧能自己构造的，需要 Manager 后端配合。排期用
`/uat-scenario update manager-login-and-connect 增加 F/G/H` 发起新一轮 seed-request。
