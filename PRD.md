# TFRobot Client — 产品需求文档 (PRD)

> **版本**: 1.0
> **日期**: 2026-02-26
> **状态**: 待评审

---

## 1. 产品概述

### 1.1 产品定位

TFRobot Client 是一个跨平台桌面应用，为 A2C-SMCP（Agent To Computer SMCP）协议的 Rust 实现（`smcp-computer` crate）提供可视化管理界面。它使用户能够：

- 管理 MCP（Model Context Protocol）服务器的配置与生命周期
- 连接 SMCP 服务器并加入协作 Office
- 管理输入变量与 placeholder 体系
- 浏览和调试可用工具及桌面资源
- 查看结构化日志与工具调用历史

### 1.2 目标用户

| 用户角色 | 描述 | 技术水平 |
|----------|------|----------|
| AI 应用开发者 | 使用 MCP 服务器构建 AI Agent 工具链的开发者，熟悉 MCP 协议、JSON Schema、VRL | 高 |
| 技术团队运维 | 负责维护 AI Agent 基础设施，需要监控服务器状态、查看日志、管理连接 | 中 |

**设计原则**：主要面向开发者，但通过良好的 UI 设计让运维人员也能轻松使用基础功能。高级功能（VRL、tool_meta、手动调试）放在折叠/高级选项中。

### 1.3 与 CLI 的关系

本产品的核心目标是与 `smcp-computer` CLI 的功能对齐，同时利用 GUI 的优势提供以下增强：

| CLI 能力 | GUI 对齐 | GUI 增强 |
|----------|---------|---------|
| `server add/rm` | ✅ 表单化配置 | 可视化状态指示器、批量操作 |
| `start/stop` | ✅ 按钮操作 | 实时状态徽标、一键启停全部 |
| `socket connect/join/leave` | ✅ 连接管理 | 连接 Profile 保存、分步向导 |
| `inputs` 系列 | ✅ 变量管理 | 独立面板 + MCP 配置中可视化引用 |
| `tools` | ✅ 工具列表 | inputSchema 可视化、参数表单、调用测试 |
| `tc` (tool call debug) | ✅ 调用测试 | 历史重放、结果对比 |
| `desktop` | ✅ 资源浏览 | 窗口列表 + 内容预览 |
| `history` | ✅ 调用历史 | SQLite 持久化、筛选、导出 |
| `status` | ✅ 状态概览 | Dashboard 首页、卡片式指标 |
| `render` | ✅ 渲染测试 | 变量面板中实时预览解析结果 |
| `mcp` (show config) | ✅ 配置查看 | 导入/导出 JSON + 兼容 Claude Desktop 格式 |
| `notify update` | ✅ 自动触发 | MCP/变量变更时自动同步 SMCP 服务器 |
| — | — | 🆕 亮/暗主题切换 |
| — | — | 🆕 系统托盘 |
| — | — | 🆕 运行时检测与安装引导 |
| — | — | 🆕 Resource 浏览与调用测试 |

---

## 2. 信息架构与导航

### 2.1 分组侧边栏结构

```
┌─────────────────────────────────────────────┐
│  TFRobot Client              [亮/暗] [语言]  │
├──────────┬──────────────────────────────────┤
│          │                                  │
│ 概览     │                                  │
│  Dashboard│        主内容区域                │
│          │                                  │
│ ─────── │                                  │
│ 配置     │                                  │
│  MCP 服务器│                                │
│  输入变量  │                                │
│          │                                  │
│ ─────── │                                  │
│ 连接     │                                  │
│  SMCP 服务器│                               │
│  桌面资源  │                                │
│          │                                  │
│ ─────── │                                  │
│ 开发     │                                  │
│  调试面板  │                                │
│  日志     │                                  │
│          │                                  │
│ ─────── │                                  │
│ 系统     │                                  │
│  设置     │                                  │
│          │                                  │
├──────────┴──────────────────────────────────┤
│ 状态栏: [连接状态] [MCP: 3/5] [工具: 42]     │
└─────────────────────────────────────────────┘
```

### 2.2 导航分组

| 分组 | 页面 | 说明 |
|------|------|------|
| **概览** | Dashboard | 全局状态概览，系统健康度 |
| **配置** | MCP 服务器 | MCP 服务器的 CRUD 与生命周期管理 |
| | 输入变量 | Input/Variable 定义与值管理 |
| **连接** | SMCP 服务器 | SMCP 连接 Profile 管理与连接控制 |
| | 桌面资源 | Desktop 资源浏览（window:// URI） |
| **开发** | 调试面板 | 工具浏览、调用测试、Resource 测试 |
| | 日志 | 结构化日志查看、筛选、导出 |
| **系统** | 设置 | 主题、语言、运行时路径、关于 |

---

## 3. 功能模块详细设计

### 3.1 Dashboard（概览首页）

**对应 CLI**: `status` 命令

**功能描述**：
应用首页，用卡片式布局聚合展示系统全局状态。

**信息卡片**：

| 卡片 | 内容 | 数据源 |
|------|------|--------|
| SMCP 连接状态 | 连接/断开状态、服务器地址、当前 Office、计算机名称 | Socket.IO client |
| MCP 服务器概况 | 总数、运行中数量、停止数量、错误数量 | MCPServerManager |
| 可用工具统计 | 总工具数、按服务器分布 | get_available_tools() |
| 最近活动 | 最近 5 条工具调用记录（时间、工具名、成功/失败） | tool_history |
| 系统信息 | 运行时检测状态（Node.js、Python、uv 等） | RuntimeService |

**交互**：
- 各卡片可点击跳转到对应详情页面
- 错误状态卡片高亮显示
- 支持手动刷新

---

### 3.2 MCP 服务器管理

**对应 CLI**: `server add/rm`, `start/stop`, `mcp`

**3.2.1 服务器列表**

展示所有已配置的 MCP 服务器，表格列：

| 列 | 说明 |
|----|------|
| 名称 | 服务器名称 |
| 类型 | Stdio / SSE / HTTP 标签 |
| 状态 | 运行中(绿) / 已停止(灰) / 错误(红) / 启动中(蓝) |
| 工具数 | 该服务器提供的工具数量 |
| 消息 | 最新状态消息或错误信息 |
| 操作 | 启动/停止、编辑、删除 |

**批量操作栏**：
- 全部启动 / 全部停止 / 刷新状态
- 导入配置 / 导出配置

**3.2.2 服务器配置表单**

Modal 或 Drawer 形式，根据类型动态渲染：

**基础配置**（所有类型）：
- 名称（必填，唯一）
- 类型选择：Stdio / SSE / HTTP（创建后不可变）
- 是否禁用（disabled 开关）

**Stdio 类型**：
- 命令（Command）（必填，如 `npx`、`python`）
- 参数列表（Args）（动态增删）
- 工作目录（CWD）（可选，目录选择器）
- 环境变量（Env）（键值对，动态增删）
- 编码（Encoding）（默认 `utf-8`）

**SSE / HTTP 类型**：
- URL（必填）
- 请求头（Headers）（键值对，动态增删）

**高级设置**（折叠区域）：
- `forbidden_tools`：禁用工具列表（文本输入，逗号分隔或标签模式）
- `default_tool_meta`：默认工具元数据
  - tags（标签列表）
  - auto_apply（布尔开关）
- `tool_meta`：每个工具的自定义元数据（JSON 编辑器或键值表单）
  - alias（别名）
  - tags（标签）
  - auto_apply（布尔）
  - return_mapping（返回值映射）
- `vrl`：VRL 转换脚本（纯文本编辑器，monospace 字体，可折叠）

**3.2.3 配置导入/导出**

**导入**：
- 支持格式：
  1. CLI 原生 JSON 格式（`{ "servers": [...], "inputs": [...] }` 结构）
  2. Claude Desktop 格式（`claude_desktop_config.json` 中的 `mcpServers` 字段）
- 导入流程：选择文件 → 自动检测格式 → 预览将导入的服务器列表 → 确认导入
- 冲突处理：同名服务器提示覆盖/跳过/重命名

**导出**：
- 导出为 CLI 兼容的 JSON 格式
- 可选择导出全部或勾选的服务器
- 导出文件包含 servers 和 inputs 两部分

---

### 3.3 输入变量管理

**对应 CLI**: `inputs` 系列命令（`load`、`add`、`update`、`rm`、`get`、`list`、`value` 系列）

**3.3.1 变量定义面板**

左侧列表 + 右侧详情的布局：

**变量列表**：
| 列 | 说明 |
|----|------|
| ID | 变量唯一标识 |
| 类型 | PromptString / PickString / Command |
| 标签 | 显示名称（label） |
| 当前值 | 缓存的当前值（未设置时显示默认值或"-"） |
| 引用数 | 被多少个 MCP 配置引用（`${input:id}` 出现次数） |

**变量类型表单**：

- **PromptString**：
  - ID（必填）
  - Label（显示名称）
  - Description（描述，可选）
  - Default value（默认值，可选）
  - 当前值输入框

- **PickString**：
  - ID（必填）
  - Label
  - Description
  - Options（选项列表，动态增删，每个选项包含 label + value）
  - 当前值下拉选择

- **Command**：
  - ID（必填）
  - Label
  - Command（Shell 命令）
  - Args（参数列表）
  - 当前值显示（只读，来自命令执行结果）
  - 手动执行按钮（重新运行命令获取值）

**3.3.2 可视化引用**

在 MCP 服务器配置表单中：
- 自动识别配置值中的 `${input:id}` placeholder
- 在 placeholder 旁显示当前绑定值（Tooltip 或内联标签）
- 如果引用的变量未定义，显示警告标记
- 点击 placeholder 可跳转到变量定义面板

**3.3.3 批量操作**

- 从文件导入变量定义（CLI 兼容的 JSON 格式）
- 导出变量定义
- 清空所有缓存值

---

### 3.4 SMCP 服务器连接

**对应 CLI**: `socket connect/join/leave`, `status`（Socket.IO 部分）

**3.4.1 连接 Profile 管理**

连接配置可保存为 Profile，支持多个 Profile 切换：

**Profile 字段**：
| 字段 | 说明 | 存储 |
|------|------|------|
| Profile 名称 | 用户自定义名称 | 明文 |
| 服务器 URL | Socket.IO 服务器地址（如 `http://localhost:3000`） | 明文 |
| Namespace | Socket.IO namespace（默认 `/smcp`） | 明文 |
| Office ID | 要加入的 Office 标识 | 明文 |
| Computer 名称 | 在 Office 中显示的名称 | 明文 |
| API Key | 认证密钥 | **Keychain 加密存储** |
| 自定义 Headers | HTTP 请求头（键值对） | **敏感值存 Keychain** |

自动连接不属于 Profile 本身，由每个 Computer 的连接策略独立管理。

**Profile 列表**：
- 显示所有已保存 Profile
- 当前连接的 Profile 高亮标记
- 支持创建、编辑、删除、复制 Profile

**3.4.2 新建连接向导**

分步引导创建新连接：

**Step 1 - 服务器配置**：
- 输入 URL、Namespace
- 输入 API Key（可选，密码输入框）
- 配置自定义 Headers
- 测试连接按钮

**Step 2 - Office 配置**：
- 输入 Office ID
- 输入 Computer 名称
- 设置自动连接/重连选项

**Step 3 - 确认并保存**：
- 预览所有配置
- 输入 Profile 名称
- 保存 Profile
- 可选：立即连接

**3.4.3 连接状态面板**

当已连接时，显示：
- 连接状态指示器（绿色=已连接，黄色=重连中，红色=断开）
- 当前服务器地址
- 当前 Office / Computer 名称
- 连接时长
- 断开连接按钮
- Office 内其他成员列表（来自 `list_room`）

**3.4.4 状态同步**

当本地状态发生以下变更时，**自动**向 SMCP 服务器发送通知：
- MCP 服务器启停导致工具列表变化 → `emit_update_tool_list()`
- 输入变量变更导致配置变化 → `emit_update_config()`
- 桌面资源变化 → `emit_update_desktop()`

---

### 3.5 桌面资源浏览

**对应 CLI**: `desktop` 命令

**功能定位**：核心展示功能，专门的 Resources 页面。

**3.5.1 窗口资源列表**

展示 `window://` URI 体系下的所有资源：

| 列 | 说明 |
|----|------|
| URI | window:// 格式的资源地址 |
| 标题 | 窗口/资源标题 |
| 优先级 | 优先级数值（如有） |
| 全屏 | 是否全屏状态 |

**交互**：
- 点击资源行展开查看内容预览
- 手动刷新按钮
- 支持 URI 筛选/搜索

**3.5.2 资源详情**

选中资源后显示：
- 资源基本信息（URI、标题、优先级、全屏状态）
- 内容预览区域（文本内容或截图）
- 可选尺寸参数（调整获取尺寸）

---

### 3.6 调试面板

**对应 CLI**: `tc`、`tools`、`render`、`history`

**设计原则**：自研调试 UI，与应用共享同一个 `smcp-computer` 实例，保证状态一致性。

**3.6.1 工具浏览器**

左侧工具列表 + 右侧详情：

**工具列表**：
| 列 | 说明 |
|----|------|
| 工具名 | 完整工具名称 |
| 来源 | 所属 MCP 服务器名称 |
| 描述 | 工具描述摘要 |
| 标签 | tool_meta 中的 tags |

- 支持按服务器、标签筛选
- 支持名称搜索

**工具详情**：
- 完整描述
- inputSchema 可视化展示（JSON Schema → 树形结构或表格）
- tool_meta 信息（别名、标签、auto_apply）

**3.6.2 工具调用测试**

选中工具后，可进行手动调用测试：

- **参数表单**：根据 inputSchema 自动生成表单
  - 支持的 Schema 类型：string、number、boolean、object、array、enum
  - 必填字段标记
  - 默认值预填
  - 提供「JSON 编辑器」模式切换（高级用户直接编辑 JSON）
- **执行按钮**：调用 `execute_tool()` 并显示加载状态
- **超时设置**：可选的 timeout 参数
- **结果展示**：
  - 文本内容：格式化显示（支持 Markdown 渲染）
  - 图片内容：直接预览
  - 错误信息：红色高亮显示
  - 返回耗时

**3.6.3 Resource 浏览与调用测试**

调试面板中的 Resource 调试标签页：

- **Resource 列表**：浏览 MCP 服务器提供的 Resource（resources/list）
- **Resource 读取**：选中 Resource 后调用 resources/read 查看内容
- **Resource 模板**：支持 Resource Template 的参数填写与调用

**3.6.4 调用历史与重放**

**历史列表**：
| 列 | 说明 |
|----|------|
| 时间 | 调用时间戳 |
| 工具 | 工具名称 |
| 服务器 | 来源 MCP 服务器 |
| 状态 | 成功 ✅ / 失败 ❌ |
| 耗时 | 执行时间 |

**详情展开**：
- 完整输入参数（JSON 格式化）
- 完整返回结果
- 错误信息（如有）

**重放功能**：
- 「重放」按钮：用相同参数重新调用
- 「编辑并调用」按钮：打开参数编辑器，修改后调用

---

### 3.7 日志系统

**对应 CLI**: `history` + 应用日志

**3.7.1 日志存储**

**后端**：SQLite 数据库

**日志表结构**：
```sql
CREATE TABLE logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,         -- ISO 8601
    level TEXT NOT NULL,             -- DEBUG/INFO/WARN/ERROR
    category TEXT NOT NULL,          -- mcp/smcp/tool_call/system
    source TEXT,                     -- 来源服务器或模块
    message TEXT NOT NULL,           -- 日志消息
    details TEXT,                    -- JSON 格式的详细数据
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);
```

**日志类别**：
| 类别 | 说明 |
|------|------|
| `mcp` | MCP 服务器通信日志（启停、工具注册、错误） |
| `smcp` | SMCP 连接事件（连接、加入、离开、通知） |
| `tool_call` | 工具调用记录（参数、结果、耗时） |
| `system` | 应用系统日志（启动、配置变更、运行时检测） |

**3.7.2 日志查看器 UI**

**筛选栏**：
- 时间范围选择（快捷：最近1小时/今天/最近7天/自定义）
- 级别筛选（多选：DEBUG/INFO/WARN/ERROR）
- 类别筛选（多选：mcp/smcp/tool_call/system）
- 来源筛选（下拉：具体 MCP 服务器名称）
- 关键词搜索

**日志列表**：
- 时间线形式展示
- 颜色区分级别（DEBUG 灰色、INFO 蓝色、WARN 黄色、ERROR 红色）
- 类别标签
- 点击展开详情（JSON 格式化展示 details 字段）

**3.7.3 导出**

- 导出为 JSON 文件
- 支持按当前筛选条件导出
- 可选时间范围

**3.7.4 日志清理**

- 自动清理：30 天过期自动删除
- 手动清理：设置中提供清空日志按钮（需确认）

---

### 3.8 设置

**3.8.1 外观**

- **主题切换**：亮色 / 暗色 / 跟随系统
  - 通过 Ant Design 5 的 Design Token + `algorithm` 实现
  - `theme.defaultAlgorithm`（亮色）/ `theme.darkAlgorithm`（暗色）
  - 偏好存储到本地配置

- **语言选择**：中文 / English
  - i18n 架构预留扩展接口，支持未来添加新语言
  - 语言包采用异步加载

**3.8.2 运行时**

- **运行时检测**：显示当前系统中检测到的运行时状态
  | 运行时 | 路径 | 版本 | 状态 |
  |--------|------|------|------|
  | Node.js | /usr/local/bin/node | v20.x | ✅ 已安装 |
  | Python | — | — | ❌ 未检测到 |
  | uv | /usr/local/bin/uv | 0.x | ✅ 已安装 |
  | pnpm | /usr/local/bin/pnpm | 9.x | ✅ 已安装 |

- **缺失提示**：未检测到的运行时提供安装引导链接
- **自定义路径**：高级选项中允许手动指定运行时路径

**3.8.3 数据管理**

- 日志清理（手动触发）
- 导出所有配置（MCP 服务器 + 变量 + Profile 连接配置）
- 重置应用（清除所有数据，需二次确认）

**3.8.4 关于**

- 应用版本号
- smcp-computer 库版本
- 检查更新（Tauri updater）
- 开源协议
- 反馈链接

---

## 4. 技术架构

### 4.1 前端技术栈

| 技术 | 用途 |
|------|------|
| React 18 | UI 框架 |
| TypeScript | 类型安全 |
| Ant Design 5 | UI 组件库 + Design Token 主题系统 |
| Zustand | 状态管理 |
| i18next | 国际化（中/英） |
| Vite | 构建工具 |

### 4.2 后端技术栈

| 技术 | 用途 |
|------|------|
| Tauri 2.x | 桌面框架 |
| smcp-computer | MCP 管理 + SMCP 连接 + 工具执行 |
| SQLite (tauri-plugin-sql) | 日志持久化 |
| keyring | 系统级凭证存储 |
| tokio | 异步运行时 |

### 4.3 前后端通信

所有前后端交互通过 Tauri IPC（`invoke`）：

```
Frontend (React) --invoke()--> Tauri Commands ---> smcp-computer / Services
                <--Result/Error--
```

**状态同步策略**：
- 后端为数据真实来源（Source of Truth）
- 前端每次操作后主动刷新状态
- SMCP 事件通过 Tauri Event System 推送到前端

### 4.4 数据存储分层

| 数据类型 | 存储位置 | 格式 |
|----------|----------|------|
| MCP 服务器配置 | App Data 目录 JSON 文件 | JSON |
| 输入变量定义 | App Data 目录 JSON 文件 | JSON |
| 连接 Profile | App Data 目录 JSON 文件 | JSON |
| API Key / 敏感 Headers | 系统 Keychain | 加密 |
| 日志 | App Data 目录 SQLite | SQLite |
| 用户偏好（主题、语言） | App Data 目录 JSON 文件 | JSON |

---

## 5. Tauri 命令清单

基于当前 `smcp-computer` crate API 和产品需求，规划以下 Tauri Command：

### 5.1 MCP 服务器管理

| 命令 | 参数 | 返回 | 说明 |
|------|------|------|------|
| `get_mcp_servers` | — | `Vec<ServerInfo>` | 获取所有服务器及其状态 |
| `add_mcp_server` | `config: MCPServerConfig` | `()` | 添加服务器配置 |
| `update_mcp_server` | `config: MCPServerConfig` | `()` | 更新服务器配置 |
| `remove_mcp_server` | `name: String` | `()` | 删除服务器配置 |
| `start_mcp_server` | `name: String` | `()` | 启动单个服务器 |
| `stop_mcp_server` | `name: String` | `()` | 停止单个服务器 |
| `start_all_servers` | — | `()` | 启动全部 |
| `stop_all_servers` | — | `()` | 停止全部 |
| `import_config` | `path: String, format: String` | `ImportResult` | 导入配置（cli/claude_desktop） |
| `export_config` | `path: String, names: Option<Vec<String>>` | `()` | 导出配置 |

### 5.2 输入变量管理

| 命令 | 参数 | 返回 | 说明 |
|------|------|------|------|
| `list_inputs` | `instance_id: String` | `Vec<InputDefinition>` | 列出指定 Computer 的变量定义 |
| `get_input` | `instance_id: String, id: String` | `Option<InputDefinition>` | 获取指定 Computer 的单个变量定义 |
| `add_or_update_input` | `instance_id: String, input: InputDefinition` | `()` | 添加或更新变量定义，并同步到该 Computer 的 SMCP runtime |
| `remove_input` | `instance_id: String, id: String` | `()` | 删除变量定义和缓存值，并同步到该 Computer 的 SMCP runtime |
| `list_input_values` | `instance_id: String` | `HashMap<String, Value>` | 列出指定 Computer 的缓存值 |
| `get_input_value` | `instance_id: String, id: String` | `Option<Value>` | 获取缓存值 |
| `set_input_value` | `instance_id: String, id: String, value: Value` | `()` | 设置缓存值 |
| `remove_input_value` | `instance_id: String, id: String` | `()` | 删除缓存值 |
| `clear_input_values` | `instance_id: String` | `()` | 清空指定 Computer 的缓存值 |
| `import_inputs` | `instance_id: String, path: String` | `usize` | 从文件导入变量定义，并同步到该 Computer 的 SMCP runtime |

### 5.3 SMCP 连接管理

| 命令 | 参数 | 返回 | 说明 |
|------|------|------|------|
| `list_manual_smcp_targets` | — | `Vec<ManualSmcpTarget>` | 列出全局 Manual SMCP 连接目标 |
| `save_manual_smcp_target` | `target: ManualSmcpTarget, api_key_action` | `ManualSmcpTarget` | 保存全局 Manual SMCP 目标，密钥独立写入 keychain |
| `delete_manual_smcp_target` | `target_id: String` | `()` | 删除未连接中的 Manual SMCP 目标 |
| `connect_connection_target` | `instance_id: String, target_id: String` | `()` | 将已运行的 Computer 连接到 Manual SMCP 目标 |
| `manager_connect_smcp` | `instance_id, employee_id, robot_account_id, ...` | `()` | 通过 Manager token-exchange 将已运行的 Computer 连接到机器人 |
| `disconnect_smcp` | `instance_id: String` | `()` | 断开指定 Computer 的 SMCP 连接 |
| `get_connection_status` | `instance_id: String` | `ConnectionStatus` | 获取指定 Computer 的连接状态 |

### 5.3.1 Computer 连接策略

| 命令 | 参数 | 返回 | 说明 |
|------|------|------|------|
| `update_computer_connection_policy` | `id, target, auto_connect` | `ComputerInstanceStatus` | 保存 Computer 选中的连接目标和自动连接策略 |
| `connect_computer_connection_target` | `id: String` | `()` | 按 Computer 当前策略连接，要求 Computer 已运行 |
| `disconnect_computer_connection_target` | `id: String` | `()` | 断开 Computer 当前连接 |

### 5.4 桌面资源

| 命令 | 参数 | 返回 | 说明 |
|------|------|------|------|
| `get_desktop` | `instance_id: String, uri: Option<String>` | `Vec<Window>` | 获取指定 Computer 的桌面资源 |

### 5.5 调试

| 命令 | 参数 | 返回 | 说明 |
|------|------|------|------|
| `get_available_tools` | `instance_id: String` | `Vec<SMCPTool>` | 获取指定 Computer 的可用工具 |
| `get_debug_resources` | `instance_id: String, server_name: String, cursor` | `DebugResourcesResponse` | 浏览指定 Computer 上 MCP server 暴露的资源 |
| `execute_tool` | `instance_id, tool_name, params, timeout` | `CallToolResult` | 在指定 Computer 上执行工具调用 |
| `get_tool_history` | `instance_id: String` | `Vec<ToolCallHistoryRecord>` | 获取指定 Computer 的工具调用历史 |
| `get_tool_history` | — | `Vec<ToolCallRecord>` | 获取调用历史 |
| `list_resources` | `server: Option<String>` | `Vec<Resource>` | 列出 MCP Resource |
| `read_resource` | `server: String, uri: String` | `ResourceContent` | 读取 Resource 内容 |

### 5.6 日志

| 命令 | 参数 | 返回 | 说明 |
|------|------|------|------|
| `get_logs` | `filter: LogFilter` | `Vec<LogEntry>` | 查询日志 |
| `export_logs` | `path: String, filter: LogFilter` | `()` | 导出日志 |
| `clear_logs` | `before: Option<String>` | `()` | 清除日志 |

### 5.7 设置

| 命令 | 参数 | 返回 | 说明 |
|------|------|------|------|
| `get_settings` | — | `AppSettings` | 获取应用设置 |
| `update_settings` | `settings: AppSettings` | `()` | 更新应用设置 |
| `detect_runtimes` | — | `Vec<RuntimeInfo>` | 检测运行时状态 |
| `get_app_info` | — | `AppInfo` | 获取应用信息 |

---

## 6. 数据模型

### 6.1 前端 TypeScript 类型

```typescript
// MCP 服务器配置（与 Rust 后端对齐）
interface MCPServerConfig {
  name: string;
  type: 'stdio' | 'sse' | 'http';
  disabled?: boolean;
  forbidden_tools?: string[];
  tool_meta?: Record<string, ToolMeta>;
  default_tool_meta?: ToolMeta;
  server_parameters: StdioParams | SseParams | HttpParams;
  vrl?: string;
}

interface ToolMeta {
  alias?: string;
  tags?: string[];
  auto_apply?: boolean;
  return_mapping?: Record<string, string>;
}

// 输入变量
type InputDefinition =
  | { type: 'PromptString'; id: string; label: string; description?: string; default?: string }
  | { type: 'PickString'; id: string; label: string; description?: string; options: PickOption[] }
  | { type: 'Command'; id: string; label: string; command: string; args?: string[] };

// 连接 Profile
interface ConnectionProfile {
  name: string;
  url: string;
  namespace: string;
  office_id: string;
  computer_name: string;
  api_key_ref?: string;     // Keychain 引用（不存实际值）
  headers?: Record<string, string>;
}

// 连接状态
interface ConnectionStatus {
  connected: boolean;
  url?: string;
  office_id?: string;
  computer_name?: string;
  connected_at?: string;
  members?: SessionInfo[];
}

// 工具
interface SMCPTool {
  name: string;
  description?: string;
  inputSchema: object;      // JSON Schema
  server: string;           // 来源服务器
  meta?: ToolMeta;
}

// 工具调用记录
interface ToolCallRecord {
  id: string;
  timestamp: string;
  server: string;
  tool_name: string;
  params: object;
  result?: CallToolResult;
  success: boolean;
  error?: string;
  duration_ms: number;
}

// 日志条目
interface LogEntry {
  id: number;
  timestamp: string;
  level: 'DEBUG' | 'INFO' | 'WARN' | 'ERROR';
  category: 'mcp' | 'smcp' | 'tool_call' | 'system';
  source?: string;
  message: string;
  details?: object;
}

// 日志筛选器
interface LogFilter {
  time_from?: string;
  time_to?: string;
  levels?: string[];
  categories?: string[];
  source?: string;
  keyword?: string;
  limit?: number;
  offset?: number;
}

// 桌面窗口
interface DesktopWindow {
  uri: string;
  title: string;
  priority?: number;
  fullscreen: boolean;
  content: string;
}

// Resource
interface MCPResource {
  uri: string;
  name: string;
  description?: string;
  mimeType?: string;
  server: string;
}

// 应用设置
interface AppSettings {
  theme: 'light' | 'dark' | 'system';
  language: 'zh' | 'en';
  log_retention_days: number;
  custom_runtime_paths?: Record<string, string>;
}
```

---

## 7. 实现里程碑

### Phase 1: 核心基础（已完成 ✅）

- [x] 项目初始化（Tauri + React + TypeScript）
- [x] MCP 服务器 CRUD 与生命周期管理
- [x] 前端 Zustand 状态管理
- [x] i18n 基础架构（中/英）
- [x] Ant Design 集成

### Phase 2: 连接与变量

- [ ] SMCP 连接 Profile 管理（CRUD + Keychain 集成）
- [ ] 连接向导（分步创建 Profile）
- [ ] 连接状态面板（实时状态、成员列表）
- [ ] Input 变量管理面板（定义 + 值）
- [ ] MCP 配置中的 placeholder 可视化引用
- [ ] 配置导入/导出（CLI JSON + Claude Desktop 格式）
- [ ] 自动状态同步（notify update 自动触发）

### Phase 3: 调试与资源

- [ ] 工具浏览器（列表 + inputSchema 可视化）
- [ ] 工具调用测试（参数表单生成 + 结果展示）
- [ ] Resource 浏览与读取测试
- [ ] 调用历史列表 + 详情展开
- [ ] 历史重放功能
- [ ] 桌面资源浏览页面（window:// 列表 + 内容预览）

### Phase 4: 日志与仪表盘

- [ ] SQLite 日志后端（tauri-plugin-sql）
- [ ] 日志写入集成（MCP/SMCP/tool_call/system 类别）
- [ ] 日志查看器 UI（筛选、搜索、展开详情）
- [ ] 日志导出
- [ ] Dashboard 首页（卡片式状态概览）

### Phase 5: 主题与设置

- [ ] 亮/暗主题切换（Ant Design 5 Design Token）
- [ ] 设置页面（主题、语言、运行时路径）
- [ ] 运行时检测与安装引导
- [ ] 日志自动清理（30 天过期）
- [ ] MCP 服务器高级配置表单（tool_meta、forbidden_tools、VRL）

### Phase 6: 系统集成与分发

- [ ] 系统托盘（Tauri tray）
- [ ] 最小化到托盘
- [ ] Tauri updater 集成
- [ ] 应用打包（DMG / MSI / NSIS）
- [ ] 代码签名与公证（macOS）

---

## 8. 非功能性需求

### 8.1 性能

- 应用冷启动时间 < 3 秒
- MCP 服务器状态刷新 < 500ms
- 日志查询（10万条以内） < 1 秒
- 工具列表加载 < 1 秒

### 8.2 安全

- API Key 和敏感 Headers 仅存储在系统 Keychain
- 不在日志中记录凭证明文
- 前端不缓存敏感凭证

### 8.3 可靠性

- SMCP 连接状态可观测，断开后由用户或 Computer 级策略重新连接
- MCP 服务器异常退出自动标记错误状态
- 应用崩溃不丢失配置数据（配置即时持久化）

### 8.4 可访问性

- 所有 UI 组件支持键盘导航
- 状态信息不仅依赖颜色（配合图标和文字）
- 表单错误信息清晰可读

### 8.5 国际化

- 所有用户可见文本通过 i18n key 管理
- 翻译文件采用嵌套 JSON 结构
- 语言切换即时生效，无需重启
- 架构预留新语言扩展能力

---

## 附录 A: CLI 命令 → GUI 功能映射表

| CLI 命令 | GUI 位置 | 实现阶段 |
|----------|---------|---------|
| `help` | 应用内引导 / 帮助文档 | Phase 5 |
| `status` | Dashboard 首页 | Phase 4 |
| `mcp` | MCP 服务器列表页 | Phase 1 ✅ |
| `tools` | 调试面板 > 工具浏览器 | Phase 3 |
| `history [n]` | 调试面板 > 调用历史 | Phase 3 |
| `server add <json\|@file>` | MCP 服务器 > 添加表单 / 导入 | Phase 1 ✅ / Phase 2 |
| `server rm <name>` | MCP 服务器 > 删除操作 | Phase 1 ✅ |
| `start <name\|all>` | MCP 服务器 > 启动按钮 | Phase 1 ✅ |
| `stop <name\|all>` | MCP 服务器 > 停止按钮 | Phase 1 ✅ |
| `socket connect [url]` | SMCP 服务器 > 连接 | Phase 2 |
| `socket join <office> <name>` | SMCP 服务器 > 加入 Office | Phase 2 |
| `socket leave` | SMCP 服务器 > 断开 | Phase 2 |
| `notify update` | 自动触发（透明） | Phase 2 |
| `inputs load @<file>` | 输入变量 > 导入 | Phase 2 |
| `inputs add/update/rm/get/list` | 输入变量管理面板 | Phase 2 |
| `inputs value *` | 输入变量 > 值管理 | Phase 2 |
| `tc <json\|@file>` | 调试面板 > 工具调用测试 | Phase 3 |
| `render <json\|@file>` | 输入变量 > 可视化预览 | Phase 2 |
| `desktop [size] [uri]` | 桌面资源页面 | Phase 3 |
| `quit / exit` | 窗口关闭 / 托盘退出 | Phase 6 |

---

## 附录 B: 配置格式兼容性

### CLI 原生格式

```json
{
  "servers": [
    {
      "name": "playwright",
      "type": "stdio",
      "disabled": false,
      "forbidden_tools": [],
      "tool_meta": {},
      "default_tool_meta": { "tags": ["browser"], "auto_apply": true },
      "server_parameters": {
        "command": "npx",
        "args": ["@playwright/mcp@latest"],
        "env": null,
        "cwd": null,
        "encoding": "utf-8"
      },
      "vrl": null
    }
  ],
  "inputs": [
    {
      "type": "PromptString",
      "id": "api-key",
      "label": "API Key",
      "default": ""
    }
  ]
}
```

### Claude Desktop 格式（导入兼容）

```json
{
  "mcpServers": {
    "playwright": {
      "command": "npx",
      "args": ["@playwright/mcp@latest"],
      "env": {}
    },
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
    }
  }
}
```

**转换规则**：
- `mcpServers` 中的 key 映射为服务器 `name`
- 所有服务器类型设为 `stdio`
- `command` → `server_parameters.command`
- `args` → `server_parameters.args`
- `env` → `server_parameters.env`
- 其他高级字段（tool_meta、vrl 等）设为默认值
