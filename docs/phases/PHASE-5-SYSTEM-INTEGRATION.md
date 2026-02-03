# Phase 5: 系统集成

## 目标

实现系统托盘常驻、自动更新、设置页面等系统级功能。

## 前置条件

- [ ] Phase 1-4 核心功能完成

## 任务清单

### 5.1 系统托盘

#### 5.1.1 后端：托盘菜单

更新 `src-tauri/src/lib.rs`：

- [ ] 创建托盘图标和菜单
- [ ] 菜单项：
  - 显示主窗口
  - 启动所有服务器
  - 停止所有服务器
  - 分隔线
  - 退出

```rust
use tauri::{
    menu::{Menu, MenuItem},
    tray::{TrayIcon, TrayIconBuilder},
};
```

#### 5.1.2 后端：窗口关闭行为

- [ ] 关闭窗口时隐藏而非退出
- [ ] 托盘图标点击显示窗口
- [ ] "退出" 菜单项才真正退出应用

#### 5.1.3 托盘图标状态

- [ ] 准备三种图标：正常、活跃（有服务运行）、错误
- [ ] 根据 MCP Server 状态动态切换图标

### 5.2 自动更新

#### 5.2.1 配置更新服务器

更新 `src-tauri/tauri.conf.json`：

```json
{
  "plugins": {
    "updater": {
      "pubkey": "YOUR_PUBLIC_KEY",
      "endpoints": [
        "https://your-update-server.com/tfrobot-client/{{target}}/{{arch}}/{{current_version}}"
      ]
    }
  }
}
```

#### 5.2.2 后端：更新检查

- [ ] 应用启动时检查更新
- [ ] `check_for_updates()` 命令
- [ ] `install_update()` 命令

#### 5.2.3 前端：更新提示

- [ ] 检测到新版本时弹出 Modal
- [ ] 显示更新日志（如果有）
- [ ] "立即更新" / "稍后提醒" 按钮

### 5.3 设置页面

#### 5.3.1 设置项

创建 `src/components/Settings/` 目录：

- [ ] `Settings.tsx` - 主设置页面
- [ ] `GeneralSettings.tsx` - 通用设置
  - 语言切换（中文/英文）
  - 开机自启动（可选）
  - 关闭行为（最小化到托盘/直接退出）
- [ ] `RuntimeSettings.tsx` - 运行时设置
  - Node.js 路径（内置/系统/自定义）
  - Python 路径（内置/系统/自定义）
- [ ] `AboutSection.tsx` - 关于
  - 版本号
  - 检查更新按钮
  - GitHub 链接
  - 许可证信息

#### 5.3.2 设置持久化

- [ ] 创建 `src-tauri/src/services/settings.rs`
- [ ] 使用 JSON 文件存储设置
- [ ] 提供 Tauri 命令读写设置

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub language: String,           // "en" | "zh"
    pub close_to_tray: bool,
    pub auto_start: bool,
    pub node_path: RuntimePath,
    pub python_path: RuntimePath,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuntimePath {
    Builtin,
    System,
    Custom(String),
}
```

### 5.4 开机自启动（可选）

- [ ] macOS: 使用 `launchd` 或 Login Items
- [ ] Windows: 使用注册表
- [ ] 使用 `tauri-plugin-autostart` 插件

### 5.5 前端：Settings Store

创建 `src/stores/settingsStore.ts`：

- [ ] 状态：`settings`, `loading`
- [ ] Actions：`fetchSettings`, `updateSettings`
- [ ] 语言变更时调用 `i18n.changeLanguage()`

## 验收标准

1. 关闭窗口后应用最小化到托盘
2. 托盘菜单各功能正常工作
3. 可以检查并安装更新
4. 设置页面可以切换语言并立即生效
5. 设置在重启后保持

## 预计文件变更

```
src-tauri/
├── tauri.conf.json               # 更新 updater 配置
├── src/
│   ├── lib.rs                    # 添加托盘逻辑
│   └── services/
│       └── settings.rs           # 新增
├── icons/
│   ├── tray-normal.png           # 新增
│   ├── tray-active.png           # 新增
│   └── tray-error.png            # 新增

src/
├── stores/
│   └── settingsStore.ts          # 新增
├── components/
│   └── Settings/
│       ├── index.tsx             # 新增
│       ├── Settings.tsx          # 新增
│       ├── GeneralSettings.tsx   # 新增
│       ├── RuntimeSettings.tsx   # 新增
│       └── AboutSection.tsx      # 新增
├── App.tsx                       # 修改
└── locales/                      # 补充翻译
```

## 依赖添加

```toml
# src-tauri/Cargo.toml
tauri-plugin-autostart = "2"  # 如果需要开机自启动
```

```bash
# 前端无额外依赖
```
