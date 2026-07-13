---
name: requirement-review
description: 评审功能需求、技术需求、重构方案、Jira/GitHub Issue 草稿以及 SDK/client 边界设计，检查产品目标是否清晰、是否符合最佳实践、是否匹配现有架构、职责划分是否明确、是否可测试、是否具备可实施性。当需要判断需求是否范围合理、是否符合项目架构，或 A2C-SMCP SDK 相关需求是否在 tfrobot-client 与 rust-sdk 之间正确分层时使用。
---

# 需求评审

使用本 skill 在实施或创建工单前评审需求。重点判断需求是否自洽、是否归属正确层级、是否符合现有架构，以及是否具体到工程师无需猜测即可实现。

## 评审流程

1. 先基于真实上下文评审。可用时检查相关代码、已有 issue、计划文档或设计说明。
2. 判断需求类型：Feature、Task、Bug、Refactor，或跨项目 SDK/client 变更。
3. 判断责任归属：client、SDK、后端、前端、operator，或多项目协作。
4. 按下方清单逐项评审需求。
5. 先输出问题，按严重程度排序：
   - Blocker：实施或创建工单前必须修正。
   - Major：很可能造成返工、职责不清或架构漂移。
   - Minor：措辞、验收标准或边界场景需要改进。
6. 最后给出明确建议：接受、修订、拆分、转移到其他项目，或因缺少关键信息暂缓。

不要默认接受用户提出的技术方案。必须判断它是否属于正确层级、是否是合适抽象、是否符合长期边界。

## 通用检查清单

每个需求都要检查：

- 目标清晰：说明谁需要什么能力，以及为什么需要。
- 范围边界：明确哪些在范围内，哪些不在范围内。
- 责任归属：每项改动都能映射到正确项目或模块。
- 架构一致性：遵守现有模块边界，避免重复已有能力。
- 数据所有权：明确哪个系统是 source of truth。
- API 形态：避免把产品特定概念泄漏到通用层。
- 持久化边界：说明什么数据保存在哪里，以及为什么。
- 运行时行为：定义生命周期、同步、事件和失败处理。
- 迁移方案：配置所有权变化时覆盖已有数据、回滚和降级风险。
- 验收标准：包含具体、可验证的完成条件。
- 可测试性：按需列出单元、集成、UI/API 测试场景。
- 运行风险：考虑兼容性、数据丢失、安全性和可观测性。

## 架构一致性

评审设计时确认：

- 每个 source of truth 只有一个所有者。除非有明确迁移窗口和清理方案，否则避免双写或影子配置。
- 不混淆运行时状态和持久化配置。
- UI 便利字段不下沉到低层库。
- 跨项目需求按拥有仓库拆分，不打包成一个实现任务。
- 通用 SDK API 不引入产品特定命名、流程、鉴权假设或业务实体。
- client 特定流程不强迫 SDK 改 schema，除非多个消费方都会受益。
- 正常运行时状态同步优先使用事件驱动。若出现轮询，必须要求说明理由。

## A2C-SMCP SDK/client 边界评审

当需求涉及 `a2c-smcp`、`rust-sdk`、`smcp-computer` 或 SDK/client 职责划分时，必须使用本节。

先给出明确结论：该需求归 SDK、归 client，还是必须拆成 SDK 子任务和 client 子任务。不要只说“需要两边配合”。

### 职责边界

默认边界如下，不符合时必须在 Findings 中说明原因。

- `rust-sdk`/A2C-SMCP SDK 只管理可被多个 client 复用的通用 Computer 能力：
  - MCP servers
  - inputs
  - input values
  - skill home
  - marketplace/plugin governance
  - tool/runtime lifecycle
  - 通用 config import/export，不包含产品语义字段
  - 通用 runtime/config/status events
  - Computer 配置的 schema 校验、读写、删除、迁移和兼容
  - 不依赖 tfrobot-client UI、Manager、robot/employee、token exchange 的能力
- `tfrobot-client` 只管理产品语义、用户体验和业务绑定：
  - 本地 profile 元数据，例如 `id`、`name`、`description`
  - `computer_instances.json` 作为 client 侧 Computer 管理/索引文件
  - connection target 选择
  - manual SMCP targets 和密钥
  - Manager session
  - robot/employee 选择
  - `connection_policy`
  - `robot_binding`
  - auto-connect 偏好和触发策略
  - token exchange 和 token refresh
  - 跨 Computer 防重复连接
  - UI 展示、表单默认值、向导流程、错误文案和产品级埋点

### 归属判定规则

- 若能力离开 `tfrobot-client` 后仍然有独立价值，且不包含产品身份、业务绑定或 UI 偏好，倾向归 SDK。
- 若能力依赖用户选择、产品工作流、Manager 会话、robot/employee、token 或 connection policy，归 client。
- 若 SDK 只需要暴露通用事件/API，client 再把事件解释成产品行为，则拆分：SDK 提供通用机制，client 实现业务策略。
- 若需求要求“SDK 帮 client 记住某个产品选择”，默认归 client；除非能证明这是所有 SDK 消费方共享的通用配置。
- 若需求要求“client 读取 SDK 内部文件来完成业务逻辑”，标记为 Blocker，要求改为 SDK API/event。
- 若需求把 client 字段塞进 SDK import/export、ledger、config 或 runtime state，标记为 Blocker。

面向 SDK 的需求必须说明：

- 为什么该能力足够通用，应该归 SDK 管。
- 哪些 client-owned 字段必须排除在 SDK 持久化之外。
- SDK API 会持久化什么，不会持久化什么。
- create/open/load/update/delete/import/export 的行为边界。
- 配置变更后 runtime 如何同步。
- client 如何观察 status/config 变化，而不读取 SDK 内部文件。
- 哪些测试用于保护职责边界。

面向 client 的需求必须说明：

- 哪些字段只存在于 client 侧索引或 profile。
- client 如何调用 SDK 的通用 API，而不是绕过 API 读取 SDK 内部文件。
- client 如何将 SDK 的通用 status/config event 映射为产品状态。
- token、secret、Manager session、robot/employee 选择和 connection policy 的所有权。
- 跨 Computer 连接去重、自动连接和失败恢复的产品策略。
- 哪些测试证明 client 没有把产品语义下沉到 SDK。

以下情况一律标记为 Blocker：

- SDK 需求要求 `rust-sdk` 持久化产品特定字段，例如 `name`、`description`、`connection_policy`、`robot_binding`、Manager session、manual target secrets、robot 选择策略或 auto-connect 偏好。
- client 需求要求直接读写 SDK 内部 ledger/config 文件，而不是使用 SDK API。
- 一个工单同时修改 SDK schema 和 client 业务流程，但没有拆分职责、依赖顺序和验收边界。
- 将 `computer_instances.json` 迁移到 SDK 管理，却仍保留 client 侧 source of truth，形成双写。
- SDK API 使用 tfrobot-client 的产品命名、鉴权假设、Manager 概念或 robot/employee 实体。

推荐拆分形态：

- SDK 子任务：定义通用 schema/API/event、迁移、兼容、单元测试和 SDK 级集成测试。
- Client 子任务：定义 profile/index 结构、产品策略、UI/流程、token/secret 管理和端到端验收。
- 跨项目验收：证明 SDK 可以被非 tfrobot-client 消费方使用，且 client 不读取 SDK 内部文件。

## 输出格式

使用以下结构：

```markdown
## Findings

- **Blocker/Major/Minor**: [问题]
  Evidence: [文件/需求原文/上下文]
  Impact: [为什么重要]
  Recommendation: [具体修改建议]

## Recommended Shape

[简短改写后的需求、拆分方案或职责归属决策。]

## Acceptance Criteria Gaps

- [缺失的可验证条件]

## Residual Risks

- [修订后仍然存在的风险]
```

如果没有发现问题，直接说明没有发现阻塞项，同时列出剩余测试或发布风险。

## 质量门槛

出现以下情况时，要求修订或阻止继续推进：

- 需求混合 SDK 和 client 职责，但没有明确边界。
- API 命名围绕单个 client 产品，而不是通用领域概念。
- 持久化所有权不清楚。
- 配置所有权变化缺少已有数据迁移方案。
- 验收标准无法客观验证。
- 设计要求 client 读取 SDK 内部 ledger/config 文件。
- 缺少业务规则，导致实现方必须猜测。
