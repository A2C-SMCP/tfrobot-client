# Issue #86：系统授权验收记录

需求：https://github.com/A2C-SMCP/tfrobot-client/issues/86

## 证据状态

本次开发环境为 macOS 14.2（23C64），arm64。本机已安装 0.2.4 应用的签名元数据包含 Developer ID、hardened runtime 和 stapled 公证票据；它不是本次改动的验收包。用户在本次会话确认暂时无法复现；尚未确认触发原问题的外部 MCP 和操作顺序。System Events 请求进程、实际弹窗次数及下面的签名包矩阵均待现场验证，不以单元测试代替。

## 授权清单

| 场景 | 用途与触发条件 | 必要性 / 拒绝影响 |
|---|---|---|
| 登录恢复 | 从钥匙串读取已保存的 Manager 登录凭据 | 恢复身份需要；拒绝后可使用未登录功能，凭据访问暂停，可主动重试 |
| 旧应用配置迁移 | 迁移 Computer、连接目标和 SDK 配置 | 不搬运秘密，不访问钥匙串 |
| 手动连接 | 实际连接时读取 API key，提交时核对权威凭据 | 保留外部凭据修改检测；已连接的重复请求不读取 |
| 保存/删除目标凭据 | 用户操作触发快照、修改与必要回滚 | 保留系统存储和回滚语义；失败不假称成功 |
| 变量列表 | 展示已保存元数据、普通值 | 不读秘密；旧变量通过“导入旧变量”主动迁移 |
| 变量导入/修改/删除 | 验证旧存储、保存密码、清理旧值及回滚 | 只操作选中变量，显式批量导入/清除例外；拒绝不阻塞无关普通变量 |
| MCP 输入解析 | 实际使用已保存密码的工具启动/调用 | 仅业务所需；单次解析内共享读取结果，不跨操作保存快照 |
| HTTP MCP OAuth | SDK 加载、刷新、保存、清除 OAuth 凭据 | 钥匙串保存秘密；相同条目的并发读取合并，拒绝后暂停自动访问 |
| 安装更新 | 用户确认替换应用，目标目录权限不足时系统提权 | 更新确认已说明可能请求管理员认证；不是 MCP 启动的默认权限 |
| 外部 stdio MCP / 工具命令 | 由外部可执行文件及其子进程决定 | Automation、辅助功能、录屏、系统设置须对应具体功能，不能笼统认定必需 |
| 登录 shell / 运行时探测 | PATH 恢复、执行运行时版本检测 | profile、shim 可能有副作用，尚未证实为本次来源；不扩大权限或改动 PATH 策略 |
| 文件选择与访问 | 用户选择导入/导出/附件/工作目录后访问文件 | 受保护目录可能触发系统隐私提示，区别于管理员认证 |
| 浏览器、网络、本地监听 | OAuth 浏览器、loopback 回调、局域网连接 | 依赖实际网络路径和系统设置，不能由静态代码推定固定提示次数 |

## 进程归因步骤

1. 使用包含本次改动的签名包，记录版本、构建 SHA/工作区补丁标识、签名身份、OS、MCP 名称与明确操作顺序。不要重置 TCC 或用户钥匙串。
2. 先仅启动应用，再分别启动单个 Computer、单个 MCP、多个 MCP；记下每一步时间和系统弹窗中的请求应用与目标名称。把“控制 System Events”与“希望进行更改/管理员认证”分别记录。
3. `permission_diagnostics` 的 `mcp.start_requested` / `mcp.start_finished` 包含 operation_id、Computer、bundle、客户端 PID 和完成结果。客户端 PID 不是 MCP 子进程 PID，不据此推断请求来源。
4. 弹窗停留期间用活动监视器的进程层级核对请求应用；必要时一次性运行 `ps -axo pid=,ppid=,comm=`，只提取客户端及其后代的 PID、父 PID、可执行文件名称，与操作时间对应。不要采集参数、环境变量或完整工具日志；短命进程未捕获时标记未知，不能猜测。
5. 锁定 SDK 0.4.1 的 stdio 启动实现未向客户端提供本次已验证可用的子进程 PID 接口。本次只补足客户端动作关联；若现场系统证据不足，进一步评估上游进程事件能力，不能宣称已经完成归因。

## 签名包验收矩阵（全部待现场执行）

| 环境 | 操作 | 必须记录 |
|---|---|---|
| 新装 | 启动、登录、连接、输入密码变量、OAuth | 每步实际弹窗数量、用途、请求进程 |
| 旧配置迁移 | 启动、只看变量列表、主动导入、失败后重试 | 启动迁移和列表不读秘密；主动导入允许必要访问 |
| 开发版转正式版 | 使用原钥匙串条目连接 | 签名/ACL 差异与实际提示，不能要求全部始终允许 |
| 正式版升级 | 更新确认、安装、恢复连接 | 管理员提示只在必要替换时出现，凭据不丢失 |
| 多 MCP | 分别及并发启动，包含 OAuth / 密码输入 | 调用次数与弹窗次数分别记录，来源逐个归因 |
| 拒绝授权 | 拒绝后等待原业务事件、浏览其他页面、使用普通变量 | 无自动弹窗循环，无关功能可用 |
| 主动重试 | 点击“重试凭据访问”，再执行原业务动作 | 只解除指定条目暂停，不自动重放写入/删除；能够恢复 |
| 外部改密 | 连接建立期间从系统钥匙串修改 API key | 提交校验拒绝过期凭据，不使用进程内 revision 冒充外部一致性 |

## 自动化验证

本次已执行结果（2026-09-16）：

- Rust：551 个单元测试通过、4 个显式忽略；迁移集成 2 个通过；真实 SMCP 生命周期 28 个通过，含新增“重复连接零追加读取”用例。
- 真实系统 Keychain：随机隔离条目的写入、读取、删除测试 1 个通过；未操作用户凭据。
- `cargo fmt --check`、TypeScript、本次源码 ESLint 和全 targets Clippy（`-D warnings`）最终复验通过。全仓 ESLint 用命令行排除已有 `experiments/**/target/**` 生成产物后为 0 错误、6 项既有警告；未修改项目 lint 配置。
- 前端针对性验证：5 文件 / 40 项通过。完整首轮为 601 通过、14 失败，主要为与编译并发负载下的 5000ms 超时；单 worker、30 秒测试超时的完整复跑为 618/619 通过，剩余 McpServerForm 用例超时。随后单独复跑该文件 42/42 全部通过；未修改断言或项目默认超时。即全套用例经复验覆盖通过，但不是单次完整运行全绿。
- 隔离审查：两个无父上下文只读审查代理完成审查及复核，最终 0 阻塞、1 非阻塞建议。首次提出的多凭据可辨识上下文、实际操作用途说明已补齐；阻塞迁移任务持有自身 Computer 操作租约，输入解析保留原运行时生命周期。

已知非阻塞建议：历史索引保留的已删除/缺失值条目可能导致再次进入变量页面时重新显示“导入旧变量”。列表与待迁移判断不读取秘密；用户再次点击导入仍可能检查这些历史条目。后续可用迁移检查记录/tombstone 消除此提示，默认 block 审查范围本次未扩展处理该建议。

复验命令：

```sh
pnpm fmt:check
pnpm lint:ts
pnpm exec eslint . --ignore-pattern 'experiments/**/target/**'
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
pnpm exec vitest run --maxWorkers=1 --testTimeout=30000
pnpm exec vitest run --maxWorkers=1 --testTimeout=30000 src/test/components/McpServerForm.test.tsx
cargo test --manifest-path src-tauri/Cargo.toml --lib --test smcp_connection_lifecycle_test --test computer_registry_migration_test
cargo test --manifest-path src-tauri/Cargo.toml --lib services::keychain::tests::test_credential_operations -- --ignored --exact
```

最后一项使用随机隔离条目，执行真实 Keychain 写入/读取/删除；不能代表签名安装包的 ACL 和弹窗行为。本机未安装 cargo-nextest，Rust 检查使用 cargo test。全量 ESLint 目前被已有 experiments 构建目录中的生成 JS 阻塞；单独记录本次源码检查结果。
