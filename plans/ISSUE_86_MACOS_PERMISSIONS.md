# Issue #86：macOS 系统授权治理实施计划

状态：用户已批准；本地实现和隔离审查已完成，最终测试结果及现场验收见 docs/acceptance/issue-86-macos-permissions.md。2026-09-16。
需求：https://github.com/A2C-SMCP/tfrobot-client/issues/86
分析基线：dev-0.2.5，89c8bb99bf274fbfc9cdc21702a5a771f81d883f。

## 目标与追踪

让用户仅在需要凭据或系统能力的业务动作中遇到授权提示，理解用途和拒绝影响；减少重复访问，保留并发保护、事务回滚和系统安全存储。复用 #86（已标记 in-progress），当前按客户端内独立交付任务管理；确认 SDK 阻塞后再建立关联依赖，不预先重复建单。

必须完成 System Events 请求进程的现场归因和真实签名安装包验收。调用计数下降不代表弹窗次数下降。不得通过扩大 entitlements、修改 TCC、要求始终允许或关闭安全检查消除提示。

## 当前证据与方案判断

- `services/config_migration.rs` 的 build_plan/verify_plan 各读一遍全部手动目标凭据，迁移本身不搬运秘密。移除这些读取和不再需要的参数，保留文件、SDK 配置一致性校验与回滚。
- `commands/connection.rs` 首次读秘密早于 AlreadyConnected 判断，commit 又读一次用于并发校验。先完成无需秘密的判定；统一凭据 mutation 后再设计 revision/snapshot 校验，不能直接删第二次检查。
- `SecretStore` 已有可注入接口和内存实现；增强现有边界，不分别在 OAuth、输入变量、Manager 建缓存。
- OAuth 适配器使用 spawn_blocking，但将存储错误统一映射为 OperationFailed。需保留客户端可判定的拒绝状态，避免后台状态查询重复访问系统存储。
- 输入列表投影 view_for 已经不读秘密；旧数据迁移仍读秘密且承担冲突裁决、清理和回滚。保留已迁移列表的零秘密访问，并把需要授权的旧数据迁移变为明确、可重试的动作，不按元数据猜测秘密存在。
- 现有 `docs/diagnostics/2026-09-16-macos-system-prompts.md` 是未跟踪的用户工作区材料，仅作为证据参考，不覆盖或暂存。
- 启动登录 shell 可能执行用户 profile，当前超时不终止子进程；这是归因候选，尚不是 System Events 已确认来源。先实测，若需要改变 PATH 策略，补充影响评估再确认，不顺带重构。

## 实施顺序及文件范围

1. **凭据协调层**：`src-tauri/src/services/keychain.rs`、`src-tauri/src/lib.rs`。按 key 协调读写、合并并发读；结果限于明确业务操作生命周期，避免无限期保存秘密。写入、清除、回滚统一失效并推进修订状态。拒绝后关闭该凭据的自动访问，只有对应用户主动重试重新开启；其他 key 不受影响。阻塞系统调用不得持有全局锁或阻塞异步执行线程。日志只含用途、关联 ID、操作类别和结果分类。
2. **迁移与连接**：`services/config_migration.rs`、`commands/connection.rs` 及现有迁移/连接测试。移除迁移预检秘密读取；已连接判定前不读秘密；将应用内并发 mutation 纳入统一校验。实施前验证外部 Keychain 修改的检测手段：仅进程内 revision 不等价于原有二次读取，无法保留外部变更检测时保留必要权威读取，并报告计数边界，不静默削弱语义。
3. **输入、登录和 OAuth**：`services/input_entry_store.rs`、`commands/inputs.rs`、`services/input_resolver.rs`、`services/manager_client.rs`、`services/oauth_credential_store.rs`、`commands/mcp.rs` 和对应测试。复用协调层；旧数据迁移失败不阻塞其他页面/普通变量；拒绝恢复登录时允许进入未登录状态；状态查询不得解除拒绝保护。
4. **用途和主动重试 UI**：`src/components/InputVariables/`、`src/components/ManagerAccount/`、`src/components/RobotConnectionPanel/index.tsx`、`src/components/Settings/AboutSection.tsx`、`src/locales/{en,zh}/translation.json` 及对应组件测试。实际授权位置说明读取用途和拒绝影响；复用连接/登录/MCP 启动入口承载明确重试；更新确认补充替换应用可能需要管理员认证。
5. **归因与验收记录**：复用 `services/computer/runtime_lifecycle.rs`、`services/computer_runtime_events.rs`、`services/observability/` 已有事件边界，按必要性补充 Computer、bundle、启动动作关联信息。新增 `docs/acceptance/issue-86-macos-permissions.md`，记录操作时间、进程 PID/父进程、可执行文件身份和系统提示请求方，不记录命令参数、环境变量或秘密。优先复用 SDK 事件/现场系统证据；若 SDK 不暴露必要 PID，不伪造归因，明确提出上游依赖。不新增定时轮询。

## 验证与门禁

- 调用计数：配置迁移零秘密读取，已迁移列表零读取，已连接重复点击零读取，同一操作重复解析/并发读取合并；不同 key 独立。
- 安全与一致性：读写交错、目标修改/删除、凭据替换、失败回滚、外部 Keychain 修改、OAuth 清除、拒绝后后台访问不再弹窗、主动重试恢复。
- 真实设施：运行现有真实 SMCP 连接生命周期测试及新增相关用例；真实 Keychain 用隔离测试条目验证读写/失效/删除。模拟存储只用于可控并发和调用计数，不能替代这些测试。
- 质量：`pnpm fmt:check`、`pnpm lint`、`pnpm test`、`pnpm test:rust`；实际触及的新增真实设施路径必须运行至 PASS。全量重型 E2E 另列验收命令。
- 签名包矩阵：新装、旧配置迁移、开发版转正式版、升级、多 MCP、拒绝/主动重试；记录 OS、应用版本、构建 SHA、签名身份、动作及实际弹窗次数。用造成原问题的 MCP 配置现场复现 System Events，区分 Automation 与管理员认证。无现场证据不能标记该项通过。
- 测试后按 add-feature Phase 5.5，使用无父上下文的只读子代理审查完整需求和全部 diff，阻塞项清零后才交付。

## 实施边界与待具备条件

需要可复现问题的 MCP 配置/动作及签名包验收环境；优先从本机非敏感元数据发现，缺失时再向用户索取。真实授权对话框中的选择由用户操作，不自动接受或改变系统授权设置。

现有 Chat、测试 setup、诊断文档、Rust 构建产物脚本等未提交改动均保留。批准前不修改业务代码；批准实施不视为批准 commit/push/PR。完成本地验证后单独请求交付授权，真实签名包和 System Events 归因未通过时保持 Issue 未完成。
