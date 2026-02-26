# Phase 3: 调试与资源 — 技术执行 Spec

> **状态**: 待开发
> **对应 PRD**: 3.5 桌面资源浏览, 3.6 调试面板
> **前置**: Phase 2 (AppState 已切换为 Computer 实例)

---

## 1. 目标

实现工具浏览器、工具调用测试、Resource 浏览/读取测试、调用历史与重放、桌面资源浏览页面。完成后对齐 CLI 的 `tools`、`tc`、`desktop`、`history` 命令能力，并通过 GUI 优势提供 inputSchema 可视化和历史重放。

---

## 2. 后端变更

### 2.1 新增 Tauri Commands (`src-tauri/src/commands/debug.rs` — 新建)

```rust
use crate::AppState;
use smcp_computer::computer::ToolCallRecord;
use smcp_computer::mcp_clients::{CallToolResult, Tool, Resource, ReadResourceResult};
use tauri::State;
use uuid::Uuid;

/// 获取所有可用工具（聚合所有已启动 MCP 服务器的工具）
#[tauri::command]
pub async fn get_available_tools(
    state: State<'_, AppState>,
) -> Result<Vec<ToolWithServer>, String> {
    let computer = state.computer.read().await;
    let tools = computer.get_available_tools().await.map_err(|e| e.to_string())?;

    // Tool 结构体本身不携带 server 信息
    // 需要从 MCPServerManager 获取 server→tool 映射
    // 方案: 遍历每个 server 的 tool 列表进行归属标注
    // smcp-computer 的 Tool.meta 中包含 a2c_tool_meta，其中有 server_name
    let result: Vec<ToolWithServer> = tools.into_iter().map(|tool| {
        let server = tool.meta.as_ref()
            .and_then(|m| m.get("server_name"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        ToolWithServer { tool, server }
    }).collect();

    Ok(result)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolWithServer {
    #[serde(flatten)]
    pub tool: Tool,
    pub server: String,
}

/// 执行工具调用测试
#[tauri::command]
pub async fn execute_tool(
    state: State<'_, AppState>,
    tool_name: String,
    params: serde_json::Value,
    timeout: Option<f64>,
) -> Result<ToolCallResponse, String> {
    let computer = state.computer.read().await;
    let req_id = Uuid::new_v4().to_string();
    let start = std::time::Instant::now();

    let result = computer
        .execute_tool(&req_id, &tool_name, params.clone(), timeout)
        .await;

    let duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(call_result) => Ok(ToolCallResponse {
            success: !call_result.is_error,
            result: Some(call_result),
            error: None,
            duration_ms,
        }),
        Err(e) => Ok(ToolCallResponse {
            success: false,
            result: None,
            error: Some(e.to_string()),
            duration_ms,
        }),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResponse {
    pub success: bool,
    pub result: Option<CallToolResult>,
    pub error: Option<String>,
    pub duration_ms: u64,
}

/// 获取工具调用历史记录
#[tauri::command]
pub async fn get_tool_history(
    state: State<'_, AppState>,
) -> Result<Vec<ToolCallRecord>, String> {
    let computer = state.computer.read().await;
    computer.get_tool_history().await.map_err(|e| e.to_string())
}

/// 列出某个/所有 MCP 服务器的 Resources
/// 需通过 MCPClientProtocol::list_windows 获取
#[tauri::command]
pub async fn list_mcp_resources(
    state: State<'_, AppState>,
    server: Option<String>,
) -> Result<Vec<ResourceWithServer>, String> {
    let computer = state.computer.read().await;
    // 获取所有服务器状态，筛选运行中的
    let statuses = computer.get_server_status().await;
    let mut all_resources = Vec::new();

    for (name, running, _) in &statuses {
        if !running { continue; }
        if let Some(ref filter) = server {
            if name != filter { continue; }
        }
        // 通过 manager 调用各 client 的 list_windows
        // Computer 没有直接暴露 list_resources，需要通过 manager 或 call_tool
        // 替代方案: 使用 MCP protocol 的 resources/list 工具
        // 实际实现取决于 smcp-computer 是否暴露此 API
        // 如果不直接暴露，可通过 manager 内部访问
    }

    Ok(all_resources)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceWithServer {
    pub server: String,
    #[serde(flatten)]
    pub resource: Resource,
}

/// 读取单个 Resource 内容
#[tauri::command]
pub async fn read_mcp_resource(
    state: State<'_, AppState>,
    server: String,
    uri: String,
) -> Result<ReadResourceResult, String> {
    // 通过 MCPServerManager 内部 client 的 get_window_detail 方法
    // 需要构造 Resource 对象传入
    let resource = Resource {
        uri: uri.clone(),
        name: uri.clone(),
        description: None,
        mime_type: None,
    };
    // 实际调用路径取决于 Computer/MCPServerManager 是否暴露按 server 读取的 API
    // 可能需要在 MCPServerManager 上新增一个公开方法
    // 或使用 call_tool 调用 MCP 的 resources/read
    Err("Not yet implemented - depends on smcp-computer API".to_string())
}
```

**注意**: `list_mcp_resources` 和 `read_mcp_resource` 的实现依赖 `smcp-computer` crate 是否在 `Computer` 或 `MCPServerManager` 上暴露了 `list_windows` / `get_window_detail` 的公开方法。如果 crate 仅在 `MCPClientProtocol` trait 层面有这些方法，需要在 MCPServerManager 上新增委托方法，或升级 crate 版本后使用。

**备选方案**: 如果 crate API 不支持，可通过以下方式间接实现:
1. 扩展 `MCPServerManager` 新增 `pub async fn list_resources(&self, server: &str)` 方法（需要 fork 或提 PR）
2. 使用 MCP 协议的 `resources/list` 和 `resources/read` 作为工具调用

### 2.2 桌面资源命令 (`src-tauri/src/commands/desktop.rs` — 新建)

```rust
use crate::AppState;
use smcp_computer::desktop::WindowInfo;
use tauri::State;

/// 获取桌面资源（window:// URI 体系）
#[tauri::command]
pub async fn get_desktop(
    state: State<'_, AppState>,
    size: Option<String>,
    uri: Option<String>,
) -> Result<Vec<WindowInfo>, String> {
    let computer = state.computer.read().await;
    // Computer::get_desktop 的签名: async fn get_desktop(&self, size, uri)
    // 返回 Desktop (String 别名) — 这是序列化的桌面信息
    // 实际需要解析为 WindowInfo 列表
    let desktop_str = computer
        .get_desktop(size, uri)
        .await
        .map_err(|e| e.to_string())?;

    // desktop_str 是 JSON 字符串或结构化数据
    // 解析为 Vec<WindowInfo>
    let windows: Vec<WindowInfo> = serde_json::from_str(&desktop_str)
        .unwrap_or_default();

    Ok(windows)
}
```

### 2.3 新增依赖

```toml
# Cargo.toml
uuid = { version = "1", features = ["v4"] }
```

### 2.4 命令注册

`lib.rs` 的 `generate_handler![]` 新增:
```rust
commands::debug::get_available_tools,
commands::debug::execute_tool,
commands::debug::get_tool_history,
commands::debug::list_mcp_resources,
commands::debug::read_mcp_resource,
commands::desktop::get_desktop,
```

---

## 3. 前端变更

### 3.1 新增 Store: debugStore

**文件**: `src/stores/debugStore.ts`

```typescript
import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';

export interface Tool {
  name: string;
  description: string;
  inputSchema: Record<string, unknown>;  // JSON Schema
  annotations?: {
    title: string;
    readOnlyHint: boolean;
    destructiveHint: boolean;
    openWorldHint: boolean;
  };
  meta?: Record<string, unknown>;
  server: string;
}

export interface Content {
  type: 'text' | 'image' | 'resource';
  text?: string;
  data?: string;       // base64 for image
  mime_type?: string;
  uri?: string;
}

export interface CallToolResult {
  content: Content[];
  is_error: boolean;
  meta?: Record<string, unknown>;
}

export interface ToolCallResponse {
  success: boolean;
  result?: CallToolResult;
  error?: string;
  duration_ms: number;
}

export interface ToolCallRecord {
  timestamp: string;
  req_id: string;
  server: string;
  tool: string;
  parameters: Record<string, unknown>;
  timeout?: number;
  success: boolean;
  error?: string;
}

export interface MCPResource {
  uri: string;
  name: string;
  description?: string;
  mime_type?: string;
  server: string;
}

export interface ResourceContent {
  contents: Array<{
    uri: string;
    text: string;
    mime_type?: string;
  }>;
}

interface DebugState {
  // Tools
  tools: Tool[];
  toolsLoading: boolean;
  selectedTool: Tool | null;
  lastCallResult: ToolCallResponse | null;
  calling: boolean;

  // History
  history: ToolCallRecord[];
  historyLoading: boolean;

  // Resources
  resources: MCPResource[];
  resourcesLoading: boolean;
  resourceContent: ResourceContent | null;

  // Actions
  fetchTools: () => Promise<void>;
  selectTool: (tool: Tool | null) => void;
  executeTool: (toolName: string, params: Record<string, unknown>, timeout?: number) => Promise<ToolCallResponse>;
  fetchHistory: () => Promise<void>;
  fetchResources: (server?: string) => Promise<void>;
  readResource: (server: string, uri: string) => Promise<ResourceContent>;
}
```

### 3.2 新增 Store: desktopStore

**文件**: `src/stores/desktopStore.ts`

```typescript
export interface WindowInfo {
  server_name: string;
  resource: {
    uri: string;
    name: string;
    description?: string;
    mime_type?: string;
  };
  read_result: {
    contents: Array<{
      uri: string;
      text: string;
      mime_type?: string;
    }>;
  };
}

interface DesktopState {
  windows: WindowInfo[];
  loading: boolean;
  error: string | null;
  selectedWindow: WindowInfo | null;

  fetchDesktop: (size?: string, uri?: string) => Promise<void>;
  selectWindow: (window: WindowInfo | null) => void;
}
```

### 3.3 调试面板页面

**文件**: `src/components/DebugPanel/index.tsx`

顶部 Tabs 切换三个子面板:

```typescript
<Tabs defaultActiveKey="tools" items={[
  { key: 'tools', label: t('debug.tools'), children: <ToolBrowser /> },
  { key: 'resources', label: t('debug.resources'), children: <ResourceBrowser /> },
  { key: 'history', label: t('debug.history'), children: <CallHistory /> },
]} />
```

#### 3.3.1 工具浏览器 (`DebugPanel/ToolBrowser.tsx`)

左右分栏 (Ant Design Row/Col 或 Splitter):

**左栏 (30%)** — 工具列表:
- 顶部搜索框 (Input.Search) + 服务器筛选 (Select)
- 工具列表 (List 或 Table)
- 每项显示: 名称、来源服务器 Tag、描述片段
- 点击选中工具

**右栏 (70%)** — 工具详情 + 调用测试:
- **详情区**: 完整描述、inputSchema 树形展示 (Tree 组件或自定义)、annotations 信息
- **调用测试区**:
  - 参数表单 — 根据 inputSchema 动态生成（见 3.3.2）
  - JSON 编辑器模式切换 (Segmented: "表单" / "JSON")
  - Timeout 输入 (InputNumber, 可选)
  - 执行按钮 (Button, loading state)
  - 结果展示区 (见 3.3.3)

#### 3.3.2 JSON Schema → 表单生成器 (`DebugPanel/SchemaForm.tsx`)

核心组件，根据 JSON Schema 的 `properties` 递归生成 Ant Design Form.Item:

| Schema type | Ant Design 组件 | 说明 |
|-------------|-----------------|------|
| `string` | `Input` | 普通文本输入 |
| `string` + `enum` | `Select` | 下拉选择 |
| `number` / `integer` | `InputNumber` | 数字输入 |
| `boolean` | `Switch` | 开关 |
| `object` | 嵌套 Form.Item 组 | 递归渲染子属性 |
| `array` | `Form.List` | 动态增删列表项 |
| `string` + format `uri` | `Input` + URL 验证 | URL 输入 |

```typescript
interface SchemaFormProps {
  schema: Record<string, unknown>;  // JSON Schema object
  onValuesChange?: (values: Record<string, unknown>) => void;
  form: FormInstance;
}

function SchemaForm({ schema, form }: SchemaFormProps) {
  const properties = schema.properties as Record<string, any>;
  const required = (schema.required as string[]) || [];

  return (
    <>
      {Object.entries(properties).map(([key, propSchema]) =>
        renderFormItem(key, propSchema, required.includes(key))
      )}
    </>
  );
}

function renderFormItem(key: string, schema: any, isRequired: boolean): ReactNode {
  // 根据 schema.type 分发到不同组件
  // 递归处理 object 和 array
}
```

#### 3.3.3 调用结果展示 (`DebugPanel/ToolCallResult.tsx`)

```typescript
interface ToolCallResultProps {
  response: ToolCallResponse;
}

function ToolCallResultView({ response }: ToolCallResultProps) {
  return (
    <div>
      {/* 状态行: 成功/失败 + 耗时 */}
      <Result
        status={response.success ? 'success' : 'error'}
        subTitle={`${response.duration_ms}ms`}
      />

      {/* 内容渲染 */}
      {response.result?.content.map((item, i) => {
        switch (item.type) {
          case 'text':
            return <Typography.Paragraph key={i}><pre>{item.text}</pre></Typography.Paragraph>;
          case 'image':
            return <Image key={i} src={`data:${item.mime_type};base64,${item.data}`} />;
          case 'resource':
            return <Tag key={i}>{item.uri}</Tag>;
        }
      })}

      {/* 错误信息 */}
      {response.error && <Alert type="error" message={response.error} />}
    </div>
  );
}
```

#### 3.3.4 Resource 浏览器 (`DebugPanel/ResourceBrowser.tsx`)

左右分栏:

**左栏** — Resource 列表:
- 服务器筛选 Select
- 刷新按钮
- Resource 列表 (Table: URI, Name, MIME Type, Server)
- 点击选中

**右栏** — Resource 详情:
- URI, Name, Description, MIME Type
- "读取" 按钮
- 内容展示区（文本预格式化 / 图片预览）

#### 3.3.5 调用历史 (`DebugPanel/CallHistory.tsx`)

Table + 可展开行:

```typescript
const columns = [
  { title: t('debug.history.time'), dataIndex: 'timestamp', render: formatTime },
  { title: t('debug.history.tool'), dataIndex: 'tool' },
  { title: t('debug.history.server'), dataIndex: 'server' },
  { title: t('debug.history.status'), dataIndex: 'success', render: renderStatus },
];

<Table
  columns={columns}
  dataSource={history}
  expandable={{
    expandedRowRender: (record) => (
      <Descriptions>
        <Descriptions.Item label="Parameters">
          <pre>{JSON.stringify(record.parameters, null, 2)}</pre>
        </Descriptions.Item>
        <Descriptions.Item label="Error">{record.error}</Descriptions.Item>
      </Descriptions>
    ),
  }}
/>
```

**重放按钮**: 在展开行中:
```typescript
<Button onClick={() => {
  selectTool(findToolByName(record.tool));
  // 预填参数到 SchemaForm
  form.setFieldsValue(record.parameters);
}}>
  {t('debug.history.replay')}
</Button>
```

### 3.4 桌面资源页面

**文件**: `src/components/DesktopResources/index.tsx`

上下布局:
- **上方**: 操作栏 — 刷新按钮 + URI 搜索框 + 可选尺寸参数 InputNumber
- **下方**: 窗口列表 (Table) + 选中展开详情

Table 列:
| 列 | dataIndex | 说明 |
|----|-----------|------|
| URI | `resource.uri` | window:// 格式 |
| 标题 | `resource.name` | 窗口标题 |
| 来源服务器 | `server_name` | MCP 服务器名 |

点击行展开:
- 资源元信息 (Descriptions)
- 内容预览区 — 渲染 `read_result.contents[0].text`，如果 mime_type 是图片则渲染 Image

子组件:
- `DesktopResources/WindowList.tsx`
- `DesktopResources/WindowDetail.tsx`

---

## 4. i18n 新增 Key

```json
{
  "debug": {
    "title": "Debug Panel",
    "tools": "Tools",
    "resources": "Resources",
    "history": "History",
    "search": "Search tools...",
    "filterByServer": "Filter by server",
    "allServers": "All servers",
    "toolDetail": "Tool Detail",
    "inputSchema": "Input Schema",
    "annotations": "Annotations",
    "callTest": "Call Test",
    "params": "Parameters",
    "formMode": "Form",
    "jsonMode": "JSON",
    "timeout": "Timeout (seconds)",
    "execute": "Execute",
    "result": "Result",
    "noTools": "No tools available. Start MCP servers first.",
    "history": {
      "time": "Time",
      "tool": "Tool",
      "server": "Server",
      "status": "Status",
      "duration": "Duration",
      "params": "Parameters",
      "replay": "Replay",
      "editAndCall": "Edit & Call",
      "noHistory": "No call history yet."
    },
    "resources": {
      "uri": "URI",
      "name": "Name",
      "mimeType": "MIME Type",
      "server": "Server",
      "read": "Read",
      "content": "Content",
      "noResources": "No resources available."
    }
  },
  "desktop": {
    "title": "Desktop Resources",
    "refresh": "Refresh",
    "searchUri": "Search URI...",
    "size": "Size",
    "windowUri": "Window URI",
    "windowTitle": "Title",
    "sourceServer": "Source Server",
    "preview": "Preview",
    "noWindows": "No desktop resources available."
  }
}
```

---

## 5. 文件清单

### 新建文件
```
src-tauri/src/commands/debug.rs
src-tauri/src/commands/desktop.rs
src/stores/debugStore.ts
src/stores/desktopStore.ts
src/components/DebugPanel/index.tsx
src/components/DebugPanel/ToolBrowser.tsx
src/components/DebugPanel/SchemaForm.tsx
src/components/DebugPanel/ToolCallResult.tsx
src/components/DebugPanel/ResourceBrowser.tsx
src/components/DebugPanel/CallHistory.tsx
src/components/DesktopResources/index.tsx
src/components/DesktopResources/WindowList.tsx
src/components/DesktopResources/WindowDetail.tsx
```

### 修改文件
```
src-tauri/src/commands/mod.rs   — 新增 debug, desktop 模块
src-tauri/src/lib.rs            — 注册新命令
src-tauri/Cargo.toml            — 新增 uuid
src/App.tsx                     — debug 和 resources 路由对接组件
src/locales/en/translation.json — 新增 key
src/locales/zh/translation.json — 新增 key
```

---

## 6. 技术风险与决策

### 6.1 Resource API 可用性

`smcp-computer` 的 `Computer` 结构体没有直接暴露 `list_resources` / `read_resource` 方法。`MCPClientProtocol` trait 有 `list_windows` 和 `get_window_detail`，但这些方法在各个 client 实现上（StdioMCPClient 等），不通过 `MCPServerManager` 公开暴露。

**解决方案优先级**:
1. **检查 crate 版本更新** — 如果 `smcp-computer` 后续版本暴露了这些方法，直接使用
2. **通过 MCP 协议工具调用** — 将 `resources/list` 和 `resources/read` 作为特殊的 tool call 执行
3. **Fork 或提 PR** — 在 MCPServerManager 上新增委托方法

### 6.2 Tool → Server 归属映射

`Computer::get_available_tools()` 返回聚合后的 `Vec<Tool>`，但 Tool 中不直接标注来源服务器。需要:
- 检查 `tool.meta` 中是否有 `server_name` 字段（smcp-computer 可能在聚合时注入）
- 若无，需要遍历各服务器的工具列表手动归属

### 6.3 JSON Schema 表单生成的边界

不追求覆盖全部 JSON Schema 特性，MVP 支持:
- `type`: string, number, integer, boolean, object, array
- `enum`: 转为 Select
- `required`: 标记必填
- `default`: 预填默认值
- `description`: 作为 Form.Item 的 tooltip

不支持: `anyOf`、`oneOf`、`$ref`、`patternProperties`、复杂 `if/then/else` — 这些场景用户可切换到 JSON 编辑器模式手写 JSON。

---

## 7. 验收标准

1. 工具浏览器正确列出所有已启动 MCP 服务器的工具，显示名称、来源服务器、描述
2. 选中工具后展示 inputSchema 的结构化视图
3. 能通过自动生成的表单或 JSON 编辑器填写参数并执行工具调用
4. 调用结果正确显示文本内容、图片内容、错误信息和耗时
5. 调用历史列表按时间倒序展示，可展开查看参数和错误
6. 历史重放功能能将参数预填到调用表单
7. Resource 浏览器能列出 MCP 服务器提供的 Resource（在 API 可用的前提下）
8. 桌面资源页面能展示 window:// URI 列表及其内容预览
9. 表单/JSON 模式可自由切换，JSON 模式的内容与表单值同步
