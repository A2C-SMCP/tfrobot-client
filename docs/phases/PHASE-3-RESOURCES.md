# Phase 3: Desktop 资源浏览器

## 目标

实现 Desktop 资源的浏览功能，允许用户查看 MCP Server 暴露的各类资源。

## 前置条件

- [ ] Phase 1 完成（MCP Server 管理）
- [ ] Phase 2 完成（SMCP 连接）

## 任务清单

### 3.1 后端：资源获取命令

创建 `src-tauri/src/commands/resources.rs`：

- [ ] `get_desktop_resources()` - 获取所有桌面资源
- [ ] `get_resource_content(uri)` - 获取单个资源内容
- [ ] `refresh_resources()` - 强制刷新资源列表

**资源类型**:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopResource {
    pub uri: String,           // 如 "window://app-name", "file://path"
    pub name: String,          // 显示名称
    pub resource_type: ResourceType,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResourceType {
    Window,
    File,
    Clipboard,
    Custom(String),  // MCP Server 自定义资源
}
```

### 3.2 后端：集成 smcp-computer desktop 模块

- [ ] 调用 `computer.get_desktop().await`
- [ ] 从各 MCP Server 收集 `resources` 列表
- [ ] 合并并去重

### 3.3 前端：Resources Store

创建 `src/stores/resourceStore.ts`：

- [ ] 状态：`resources`, `selectedResource`, `loading`
- [ ] Actions：`fetchResources`, `selectResource`, `refreshResources`

### 3.4 前端：资源浏览器组件

创建 `src/components/ResourceBrowser/` 目录：

- [ ] `ResourceBrowser.tsx` - 主组件
  - 左侧：资源树/列表
  - 右侧：资源详情/预览
- [ ] `ResourceTree.tsx` - 资源树形结构
  - 按类型分组（窗口/文件/剪贴板/其他）
  - 支持展开/折叠
- [ ] `ResourceDetail.tsx` - 资源详情面板
  - 显示 URI、名称、类型、描述
  - 对于文件类型可显示预览
- [ ] `RefreshButton.tsx` - 手动刷新按钮

### 3.5 UI 设计要点

- 使用 Ant Design 的 `Tree` 组件展示资源层级
- 使用 `Descriptions` 组件展示资源详情
- 文件类型资源可使用 `Image` 或 `Typography.Paragraph` 预览
- 刷新按钮带 loading 状态

## 验收标准

1. 可以查看当前所有 MCP Server 暴露的资源列表
2. 资源按类型正确分组显示
3. 点击资源可查看详细信息
4. 手动刷新按钮功能正常

## 预计文件变更

```
src-tauri/src/
├── commands/
│   ├── mod.rs                    # 添加 resources 模块
│   └── resources.rs              # 新增

src/
├── stores/
│   └── resourceStore.ts          # 新增
├── components/
│   └── ResourceBrowser/
│       ├── index.tsx             # 新增
│       ├── ResourceBrowser.tsx   # 新增
│       ├── ResourceTree.tsx      # 新增
│       ├── ResourceDetail.tsx    # 新增
│       └── RefreshButton.tsx     # 新增
├── App.tsx                       # 修改
└── locales/                      # 补充翻译
```

## 注意事项

- 资源列表可能较大，考虑分页或虚拟滚动
- 某些资源内容获取可能耗时，需要 loading 状态
- 文件资源预览需考虑安全性，避免执行恶意内容
