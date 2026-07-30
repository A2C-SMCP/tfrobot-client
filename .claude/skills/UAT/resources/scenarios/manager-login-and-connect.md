# UAT 场景：Manager 登录 → 选中员工即连（已迁移）

本场景已迁移到 CTO 层 UAT，不在 tfrobot-client 项目内继续维护详细步骤。

| 原用例 | CTO 场景 | 迁移原因 |
|---|---|---|
| ML-01 ~ ML-15、ML-F1、ML-G1、ML-H1、ML-99 | `cto_assistant/.claude/skills/UAT/resources/scenarios/desktop-manager-smcp.md` | 覆盖桌面端登录 Manager、拉取数字员工、生成连接 profile、连接 TFRobotServer SMCP，属于独立客户端跨运行时链路 |

tfrobot-client 项目仅继续维护纯客户端组件、状态管理、i18n 与本地单元/组件测试。
