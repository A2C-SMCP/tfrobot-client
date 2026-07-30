# Rust SDK 能力审查报告模板

## 结论摘要

- 总体结论：满足 / 部分满足 / 不满足 / 证据不足
- SDK 候选：`<ref>` @ `<full-sha>`
- client 基线：`<branch-or-head>` @ `<full-sha>`
- client 依赖状态：HEAD / index / worktree / lockfile
- 关键阻塞：

## 需求与验收矩阵

| ID | 可验证需求 | 成功标准 | 结论 | 证据等级 |
|---|---|---|---|---|

证据等级使用：`源码+实验`、`仅源码`、`仅实验`、`不足`。

## 版本基线

| 对象 | 来源/ref | 完整 SHA | 获取时间 | 工作区状态 |
|---|---|---|---|---|

说明客户端固定 rev 到候选 SHA 的 commit 距离、相关变更和无法确认的版本信息。

## 调用链与源码证据

按验收项列出：

```text
需求 → client command/service → adapter → SDK public API → implementation → observable
```

- client 证据：`<absolute-or-repo-relative-path>:<line>`
- SDK 证据：`<path-at-full-sha>:<line>`
- 解释：

## 实验执行记录

| 实验 | 版本 | 命令 | 样本/环境 | 退出码 | 关键结果 |
|---|---|---|---|---|---|

列出原始输出和实验文件位置，标明 mock、单机、小样本或平台限制。

## 确认的问题

### SDK-01 `<问题标题>`

- 严重性：Blocker / Major / Minor
- 判定：确认的 SDK 问题 / SDK 与 client 均需修改
- 违反的验收项：
- 源码证据：
- client 调用证据：
- 实验证据：
- 已排除因素：
- 用户影响：

如果没有满足完整证据链，不要放在本节，移入“未决问题与限制”。

## 期望修改方向

### SDK-01

- 公共契约：
- 行为与错误语义：
- 并发/取消/生命周期：
- 配置、迁移与兼容：
- SDK 测试：
- client 接入与测试：
- 临时替代方案及限制：

## Client 侧问题

列出 SDK 已满足但 client adapter、参数、错误映射、产品策略或 UI 未正确使用的能力，避免错误归因给 SDK。

## 未决问题与限制

- 证据不足项：
- 未覆盖平台或场景：
- 外部环境限制：
- 需要 SDK 维护者确认的契约：

## 实验产物

- 实验目录：
- 原始输出：
- 保留原因或清理状态：
