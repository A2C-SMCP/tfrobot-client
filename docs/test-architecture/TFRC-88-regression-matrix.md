# TFRC-88 / TFRC-96 自动化回归矩阵

本文档记录 TFRC-88「将 TFRSManager 登录态升级为 Computer 连接的一等 Context」
的验收标准如何由 TFRC-89～TFRC-96 的自动化测试守护。TFRC-96 是纯测试任务；
测试发现的生产问题必须回到对应前置子任务修复。

## 父级验收标准映射

| # | TFRC-88 不变量 | 自动化证据 |
|---|---|---|
| 1 | Tauri 后端提供完整、脱敏、revision 单调的 Manager Context，凭据不进入前端 | `services::manager_context::tests::transitions_are_monotonic_and_noops_do_not_advance_revision`、`context_snapshot_serialization_contains_no_credential_fields`；`manager_client_integration_test::current_user_returns_complete_redacted_context_identity` |
| 2 | Manager Robot 资源与 Computer target 使用 `ContextKey + employeeId` 作用域 | `managerStore` — `scopes employee lists by ContextKey plus revision, including identical employee IDs`；`commands::connection::tests::mismatched_context_makes_the_scoped_binding_dormant_and_disables_auto_connect` |
| 3 | 每次连接先校验 Context，并重新解析最新 robotAccountId/connection-info | `commands::connection::tests::manager_robot_resolution_uses_the_latest_visible_account`、`manager_connection_params_use_fresh_discovery_instead_of_profile_diagnostics`、`manager_robot_resolution_rejects_missing_account` |
| 4 | logout、401、账户切换会取消刷新并断开 Manager 连接，Manual SMCP 不受影响 | `smcp_connection_lifecycle_test` — `manager_context_cleanup_disconnects_manager_and_aborts_refresh_task`、`manager_context_cleanup_failure_still_drops_local_connection_authority`、`manager_context_cleanup_leaves_manual_smcp_connection_untouched`；`manager_client_integration_test::context_unauthorized_clears_identity_and_emits_expiry_once` |
| 5 | 原 Context binding 保留为 dormant，错误 Context 下不可连接 | `commands::manager::tests::lifecycle_cleanup_updates_persisted_profiles_and_runtime_mirrors`、`departing_manager_target_becomes_dormant_and_disables_auto_connect`；`RobotConnectionPanel` — `does not present a target from another Context as selected or connectable` |
| 6 | 重复登录或切换后不隐式重连，旧请求/连接/binding commit 被拒绝 | `manager_client_integration_test` — `stale_success_after_relogin_is_rejected_before_reaching_the_caller`、`concurrent_switch_unauthorized_and_logout_serialize_cleanup_without_old_commits`、`departing_context_cleanup_rejects_an_old_generation_commit_before_side_effects`；`managerStore` — `creates a fresh resource scope for a same-account revision` |
| 7 | signed-out 时本地 Computer、MCP、Skills 与 Manual SMCP 仍可用 | `smcp_connection_lifecycle_test::manager_context_cleanup_leaves_manual_smcp_connection_untouched`；`RobotConnections` — `guides signed-out users to the global account entry without owning login`；`RobotConnectionPanel` — `shows a Manager login guide and never loads Manual SMCP targets` |
| 8 | v1 profile/session 安全迁移；旧无作用域 Manager target 禁止自动连接并提示重新选择 | `services::config::tests::legacy_manager_profile_migrates_to_needs_rebind_with_backup_and_no_auto_connect`、`invalid_legacy_manager_profile_remains_intact_for_recovery`、`legacy_manual_profile_preserves_target_and_auto_connect_semantics`；`services::settings::tests::global_manager_session_v1_requires_a_fresh_environment_scoped_login`；`RobotConnectionPanel` — `lets a migrated needs-rebind profile explicitly select a current Context Robot` |
| 9 | 顶栏账户入口在所有页面持续可见 | `App` — `keeps the global Manager account entry visible on every page` |
| 10 | signed-out 显示登录；signed-in 显示组织、账户与标识 | `GlobalManagerAccount` — `opens an app-wide sign-in dialog and restores focus with Escape`、`shows current Context and switches only through the store transaction action` |
| 11 | 菜单展示环境/组织/账户，并支持账户切换、重新登录和退出 | `GlobalManagerAccount` — `shows current Context and switches only through the store transaction action`、`surfaces pending account selection from every page entry`；`managerStore` — `switches accounts through the backend transaction and accepts only its Context snapshot`、`logout commits the backend signed-out Context without mutating it locally` |
| 12 | 切换与退出复用后端 Context 事务并执行安全清理 | `manager_client_integration_test::context_switch_account_cleans_departing_context_and_publishes_one_final_snapshot`、`generation_guarded_commit_excludes_logout_until_local_side_effect_finishes`；`managerStore` — `reconciles signed-out Context when logout completes with cleanup diagnostics` |
| 13 | 账户入口支持键盘、焦点与窄窗口布局，且不和主题/语言冲突 | `GlobalManagerAccount` — `opens an app-wide sign-in dialog and restores focus with Escape`；Playwright `Global Manager account entry › remains keyboard accessible beside theme and language in a narrow window` |
| 14 | 覆盖多账户隔离、切换竞态、会话过期、迁移、Manual 不受影响与顶栏状态 | 本表 1～13；`manager_client_integration_test::concurrent_switch_unauthorized_and_logout_serialize_cleanup_without_old_commits`；`managerStore` — `drops a successful employee response that resolves after an account switch`、`auth-expired never synthesizes identity and reconciles the backend snapshot` |

## TFRC-96 安全矩阵

| 场景类型 | 成功路径 | 拒绝 / fail-closed | 失败清理 | 并发竞态 |
|---|---|---|---|---|
| Context | 登录、选账户、恢复、切换均发布单一权威快照 | 非法 identity、stale generation 与无 session 被拒绝 | 401 与 logout 最终清除本地 session | switch / 401 / logout 串行化，旧 commit 不执行 |
| Robot / Computer | 当前 Context 显式选择后 binding 为 active | 跨 Context、不可见 Robot、needs-rebind 禁止连接 | Manager teardown 失败仍清空本地 handle 并保留诊断 | lifecycle cleanup 与 policy commit 遵循统一锁序 |
| 持久化 | v1 profile 原子迁移并保留 backup | 无作用域 Manager target 不获权威 Context | 非法迁移保留原文件；session v1 明确 signed-out | 持久化 profile 与 runtime mirror 同一事务收敛 |
| 前端 | 当前 Context 资源、账户菜单与全局入口正常工作 | stale success/error 不回写新 revision | 后端清理诊断仍显示最终 signed-out | 相同 employeeId、快速切换与重复登录按 scope 隔离 |

## 交付门禁

TFRC-96 完成前必须运行：

```bash
pnpm fmt:check
pnpm lint
pnpm test
pnpm test:e2e
pnpm build
cargo test --manifest-path src-tauri/Cargo.toml
git diff --check
```

Rust 测试包含轻量真实 TCP HTTP Manager 契约与真实 Socket.IO 生命周期测试；
Playwright 覆盖浏览器中的顶栏键盘、焦点和窄窗口布局。只有完整门禁通过、隔离
代码审查给出 APPROVE 后，TFRC-89～TFRC-96 与父任务 TFRC-88 才能进入完成状态。
