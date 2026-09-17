# Issue #88 Marketplace IPC 字段契约修复计划

状态：实现、验证及隔离审查完成；用户已授权 commit、push 和关单，交付执行中。最终交付 SHA 与工单状态见 GitHub #88 / TFRC-136。

来源：https://github.com/A2C-SMCP/tfrobot-client/issues/88
关联：TFRC-136；目标版本 v0.2.5；当前分支 dev-0.2.5。

## 需求及范围

用户在 Computer → Marketplace 填写远程 Git URL 后，添加请求应通过真实 Tauri 解析并完成可访问仓库的添加。本地仓库添加无回归。更新请求应正确解析，但继续返回现有“SDK 尚无原子更新 API”的业务限制，原来源保持不变。返回的来源摘要符合前端 displayGitUrl 声明。测试及证据使用无凭据 URL。

复用 GitHub #88 追踪，已添加 in-progress；关联 Jira 已只读核对。无新增工单或上游 SDK 变更需求。工作区已有其他任务修改，实施只修改下列本任务文件，保留其他修改。

## 已核对的实现及决策

- MarketplaceTab.tsx 构造 camelCase JSON，skillStore.ts 原样调用 Tauri，符合项目契约。
- commands/marketplace.rs 两个来源枚举仅使用 rename_all；其作用是转换变体名，未转换变体内字段。
- 在 MarketplaceSource 与 MarketplaceSourceSummary 上添加 rename_all_fields = "camelCase"，统一请求和返回值；不增加前端特殊字段转换。
- update_marketplace_core 的禁用来源更新保护保留。
- 现有组件/store 测试 mock invoke，marketplace_integration_test.rs 直接调用 core；二者不能证明真实 IPC 通过。
- 原生验收复用 e2e/chat-restoration 的真实 WKWebView、事件驱动运行方式，不增加轮询。

## 文件与实施顺序

1. src-tauri/src/commands/marketplace.rs：补真实前端 JSON 的 Serde 回归测试，覆盖添加/更新 × 远程/本地、缺字段错误及摘要 camelCase 序列化；先验证失败，再修正两个枚举的字段命名。
2. src/test/components/MarketplaceTab.test.tsx、src/test/stores/skillStore.test.ts：复用并按缺口补充真实表单/store 的请求与摘要契约断言。
3. src-tauri/examples/marketplace_ipc_acceptance.rs（新增）、e2e/marketplace-ipc/（新增验收页面、构建配置、运行脚本、说明）：以临时独立 AppState、真实生产 Marketplace 命令及 SDK，运行 WebView → Tauri IPC → Git 仓库添加。使用本机提供的无凭据 Git 测试仓库服务验证 remoteGit，另验证 localGit、摘要字段、更新业务拒绝且原来源不变。复用现有 fixture/初始化方式，禁止模拟所验证的 IPC、Git 克隆与 SDK 操作。
4. src-tauri/Cargo.toml：按现有 example 模式声明原生验收入口及必要 feature。

## 验证与门控

- 回归测试先在旧映射下失败，修复后通过。
- Rust Marketplace 单元与相关集成测试；前端 MarketplaceTab / skillStore 测试。
- cargo fmt 检查、相关 Clippy、TypeScript 与 ESLint 检查；区分已有工作区问题与本次引入问题。
- 当次构建并执行原生 IPC 验收至 PASS，验证远程和本地添加成功，更新进入业务限制。若真实环境无法运行，明确阻塞，不将 mock 或单纯 Serde 测试视为验收完成。
- 使用 fork_turns=none 的只读子代理执行完整隔离审查，阻塞问题修复后重新审查。
- 新增代码/测试文件形成完整内容后仅按精确路径暂存供审查；已有文件不暂存。提交、推送及 PR 等待后续明确授权。

## 风险

业务改动集中于两个 IPC 类型，风险低；原生验收需要可用 macOS GUI 和构建环境。当前工作区有较多未提交修改，质量门禁可能反映其他任务问题。验收使用独立临时数据，不读写真实用户仓库配置或凭据。


## 实施与验证记录（2026-09-16）

- 产品修改限定于两个来源枚举的字段映射。已有前端组件/store 测试已覆盖远程和本地表单 payload，直接复用，无需修改前端生产代码或重复增加同层测试。
- 新增 3 个 Rust 契约测试：添加/更新 × 远程/本地请求、来源摘要、缺失字段。旧映射下前两项失败（missing field git_url / display_git_url），修复后通过。
- 原生验收使用生产 skillStore、真实 WKWebView/Tauri 命令、SDK 与 Git。测试 Git 服务采用官方 git http-backend，支持 SDK 的 --depth 1 浅克隆；最初静态 HTTP 服务不支持浅克隆，已替换，仅影响测试设施。
- Rust Marketplace 单元测试：8/8；Marketplace 相关集成测试：9/9；MarketplaceTab/skillStore：35/35。
- TypeScript 生产配置及验收专用 tsconfig 检查通过；cargo fmt 检查通过。
- Clippy --lib --tests --example marketplace_ipc_acceptance --features marketplace-ipc-acceptance -- -D warnings 通过。
- pnpm lint:eslint 全仓扫描被既有 experiments 下的编译产物阻断（24 个解析错误）；不修改项目 lint 配置，使用 pnpm exec eslint . --ignore-pattern 'experiments/**' 复核后为 0 错误、6 条既有警告。
- 原生验收：remoteGit 添加和目录读取 PASS；localGit 添加和目录读取 PASS；两种更新来源均到达 atomic update API 业务拒绝，完整 governance 保持一致；远程 Git HTTP 请求实际发生 3 次。
- 原生证据：/private/var/folders/v7/6wb5d0ks3rx3v0wg682l2m6c0000gn/T/marketplace-ipc-gi8Y5D/result.json。重跑步骤见 e2e/marketplace-ipc/README.md。
- git diff --check 和 git diff --cached --check 通过。无真实凭据参与本次验收。
- 用户已授权提交、推送到当前 dev-0.2.5 分支并关单；核实远端提交后回写 GitHub #88 和 TFRC-136 的交付证据。


## 隔离审查结论

- fork_turns=none 只读 code-review：APPROVE，0 阻塞、1 非阻塞建议。
- W1：e2e/marketplace-ipc/run.mjs 超时/子进程启动失败会跳过 result.json 保存；正常完成路径已有证据。建议后续统一记录异常失败原因和临时目录。
- 按 add-feature 默认 fix-review block 范围，无需修复项；保留 W1，不扩大当前修改。
- 审查者核对了原生证据、生产 store 调用链和两部分 diff。代码未经审查后修改。
