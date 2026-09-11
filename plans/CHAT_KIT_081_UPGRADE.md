# Chat Kit 0.8.1 实施计划

日期：2026-09-10。追踪：[tfrobot-client#79](https://github.com/A2C-SMCP/tfrobot-client/issues/79)。状态：用户已回复“确认”，实现完成；前端 60 文件 / 591 项测试及构建通过，隔离审查 APPROVE、无阻塞项。ESLint 生成目录限制、非阻塞测试建议和真实桌面未验收项见升级指导。代码尚未提交。

## 需求与方案评审

用户在 0.8.1 升级指导之后调用 add-feature。实现目标：统一升级 Chat Kit 包组，在现有宿主内使用默认会话内存缓存，补齐资源错误分类和中英文文案，保持既有 Tauri 资源访问与身份隔离。

采纳现有指导的默认内存缓存方案：Kit 已负责缓存、workspace selection 与只读恢复，无需 client 二次维护消息状态。磁盘持久化不属于此次需求；不引入持久化 scope、文件存储或附件恢复服务。

资源错误转换放在现有 TypeScript port 的公开边界，Rust 继续保留自身错误类型与详细诊断。这是两个既有契约之间的宿主适配，不应为匹配 UI 包而改变原生 IPC 的错误域。IPC 失败与媒体读取失败仍沿各自通道处理。

风险等级：中。新增默认缓存会改变异步快照顺序；资源 Retry 行为受错误码影响；两者均需真实 Kit 行为测试。没有新增上游依赖、轮询或服务端权限调整。

## 文件与实施顺序

| 顺序 | 文件 | 改动 |
| --- | --- | --- |
| 1 | `package.json`、`pnpm-lock.yaml` | 主包与五个 overrides 精确设为 0.8.1，确认解析结果与其余依赖范围 |
| 2 | `src/components/Chat/chatResources.ts` | 公共 resolve/open/download 边界统一转换错误；保留取消、迟到注册释放、宿主细分诊断与原生动作 |
| 3 | `src/components/Chat/chatBridge.ts` | 增加 5 个缓存和 19 个资源文案映射；默认内存缓存沿用 Kit；保留工作区原有日志改动 |
| 4 | `src/locales/{zh,en}/translation.json` | 补齐公开文案，保留资源服务自身的详细错误描述 |
| 5 | `src/test/components/ChatResources.test.ts`、`ChatBridge070.test.ts`、`ChatAttachments080.test.ts` | 更新旧错误断言并增加映射、取消、重试条件、真实资源 UI 与文案覆盖；复用已有 fixture/helper |
| 6 | `src/test/components/ChatCache081.test.tsx`（拟新增）及按需复用的测试 helper | 真实 Kit + 本地 HTTP/Socket.IO fixture 验证缓存与同步，覆盖实际 workspace/UI；不 mock 承载新缓存契约的 Runtime/UI |
| 7 | 本计划、升级指导及 `.claude/skills/UAT/resources/scenarios/chat-kit-081.md`（拟新增） | 记录本次验证、限制与真实桌面验收步骤 |

`src/components/Chat/index.tsx` 预计无需业务修改；测试如需导出既有 compact workspace 作为真实挂载入口，可增加命名导出，不复制或重写组件。现有 Chat 日志、测试 log mock 和 Rust artifact 脚本均保留，不纳入额外整改。

## 测试计划

本次启用缓存执行路径，命中 add-feature Phase 4 的真实路径验证要求。新增专项必须当次真跑，不能只给出命令或断言配置对象：

1. 本地 HTTP/Socket.IO fixture 驱动真实发布包加载 A、B，再延迟 A 的同步响应；真实 workspace/UI 在服务端完成前显示缓存，完成后显示最新结果。
2. 网络失败维持缓存只读，认证/授权/not-found 失效；检查缓存不赋予发送、中断及旧 Ask User 操作权。快速切换和迟到响应不覆盖新会话；草稿按会话保留。
3. 真实 resource UI 驱动 port 的显示/打开/下载，覆盖 permission/not_found/timeout/busy/cancelled 与未知错误、Retry 条件、两种语言及取消无失败提示。保留现有句柄撤销与原生操作 IPC 回归。
4. 检查提示关闭后的状态与再出现条件，事件工具栏移除后详情选择可用；原有菜单往返/身份切换回归继续通过。
5. 执行 `pnpm lint:ts`、`pnpm lint:eslint`、`pnpm test`、`pnpm build`。只有实际涉及 Rust 修改才增加对应 Rust 测试及 Clippy；本方案不要求修改 Rust。

测试依赖优先复用现有包与测试工具；若本地 Socket.IO fixture 需新增直接开发依赖 `socket.io`，固定为与当前客户端 4.8.3 对齐的版本，限于测试使用。

## UAT / Seed 影响

新增会话缓存与资源文案桌面验收场景，覆盖 A/B/A、断网后只读、菜单往返、切 Robot/身份、真实图片、原生保存取消和错误文案。沿用既有已授权测试环境与账号，不修改 Seed，不新建业务数据集或授权模型。真实 Beta/Tauri 无法执行时明确标注未验收，不伪称通过；#76 既有未验收媒体/Range边界继续记录。

## 审查与交付门槛

新增代码/测试文件形成可审查内容后仅对精确新文件路径暂存；已有改动保持原暂存状态。质量检查通过后按 add-feature Phase 5.5 拉起 `fork_turns="none"` 只读审查代理，提供完整需求、确认边界、文件清单与测试结果，读取 code-review rubric 并自行核对工作区及暂存 diff。默认 block 模式处理阻塞项，修复后重新完整隔离审查。

不在本次计划确认中请求 commit/push/PR 授权。完成实现与审查后再提供具体交付结果供用户 Review。回退缓存可用 `cache: false`；完整回退需同时还原六包、lockfile 与 0.8.1 专用错误类型引用。
