# TFRobot Client Bash MCP 工具选型决策报告

> 检查日期：2026-08-12
> 研究方式：项目调用链核查、候选官方文档与源码静态审阅；未执行第三方候选代码
> 适用项目：`tfrobot-client` 0.2.x / A2C-SMCP rust-sdk `6fe58efd15d9e12e9aef5ba7bf4f7c1075e9a419`

## 决策摘要

- **结论：GO（分阶段自研集成，不直接嵌入任一现成 Agent 的 Bash 工具）**
- **生产方案：** 在 `tfrobot-client` 内实现独立的、保留 Bundle ID 的 Rust 内存 MCP Provider（暂称 `tfrobot_shell`），复用项目已有 `MCPClientProtocol`/`ClientFactory` 注入点。
- **参考实现：** 以 Codex 的“审批与 OS 沙箱分层、进程组回收、输出限流”作为主架构参考，以 `mcp-shell-server` 的 argv 模式、参数硬化和测试样例作为输入校验参考，以 Pi 的流式输出/截断体验作为交互参考。
- **PoC 备选：** 如需先验证模型调用效果，可把固定版本 `mcp-shell-server==1.1.8` 作为短期 stdio MCP 探针；它不应在无额外沙箱时承担生产包安装或任意命令执行。
- **不推荐：** 直接嵌入 Codex、Claude Code、Pi、Desktop Commander 或 `mcp-server-commands` 作为生产 Bash 核心。
- **置信度：中高。** 架构方向的证据充分；跨平台 OS 沙箱、审批 UI 与 SDK 取消语义仍需实现前实验。
- **一句话理由：** 当前项目真正缺的不是 MCP 进程管理，而是一个能在远端 Agent 调用路径上自行完成审批、隔离、进程监督和审计的受控 Shell 执行域。

## 1. 决策问题与边界

### 1.1 目标场景

面向连接到 TFRobot Client Computer 的 Agent，提供以下命令行能力：

1. 编程：构建、测试、格式化、Git 状态与有限写操作。
2. 搜索：`rg`、`find`、`git grep` 等只读检索。
3. 项目依赖安装：如 `npm/pnpm/cargo/pip/poetry` 的项目级安装。
4. 长任务治理：超时、取消、子进程回收、输出限流与审计。

### 1.2 硬约束

- 以 MCP 工具暴露给 Agent，并纳入现有 Computer 生命周期。
- 默认不得把宿主机、用户主目录、密钥环境变量和无限制网络直接暴露给模型。
- 包安装不能仅按“命令名在允许列表”判定安全，因为安装器会执行生命周期脚本、编译器和任意子进程。
- 远端调用必须在服务端执行前完成授权；只在提示词中约束或只依赖 UI 调试入口都不成立。
- 应支持 macOS、Linux，并为 Windows 明确降级或后续路线。

### 1.3 非目标

- 不构建完整终端模拟器或 IDE。
- 首期不提供 SSH、`sudo`、系统包管理器和宿主机任意目录访问。
- 不用 Shell 重复实现已有的 MCP 文件、资源、Client Control 与桌面工具。
- 不把 Agent 框架整体嵌入客户端；只借鉴其执行器设计。

### 1.4 关键假设

- **假设 A：** Agent 可能处理不可信仓库、网页内容或依赖包，因此要按“可能发生提示注入与供应链代码执行”设计。
- **假设 B：** “安装包”首期指项目依赖，不包含 Homebrew/APT/系统级 Python 等宿主机变更。
- **假设 C：** 用户愿意对高风险命令逐次授权，或者配置有边界的会话级策略。

若这些假设不成立，尤其是明确限定为“单用户、完全可信仓库、只读命令”，可以采用更轻量的现成 MCP。

## 2. 场景与问题证据

| 主张 | 证据 | 类型 | 反证/限制 | 置信度 |
|---|---|---|---|---|
| Shell 能显著补齐 Agent 的编程闭环 | Codex、Claude Code、Pi、Gemini CLI 均把 Shell 作为核心工具 | 外部产品/源码 | 不能证明所有用户都需要写操作 | 高 |
| 命令允许列表不能替代沙箱 | Desktop Commander 明确说明目录限制与 blocklist 可被终端命令绕过；包管理器还能执行脚本 | 官方安全说明 | 对严格只读 argv 子集仍有价值 | 高 |
| 远端 Agent 调用需要独立审批关口 | 当前 SDK 的 cancellable 路径直接调用工具，不经过普通 `execute_tool` 的 confirm 回调 | 项目依赖源码 | 未来 SDK 可能新增统一审批回调 | 高 |
| 直接 shell 字符串拼接是高风险实现 | Git MCP Server 曾因 `child_process.exec` 命令注入产生 CVE-2025-53107 | 安全公告 | 修复后的实现不代表所有 shell 字符串都不可用 | 高 |

成功指标：

- 只读搜索、构建和测试命令的成功率满足 Agent 工作流，无需暴露宿主机全部权限。
- 高风险命令 100% 在 spawn 前命中审批或明确拒绝。
- 超时/取消后进程树可验证地退出；输出与历史记录均有上限和脱敏。
- 默认配置下不能读工作区外文件、继承父进程密钥或访问未授权网络目标。
- 项目依赖安装只写入工作区/隔离缓存，不允许系统级安装。

## 3. 项目现状与根因

| 项 | 当前状态 | 项目证据 | 对决策的影响 |
|---|---|---|---|
| 技术栈 | Tauri 2、Rust、Tokio，已依赖 shell 插件 | [`Cargo.toml`](../src-tauri/Cargo.toml) | 优先采用 Rust 内存 Provider，减少附加运行时 |
| MCP 生命周期 | SDK Computer 已管理 MCP 注册、启动、工具发现、执行和取消 | [`debug.rs`](../src-tauri/src/commands/debug.rs) | 不必再引入一套 MCP 管理层 |
| 内存 MCP 扩展点 | Client Control 用保留 Bundle ID 截获 stdio 配置，在 spawn 前返回 `MCPClientProtocol` 实现 | [`provider.rs`](../src-tauri/src/services/client_control/provider.rs) | 可按同一模式实现 `tfrobot_shell`，无 sidecar 协议损耗 |
| 授权策略基础 | Client Control 已有默认关闭、工具范围、目标范围与风险分类 | [`policy.rs`](../src-tauri/src/services/client_control/policy.rs) | 可复用策略思想与持久化模式，但不应混入同一工具域 |
| 可观测性 | 工具调用有 request ID、120 秒默认超时、历史记录和敏感字段脱敏 | [`debug.rs`](../src-tauri/src/commands/debug.rs) | 可复用关联 ID 和审计管线 |
| Tauri 权限 | 默认窗口当前拥有 execute/spawn/kill 权限 | [`default.json`](../src-tauri/capabilities/default.json) | 这是 UI 能力，不是远端 Agent 的命令安全边界；后续应收紧 |
| SDK 版本 | 固定到 rust-sdk commit `6fe58e…` | [`Cargo.toml`](../src-tauri/Cargo.toml) | 结论可复现，但升级 SDK 后需复核执行链 |

### 3.1 已有覆盖

- MCP server 配置、运行状态、工具发现和调用。
- 可取消工具调用、默认超时、工具历史、错误脱敏。
- 内存 Provider 注入点和默认拒绝的远端控制策略范式。

### 3.2 真正缺口

1. Shell 专用的命令模型与风险分类。
2. spawn 前的审批状态机，而不是执行后的日志。
3. 工作区文件系统、网络、环境变量与子进程的 OS 级边界。
4. 进程树/PTY/输出背压等 Shell 生命周期管理。
5. 包安装的独立高风险策略。

### 3.3 关键调用链发现

项目 UI 调试入口调用 `execute_tool_cancellable` 并生成 request ID；SDK 的远端 Socket.IO 工具调用也走 cancellable 路径。固定 SDK 版本中，普通 `execute_tool` 会触发 confirm callback，但 `execute_tool_cancellable` 直接执行 MCP tool。因此：

> **推断：** 仅把第三方 Bash MCP 注册进当前 Computer，并不会自动获得 Tauri UI 审批。审批必须进入 Shell Provider 的 spawn 前路径，或先推动 rust-sdk 提供统一、可取消的审批钩子。

SDK 证据：[`computer.rs`](https://github.com/A2C-SMCP/rust-sdk/blob/6fe58efd15d9e12e9aef5ba7bf4f7c1075e9a419/crates/smcp-computer/src/computer.rs)、[`socketio_client.rs`](https://github.com/A2C-SMCP/rust-sdk/blob/6fe58efd15d9e12e9aef5ba7bf4f7c1075e9a419/crates/smcp-computer/src/socketio_client.rs)。

## 4. 外部方案证据研究

### 4.1 候选与版本

| 候选 | 类型/许可证 | 本次核查版本 | 维护状态 | 定位 |
|---|---|---:|---|---|
| [OpenAI Codex](https://github.com/openai/codex) | Agent / Apache-2.0 | `2230d644…` | 活跃 | 最强架构参考，可选重型 sidecar |
| [Claude Code](https://github.com/anthropics/claude-code) | 闭源产品；仓库 All Rights Reserved | 2026-08-12 文档 | 活跃 | 产品与安全模型参考，不能复用核心实现 |
| [Pi](https://github.com/earendil-works/pi) | Agent / MIT | `534bcbff…` | 活跃 | 轻量执行与流式 UX 参考 |
| [Gemini CLI](https://github.com/google-gemini/gemini-cli) | Agent / Apache-2.0 | 2026-08-12 文档 | 活跃 | 沙箱扩权、PTY 与策略参考 |
| [`mcp-shell-server`](https://github.com/tumf/mcp-shell-server) | 专用 MCP / MIT | `v1.1.8`, `b0404b1c…` | 活跃 | 最适合 PoC 的现成 MCP |
| [Desktop Commander MCP](https://github.com/wonderwhy-er/DesktopCommanderMCP) | 终端/文件 MCP / MIT | `v0.2.47`, `9bd8422d…` | 活跃 | 功能完整，但宿主机安全边界不足 |
| [`mcp-server-commands`](https://github.com/g0t4/mcp-server-commands) | 命令 MCP / MIT | `v0.8.2`, `bc62283f…` | 活跃 | 极简，但生产安全能力不足 |
| [Strands Shell](https://github.com/strands-agents/shell) | 用户态 Shell / Apache-2.0 | `b8b10223…` | 活跃 | 安全受限 Shell；不能执行任意宿主二进制 |

本次源码快照位于 `/tmp/tfrobot-bash-tool-research.R4ktWN`，便于复核；未运行这些项目，也未删除该临时目录。

### 4.2 Agent 内置 Bash 工具对比

| 方案 | 执行模型 | 审批/策略 | 隔离 | 生命周期 | 对本项目的结论 |
|---|---|---|---|---|---|
| Codex | Rust executor；命令解析后运行，支持独立 command exec 协议 | 安全命令识别、按需审批，审批与沙箱分层 | macOS Seatbelt、Linux Landlock/bubblewrap、Windows restricted token 等 | 超时、取消、进程组 kill、输出限制、PTY/流式能力 | **最佳设计参考；不建议直接依赖。** app-server/sandboxing 是大型内部 workspace，包装后多一层 JSON-RPC 和分发依赖 |
| Claude Code | 产品内 Bash tool | `deny → ask → allow`，复合命令逐段匹配，hooks | macOS Seatbelt、Linux bubblewrap/WSL2，文件与网络边界 | 产品提供后台任务和会话行为 | **仅作参考。** [许可证](https://github.com/anthropics/claude-code/blob/main/LICENSE.md)不是开源许可证，无法嵌入核心实现 |
| Pi | TypeScript `spawn` 用户 shell，可替换 Bash operations | README 明确无内置权限系统 | 推荐外置 Gondolin/Docker/OpenShell | AbortSignal、进程树 kill、流式输出、截断后写临时完整日志 | **适合借鉴 UX，不适合直接生产。** 默认继承用户权限，且 TS sidecar 与 Rust 主体不匹配 |
| Gemini CLI | shell/PTY，支持后台进程 | policy engine + 手工确认，安装等命令可申请 sandbox expansion | Seatbelt、Docker/Podman、Windows/tool-level sandbox | 交互式 PTY、后台 PID | **参考包安装的“受控扩权”模型**，不直接嵌入整套 Agent |

关键来源：Codex 的 [`app-server command/exec`](https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md) 与 [`exec.rs`](https://github.com/openai/codex/blob/main/codex-rs/core/src/exec.rs)；Claude Code 的[沙箱](https://code.claude.com/docs/en/sandboxing)和[权限规则](https://code.claude.com/docs/en/permissions)；Pi 的 [README](https://github.com/earendil-works/pi)；Gemini CLI 的 [Shell tool](https://github.com/google-gemini/gemini-cli/blob/main/docs/tools/shell.md) 与[沙箱文档](https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/sandbox.md)。

### 4.3 专用 Bash/MCP 工具对比

| 方案 | 优点 | 阻塞性缺口 | 适用结论 |
|---|---|---|---|
| `mcp-shell-server` | MCP stdio；argv 直接执行；命令/参数 allowlist；最小环境；超时和 1 MiB 输出上限；限制重定向逃逸；对 shell/interpreter、`find -exec`、`tar`、Git 外部程序等有硬化 | 目录只校验“绝对且存在”，没有 allowed-root；不是 OS 沙箱；默认禁用解释器会妨碍编程；无 PTY/后台会话；包管理器子进程仍可越过命令 allowlist | **PoC 首选，生产不应原样采用** |
| Desktop Commander | 交互进程、会话管理、分页输出、kill/list、搜索和文件工具齐全 | 官方 [`SECURITY.md`](https://github.com/wonderwhy-er/DesktopCommanderMCP/blob/main/SECURITY.md) 明确：allowedDirectories 不是沙箱，终端可越界；blocklist 可通过替换、绝对路径和解释器绕过；与项目已有文件/搜索能力重叠，攻击面大 | **拒绝宿主机生产模式；仅限容器内可信开发环境** |
| `mcp-server-commands` | 代码少、接入快，支持直接 argv 或 `shell:true`、超时与进程组 kill | 无 allowlist、目录边界、环境隔离、OS 沙箱和输出上限；stdout/stderr 持续累积；项目 TODO 仍列出限制与后台任务 | **拒绝生产；甚至 PoC 也应优先选 mcp-shell-server** |
| Strands Shell | Rust、VFS、URL allowlist/SSRF 防护、凭据注入、资源限制、内置 MCP；不 fork/exec 宿主进程 | 正因不 fork/exec，无法运行任意编译器、Git 或 npm/cargo/pip，不能满足“安装包”核心场景；项目也强调它是 mediation layer，不是对抗性沙箱 | **可作只读/受限模式，不是主 Bash** |

### 4.4 安全反证

- Desktop Commander 自身承认目录与命令 blocklist 是 guardrail，不是安全边界。[官方安全说明](https://github.com/wonderwhy-er/DesktopCommanderMCP/blob/main/SECURITY.md)
- `cyanheads/git-mcp-server <= 2.1.4` 曾因把用户输入传入 `child_process.exec` 形成命令注入，修复版本为 2.1.5。[GitHub Advisory GHSA-3q26-f695-pp76](https://github.com/advisories/GHSA-3q26-f695-pp76)
- Claude Code 将“tool permission”和“OS sandbox”明确拆成两层，反向证明单层允许列表不能承担完整隔离职责。[官方沙箱说明](https://code.claude.com/docs/en/sandboxing)

## 5. 方案比较与硬门槛

评分采用 1–5 分；先看硬门槛，再看加权分。权重：安全边界 30%、场景覆盖 20%、项目适配 20%、生命周期 15%、维护/许可证 10%、分发体积 5%。

| 方案 | 安全 | 覆盖 | 适配 | 生命周期 | 维护/许可 | 体积 | 加权分 | 硬门槛结果 |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| **首方 Rust 内存 Provider** | 4 | 5 | 5 | 4 | 4 | 4 | **4.40** | 通过；沙箱与审批需实现 |
| Codex app-server sidecar | 5 | 5 | 2 | 5 | 5 | 1 | 4.20 | 通过能力门槛，但集成/分发成本过高 |
| `mcp-shell-server` | 3 | 3 | 3 | 3 | 4 | 3 | 3.10 | PoC 通过；生产沙箱门槛失败 |
| Desktop Commander | 2 | 5 | 3 | 5 | 4 | 2 | 3.45 | 生产目录隔离门槛失败 |
| Pi Bash executor | 2 | 4 | 2 | 4 | 4 | 3 | 2.95 | 默认权限/沙箱门槛失败 |
| Strands Shell | 5 | 1 | 4 | 3 | 4 | 4 | 3.55 | 安装包/编程覆盖硬门槛失败 |
| `mcp-server-commands` | 1 | 4 | 3 | 2 | 3 | 4 | 2.50 | 多个生产安全门槛失败 |

Claude Code 因核心实现不可复用，只作为设计参考，不参与可落地候选评分。分数不是成熟度排名，而是针对本项目边界的决策工具。

## 6. 决策理由

### 6.1 支持结论的最强证据

1. 项目已经有可复用的内存 MCP Provider 注入模式，不需要为了 Shell 再引入 Python/Node/Agent sidecar。
2. 现成专用 MCP 要么缺少 OS 沙箱与根目录边界，要么无法执行真实宿主二进制；没有候选同时满足编程、搜索、安装包和生产隔离。
3. Codex、Claude Code、Gemini CLI 的共同成熟做法是“策略/审批 + OS 沙箱 + 进程监督”，而不是单纯的命令 allowlist。
4. 当前 SDK 的远端 cancellable 路径不会自动获得 confirm，因此审批必须在本项目控制的执行边界内。

### 6.2 最强反对意见及回应

**反对意见：自研 Shell 容易重造轮子，直接接 Desktop Commander 或 Codex 更快。**

回应：不应自研 shell 语法、终端模拟器或 Agent 框架；建议自研的是一个很窄的安全适配层，底层仍调用系统进程与成熟 OS 沙箱。Desktop Commander 的官方安全边界不满足宿主生产使用；Codex 的执行器质量更高，但其 app-server 与内部 workspace 依赖会把另一套 Agent 运行时和协议生命周期带进客户端。项目现有 Provider seam 使窄层自研的集成成本明显降低。

### 6.3 被否决方案

- **直接复用 Claude Code：** 核心非开源，法律与技术上都不适合作为嵌入依赖。
- **直接复用 Pi：** 轻量但明确无权限系统；需要自行补齐的部分正是本需求最关键的部分。
- **Desktop Commander：** 终端体验成熟，但自述 guardrail 可绕过，并与已有工具域重复。
- **Codex sidecar：** 能力强但过重；保留为“若团队明确接受额外二进制和双协议栈”时的 B 方案。
- **Strands Shell：** 安全性好但无法执行目标命令；可作为未来受限只读工具，不应混淆为宿主 Bash。

### 6.4 结论失效条件

以下任一条件成立时应重开选型：

- 产品范围改成仅容器内运行，且 Desktop Commander/Codex sidecar 的镜像成本可接受。
- rust-sdk 提供统一、可取消、可持久化的 tool approval hook，能在 MCP 调用前完成 UI 审批；届时可减少 Provider 内审批逻辑。
- 首期必须支持 Windows 与完整 PTY/后台交互，而团队无法投入跨平台进程监督；此时 Codex app-server sidecar 的相对价值会上升。
- “安装包”实际包含 `sudo`/系统包管理器；这已经不是本报告的项目级依赖范围，应独立立项为受控主机管理能力。

## 7. 推荐系统方案

### 7.1 总体边界

```text
Remote Agent
    │ MCP tool call
    ▼
A2C-SMCP Computer
    │ reserved Bundle ID / MCPClientProtocol
    ▼
tfrobot_shell Provider
    ├─ Schema normalization (argv first, shell string explicit)
    ├─ Risk classifier + per-Computer policy
    ├─ Approval broker (deny / allow once / allow session)
    ├─ Environment and secret scrubber
    ├─ Workspace / network sandbox launcher
    ├─ Process supervisor (timeout, cancel, process tree, output cap)
    └─ Redacted audit + correlation ID
```

Shell 应使用独立 Provider，而不是加入 Client Control catalog：Client Control 管的是应用/Computer 控制面，Shell 管的是宿主进程与工作区执行面，风险模型、配置、审计字段和生命周期均不同。

### 7.2 MCP 工具面

首期只暴露一个工具，避免模型在多个近义工具间误选：

```json
{
  "name": "shell_execute",
  "arguments": {
    "argv": ["cargo", "test", "--workspace"],
    "cwd": ".",
    "timeout_seconds": 120,
    "network": "deny"
  }
}
```

规则：

- `argv` 为默认模式，不经过 shell 展开。
- `command` 字符串模式只为管道、重定向、条件执行提供，必须显式声明，默认逐次审批并提升风险级别。
- `cwd` 必须解析到配置的 workspace root 内；拒绝绝对越界、`..` 与 symlink 逃逸。
- 返回结构化 `exit_code`、截断标记、stdout/stderr、duration、cancelled/timed_out；总输出和单流输出均设上限。
- 首期不支持持久 shell session；PTY/后台任务在真实使用证据出现后再加入。

### 7.3 风险与审批模型

| 风险级别 | 示例 | 默认动作 |
|---|---|---|
| ReadOnly | `rg`、`git status`、`cargo metadata` | 在工作区沙箱内按策略自动执行 |
| WorkspaceWrite | formatter、代码生成、`git add` | 逐次或会话授权；限制写入工作区 |
| BuildExecute | `cargo test`、`npm test`、运行本地脚本 | 沙箱执行；默认禁网；继承最小环境 |
| DependencyInstall | `pnpm install`、`pip install`、`cargo fetch` | **每次审批**；受限网络；隔离 HOME/cache；允许工作区写入 |
| HostMutation | `sudo`、系统包管理器、服务管理、工作区外写入 | 首期硬拒绝 |

包安装尤其要遵循：包管理器名在 allowlist 中不等于安全；审批对象应包含命令、cwd、网络域、写入范围和将被传入的非敏感环境摘要。

### 7.4 沙箱与进程监督

- macOS：优先 Seatbelt profile；仅绑定 workspace、必要工具链路径和批准的网络目标。
- Linux：优先 bubblewrap，必要时结合 namespaces/seccomp/cgroup；缺少运行条件时 fail closed 或降级为只读命令集。
- Windows：首期可只开放低风险 argv 子集；完整支持需 Job Object、restricted token/AppContainer 方向的专项实验。
- 所有平台：新进程组/Job，取消或超时终止整棵进程树；设置输出、运行时间、并发数和临时文件配额。
- 默认清空父进程环境，仅注入明确允许的 `PATH`、locale、代理与每次调用凭据；凭据不得写入命令日志。

## 8. 分阶段实施

### 阶段 0：静态设计确认与 PoC（1 个短迭代）

- 可选用固定版本 `mcp-shell-server==1.1.8`，只开放 `rg/git status` 等只读命令，用现有 stdio MCP 验证模型 schema、错误返回和取消行为。
- 禁止包安装、解释器、shell string 和工作区外 cwd。
- 同时做首方 Provider seam 的最小探针，验证远端调用到审批 UI 的异步闭环。
- 退出条件：确定工具 schema，证明取消能回收子进程，证明 UI 审批发生在 spawn 前。

### 阶段 1：首方 MVP

- 实现 `tfrobot_shell` 内存 Provider、argv 执行、workspace root、环境清理、超时/取消/进程树回收、输出上限和审计。
- 只覆盖搜索、构建、测试和有限工作区写操作；默认关闭，按 Computer 开启。
- 补充恶意参数、symlink 逃逸、超时子进程、stderr flood、秘密脱敏测试。

### 阶段 2：包安装闭环

- 接入审批 UI、隔离 HOME/cache、registry/domain allowlist 与网络审计。
- 对项目依赖安装提供专门风险分类；保持系统包与 `sudo` 硬拒绝。
- 在 macOS/Linux 做真实 sandbox escape 与生命周期脚本测试。

### 阶段 3：按证据扩展

- 只有在日志证明 Agent 频繁需要交互式 CLI 时，才增加 PTY、stdin continuation 和后台 session。
- 根据 Windows 用户量决定是否投入完整隔离，而非无条件复制 Unix 实现。

## 9. 验收、回退与实验

### 9.1 必须通过的验收

1. `rg`、`git status`、构建和测试可在工作区成功执行。
2. `../`、绝对路径和 symlink 无法访问工作区外的测试机密。
3. 命令替换、备用解释器、`find -exec`、Git external program 配置等绕过样例被拒绝或被沙箱拦截。
4. `npm/pip` 测试包的恶意 install script 无法读父进程密钥、写工作区外或访问未授权域名。
5. timeout/cancel 后主进程与孙进程全部退出；大输出不会造成 OOM 或无限历史增长。
6. 所有高风险调用都有 request ID、决策、调用方、风险级别、退出状态与脱敏摘要。
7. 默认配置和策略损坏/缺失时 fail closed。

### 9.2 尚未完成的实验

本报告没有运行第三方代码，也没有执行沙箱逃逸或模型成功率实验。以下未知项可能改变实现细节，但不会改变“需要首方安全边界”的主结论：

- 当前 rust-sdk 的取消是否能可靠传播到 Provider 内部孙进程。
- Seatbelt/bubblewrap 对项目工具链、包缓存和代理配置的最小权限集合。
- 审批 UI 在远端并发 tool call 下的超时、断线和重放语义。
- Windows 的可接受降级范围。

这些属于实现前必须确认的实验，需另行使用实验工作流设计并经用户确认后执行。

### 9.3 回退策略

- Provider 默认关闭；出现安全或稳定性问题时按 Computer 禁用，不影响其他 MCP。
- 阶段 0 sidecar 使用固定版本和独立配置，可直接移除。
- 阶段 1 保持 argv-only；shell string、包安装和 PTY 分别用 feature/policy gate 控制，任何一层可独立回退。

## 10. 下一步

| 动作 | 目的 | 前置依赖 | 完成标准 |
|---|---|---|---|
| 确认产品边界 | 固化 OS 范围、项目包/系统包定义、信任模型 | 产品决策 | 形成可验收约束 |
| 设计最小实验 | 验证 Provider 注入、spawn 前审批、取消与沙箱 | 用户确认实验方案 | 四项关键假设均有可判别结果 |
| 形成 `tfrobot_shell` 技术需求 | 明确 schema、策略、审计、错误码与配置归属 | 实验结果 | 可直接拆任务开发 |
| 与 rust-sdk 评审审批钩子 | 决定审批留在 Client 还是下沉 SDK | SDK 维护者输入 | 无双重审批、远端/本地语义一致 |
| 阶段 1 实现与安全回归 | 交付 argv-only MVP | 技术需求通过 | 通过 9.1 中除包安装外的验收 |

## 最终选型

**生产选型：首方 Rust 内存 MCP Provider `tfrobot_shell`。**

**技术参考优先级：Codex > `mcp-shell-server` > Pi/Gemini CLI。**

**PoC 现成工具：固定版本 `mcp-shell-server==1.1.8`，仅用于只读/低风险调用验证。**

这个选择保留了当前 A2C-SMCP Computer 的统一生命周期与审计，避免引入第二套 Agent/JSON-RPC 运行时，也把最重要的安全决策留在项目真正可控的 spawn 边界内。
