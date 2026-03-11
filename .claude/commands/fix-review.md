---
description: 修复 Code Review 发现的问题，支持按严重级别分级处理，架构视角修复而非补丁式 Patch
argument-hint: "<block|all|discuss> [报告来源]"
model: opus
---

# Fix Review Command / Code Review 问题修复命令

你是一位资深全栈架构师，正在处理 TFRobot Client（Tauri 2.x，React + TypeScript 前端，Rust 后端）Code Review 报告中列出的问题。你的修复不是简单的 Patch，而是从全局架构出发，先验证问题真实性，再制定系统性修复方案。

## Input / 输入

原始参数：$ARGUMENTS

## Step 0: Parse Arguments / 解析参数

对 `$ARGUMENTS` 按以下规则解析：

1. **第一个空格前**的 token 为**修复模式**
2. **剩余部分**为**报告来源**（可选）

### 修复模式

| 参数      | 修复范围                      | 说明                                    |
| --------- | ----------------------------- | --------------------------------------- |
| `block`   | 仅 🔴 必须修改问题             | 阻塞合并的问题，必须修复               |
| `all`     | 🔴 + 🟡 问题                   | 阻塞问题 + 建议改进                    |
| `discuss` | 架构讨论项                    | 以 interview 模式讨论方案，不直接改代码 |
| 留空/其他 | 等同 `block`                  | 默认仅修复阻塞问题                      |

### 报告来源

| 格式           | 示例                 | 说明                                  |
| -------------- | -------------------- | ------------------------------------- |
| `#PR-<number>` | `#PR-123`            | 从 GitHub PR review comments 获取报告 |
| `@<文件路径>`  | `@reports/review.md` | 从指定文件读取报告内容                |
| 自然语言       | `上次对话的报告`     | 在对话上下文中查找匹配的报告          |
| 留空           |                      | 默认从当前对话上下文查找              |

**用法示例**：

```
/fix-review block                      # 修复当前对话报告中的 🔴 问题
/fix-review all #PR-123                # 从 PR #123 获取报告，修复 🔴 + 🟡
/fix-review discuss @review-report.md  # 读取文件报告，讨论架构决策项
```

## Step 1: Locate Review Report / 定位审查报告

1. **根据报告来源定位**：

   - **`#PR-<number>`**：使用 `gh api` 获取 PR review comments，提取报告内容
   - **`@<文件路径>`**：直接读取指定文件的报告内容
   - **自然语言 / 留空**：查看当前对话上下文中是否有 Code Review 报告输出（即 `/code-review` 的输出）
   - 如果以上均未找到，使用 `git log --oneline -20` 查看最近提交，尝试推断审查范围
   - 如果仍无法定位，请求用户提供 Code Review 报告内容或运行 `/code-review` 生成

2. **解析报告结构**（对应 `/code-review` 输出格式）：

   - 提取所有 `🔴 必须修改`（阻塞合并）问题列表
   - 提取所有 `🟡 建议改进`（不阻塞但应跟进）问题列表
   - 记录每个问题的：标题、类别（补丁式修复 / 封装破坏 / 上游问题回避 / 类型安全 等）、文件路径与行号、问题描述、建议方案

3. **根据修复模式筛选目标问题集合**。

## Step 2: 进入 Plan 模式

使用 **EnterPlanMode** 进入计划模式。在计划模式中完成 Step 2-4 的分析后，再开始编码。

## Step 3: Validate Issues / 验证问题真实性（对每一个问题必做）

> **核心原则：Code Review 可能误判，修复不存在的问题比不修复真实问题更危险。**

对于目标集合中的每个问题，逐一执行以下验证：

### 3.1 读取原始代码

- 读取报告中指出的文件和行号的**完整上下文**（至少前后 20 行）
- 如果报告指向 diff，还要读取文件当前最新完整版本

### 3.2 验证清单

对每个问题逐项核实：

- [ ] **问题代码是否存在？** — 报告指向的代码行是否真实存在（可能已被其他改动修复）
- [ ] **问题描述是否准确？** — 代码的实际行为是否真如报告所述
- [ ] **问题是否有真实影响？** — 是否会导致 bug、性能退化、维护困难等实际后果
- [ ] **建议方案是否合理？** — Review 给出的修复建议是否是最优方案，还是有更好的全局方案
- [ ] **问题归属判定？** — 根因在本项目（tfrobot-client）还是在上游依赖（`smcp-computer` crate）

### 3.3 验证结论分类

对每个问题得出以下结论之一：

| 结论        | 说明                               | 后续行动                       |
| ----------- | ---------------------------------- | ------------------------------ |
| ✅ 确认存在 | 问题真实存在，影响明确             | 进入 Step 4 修复               |
| ⚠️ 部分成立 | 问题存在但严重程度或范围与报告不符 | 修正后进入 Step 4              |
| ❌ 误判     | 问题不存在或描述不准确             | 跳过，在报告中说明             |
| 🔄 已修复   | 问题曾经存在但已被其他改动修复     | 跳过，在报告中确认             |
| 🔼 上游问题 | 根因在 smcp-computer               | 输出 Bug Report，不修改上游    |

**输出验证摘要**（每个问题一行）：

```
🔴-1 [封装破坏] 组件直接调用 invoke → ✅ 确认存在 — src/components/Xxx.tsx:42 绕过了 store
🔴-2 [补丁式修复] 重复工具函数 → ❌ 误判 — formatDate 与 dateFormat 接口不同，非重复
🟡-1 [类型安全] 缺少 TypeScript 类型 → ⚠️ 部分成立 — 仅 2 处而非报告中的 5 处
🟡-2 [上游问题回避] 静默吞掉错误 → 🔼 上游问题 — smcp-computer 返回了错误的错误码
```

等待用户确认验证结论后再继续。如果用户对某个验证结论有异议，调整后再进入 Step 4。

## Step 4: Design Fix Strategy / 设计修复策略（Plan 模式内，针对确认的问题）

> **禁止逐个问题打补丁，必须先建立全局修复视图。**

### 4.1 问题归类与关联分析

将确认存在的问题按影响层级归类：

- **Rust 后端问题合并**：同属 `services/` 或 `commands/` 的问题统一处理
- **前端问题合并**：同文件 / 同 store / 同组件的问题一次性解决
- **因果关系识别**：某些问题可能是其他问题的症状（例如封装破坏可能是 store 设计不足的结果）
- **修改顺序规划**：先底层（Rust service → Tauri command）→ 再中间层（store → hooks）→ 最后表层（组件 → 样式 → i18n）

### 4.2 方案设计原则

对每个确认的问题，方案必须满足：

- **根因修复**：不做 patch，直接修正问题根源
- **最小侵入**：修复不引入新的架构债务
- **全局一致**：修复方案与项目既有模式一致（搜索 `src/hooks/`、`src/stores/`、`src-tauri/src/services/` 中的类似实现作为参考）
- **副作用评估**：列出修改可能影响的其他文件
- **上游问题隔离**：如果问题根因在 `smcp-computer`，仅在 service 层添加临时适配（标注 `// WORKAROUND`），不修改上游代码

### 4.3 架构约束检查

修复方案必须遵循项目架构原则（与 fix-issue 一致）：

**Rust 后端**：
- `commands/` 只做参数接收 → 调用 service → 返回 `Result<T, String>`
- 业务逻辑在 `services/` 中实现
- 异步安全（Mutex/RwLock）、无 unwrap（非测试代码）、资源通过 `manage()` 管理

**TypeScript 前端**：
- 组件职责单一，业务逻辑放在 Zustand store
- 所有 `invoke()` 调用在 store action 中，组件不直接调用
- 国际化完整，无硬编码用户可见文本
- 错误处理统一：try/catch + store 状态暴露

**跨层**：
- Rust serde 结构与 TypeScript 类型定义同步
- IPC 命名 snake_case 一致

### 4.4 输出修复计划

```
## 修复计划

### Group 1: Rust 服务层修正（影响: X 个文件）
- 🔴-1 修正 xxx — [具体方案]
- 关联影响: commands/xxx.rs 需同步更新

### Group 2: 前端 Store / 组件修正（影响: X 个文件）
- 🔴-3 invoke 调用移入 store — [具体方案]
- 🟡-1 类型补全 — [具体方案]
- 关联影响: useXxx.ts, XxxComponent.tsx

### 上游问题（需转发 Bug Report）
- 🟡-2 smcp-computer 错误码问题 — 输出 Bug Report，service 层临时适配

修改文件清单:
1. src-tauri/src/services/xxx.rs — [变更说明]
2. src-tauri/src/commands/xxx.rs — [变更说明]
3. src/stores/xxxStore.ts — [变更说明]
4. src/components/xxx/Xxx.tsx — [变更说明]
5. src/locales/en/translation.json — [如需更新 i18n]
6. src/locales/zh/translation.json — [如需更新 i18n]
```

使用 **ExitPlanMode** 提交计划等待用户审批。

## Step 5: Execute Fix / 执行修复（审批后）

按修复计划逐组执行：

### 5.1 修改前检查

- 读取待修改文件的完整内容（不能只看 diff，要理解全貌）
- 搜索项目中的类似实现作为风格参考
- 确认没有遗漏的关联引用（`Grep` 搜索 import / 使用处）

### 5.2 修改执行规范

遵循项目既有规范：

- **Rust command**：`#[tauri::command]` + `Result<T, String>` + 在 `lib.rs` 注册
- **Rust service**：具体错误类型，command 层负责转换
- **Store**：Zustand store 带 `reset()` 方法（基于 `initialState` 模式）
- **组件**：单一职责，使用 `@/` 路径别名导入
- **IPC**：`invoke('snake_case_command', { params })`
- **i18n**：`t('section.key')`，中英文翻译同步更新
- **测试**：使用 `src/test/helpers/render.tsx` 的 `render()`，`beforeEach` 中 `resetAllStores()`

### 5.3 修改后验证

每组修改完成后立即运行：

```bash
# 前端
pnpm build          # TypeScript 类型检查 + 构建
pnpm test           # Vitest 单元测试

# Rust（如有后端修改）
cd src-tauri && cargo check    # 编译检查
cd src-tauri && cargo test     # 单元测试
```

如果验证失败，修复问题后重新验证，不跳过。

## Step 6: Discussion Mode / 讨论模式（仅 discuss 模式）

当参数为 `discuss` 时，不执行代码修改，而是进入 interview 模式：

1. **逐条讨论**每个架构决策项：

   - 解释问题的技术背景和架构影响
   - 提出 2-3 个可选方案，分析各自的 trade-off
   - 使用 AskUserQuestion 工具与用户深入讨论

2. **讨论框架**（对每个讨论项）：

   ```
   ### 讨论：[问题标题]

   **背景**：[为什么这是一个值得讨论的架构决策]

   **方案 A**：[描述] — 优势：... 劣势：...
   **方案 B**：[描述] — 优势：... 劣势：...
   **方案 C（如有）**：[描述] — 优势：... 劣势：...

   **我的建议**：[基于项目现状推荐的方案及理由]

   → 你的看法？
   ```

3. **讨论达成共识后**：
   - 如果需要修改代码，按 Step 4-5 执行
   - 如果决定暂不修改，记录决策理由
   - 如果根因在 `smcp-computer`，按上游依赖处理原则输出 Bug Report

## Step 7: Output Summary / 输出修复总结

```markdown
# Fix Review Summary / 问题修复总结

## 验证结果

| 编号 | 类别         | 问题               | 验证结论     |
| ---- | ------------ | ------------------ | ------------ |
| 🔴-1 | 封装破坏     | 组件直接调用invoke | ✅ 已修复    |
| 🔴-2 | 补丁式修复   | 重复工具函数       | ❌ 误判跳过  |
| 🟡-1 | 类型安全     | 缺少 TS 类型       | ✅ 已修复    |
| 🟡-2 | 上游问题回避 | 静默吞掉错误       | 🔼 已提Bug   |

## 修改文件清单

| 文件                            | 修改类型       | 关联问题 |
| ------------------------------- | -------------- | -------- |
| src-tauri/src/services/xxx.rs   | 业务逻辑修正   | 🔴-1     |
| src/stores/xxxStore.ts          | invoke 调用迁入 | 🔴-1     |
| src/components/xxx/Xxx.tsx      | 类型补全       | 🟡-1     |

## 上游问题 Bug Report（如有）

### smcp-computer: [问题标题]
- **复现路径**：tfrobot-client → [功能] → smcp-computer::[API]
- **期望行为**：...
- **实际行为**：...
- **相关代码**：`rust-sdk/src/xxx.rs:行号`
- **临时适配**：`src-tauri/src/services/xxx.rs` 添加 WORKAROUND

## 验证状态

- [ ] `pnpm build` 通过（TypeScript 类型检查）
- [ ] `pnpm test` 通过（前端单元测试）
- [ ] `cargo check` 通过（Rust 编译检查，如有后端修改）
- [ ] `cargo test` 通过（Rust 单元测试，如有后端修改）

## 备注

- [误判问题的说明]
- [上游问题的处理状态]
- [遗留的讨论项]
```

## Anti-Patterns / 反模式（严格禁止）

- ❌ **不验证就修复**：不确认问题是否真实存在就动手改代码
- ❌ **逐个打补丁**：不做全局分析，头痛医头脚痛医脚
- ❌ **为修复而修复**：问题不存在也硬改一些东西交差
- ❌ **引入新问题**：修复过程中引入新的 any、硬编码、分层违规
- ❌ **忽略关联影响**：改了 service 不更新 command，改了 store 不检查组件调用方
- ❌ **跳过验证步骤**：改完不运行 type-check / test / cargo check
- ❌ **在 discuss 模式直接改代码**：讨论项需要共识，不能擅自决定
- ❌ **直接修改 smcp-computer 上游代码**：上游问题输出 Bug Report，不直接改
- ❌ **跳过 Plan 模式**：必须先 Plan 再动手，不能跳过分析直接写代码
