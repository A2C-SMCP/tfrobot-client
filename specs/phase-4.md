# Phase 4: 日志与仪表盘 — 技术执行 Spec

> **状态**: 待开发
> **对应 PRD**: 3.1 Dashboard, 3.7 日志系统
> **前置**: Phase 2 (Computer 实例就绪), Phase 3 (工具调用/历史数据就绪)

---

## 1. 目标

实现 SQLite 日志后端、日志写入集成、日志查看器 UI、日志导出，以及 Dashboard 首页。完成后应用具备完整的运行状态可观测性。

---

## 2. 后端变更

### 2.1 SQLite 集成

**新增依赖** (`Cargo.toml`):
```toml
rusqlite = { version = "0.31", features = ["bundled"] }
```

选择 `rusqlite` 而非 `tauri-plugin-sql` 的原因:
- `tauri-plugin-sql` 是前端直接操作 SQL，不符合"后端为真实数据源"的架构
- `rusqlite` 在 Rust 侧操作，日志写入可在 service 内部透明完成
- `bundled` feature 内嵌 SQLite 二进制，无需系统安装

### 2.2 LogService 重写 (`src-tauri/src/services/logger.rs`)

完全替换现有 stub:

```rust
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: i64,
    pub timestamp: String,           // ISO 8601
    pub level: String,               // DEBUG / INFO / WARN / ERROR
    pub category: String,            // mcp / smcp / tool_call / system
    pub source: Option<String>,      // 来源服务器或模块名
    pub message: String,
    pub details: Option<String>,     // JSON string
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogFilter {
    pub time_from: Option<String>,
    pub time_to: Option<String>,
    pub levels: Option<Vec<String>>,
    pub categories: Option<Vec<String>>,
    pub source: Option<String>,
    pub keyword: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub struct LogService {
    conn: Mutex<Connection>,
}

impl LogService {
    pub fn new(app_data_dir: PathBuf) -> Result<Self, rusqlite::Error> {
        let db_path = app_data_dir.join("logs.db");
        let conn = Connection::open(&db_path)?;

        conn.execute_batch("
            CREATE TABLE IF NOT EXISTS logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                level TEXT NOT NULL,
                category TEXT NOT NULL,
                source TEXT,
                message TEXT NOT NULL,
                details TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_logs_timestamp ON logs(timestamp);
            CREATE INDEX IF NOT EXISTS idx_logs_category ON logs(category);
            CREATE INDEX IF NOT EXISTS idx_logs_level ON logs(level);
        ")?;

        Ok(Self { conn: Mutex::new(conn) })
    }

    /// 写入一条日志
    pub fn write(
        &self,
        level: &str,
        category: &str,
        source: Option<&str>,
        message: &str,
        details: Option<&serde_json::Value>,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let timestamp = Utc::now().to_rfc3339();
        let details_str = details.map(|d| d.to_string());

        conn.execute(
            "INSERT INTO logs (timestamp, level, category, source, message, details)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![timestamp, level, category, source, message, details_str],
        )?;
        Ok(())
    }

    /// 查询日志（带筛选）
    pub fn query(&self, filter: &LogFilter) -> Result<Vec<LogEntry>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        let mut sql = String::from("SELECT id, timestamp, level, category, source, message, details FROM logs WHERE 1=1");
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ref from) = filter.time_from {
            sql.push_str(" AND timestamp >= ?");
            param_values.push(Box::new(from.clone()));
        }
        if let Some(ref to) = filter.time_to {
            sql.push_str(" AND timestamp <= ?");
            param_values.push(Box::new(to.clone()));
        }
        if let Some(ref levels) = filter.levels {
            if !levels.is_empty() {
                let placeholders: Vec<String> = levels.iter().enumerate()
                    .map(|_| "?".to_string()).collect();
                sql.push_str(&format!(" AND level IN ({})", placeholders.join(",")));
                for l in levels {
                    param_values.push(Box::new(l.clone()));
                }
            }
        }
        if let Some(ref categories) = filter.categories {
            if !categories.is_empty() {
                let placeholders: Vec<String> = categories.iter().enumerate()
                    .map(|_| "?".to_string()).collect();
                sql.push_str(&format!(" AND category IN ({})", placeholders.join(",")));
                for c in categories {
                    param_values.push(Box::new(c.clone()));
                }
            }
        }
        if let Some(ref source) = filter.source {
            sql.push_str(" AND source = ?");
            param_values.push(Box::new(source.clone()));
        }
        if let Some(ref keyword) = filter.keyword {
            sql.push_str(" AND (message LIKE ? OR details LIKE ?)");
            let pattern = format!("%{}%", keyword);
            param_values.push(Box::new(pattern.clone()));
            param_values.push(Box::new(pattern));
        }

        sql.push_str(" ORDER BY timestamp DESC");

        if let Some(limit) = filter.limit {
            sql.push_str(&format!(" LIMIT {}", limit));
        } else {
            sql.push_str(" LIMIT 1000"); // 默认上限
        }
        if let Some(offset) = filter.offset {
            sql.push_str(&format!(" OFFSET {}", offset));
        }

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok(LogEntry {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                level: row.get(2)?,
                category: row.get(3)?,
                source: row.get(4)?,
                message: row.get(5)?,
                details: row.get(6)?,
            })
        })?;

        rows.collect()
    }

    /// 清理过期日志
    pub fn cleanup(&self, before_days: u32) -> Result<usize, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let cutoff = Utc::now() - chrono::Duration::days(before_days as i64);
        let cutoff_str = cutoff.to_rfc3339();

        conn.execute(
            "DELETE FROM logs WHERE timestamp < ?1",
            params![cutoff_str],
        )
        .map(|n| n)
    }

    /// 导出日志为 JSON
    pub fn export(&self, filter: &LogFilter, path: &str) -> Result<usize, Box<dyn std::error::Error>> {
        let entries = self.query(filter)?;
        let count = entries.len();
        let json = serde_json::to_string_pretty(&entries)?;
        std::fs::write(path, json)?;
        Ok(count)
    }

    /// 清空所有日志
    pub fn clear_all(&self) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM logs", [])?;
        Ok(())
    }
}
```

### 2.3 日志写入集成点

在关键操作中调用 `LogService::write()`，将 LogService 加入 AppState:

```rust
pub struct AppState {
    pub computer: Arc<RwLock<Computer<SilentSession>>>,
    pub config: Arc<ConfigService>,
    pub profile_service: Arc<ProfileService>,
    pub log_service: Arc<LogService>,
}
```

**MCP 服务器事件** (`commands/mcp.rs`):
```rust
// start_mcp_server 成功后:
state.log_service.write("INFO", "mcp", Some(&name),
    &format!("MCP server '{}' started", name), None).ok();

// start_mcp_server 失败时:
state.log_service.write("ERROR", "mcp", Some(&name),
    &format!("Failed to start MCP server '{}': {}", name, e),
    Some(&serde_json::json!({"error": e.to_string()}))).ok();

// stop / add / remove 同理
```

**SMCP 连接事件** (`commands/connection.rs`):
```rust
// connect_smcp 成功后:
state.log_service.write("INFO", "smcp", None,
    &format!("Connected to {} office {}", profile.url, profile.office_id),
    Some(&serde_json::json!({"profile": profile.name}))).ok();

// disconnect:
state.log_service.write("INFO", "smcp", None, "Disconnected from SMCP server", None).ok();
```

**工具调用** (`commands/debug.rs`):
```rust
// execute_tool 完成后:
let details = serde_json::json!({
    "params": params,
    "duration_ms": duration_ms,
    "success": response.success,
    "error": response.error,
});
state.log_service.write(
    if response.success { "INFO" } else { "ERROR" },
    "tool_call",
    Some(&tool_name),
    &format!("Tool call '{}': {}", tool_name, if response.success { "success" } else { "failed" }),
    Some(&details),
).ok();
```

**应用启动** (`lib.rs` setup):
```rust
log_service.write("INFO", "system", None, "Application started", None).ok();
// 启动时自动清理 30 天前日志
log_service.cleanup(30).ok();
```

### 2.4 日志命令重写 (`src-tauri/src/commands/logs.rs`)

替换现有 stub:

```rust
use crate::AppState;
use crate::services::logger::{LogEntry, LogFilter};
use tauri::State;

#[tauri::command]
pub async fn get_logs(
    state: State<'_, AppState>,
    filter: LogFilter,
) -> Result<Vec<LogEntry>, String> {
    state.log_service.query(&filter).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn export_logs(
    state: State<'_, AppState>,
    path: String,
    filter: LogFilter,
) -> Result<usize, String> {
    state.log_service.export(&filter, &path).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clear_logs(
    state: State<'_, AppState>,
    before_days: Option<u32>,
) -> Result<(), String> {
    match before_days {
        Some(days) => { state.log_service.cleanup(days).map_err(|e| e.to_string())?; }
        None => { state.log_service.clear_all().map_err(|e| e.to_string())?; }
    }
    Ok(())
}
```

命令注册更新（`clear_logs` 为新增）:
```rust
commands::logs::get_logs,      // 已注册，签名变更
commands::logs::export_logs,   // 已注册，签名变更
commands::logs::clear_logs,    // 新增
```

### 2.5 Dashboard 数据命令 (`src-tauri/src/commands/dashboard.rs` — 新建)

```rust
use crate::AppState;
use serde::Serialize;
use tauri::State;

#[derive(Serialize)]
pub struct DashboardData {
    pub connection: ConnectionSummary,
    pub mcp: McpSummary,
    pub tools: ToolsSummary,
    pub recent_calls: Vec<RecentCall>,
    pub runtimes: Vec<RuntimeStatus>,
}

#[derive(Serialize)]
pub struct ConnectionSummary {
    pub connected: bool,
    pub url: Option<String>,
    pub office_id: Option<String>,
}

#[derive(Serialize)]
pub struct McpSummary {
    pub total: usize,
    pub running: usize,
    pub stopped: usize,
    pub error: usize,
}

#[derive(Serialize)]
pub struct ToolsSummary {
    pub total: usize,
    pub by_server: Vec<(String, usize)>,  // (server_name, tool_count)
}

#[derive(Serialize)]
pub struct RecentCall {
    pub timestamp: String,
    pub tool: String,
    pub success: bool,
}

#[derive(Serialize)]
pub struct RuntimeStatus {
    pub name: String,         // "Node.js", "Python", "uv", "pnpm"
    pub path: Option<String>,
    pub available: bool,
}

#[tauri::command]
pub async fn get_dashboard_data(
    state: State<'_, AppState>,
) -> Result<DashboardData, String> {
    let computer = state.computer.read().await;

    // Connection
    let client = computer.get_socketio_client();
    let client_guard = client.read().await;
    let connection = match client_guard.as_ref() {
        Some(c) => ConnectionSummary {
            connected: c.get_office_id().await.is_some(),
            url: Some(c.get_url()),
            office_id: c.get_office_id().await,
        },
        None => ConnectionSummary {
            connected: false,
            url: None,
            office_id: None,
        },
    };

    // MCP
    let statuses = computer.get_server_status().await;
    let running = statuses.iter().filter(|(_, r, _)| *r).count();
    let error = statuses.iter().filter(|(_, _, msg)| msg.starts_with("Error")).count();
    let mcp = McpSummary {
        total: statuses.len(),
        running,
        stopped: statuses.len() - running - error,
        error,
    };

    // Tools
    let tools_list = computer.get_available_tools().await.unwrap_or_default();
    let tools = ToolsSummary {
        total: tools_list.len(),
        by_server: vec![], // 按 server 分组统计（需 tool.meta.server_name）
    };

    // Recent calls (最近 5 条)
    let history = computer.get_tool_history().await.unwrap_or_default();
    let recent_calls: Vec<RecentCall> = history.iter().rev().take(5).map(|r| {
        RecentCall {
            timestamp: r.timestamp.to_rfc3339(),
            tool: r.tool.clone(),
            success: r.success,
        }
    }).collect();

    // Runtimes — 简单检测系统 PATH 中是否存在
    let runtimes = detect_runtimes();

    Ok(DashboardData {
        connection,
        mcp,
        tools,
        recent_calls,
        runtimes,
    })
}

fn detect_runtimes() -> Vec<RuntimeStatus> {
    vec![
        detect_runtime("Node.js", "node"),
        detect_runtime("Python", "python3"),
        detect_runtime("uv", "uv"),
        detect_runtime("pnpm", "pnpm"),
    ]
}

fn detect_runtime(name: &str, cmd: &str) -> RuntimeStatus {
    let path = which::which(cmd).ok().map(|p| p.to_string_lossy().to_string());
    RuntimeStatus {
        name: name.to_string(),
        available: path.is_some(),
        path,
    }
}
```

**新增依赖**: `which = "6"` (运行时路径查找)

---

## 3. 前端变更

### 3.1 新增 Store: logStore

**文件**: `src/stores/logStore.ts`

```typescript
export interface LogEntry {
  id: number;
  timestamp: string;
  level: 'DEBUG' | 'INFO' | 'WARN' | 'ERROR';
  category: 'mcp' | 'smcp' | 'tool_call' | 'system';
  source?: string;
  message: string;
  details?: string;  // JSON string, 前端解析展示
}

export interface LogFilter {
  time_from?: string;
  time_to?: string;
  levels?: string[];
  categories?: string[];
  source?: string;
  keyword?: string;
  limit?: number;
  offset?: number;
}

interface LogState {
  logs: LogEntry[];
  loading: boolean;
  error: string | null;
  filter: LogFilter;

  setFilter: (filter: Partial<LogFilter>) => void;
  fetchLogs: () => Promise<void>;
  exportLogs: (path: string) => Promise<number>;
  clearLogs: (beforeDays?: number) => Promise<void>;
}
```

### 3.2 日志查看器页面

**文件**: `src/components/LogViewer/index.tsx`

上下布局:

**筛选栏** (上方，Ant Design Form inline):
```typescript
<Form layout="inline">
  {/* 时间范围 */}
  <Form.Item>
    <RangePicker showTime />
  </Form.Item>

  {/* 快捷时间按钮 */}
  <Form.Item>
    <Segmented options={[
      { label: t('logs.lastHour'), value: '1h' },
      { label: t('logs.today'), value: 'today' },
      { label: t('logs.last7days'), value: '7d' },
    ]} onChange={handleQuickTime} />
  </Form.Item>

  {/* 级别筛选 */}
  <Form.Item>
    <Select mode="multiple" placeholder={t('logs.level')}
      options={['DEBUG','INFO','WARN','ERROR'].map(l => ({ label: l, value: l }))}
    />
  </Form.Item>

  {/* 类别筛选 */}
  <Form.Item>
    <Select mode="multiple" placeholder={t('logs.category')}
      options={['mcp','smcp','tool_call','system'].map(c => ({ label: c, value: c }))}
    />
  </Form.Item>

  {/* 来源筛选 */}
  <Form.Item>
    <Select placeholder={t('logs.source')} allowClear />
  </Form.Item>

  {/* 关键词搜索 */}
  <Form.Item>
    <Input.Search placeholder={t('logs.searchKeyword')} onSearch={handleSearch} />
  </Form.Item>

  {/* 导出按钮 */}
  <Form.Item>
    <Button icon={<ExportOutlined />} onClick={handleExport}>{t('logs.export')}</Button>
  </Form.Item>
</Form>
```

**日志列表** (下方):

使用 Ant Design Table，带可展开行:

```typescript
const columns = [
  {
    title: '', width: 8,
    render: (_, record) => <div style={{
      width: 4, height: '100%', borderRadius: 2,
      backgroundColor: levelColor[record.level]
    }} />
  },
  { title: t('logs.time'), dataIndex: 'timestamp', width: 180,
    render: (ts) => dayjs(ts).format('HH:mm:ss.SSS') },
  { title: t('logs.level'), dataIndex: 'level', width: 80,
    render: (level) => <Tag color={levelColor[level]}>{level}</Tag> },
  { title: t('logs.category'), dataIndex: 'category', width: 100,
    render: (cat) => <Tag>{cat}</Tag> },
  { title: t('logs.source'), dataIndex: 'source', width: 120 },
  { title: t('logs.message'), dataIndex: 'message', ellipsis: true },
];

const levelColor = {
  DEBUG: 'default', INFO: 'blue', WARN: 'orange', ERROR: 'red'
};
```

**展开行**: 解析 `details` JSON 字符串，使用 `<pre>` 格式化展示。

**分页**: Table 分页，默认每页 50 条。通过 `filter.limit` + `filter.offset` 实现后端分页。

子组件:
- `LogViewer/LogFilter.tsx` — 筛选栏
- `LogViewer/LogTable.tsx` — 日志表格

### 3.3 Dashboard 首页

**文件**: `src/components/Dashboard/index.tsx`

卡片网格布局 (Ant Design Row + Col):

```typescript
<Row gutter={[16, 16]}>
  <Col span={12}>
    <ConnectionCard data={dashboard.connection} onClick={() => navigate('smcp')} />
  </Col>
  <Col span={12}>
    <McpCard data={dashboard.mcp} onClick={() => navigate('mcp')} />
  </Col>
  <Col span={12}>
    <ToolsCard data={dashboard.tools} onClick={() => navigate('debug')} />
  </Col>
  <Col span={12}>
    <RuntimesCard data={dashboard.runtimes} onClick={() => navigate('settings')} />
  </Col>
  <Col span={24}>
    <RecentActivityCard data={dashboard.recent_calls} onClick={() => navigate('logs')} />
  </Col>
</Row>
```

#### 3.3.1 ConnectionCard
```
┌─────────────────────────┐
│ 🔌 SMCP 连接             │
│                         │
│  ● 已连接               │
│  http://localhost:3000   │
│  Office: dev-room       │
└─────────────────────────┘
```
- 连接时: 绿色圆点 + 地址 + Office
- 断开时: 灰色圆点 + "未连接"

#### 3.3.2 McpCard
```
┌─────────────────────────┐
│ ⚡ MCP 服务器             │
│                         │
│    3        1       0   │
│  运行中   已停止   错误   │
│  ━━━━━━━━━━━━━━━━━━━━  │
│  ████████▓▓▓▓           │
└─────────────────────────┘
```
- 用 Statistic 组件展示数字
- 底部进度条表示运行比例

#### 3.3.3 ToolsCard
```
┌─────────────────────────┐
│ 🔧 可用工具    42         │
│                         │
│  playwright: 12         │
│  filesystem: 8          │
│  github: 22             │
└─────────────────────────┘
```

#### 3.3.4 RuntimesCard
```
┌─────────────────────────┐
│ 💻 运行时环境             │
│                         │
│  ✅ Node.js  /usr/...   │
│  ✅ Python   /usr/...   │
│  ✅ uv       /usr/...   │
│  ❌ pnpm     未检测到    │
└─────────────────────────┘
```

#### 3.3.5 RecentActivityCard
```
┌──────────────────────────────────────────────┐
│ 📋 最近活动                                    │
│                                              │
│  14:32:05  browser_navigate  ✅  120ms       │
│  14:31:58  read_file         ✅   45ms       │
│  14:31:22  execute_command   ❌  timeout     │
│  14:30:10  list_directory    ✅   23ms       │
│  14:29:55  browser_click     ✅   89ms       │
└──────────────────────────────────────────────┘
```

子组件:
- `Dashboard/ConnectionCard.tsx`
- `Dashboard/McpCard.tsx`
- `Dashboard/ToolsCard.tsx`
- `Dashboard/RuntimesCard.tsx`
- `Dashboard/RecentActivityCard.tsx`

### 3.4 App.tsx 路由更新

将 Dashboard 和 LogViewer 组件对接到已有的分组侧边栏:

```typescript
case 'dashboard':
  return <Dashboard />;
case 'logs':
  return <LogViewer />;
```

默认选中项改为 `'dashboard'`。

---

## 4. i18n 新增 Key

```json
{
  "dashboard": {
    "title": "Dashboard",
    "connection": "SMCP Connection",
    "mcpServers": "MCP Servers",
    "tools": "Available Tools",
    "runtimes": "Runtime Environment",
    "recentActivity": "Recent Activity",
    "running": "Running",
    "stopped": "Stopped",
    "errors": "Errors",
    "notConnected": "Not connected",
    "refresh": "Refresh"
  },
  "logs": {
    "title": "Logs",
    "time": "Time",
    "level": "Level",
    "category": "Category",
    "source": "Source",
    "message": "Message",
    "details": "Details",
    "export": "Export",
    "clear": "Clear Logs",
    "clearConfirm": "Are you sure you want to clear all logs?",
    "lastHour": "Last Hour",
    "today": "Today",
    "last7days": "Last 7 Days",
    "searchKeyword": "Search...",
    "exportSuccess": "Exported {{count}} log entries",
    "noLogs": "No logs found."
  }
}
```

---

## 5. 新增依赖

### 后端
```toml
rusqlite = { version = "0.31", features = ["bundled"] }
which = "6"
```

### 前端
```bash
pnpm add dayjs
# dayjs 用于日志时间格式化（Ant Design DatePicker 已内置 dayjs）
```

---

## 6. 文件清单

### 新建文件
```
src-tauri/src/commands/dashboard.rs
src/stores/logStore.ts
src/components/LogViewer/index.tsx
src/components/LogViewer/LogFilter.tsx
src/components/LogViewer/LogTable.tsx
src/components/Dashboard/index.tsx
src/components/Dashboard/ConnectionCard.tsx
src/components/Dashboard/McpCard.tsx
src/components/Dashboard/ToolsCard.tsx
src/components/Dashboard/RuntimesCard.tsx
src/components/Dashboard/RecentActivityCard.tsx
```

### 修改文件
```
src-tauri/Cargo.toml               — 新增 rusqlite, which
src-tauri/src/lib.rs                — AppState 添加 log_service, 注册新命令, 启动日志/清理
src-tauri/src/commands/mod.rs       — 新增 dashboard 模块
src-tauri/src/commands/logs.rs      — 全面重写（替换 stub）
src-tauri/src/commands/mcp.rs       — 操作后写入日志
src-tauri/src/commands/connection.rs — 操作后写入日志
src-tauri/src/commands/debug.rs     — execute_tool 后写入日志
src-tauri/src/services/logger.rs    — 全面重写（SQLite 实现）
src-tauri/src/services/mod.rs       — 确认 logger 模块导出
src/App.tsx                         — dashboard 和 logs 路由对接组件, 默认选中 dashboard
src/locales/en/translation.json     — 新增 key
src/locales/zh/translation.json     — 新增 key
```

---

## 7. 验收标准

1. 应用启动后自动创建 `logs.db`，写入 system 类别的启动日志
2. MCP 服务器启停操作自动写入 mcp 类别日志
3. SMCP 连接/断开操作自动写入 smcp 类别日志
4. 工具调用自动写入 tool_call 类别日志（含参数和耗时）
5. 日志查看器支持按时间范围、级别、类别、来源、关键词筛选
6. 日志条目可展开查看 details JSON
7. 日志可导出为 JSON 文件
8. 应用启动时自动清理 30 天前日志
9. Dashboard 首页正确展示 5 个信息卡片
10. Dashboard 卡片可点击跳转到对应功能页面
11. 数据刷新流畅，日志查询 10 万条以内 < 1 秒
