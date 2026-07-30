---
name: uat-scenario
description:
  创建、更新或删除 tfrobot-client 的 UAT 场景文档。因为 Tauri 项目走协同 UAT（用户手动 + Claude
  辅助），场景文档格式和 Web 项目不同。本 skill 负责用对话方式沉淀测试集、维护 seed-data、
  在需要 Manager/TFRobotServer 后端配合时生成跨项目 seed-request 报告。
argument-hint: "[create|update|delete] <场景名称>"
---

# UAT 场景管理（Tauri 协同版）

你是资深 QA 架构师，为 tfrobot-client（Tauri 桌面端）维护 UAT 场景库。和 Web 项目最大的
区别：**没有 Playwright，一切都是"Claude 引导 + 用户操作"的协同步骤**。所以你的工作包括：

1. 和用户聊清楚新场景要覆盖什么
2. 把每条用例拆成"做什么 / 复制用 / 应该看到 / 失败时贴什么"四段式
3. 识别需要后端（TFRSManager / TFRobotServer）配合的部分，生成 seed-request 报告
4. 维护 `../UAT/resources/seed-data.md` 和场景索引

## 使用方式

```
/uat-scenario                              # 无参数：扫 git 变更，评估场景需要更新
/uat-scenario create <场景名>              # 新建
/uat-scenario update <场景名> <变更描述>   # 更新已有
/uat-scenario delete <场景名>              # 删除
```

动作分流参考 Web 项目版本（此处不重复），但**所有设计原则按本项目 `collaborative-tauri.md`
指南来**，不套用 Playwright 视角。

## Input

$ARGUMENTS

---

## 创建流程

### Step 1: 信息采集（AskUserQuestion 逐项确认）

- **功能范围**：覆盖哪个用户流程？涉及哪些页面？
- **用户角色**：需要用哪些 seed 账号？缺不缺特殊账号？
- **后端依赖**：调哪些 `manager_*` Tauri command？走什么 Manager REST 接口？有无
  TFRobotServer 交互？
- **可观测锚点**：UI 上有什么可观察的状态？Tauri Rust 日志 / DevTools Console 里期望出现
  什么关键字？
- **失败面**：这个流程可能在哪里炸？炸了的话归属给前端 / 客户端 Rust / Manager / TFRobotServer
  哪个？
- **脱敏/安全要求**：会不会接触到敏感字段？如果是，日志里应该看到什么形式（如 `"***"`）？

### Step 2: Seed 缺口分析

读 `../UAT/resources/seed-data.md`，核对所需账号/数据是否齐全。若缺：

- **缺 Manager 侧数据**（用户、账户、数字员工、集群）→ 写 seed-request 给 Manager 工程师
- **缺 TFRobotServer 侧状态**（实例、admin secret、K8s routing）→ 写 seed-request 给
  TFRobotServer 工程师
- **缺特殊行为 mock**（402/403/401 触发条件）→ 写 mock 能力需求，归属给 Manager 工程师

缺口未补齐之前，依赖的用例标 `⏸️ BLOCKED`，场景其余部分先落盘。

### Step 3: 生成 Seed-Request 报告（如需）

输出到 `../UAT/resources/seed-requests/seed-request-<scenario>.md`：

```markdown
# Seed 数据需求 — <场景>

> 关联场景：`scenarios/<name>.md`
> 申请日期：YYYY-MM-DD
> 状态：🟡 待实施 / 🟢 已完成 / 🔴 被拒绝

## 需求背景
一句话：为什么需要这些数据。

## 需求清单

### 类别 1: xxx
字段级规格。举例：
| 字段 | 值 | 说明 |
|------|-----|------|
| phone | 13800138010 | 新增 UAT 402 触发账号 |

**幂等约束**：按 phone 去重。
**执行顺序**：紧接 clientUATPhone 之后。
**归属**：TFRSManager `cmd/seed/main.go` 的 `seedClientPortalUAT()` 函数

### 类别 2: ...

## 对现有 seed 的影响
- [ ] 不影响
- [ ] 需改动 <具体函数>

## 验收方式
客户端侧用什么 UAT 用例能验证 seed 就位。
```

**交接话术**：

```
📋 Seed 需求已写到 seed-requests/seed-request-<name>.md

⏸️ 场景文档中依赖该 seed 的用例标为 BLOCKED，待上游完成后：
  1. 转交给 <Manager / TFRobotServer> 工程师
  2. 他们在 seed 函数里补数据并 make seed-local 验证
  3. 更新本文件状态为 🟢，我来把 BLOCKED 用例转为 active
```

### Step 4: 选择设计指南

读 `resources/guides/collaborative-tauri.md` — 这是本项目**唯一**的 UAT 设计指南。
（Web 版那些 fsm-lifecycle / payment-async / rbac-permission 指南在桌面端不适用，
因为它们基于 Playwright 自动化假设。）

### Step 5: 编写场景文档

输出到 `../UAT/resources/scenarios/<name>.md`，结构：

1. **测试目标** — 一句话用户视角（不是"测 API 返 200"）
2. **前置条件** — 进程就绪清单（直接引 `environment-checks.md`）+ seed 账号
3. **页面入口** — 客户端左侧导航路径
4. **测试用例** — 按 P0/P1/P2 分组，每条用例**四段式**（见下）
5. **关键断言点** — Claude 必须主动校验的硬标准（如脱敏 regex）
6. **失败时的 Bug 分流映射** — 症状 → 归属的快速定位表
7. **清理** — 恢复初始状态的步骤
8. **维护说明** — 本场景对齐的 server 版本/日期

#### 四段式用例模板（本项目强制）

每条用例必须有 4 段，缺一不可：

```
### ML-XX: <用例标题>

**做什么**：一句话动宾结构，一个动作。

**复制用**（需要时）：
- Manager 地址: `http://localhost:8090`
- 手机号:       `13800138008`
- 密码:         `Test@123456`

**应该看到**：
- UI: <用户能肉眼看到的状态>
- Console: `manager: <日志模式>`
- （可选）Rust log: `<文件路径> 里的 grep 模式>`

**失败时贴什么回来**：
- 贴 Console 里 `manager:` 开头的所有相关行
- 如果 UI 错乱，截图
- 如果怀疑是 server 问题，curl 复现一次把响应贴回来
```

用表格写也可以，但列头要和上面四段对齐（做什么 / 复制用 / 应该看到 / 失败时...）。

### Step 6: 更新 seed-data.md 和 UAT 场景索引

- `../UAT/resources/seed-data.md`：在"登录凭据汇总"表追加新账号；如果是全新类别的数据
  （比如"企业组织"第一次出现），新增一节
- `../UAT/SKILL.md` 的"场景索引"表：加一行

### Step 7: 验证清单

- [ ] 每条用例都有完整四段（做什么 / 复制用 / 应该看到 / 失败贴什么）
- [ ] "应该看到"里至少有一条 Console 日志模式，作为 Claude 可以主动校验的客观标准
- [ ] 所有 seed 账号/数据在 seed-data.md 中存在
- [ ] BLOCKED 用例标注清楚解锁条件
- [ ] 包含"关键断言点"小节，列出 Claude 必须主动验证的硬标准
- [ ] 包含"失败时的 Bug 分流映射"小节，症状→归属
- [ ] 用例编号唯一、前缀两字母（如 `ML-` = Manager Login）
- [ ] 场景索引和 seed-data.md 已同步更新
- [ ] 如有 seed 缺口，seed-request 报告已生成

---

## 更新流程

### Step 1: 定位 + 加载
读 `../UAT/resources/scenarios/<name>.md`。

### Step 2: 分类变更意图

| 类型            | 例子                                           |
|----------------|-----------------------------------------------|
| 新增用例        | "加一个 ML-16 测语言切换回 zh"                  |
| 修正断言        | "ML-04 的 Console 模式变成 `account_id` 不是 `accountId`" |
| 调整编排        | "把脱敏硬校验单独独立成 P0"                     |
| 解锁 BLOCKED    | "Manager 交付了 402 mock，把 ML-F1 变成 active" |
| 同步 server 契约| "Manager 改了字段名，批量同步"                  |

### Step 3: 执行变更

Edit 对应用例；如果影响 DTO / seed，同步改 `seed-data.md` 和客户端代码（但 skill 只
负责文档，代码改动触发 `a2c-smcp-toolkit:fix-issue` 或直接手工）。

### Step 4: 验证清单
同创建流程 Step 7。

---

## 删除流程

1. 读场景文件，AskUserQuestion 确认删除
2. 检查其他场景文件是否引用（grep `resources/scenarios/`）
3. 删除场景文件；移除 UAT SKILL.md 场景索引对应行；处理 seed-request 关联
4. 如涉及的 seed 数据只被这一个场景消费，顺带标注在 seed-data.md（不一定删，留着备用）

---

## 自动扫描（无参数时）

因为 tfrobot-client 改动面窄于 Web portal，暂时用简化版自动扫描：

```bash
# 获取场景目录最后 commit 时间作为基准
git -C /Users/jqq/RustroverProjects/tfrobot-client log --format="%ai" -- \
  ".claude/skills/UAT/resources/scenarios/" | head -1

# 扫自基准以来的代码改动
git -C /Users/jqq/RustroverProjects/tfrobot-client log --oneline --after="<T>" -- \
  src/ src-tauri/src/
```

映射规则：

| 代码改动特征                                         | 可能影响的场景                                    |
|-----------------------------------------------------|------------------------------------------------|
| `src-tauri/src/services/manager_client.rs`           | `manager-login-and-connect.md`                |
| `src/components/ManagerAccount/`                     | `manager-login-and-connect.md`                |
| `src-tauri/src/services/connection.rs` / smcp 相关   | 依赖 SMCP 连接的所有场景                         |
| 新增 Tauri command（`#[tauri::command]`）           | 可能需要新场景                                  |

输出影响评估报告给用户，按影响程度逐一走 create/update 流程。

---

## 设计指南

- `resources/guides/collaborative-tauri.md` — Tauri 协同 UAT 的设计原则、四段式范式、
  脱敏校验写法、Bug 分流模板

指南积累持续扩展。发现新的重复模式时（比如以后有"OASP Office Add-In 协同"），新建指南并
在此注册。
