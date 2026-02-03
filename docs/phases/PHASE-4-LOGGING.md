# Phase 4: 日志系统

## 目标

实现双层日志架构：用户友好的工具调用时间线 + 开发者详细日志。

## 前置条件

- [ ] Phase 1 完成（MCP Server 管理）
- [ ] Phase 2 完成（SMCP 连接，用于记录工具调用）

## 任务清单

### 4.1 后端：日志服务完善

更新 `src-tauri/src/services/logger.rs`：

- [ ] 实现 `LogService` 结构
- [ ] 双存储：内存缓存 + 文件持久化
- [ ] `add_user_log(log: UserFriendlyLog)` - 添加用户日志
- [ ] `add_detailed_log(log: DetailedLog)` - 添加详细日志
- [ ] `get_user_logs(filter)` - 获取用户日志（支持时间范围过滤）
- [ ] `export_detailed_logs(path)` - 导出详细日志到文件
- [ ] `clear_old_logs()` - 清理 30 天前的日志

**日志存储路径**:
- 用户日志：`app_data_dir/logs/user_logs.json`
- 详细日志：`app_data_dir/logs/detailed/YYYY-MM-DD.log`

### 4.2 后端：工具调用日志记录

- [ ] 在工具调用前后自动记录日志
- [ ] 记录内容：工具名、参数摘要、结果状态、耗时
- [ ] 错误时记录完整堆栈

**集成点**：在 `MCPServerManager.call_tool()` 调用前后添加日志记录。

### 4.3 后端：日志命令

更新 `src-tauri/src/commands/logs.rs`：

- [ ] `get_user_logs(start_time, end_time, limit)` - 获取用户日志
- [ ] `export_logs(path)` - 导出详细日志
- [ ] `clear_logs()` - 清理所有日志

### 4.4 前端：Log Store

创建 `src/stores/logStore.ts`：

- [ ] 状态：`logs`, `loading`, `hasMore`
- [ ] Actions：`fetchLogs`, `loadMore`, `exportLogs`, `clearLogs`
- [ ] 支持实时更新（监听 Tauri 事件）

### 4.5 前端：日志面板组件

创建 `src/components/LogPanel/` 目录：

- [ ] `LogPanel.tsx` - 主面板
- [ ] `LogTimeline.tsx` - 时间线视图
  - 使用 Ant Design `Timeline` 组件
  - 每条记录显示：时间、工具名、状态徽章、耗时
  - 点击展开查看参数详情
- [ ] `LogFilter.tsx` - 过滤器
  - 时间范围选择
  - 状态筛选（成功/失败/全部）
  - 工具名搜索
- [ ] `ExportButton.tsx` - 导出按钮
  - 点击触发文件保存对话框
  - 导出为 JSON 格式

### 4.6 用户友好日志格式

```typescript
interface UserFriendlyLog {
  id: string;
  timestamp: string;      // ISO 8601
  toolName: string;
  status: 'pending' | 'success' | 'failed';
  summary: string;        // 参数摘要，如 "query: 'hello world'"
  durationMs?: number;
  error?: string;         // 失败时的错误信息
}
```

### 4.7 自动清理任务

- [ ] 应用启动时检查并清理过期日志
- [ ] 或使用定时任务（如果应用长时间运行）

## 验收标准

1. 工具调用后自动出现在日志时间线中
2. 可以按时间范围和状态过滤日志
3. 可以导出详细日志文件
4. 30 天前的日志自动清理

## 预计文件变更

```
src-tauri/src/
├── commands/
│   └── logs.rs                   # 重写
├── services/
│   └── logger.rs                 # 重写

src/
├── stores/
│   └── logStore.ts               # 新增
├── components/
│   └── LogPanel/
│       ├── index.tsx             # 新增
│       ├── LogPanel.tsx          # 新增
│       ├── LogTimeline.tsx       # 新增
│       ├── LogFilter.tsx         # 新增
│       └── ExportButton.tsx      # 新增
├── App.tsx                       # 修改
└── locales/                      # 补充翻译
```

## 性能考虑

- 内存中只保留最近 1000 条用户日志
- 详细日志直接写入文件，不保留在内存
- 时间线组件使用虚拟滚动（如果日志量大）
