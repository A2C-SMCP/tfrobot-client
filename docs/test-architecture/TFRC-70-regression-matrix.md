# TFRC-70 / TFRC-79 自动化回归矩阵

本文档记录 TFRC-70「重构详情页配置与运行信息架构」的验收标准如何由
TFRC-71～TFRC-79 的自动化测试守护。TFRC-79 负责依赖图末端的测试验收；
验收暴露的生产缺陷分别归入 TFRC-84～TFRC-87。本交付工作区同时包含这些
缺陷子任务的修复，因此生产变更必须能映射到对应子任务并通过各自回归与隔离审查。

基线：`feature/ui-ux-optimization` 的 `5579f81`。

## 父级验收标准映射

| # | TFRC-70 不变量 | 自动化证据 |
|---|---|---|
| 1 | Computer 详情页没有一级 Tabs 或独立概览，进入即为单页运行工作台 | `e2e/tests/computer-runtime-flow.spec.ts` — `keeps identity, config, runtime, and diagnostics operations semantically separate`；`src/test/components/Computer.test.tsx` — `opens a Computer single-page runtime workbench from the list` |
| 2 | 首屏组合 Runtime 状态、独立连接状态、MCP、active Skills/Tools 与诊断入口 | `src/test/components/ComputerRuntime.test.tsx` — `composes MCP runtime and active capability summary without duplicating header actions`；`e2e/tests/computer-runtime-flow.spec.ts` 的边界流程 |
| 3 | 完整 SDK Lifecycle 稳定映射到六种用户态，Connecting/Disconnecting 仍属于运行中 | `src-tauri/src/services/computer/runtime_lifecycle.rs` — `user_state_mapping_covers_every_sdk_lifecycle` |
| 4 | 连接状态独立投影为未连接、连接中、已连接、断开中 | `src/test/stores/connectionStore.test.ts` — `projects the canonical four-state connection snapshot independently of SDK lifecycle` |
| 5 | 连接状态以后端 operation 与 client-owned authority 为权威，不由前端 loading 推断 | `src/test/components/ComputerWorkbench.test.tsx` — `keeps a backend connect operation and target authoritative when policy changes`、`shows disconnect during the backend-owned disconnection transition`、`projects a backend reconnect as an authoritative connecting operation` |
| 6 | Connect 过渡与成功提交顺序正确，失败不遗留虚假 authority | `src-tauri/tests/smcp_connection_lifecycle_test.rs` — `profile_connect_exposes_backend_connecting_state_until_join_office_succeeds`、`restart_during_delayed_join_cannot_leave_authority_without_transport`、`rename_during_connect_supersedes_stale_connection_commit` |
| 7 | Disconnect 成功及失败回退符合 authority/transport 实际状态 | `src-tauri/tests/smcp_connection_lifecycle_test.rs` — `close_smcp_connection_closes_underlying_socket_after_leaving_office`、`disconnect_failure_fails_closed_when_transport_liveness_is_unproven`、`runtime_stop_bounds_pending_transport_teardown_and_clears_authority` |
| 8 | 自动 refresh/reconnect 的成功、暂时失败、永久失败、重试耗尽与 stale generation 可恢复 | `src-tauri/tests/smcp_connection_lifecycle_test.rs` — `reconnect_with_token_reconnects_socket_and_refreshes_snapshot`、`reconnect_with_token_records_diagnostic_on_sdk_build_failure`、`reconnect_terminal_failure_closes_transport_or_exposes_orphan_cleanup`、`reconnect_teardown_timeout_bounds_attempt_and_terminal_settlement`、`reconnect_with_token_stale_generation_does_not_touch_socket`、`try_install_refreshed_client_respects_generation_guard` |
| 9 | 页面刷新、重新进入和事件桥恢复会重新同步后端快照 | `src/test/stores/runtimeStore.test.ts` — `subscribes before enabling backend events and hydrates initial snapshots`、`hydrates connection authority with the initial runtime observation`、`recovers a failed event bridge and explicitly resyncs authoritative snapshots` |
| 10 | 多 Computer 的 authority、operation、revision 与错误互不串扰 | `src/test/stores/computerStore.test.ts` — `preserves raw connection authority when status refetches during reconnect`；`src/test/stores/runtimeStore.test.ts` — `rejects events from an older handle generation even with larger revisions`；`src-tauri/tests/smcp_connection_lifecycle_test.rs` — `concurrent_profile_connect_same_robot_allows_only_one_instance` |
| 11 | 页头只保留身份、双状态、启停、连接、设置和更多操作 | `e2e/tests/computer-runtime-flow.spec.ts` 的边界流程；`src/test/components/ComputerWorkbench.test.tsx` — `renders an accessible icon-only back action and one longitudinal workbench` |
| 12 | 启动/停止是主操作，重启位于更多操作，不存在 reload、reload_required 或应用配置入口 | `src-tauri/src/services/computer/runtime_lifecycle.rs` — `action_matrix_is_explicit_for_every_sdk_lifecycle`；`e2e/tests/computer-runtime-flow.spec.ts` 的边界流程 |
| 13 | 过渡态阻止冲突操作并显示当前动作 | `src/test/components/ComputerRuntime.test.tsx` — `disables MCP lifecycle controls while the Runtime lifecycle is transitional`；Lifecycle action matrix |
| 14 | 返回入口为纯图标，并具备 Tooltip、aria-label 和可点击热区 | `e2e/tests/computer-runtime-flow.spec.ts` 的边界流程会验证空文本、可访问名称与 Tooltip；`src/test/components/ComputerWorkbench.test.tsx` 的 icon-only 回归 |
| 15 | 重启、日志、复制、编辑、删除位于更多操作，删除二次确认 | `e2e/tests/computer-runtime-flow.spec.ts` 的边界流程；`src/test/components/ComputerWorkbench.test.tsx` — `keeps delete behind the more menu and a second confirmation` |
| 16 | 齿轮进入当前 Computer 的二级设置页，并可明确返回工作台 | `e2e/tests/computer-settings.spec.ts` — `opens all six settings modules and returns to the Computer runtime` |
| 17 | 设置页使用左侧纵向导航和右侧内容区 | `src/test/components/ComputerSettings.test.tsx` — `renders six persistent configuration sections in vertical navigation`；Computer settings Playwright 流程 |
| 18 | Skill Home 按 Computer 隔离，修改需确认并保持现有配置 | `src/test/components/SkillsSettings.test.tsx` — `renders only the saved Skill Home and saves a custom root after confirmation`、`chooses and opens the instance Skill Home and restores the default`；`src/test/stores/skillStore.test.ts` 的实例隔离测试 |
| 19 | active Skills 只在工作台；Skills 设置仅展示保存的 Skill Home 与相关跳转 | `e2e/tests/computer-runtime-flow.spec.ts` 的跨页边界断言；`src/test/components/SkillsSettings.test.tsx` — `links to active Skills and Plugin lifecycle management` |
| 20 | Desktop Resources 只属于运行态，默认折叠、零首屏枚举、主动加载、展开按需读取且实例隔离 | `src/test/components/DesktopResources.test.tsx` — `is collapsed by default and performs zero resource requests on render`、`enumerates resources only after an explicit load action`、`reads one resource only when its row is expanded`、`shows only the selected Computer cache`；`src-tauri/tests/desktop_resources_integration_test.rs` — `list_is_metadata_only_and_detail_performs_the_first_resource_read` |
| 21 | 六个设置模块均不暴露启动、停止、重启、连接、断开或 MCP 进程启停 | `e2e/tests/computer-settings.spec.ts` — `opens all six settings modules and returns to the Computer runtime` 会逐模块检查 Runtime 操作不存在 |
| 22 | per-Computer Input 定义、普通值与 Secret 隔离；旧全局 Input 文件不回退导入/迁移，旧未 scoped Keychain Key 不得读取 | `src-tauri/tests/computer_instance_integration_test.rs` — `mcp_configs_inputs_and_values_are_isolated_per_computer`；`src-tauri/tests/computer_registry_migration_test.rs` — `startup_does_not_import_legacy_global_inputs_or_read_unscoped_keychain_namespaces` |
| 23 | SDK 注入的 Plugin Input 不进入 Client 持久化、CRUD、import/export，且不引入持久 `managedBy` 模型 | `src-tauri/src/commands/inputs.rs` — `runtime_only_plugin_input_stays_out_of_computer_crud`；`src-tauri/tests/marketplace_integration_test.rs` — `plugin_runtime_input_is_excluded_from_client_crud_import_and_export` 覆盖 Client 持久化边界，`plugin_missing_input_stays_structured_across_enable_retry_and_cold_start` 覆盖缺失错误与冷启动 |
| 24 | 工作台不暴露新增/导入 MCP、Input、Skill Home、Plugin 生命周期或连接策略编辑 | `e2e/tests/computer-runtime-flow.spec.ts` 的边界流程会集中检查持久配置操作不存在 |
| 25 | Plugin-owned MCP 在设置中只读且能跳转精确 Plugin | `e2e/tests/computer-settings.spec.ts` — `opens the exact Plugin that authoritatively owns a read-only MCP declaration`；`src/test/components/McpConfig.test.tsx` 的 ownership 加载、失败关闭与精确跳转测试 |
| 26 | Plugin-owned MCP 在运行区无单个启停；批量操作排除它并反馈数量；User-managed 支持单个/批量及部分失败 | `src/test/components/McpRuntimeControls.test.tsx` — `shows Plugin-owned diagnostics without lifecycle buttons and opens the matching Plugin`、`reports mixed batch results with changed, unchanged, excluded, and failed counts`、`reports partial stop failures with changed, unchanged, and excluded counts`；`src-tauri/src/commands/mcp.rs` — `stop_all_summarizes_changed_unchanged_excluded_and_failed_candidates` |
| 27 | Error 与 Degraded 具有不同语义、影响范围和恢复动作，不泄漏普通 UI 技术详情 | `src/test/components/ComputerRuntime.test.tsx` — `shows a safe structured Runtime error and delegates log navigation`、`renders degraded MCP impact and explains an unavailable recovery action`；`e2e/tests/computer-runtime-flow.spec.ts` 的 Runtime event 流程 |
| 28 | 完整 Lifecycle、Generation、Revision 和事件流只位于默认折叠的高级诊断 | `src/test/components/ComputerWorkbench.test.tsx` — `maps a legacy logs destination to the expanded diagnostics region`；`e2e/tests/computer-runtime-flow.spec.ts` — Runtime event 与 Advanced Runtime Diagnostics 流程 |
| 29 | 禁用操作展示原因与下一步，不只依赖 Tooltip | `src/test/components/ComputerRuntime.test.tsx` — `renders degraded MCP impact and explains an unavailable recovery action`；`src/test/components/DesktopResources.test.tsx` 的 Runtime/MCP 不可用状态测试 |
| 30 | 配置保存只承诺“已保存”，不根据 Revision 推断当前 Runtime 已生效 | `e2e/tests/computer-settings.spec.ts` — `reports a profile mutation as saved without inferring active Runtime effect`；`src/test/stores/runtimeStore.test.ts` — `routes config and capability revisions to only their dependent consumers` |
| 31 | 旧运行类与配置类入口分别映射到工作台区块和设置模块 | `src/test/components/App.test.tsx` — `maps legacy Computer configuration routes to settings sections`、`maps legacy runtime routes to workbench sections without preserving tabs` |
| 32 | 常用桌面宽度和窄窗口下布局、键盘操作与关键动作均可达 | `e2e/tests/computer-runtime-flow.spec.ts` — `keeps the workbench accessible and responsive in a narrow desktop window`；`e2e/tests/computer-settings.spec.ts` — `keeps vertical settings navigation usable in a narrow window` |

## TFRC-79 交付门禁

以下命令共同构成 TFRC-79 的质量门禁：

```bash
pnpm lint
pnpm test
cargo nextest run --manifest-path src-tauri/Cargo.toml \
  --lib \
  --test computer_registry_migration_test \
  --test computer_instance_integration_test \
  --test marketplace_integration_test \
  --test mcp_integration_test \
  --test smcp_connection_lifecycle_test \
  --test desktop_resources_integration_test
pnpm test:e2e
```

交付前还应运行：

```bash
git diff --check
git diff --name-only feature/ui-ux-optimization...HEAD
```

单独交付 TFRC-79 时，第二条命令的结果必须只包含测试、fixture 与测试文档。
若与验收发现的缺陷子任务联合交付，生产代码差异必须明确映射到 TFRC-84～TFRC-87，
并通过对应定向回归、全量质量门禁与隔离代码审查。

## 已解决的基线阻塞

- [TFRC-84](https://turingfocus.atlassian.net/browse/TFRC-84)：
  `plugin_missing_input_stays_structured_across_enable_retry_and_cold_start`
  在基线 `5579f81` 稳定失败。根因为 reconcile 失败后 Runtime 未回滚。
  当前已在 client 侧补齐失败回滚，exact 回归与 Marketplace 全量测试通过。
- [TFRC-85](https://turingfocus.atlassian.net/browse/TFRC-85)：
  `connection_mutations_wait_for_the_computer_lifecycle_transaction_lock`
  在同一基线 exact 复验稳定失败。根因为 client 连接事务未完整遵守“锁内准备、
  锁外网络 I/O、锁内提交”。当前 connect/disconnect 已统一为 token 守卫的两阶段
  事务，新增 policy stale 与 disconnect lock 回归通过，31 个连接生命周期测试通过。
- [TFRC-86](https://turingfocus.atlassian.net/browse/TFRC-86)：
  `enabled_plugin_overrides_disabled_user_fallback_until_plugin_is_disabled`
  在基线的全 binary 与 exact 复验中稳定失败。根因为 client 重复注入 SDK 的
  Plugin provenance 投影，且 disabled fallback 的运行时接管规则不完整。当前已
  排除 read-side Plugin 投影并统一 hook 接管，exact 与 Marketplace 全量测试通过。
- [TFRC-87](https://turingfocus.atlassian.net/browse/TFRC-87)：
  `keeps identity, config, runtime, and diagnostics operations semantically separate`
  的 hit-target 断言在基线稳定失败。根因为布局收缩后只有声明尺寸、缺少最小命中区。
  当前已补充 40×40 最小尺寸，Chromium Playwright 回归通过。

当前联合交付门禁结果：Rust 全量测试通过，前端 Vitest 41 个文件 / 416 个测试通过，
Clippy `-D warnings`、格式、构建、ESLint、目标 Playwright 与 `git diff --check`
均通过；最终隔离审查未发现代码阻塞。
