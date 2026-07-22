---
name: review-rust-sdk-capability
description: 审查 A2C-SMCP rust-sdk 的远端 develop 分支最新 commit 或用户指定候选版本是否满足 tfrobot-client 的具体功能、兼容性、生命周期、配置、连接或运行时需求。用于 SDK 升级前评估、SDK 能力确认、客户端需求依赖 SDK、怀疑 SDK 缺陷、需要向 rust-sdk 维护者提出修改方向，或必须结合 tfrobot-client 本地调用链与隔离实验验证 SDK 行为的场景。要求核对精确源码版本、追踪客户端真实引用、调用 turingfocus-toolkit:run-experiment 设计并执行经用户确认的实验，并区分 SDK 问题、客户端问题和证据不足。
---

# Rust SDK 能力审查

以源码、客户端调用链和真实实验三类证据评估 SDK。不要把文档声明、类型存在、编译通过或已有测试通过单独视为“满足需求”。

## 强制规则

- 将用户需求拆成可观察、可证伪的验收项；需求含糊到无法设计实验时先澄清。
- 用户未指定版本时，将候选版本定义为审查时远端 `develop` 分支的最新 commit；用户指定 branch、tag、commit 或 PR 时，以指定 ref 覆盖该默认值。始终记录查询时间、分支或 ref、完整 commit SHA。
- 同时记录客户端依赖在 `HEAD`、index、worktree、`src-tauri/Cargo.lock` 中的状态。工作区有改动时不得擅自选择其中一层，也不得覆盖用户改动。
- 审查 SDK 的精确候选 commit。不得用远端默认分支 `HEAD`、同级仓库当前分支、Cargo 缓存或本地旧 checkout 代替远端 `develop` 的最新 commit。
- 只通过 SDK 公共 API 判断 client 可用能力；内部存在但未导出的实现不算满足。
- 至少找到一处 tfrobot-client 真实调用点或明确证明当前尚无调用点。区分 SDK 行为与 client adapter、产品策略、UI 或 Manager 逻辑。
- 必须调用 `$turingfocus-toolkit:run-experiment` 完成实验设计。严格遵守其确认门控：先提交方案，只有用户明确同意执行后才创建实验文件和运行实验。
- 默认只审查，不修改 SDK、客户端业务代码、依赖清单或锁文件。用户另行要求修复时再进入对应实现流程。
- 无法访问远端 `develop` 时明确写“无法确认 develop 最新 commit”，报告已能验证的 commit；不得把本地 `develop` 或本地 HEAD 宣称为远端最新版本。

## 第一步：建立需求与版本基线

1. 把需求转换为能力矩阵，每项写明输入、预期输出或状态变化、失败语义、并发或生命周期约束、持久化边界和兼容要求。
2. 读取 `src-tauri/Cargo.toml`、`src-tauri/Cargo.lock` 与 `git status`。必要时分别使用 `git show HEAD:<path>`、`git show :<path>` 和工作区文件识别三层差异。
3. 从 Cargo 声明提取 SDK 仓库 URL、feature 和固定 rev；不要从记忆猜测。
4. 使用远端查询确定目标 ref 的完整 SHA。用户未指定版本时，直接查询 `refs/heads/develop`，不要从远端 symbolic `HEAD` 推断候选分支；若远端不存在 `develop`，停止把任何其他分支当作默认候选，并报告版本基线无法建立。
5. 检查同级 `../rust-sdk` 的 remote、HEAD 和 dirty 状态。只有 remote 匹配且源码精确对应目标 SHA 时才直接读取；否则使用临时的隔离 checkout。不要切换或清理 SDK 同事的工作区。
6. 记录四个标识：客户端基线、客户端工作区依赖、SDK 候选、二者 commit 距离或关键 diff。

## 第二步：追踪客户端真实依赖

先搜索再阅读，不依赖固定文件列表。以这些位置作为初始锚点：

- `src-tauri/src/services/computer.rs`：Computer、MCP、SMCP、skill、marketplace/plugin runtime adapter。
- `src-tauri/src/services/sdk_config.rs`：SDK 配置 CRUD、校验、迁移、导入导出 adapter。
- `src-tauri/src/commands/`：Tauri 命令到 service 的产品调用链。
- `src-tauri/tests/contract_test.rs`：公共 API 与序列化契约。
- `src-tauri/tests/*integration_test.rs` 和 `src-tauri/tests/smcp_connection_lifecycle_test.rs`：行为与生命周期证据。

对每个验收项追踪：

```text
用户需求 → Tauri command/service → client adapter → SDK public API
         → SDK implementation → 状态/IO/错误 → client 可观察结果
```

执行以下核对：

1. 搜索所有 `a2c_smcp::`、`a2c-smcp` 和相关 wrapper，不只看直接 import。
2. 阅读调用前置条件、参数转换、默认值、错误映射、锁与 task 管理、持久化和清理逻辑。
3. 在 SDK 候选 commit 中从 public re-export 追到实现与测试，检查 feature gate 和平台条件。
4. 比较客户端固定 rev 到候选 SHA 的相关 diff，识别 API 变化、语义变化、迁移要求和回归风险。
5. 判断缺口归属。若能力依赖 Manager、robot/employee、token exchange、UI 偏好或产品策略，先检查 `.codex/skills/requirement-review/SKILL.md` 的 SDK/client 边界；需要完整边界评审时同时使用 `$requirement-review`。

## 第三步：形成静态结论与证据缺口

为每个验收项填写证据矩阵：

| 验收项 | client 调用证据 | SDK 公共 API | SDK 实现证据 | 静态判断 | 待实验问题 |
|---|---|---|---|---|---|

静态判断只能使用以下状态：

- `可能满足`：公共 API 和实现路径完整，但尚未用本项目验证行为。
- `静态确认缺口`：公共 API 缺失、无法从 client 使用，或实现明确违反需求。
- `client 侧缺口`：SDK 已提供能力，但 client 未调用、参数错误或产品逻辑不完整。
- `边界不当`：需求本身不应由 SDK 承担。
- `证据不足`：源码或需求不足以判断。

即使发现静态缺口，也要设计最小实验验证可观察影响；只有 API 根本无法表达需求时，允许用编译失败的最小调用作为关键证据。

## 第四步：使用 run-experiment 设计验证

调用 `$turingfocus-toolkit:run-experiment`，将证据矩阵中的不确定项转成实验方案并等待用户明确批准。方案至少包含：

1. **SDK 最小实验**：直接依赖候选 SHA 的源码，验证公共 API、核心语义和负向路径。
2. **client 集成实验**：以当前 tfrobot-client 工作区实际调用方式连接候选 SDK，优先复用 contract/integration/lifecycle 测试。
3. **差异对照**：需求涉及升级回归时，在客户端当前固定 rev 与候选 SHA 上执行相同样本和断言。
4. **生命周期与失败路径**：按需求覆盖取消、超时、重复调用、并发、断开重连、部分失败、回滚或资源清理，不能只测 happy path。

把实验放在 `experiments/codex-rust-sdk-capability-<topic>/` 或项目已有实验目录。若需要让 client 编译到候选 SDK，使用隔离的 client 副本、独立 harness 或 Cargo source override；只修改实验副本。不要编辑主工作区的 `src-tauri/Cargo.toml`、`Cargo.lock` 或主测试文件。

实验命令必须绑定并报告候选完整 SHA。优先运行最窄的相关测试，再补充必要回归。记录命令、环境、样本、退出码和关键输出；编译成功只证明 API 兼容，不能替代行为断言。

涉及远端服务、真实账号、敏感数据、生产或预发环境、明显费用或长耗时时，按 run-experiment 要求单独请求确认并优先提供本地 mock 方案。

## 第五步：判定问题并提出修改方向

实验执行并复核后，将每项归为：

- `满足需求`
- `确认的 SDK 问题`
- `确认的 client 问题`
- `SDK/client 均需修改`
- `需求边界不成立`
- `仍不足以判断`

只有同时具备以下证据才称为“确认的 SDK 问题”：

1. 可验证的需求或验收条件；
2. 候选 SDK 精确 SHA 的源码证据；
3. tfrobot-client 的调用或预期调用证据；
4. 可复现的实验数据，或公共 API 无法表达需求的编译证据；
5. 已排除 client 参数转换、错误映射、产品策略和环境问题。

对确认问题给出契约级修改方向，不要替 SDK 维护者凭空写补丁。至少说明：

- 期望新增或调整的公共 API、类型或事件；
- 输入、输出、错误、幂等、并发、取消与生命周期语义；
- 配置所有权、schema、迁移和向后兼容要求；
- client 接入方式以及需要保留的产品边界；
- SDK 单元/集成测试、client contract/integration 测试和跨版本回归用例；
- 已知替代方案及其限制。

## 第六步：输出与清理

在实验批准前，先输出“版本基线 + 静态审查 + 实验方案”，然后暂停等待明确授权。实验完成后读取 [报告模板](references/report-template.md) 并输出最终报告。

保持事实、推断和建议分离，所有源码证据使用文件路径与行号，所有版本使用完整 SHA。不要只写“最新版”“看起来支持”或“测试通过”。

按 `$turingfocus-toolkit:run-experiment` 的规则，在用户认可实验结果前保留实验产物；认可后只清理本次创建的文件，不触碰用户既有改动。
