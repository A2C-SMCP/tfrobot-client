# Issue #78 导航状态与生命周期实施方案（已确认）

来源：https://github.com/A2C-SMCP/tfrobot-client/issues/78

## 目标与范围

桌面客户端用户在菜单、计算机详情/设置和内部 tab 之间往返，恢复位置、选择、查询和未提交输入；聊天后台持续接收更新。只覆盖当前运行期间，不增加重启恢复、多 Robot 常驻缓存或 ChatKit 通用持久化。

复用 #78 作为统一导航生命周期能力的追踪项，已添加 in-progress。当前计划为单仓一次交付；若实施前发现可独立交付的跨项目阻塞，单独建立关联项。ChatKit #77 不是前置依赖。

## 已核实的现状

- App.tsx 条件渲染顶层页面，计算机详情还按 section 设置 React key。
- Chat 的 OwnedChatProvider、workspace 和 Rust session 都由页面子树拥有；卸载会触发释放。当前 ChatKit 为 0.8.0。
- ComputerSettings 的 switch 会卸载上一个设置页；Settings 使用默认 appearance tab。
- ActivityViewer 挂载重置查询，所有实例共用 activityStore 的一份 query/items/requestId。
- GeneralSettings 根据服务端字段变化调用 setFieldsValue，可能覆盖未保存输入。
- Manager 已有上下文 scope 和请求隔离模式；导航状态需要区分身份边界与普通数据刷新，不能将每次快照更新都当成换账号。
- EmployeeList 当前没有搜索筛选输入，不新增需求之外的筛选功能。
- 工作区已有 Chat/index.tsx、Chat/chatBridge.ts 和 test/setup.ts 修改，以及未跟踪 scripts/rust-artifact-retention.sh；这些均属于实施前基线，保留并与本任务变更区分。

## 设计决策

1. 引入应用运行期导航状态和页面活跃上下文。菜单入口恢复该菜单上次位置；显式导航携带目标和导航序号，即使目标字符串相同也能再次定位。滚动位置在切换前记录，在布局完成后恢复；显式定位优先。
2. 顶层固定页面和固定设置 section 首次访问才挂载，访问后保留组件状态。Chat 保持同一 session/client/workspace 子树；普通菜单切换不触发重建。延续 Robot 切换与身份变化时的现有 session 替换规则。
3. 活跃状态逐层传播到内部 tab。隐藏时暂停展示性工作并约束页面弹窗、浮层和图片预览；聊天传输和全局 runtime 事件继续运行。激活时使用现有事件/失效策略校准数据，不新增定时轮询。
4. 表单值与弹窗开关分别管理。普通导航保留草稿，隐藏弹窗；返回页面不自动恢复危险确认，也不自动提交。再次打开编辑器恢复草稿。数据刷新仅同步未编辑字段，提交成功后更新草稿基线。
5. 日志改为按视图键隔离查询、结果、输入、分页和展开状态；全局视图与 computer:<id> 分开。每个视图有独立请求序号，清理时递增上下文代次，防止旧响应回填。
6. 动态计算机采用当前对象的活动组件树，离开对象时保存轻量状态，不为每个历史对象保留运行资源。只为仍存在的对象保留状态，删除时清理，身份失效时清空相应身份状态。容量以当前对象集合为边界，结果/预览只保留有界数据；不通过静默淘汰丢弃草稿。
7. 复用现有 Zustand、Manager 和 runtime store，不更改后端协议或升级 ChatKit。权限撤销或对象消失时回退到有效页面，失效期间禁止旧操作回调提交。

## 文件与实施顺序

| 顺序 | 文件/模块 | 改动 |
| --- | --- | --- |
| 1 | 新增 src/stores/navigationStore.ts、src/components/Navigation/PageActivity.tsx、PageHost.tsx | 导航状态、显式目标、按需保留页面、活跃状态和清理边界 |
| 2 | src/App.tsx、src/styles/App.module.css | 接入页面宿主、菜单恢复和滚动管理，连接身份/对象失效 |
| 3 | src/components/Chat/index.tsx | 保持现有资源所有权，控制隐藏时的弹窗和历史浮层；核对 ChatKit 自带详情/附件预览 |
| 4 | src/components/Computer/index.tsx、ComputerWorkbench.tsx、useComputerWorkbenchSections.ts、ComputerSettings/index.tsx、Settings/index.tsx | 详情/设置/内部 section 恢复、显式定位、对象切换与快照恢复 |
| 5 | src/stores/activityStore.ts、src/components/ActivityViewer/index.tsx、Settings/DataSettings.tsx | 按视图隔离、请求代次、清理后刷新受影响视图，保留全局清理功能 |
| 6 | ComputerSettings/GeneralSettings.tsx、SkillsSettings.tsx、RemoteControlSettings.tsx；McpConfig/index.tsx、ConfigValueEditor.tsx；InputVariables/index.tsx；Computer/MarketplaceTab.tsx、SkillsTab.tsx、ComputerWorkbenchMoreActions.tsx；RobotConnectionPanel/index.tsx | 草稿恢复、刷新保护、隐藏弹窗、危险操作取消与异步结果隔离 |
| 7 | DesktopResources 及 DebugPanel 中有内部导航/预览状态的组件、ManagerAccount/EmployeeList.tsx | 展开/选择/分页与活跃生命周期；保留已有刷新能力 |
| 8 | src/test 下对应组件/store 测试、e2e/tests 导航场景、docs/test-architecture/issue-78-regression-matrix.md | 回归用例、逐页验收矩阵与桌面验证记录 |

新增公共抽象以调用点复用为前提；不为没有状态的页面增加空状态容器。具体新增测试文件按既有目录约定放置。

## 验证与交付条件

- 顶层菜单、计算机详情/七个设置 section、系统设置四个 tab，以及嵌套插件/MCP/调试页逐项列出：选择、查询、分页、滚动、草稿、弹窗、隐藏任务；不存在的项标 N/A。
- 组件测试覆盖非首个会话、附件草稿、事件详情和隐藏更新；记录 session open/close、factory/workspace 创建释放、会话加载与监听计数。往返不增加计数，退出与换身份按预期释放。
- Store 测试覆盖全局/多计算机日志互不覆盖、查询竞态、A→B→A 身份变化、删除与旧异步响应；表单测试覆盖后台刷新不覆盖脏字段。
- Playwright 驱动真实页面导航，覆盖懒加载、滚动、嵌套 tab、浮层及显式跳转。现有 Tauri mock 只作为浏览器回归，不代替桌面真实链路验收。
- macOS 桌面真实验证菜单/tab 往返、隐藏期间聊天更新、身份切换、权限失效、对象删除及资源计数；使用已有可用身份和实例。环境不足时记录未验证项，不将 mock 结果标为实机通过。
- 执行 pnpm lint:ts、pnpm lint:eslint、pnpm test、pnpm build 和本次新增的导航 E2E。若涉及 Rust 改动，补对应 Rust 检查；全量重型套件按项目规则处理。
- 使用无父会话继承的只读子代理，按 code-review rubric 审查完整任务变更；默认修复阻塞项并复审至清零。
- 本需求影响已有桌面 UAT 的导航行为，补导航验收场景；现阶段未发现必须新增 seed 的业务数据结构。

## 风险与审批点

风险为中高：页面保留会改变 effect、Portal 和异步回调的存活时间，必须与可见性及上下文失效一起处理。ChatKit 自带浮层和附件 UI 需在实现前核对本地 0.8.0 类型/实现；若缺少所需控制能力，先完成不依赖的模块并提出准确依赖，不能以 CSS 隐藏全局浮层代替生命周期管理。

用户已回复“确认”，按本方案实施。此文档不代表代码已完成或测试已通过；实际结果见验收矩阵，提交、推送和建 PR 另按授权处理。
