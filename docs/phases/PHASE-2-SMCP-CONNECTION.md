# Phase 2: SMCP Server 连接

## 目标

实现与远程 SMCP Server 的连接管理，包括认证凭证的安全存储。

## 前置条件

- [ ] Phase 1 完成（MCP Server 管理可用）

## 任务清单

### 2.1 后端：钥匙串服务完善

更新 `src-tauri/src/services/keychain.rs`：

- [ ] `save_credential(server_url, api_key)` - 保存凭证
- [ ] `get_credential(server_url)` - 获取凭证
- [ ] `delete_credential(server_url)` - 删除凭证
- [ ] `list_credentials()` - 列出所有已保存的服务器 URL
- [ ] 错误处理：钥匙串访问被拒绝时的友好提示

### 2.2 后端：SMCP 连接命令

更新 `src-tauri/src/commands/connection.rs`：

- [ ] `connect_smcp(server_url, computer_name, office_id)` - 连接到 SMCP Server
- [ ] `disconnect_smcp()` - 断开连接
- [ ] `get_connection_status()` - 获取当前连接状态
- [ ] `save_server_credential(server_url, api_key)` - 保存凭证
- [ ] `get_saved_servers()` - 获取已保存的服务器列表
- [ ] `delete_server_credential(server_url)` - 删除已保存的凭证

**连接状态枚举**:
```rust
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected { server_url: String, office_id: String },
    Error { message: String },
}
```

### 2.3 后端：集成 SmcpComputerClient

- [ ] 创建 `src-tauri/src/services/smcp_client.rs`
- [ ] 封装 `SmcpComputerClient` 的连接/断开逻辑
- [ ] 处理 Socket.IO 事件回调
- [ ] 实现工具列表同步（连接后向 Server 上报可用工具）

**关键集成点**:
```rust
use smcp_computer::socketio_client::SmcpComputerClient;

// 连接
let client = SmcpComputerClient::connect(url, name, api_key).await?;
client.join_office(office_id).await?;

// 上报工具
let tools = manager.available_tools().await?;
client.update_tools(tools).await?;
```

### 2.4 后端：连接状态事件

- [ ] 使用 Tauri 事件系统向前端推送状态变化
- [ ] 事件名：`smcp:status_changed`
- [ ] 事件数据：`ConnectionStatus`

### 2.5 前端：Connection Store

创建 `src/stores/connectionStore.ts`：

- [ ] 状态：`status`, `savedServers`, `loading`
- [ ] Actions：`connect`, `disconnect`, `fetchStatus`, `saveCredential`, `deleteCredential`
- [ ] 监听 Tauri 事件更新状态

### 2.6 前端：连接管理组件

创建 `src/components/Connection/` 目录：

- [ ] `ConnectionPanel.tsx` - 主面板
  - 显示当前连接状态
  - 连接/断开按钮
  - 已保存服务器快捷切换
- [ ] `ConnectForm.tsx` - 连接表单
  - Server URL 输入
  - API Key 输入（密码类型）
  - Computer Name 输入
  - Office ID 输入
  - "记住凭证" 复选框
- [ ] `SavedServerList.tsx` - 已保存服务器列表
  - 列表显示
  - 一键连接
  - 删除凭证

### 2.7 前端：状态指示器

- [ ] Header 区域添加连接状态指示器
- [ ] 使用 Ant Design 的 `Badge` 或 `Tag` 组件
- [ ] 颜色：绿色=已连接，灰色=未连接，红色=错误

## 验收标准

1. 可以输入 SMCP Server URL 和 API Key 进行连接
2. 连接成功后状态显示为"已连接"
3. 可以保存凭证，下次自动填充
4. 断开连接功能正常
5. 网络断开后能自动尝试重连（依赖 Socket.IO）

## 预计文件变更

```
src-tauri/src/
├── commands/
│   └── connection.rs             # 重写
├── services/
│   ├── keychain.rs               # 完善
│   └── smcp_client.rs            # 新增

src/
├── stores/
│   └── connectionStore.ts        # 新增
├── components/
│   └── Connection/
│       ├── index.tsx             # 新增
│       ├── ConnectionPanel.tsx   # 新增
│       ├── ConnectForm.tsx       # 新增
│       └── SavedServerList.tsx   # 新增
├── App.tsx                       # 修改（Header 状态指示器）
└── locales/                      # 补充翻译
```
