# Phase 5: 主题与设置 — 技术执行 Spec

> **状态**: 待开发
> **对应 PRD**: 3.8 设置, 3.2.2 高级配置表单
> **前置**: Phase 4 (日志系统就绪, Dashboard 就绪)

---

## 1. 目标

实现亮/暗主题切换、完整设置页面（主题、语言、运行时路径）、运行时检测与安装引导、日志自动清理配置、MCP 服务器高级配置表单（tool_meta、forbidden_tools、VRL）。

---

## 2. 后端变更

### 2.1 SettingsService (`src-tauri/src/services/settings.rs` — 新建)

```rust
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub theme: ThemeMode,
    pub language: String,              // "zh" | "en"
    pub log_retention_days: u32,       // 默认 30
    pub custom_runtime_paths: RuntimePaths,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    Light,
    Dark,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RuntimePaths {
    pub node: Option<String>,
    pub python: Option<String>,
    pub uv: Option<String>,
    pub pnpm: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            language: "en".to_string(),
            log_retention_days: 30,
            custom_runtime_paths: RuntimePaths::default(),
        }
    }
}

pub struct SettingsService {
    settings_file: PathBuf,
}

impl SettingsService {
    pub fn new(app_data_dir: PathBuf) -> Self {
        Self {
            settings_file: app_data_dir.join("settings.json"),
        }
    }

    pub fn load(&self) -> AppSettings {
        if !self.settings_file.exists() {
            return AppSettings::default();
        }
        fs::read_to_string(&self.settings_file)
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, settings: &AppSettings) -> Result<(), std::io::Error> {
        let content = serde_json::to_string_pretty(settings)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        fs::write(&self.settings_file, content)
    }
}
```

### 2.2 设置命令 (`src-tauri/src/commands/settings.rs` — 新建)

```rust
use crate::AppState;
use crate::services::settings::AppSettings;
use serde::Serialize;
use tauri::State;

#[tauri::command]
pub async fn get_settings(
    state: State<'_, AppState>,
) -> Result<AppSettings, String> {
    Ok(state.settings_service.load())
}

#[tauri::command]
pub async fn update_settings(
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<(), String> {
    state.settings_service.save(&settings).map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct RuntimeInfo {
    pub name: String,
    pub path: Option<String>,
    pub version: Option<String>,
    pub available: bool,
}

#[tauri::command]
pub async fn detect_runtimes() -> Result<Vec<RuntimeInfo>, String> {
    let runtimes = vec![
        detect_one("Node.js", "node", &["--version"]),
        detect_one("Python", "python3", &["--version"]),
        detect_one("uv", "uv", &["--version"]),
        detect_one("pnpm", "pnpm", &["--version"]),
    ];
    Ok(runtimes)
}

fn detect_one(name: &str, cmd: &str, version_args: &[&str]) -> RuntimeInfo {
    let path = which::which(cmd).ok().map(|p| p.to_string_lossy().to_string());
    let version = if path.is_some() {
        std::process::Command::new(cmd)
            .args(version_args)
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|v| v.trim().to_string())
    } else {
        None
    };

    RuntimeInfo {
        name: name.to_string(),
        available: path.is_some(),
        path,
        version,
    }
}

#[derive(Serialize)]
pub struct AppInfo {
    pub version: String,
    pub smcp_computer_version: String,
}

#[tauri::command]
pub async fn get_app_info() -> Result<AppInfo, String> {
    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        smcp_computer_version: smcp_computer::VERSION.to_string(),
    })
}
```

### 2.3 AppState 扩展

```rust
pub struct AppState {
    pub computer: Arc<RwLock<Computer<SilentSession>>>,
    pub config: Arc<ConfigService>,
    pub profile_service: Arc<ProfileService>,
    pub log_service: Arc<LogService>,
    pub settings_service: Arc<SettingsService>,  // 新增
}
```

### 2.4 命令注册

```rust
commands::settings::get_settings,
commands::settings::update_settings,
commands::settings::detect_runtimes,
commands::settings::get_app_info,
```

---

## 3. 前端变更

### 3.1 主题系统

#### 3.1.1 Theme Store (`src/stores/themeStore.ts`)

```typescript
import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

type ThemeMode = 'light' | 'dark' | 'system';

interface ThemeState {
  mode: ThemeMode;
  resolved: 'light' | 'dark';  // system 解析后的实际主题
  setMode: (mode: ThemeMode) => void;
}

export const useThemeStore = create<ThemeState>((set) => ({
  mode: 'system',
  resolved: getSystemTheme(),

  setMode: (mode) => {
    const resolved = mode === 'system' ? getSystemTheme() : mode;
    set({ mode, resolved });
    // 持久化到后端设置
    invoke('get_settings').then((settings: any) => {
      invoke('update_settings', { settings: { ...settings, theme: mode } });
    });
  },
}));

function getSystemTheme(): 'light' | 'dark' {
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}
```

#### 3.1.2 App.tsx ConfigProvider 集成

```typescript
import { theme as antTheme, ConfigProvider } from 'antd';

function App() {
  const { resolved } = useThemeStore();

  return (
    <ConfigProvider
      theme={{
        algorithm: resolved === 'dark'
          ? antTheme.darkAlgorithm
          : antTheme.defaultAlgorithm,
        // 可选: 自定义 token 覆盖
        token: {
          // colorPrimary: '#1677ff',  // 如需自定义品牌色
        },
      }}
    >
      {/* ... existing layout ... */}
    </ConfigProvider>
  );
}
```

#### 3.1.3 全局样式适配

**文件**: `src/styles/index.css`

```css
/* 暗色主题下的全局适配 */
:root {
  color-scheme: light dark;
}

/* 暗色主题下的背景色，Ant Design 5 的 token 系统会自动处理组件 */
/* 仅需处理自定义样式 */
```

**文件**: `src/styles/App.module.css`

将 header/sider/content 的硬编码颜色改为使用 Ant Design token 或 CSS 变量:

```css
.header {
  /* 删除硬编码 background: #fff; */
  /* Ant Design Layout.Header 会自动响应主题 */
}

.sider {
  /* 删除硬编码背景色 */
}
```

#### 3.1.4 系统主题变化监听

在 App.tsx 中:

```typescript
useEffect(() => {
  const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
  const handler = () => {
    const { mode } = useThemeStore.getState();
    if (mode === 'system') {
      useThemeStore.setState({ resolved: mediaQuery.matches ? 'dark' : 'light' });
    }
  };
  mediaQuery.addEventListener('change', handler);
  return () => mediaQuery.removeEventListener('change', handler);
}, []);
```

### 3.2 设置页面

**文件**: `src/components/Settings/index.tsx`

使用 Ant Design Tabs 分区:

```typescript
<Tabs defaultActiveKey="appearance" items={[
  { key: 'appearance', label: t('settings.appearance'), children: <AppearanceSettings /> },
  { key: 'runtime', label: t('settings.runtime'), children: <RuntimeSettings /> },
  { key: 'data', label: t('settings.data'), children: <DataSettings /> },
  { key: 'about', label: t('settings.about'), children: <AboutSection /> },
]} />
```

#### 3.2.1 外观设置 (`Settings/AppearanceSettings.tsx`)

```typescript
<Form layout="vertical">
  {/* 主题 */}
  <Form.Item label={t('settings.theme')}>
    <Segmented
      value={themeMode}
      options={[
        { label: t('settings.themeLight'), value: 'light' },
        { label: t('settings.themeDark'), value: 'dark' },
        { label: t('settings.themeSystem'), value: 'system' },
      ]}
      onChange={(value) => setMode(value as ThemeMode)}
    />
  </Form.Item>

  {/* 语言 */}
  <Form.Item label={t('settings.language')}>
    <Select value={language} onChange={changeLanguage}
      options={[
        { label: '中文', value: 'zh' },
        { label: 'English', value: 'en' },
      ]}
    />
  </Form.Item>
</Form>
```

#### 3.2.2 运行时设置 (`Settings/RuntimeSettings.tsx`)

```typescript
<Table
  dataSource={runtimes}
  columns={[
    { title: t('settings.runtimeName'), dataIndex: 'name' },
    { title: t('settings.runtimePath'), dataIndex: 'path',
      render: (path) => path || <Text type="secondary">{t('settings.notDetected')}</Text> },
    { title: t('settings.runtimeVersion'), dataIndex: 'version' },
    { title: t('settings.runtimeStatus'), dataIndex: 'available',
      render: (available) => available
        ? <Tag color="success">{t('settings.installed')}</Tag>
        : <Tag color="error">{t('settings.notInstalled')}</Tag> },
    { title: '', dataIndex: 'available',
      render: (available, record) => !available && (
        <Button type="link" onClick={() => openInstallGuide(record.name)}>
          {t('settings.installGuide')}
        </Button>
      )},
  ]}
  pagination={false}
/>

{/* 高级: 自定义路径 */}
<Collapse>
  <Collapse.Panel header={t('settings.customPaths')} key="paths">
    <Form layout="vertical">
      <Form.Item label="Node.js">
        <Input value={paths.node} onChange={...} placeholder="/usr/local/bin/node" />
      </Form.Item>
      <Form.Item label="Python">
        <Input value={paths.python} onChange={...} placeholder="/usr/bin/python3" />
      </Form.Item>
      {/* uv, pnpm 同理 */}
    </Form>
  </Collapse.Panel>
</Collapse>
```

安装引导: 使用 Tauri shell.open 打开对应官网:
- Node.js: https://nodejs.org/
- Python: https://python.org/
- uv: https://github.com/astral-sh/uv
- pnpm: https://pnpm.io/

#### 3.2.3 数据管理 (`Settings/DataSettings.tsx`)

```typescript
<Space direction="vertical" size="large" style={{ width: '100%' }}>
  {/* 日志保留天数 */}
  <Form.Item label={t('settings.logRetention')}>
    <InputNumber min={1} max={365} value={settings.log_retention_days}
      addonAfter={t('settings.days')}
      onChange={updateRetention} />
  </Form.Item>

  {/* 导出所有配置 */}
  <Button icon={<ExportOutlined />} onClick={exportAllConfigs}>
    {t('settings.exportAll')}
  </Button>

  {/* 清空日志 */}
  <Popconfirm title={t('logs.clearConfirm')} onConfirm={clearLogs}>
    <Button danger>{t('logs.clear')}</Button>
  </Popconfirm>

  {/* 重置应用 */}
  <Popconfirm title={t('settings.resetConfirm')} onConfirm={resetApp}>
    <Button danger type="primary">{t('settings.reset')}</Button>
  </Popconfirm>
</Space>
```

#### 3.2.4 关于 (`Settings/AboutSection.tsx`)

```typescript
<Descriptions bordered column={1}>
  <Descriptions.Item label={t('settings.appVersion')}>{appInfo.version}</Descriptions.Item>
  <Descriptions.Item label={t('settings.sdkVersion')}>{appInfo.smcp_computer_version}</Descriptions.Item>
  <Descriptions.Item label={t('settings.license')}>MIT</Descriptions.Item>
</Descriptions>

<Space style={{ marginTop: 16 }}>
  <Button onClick={checkUpdate}>{t('settings.checkUpdate')}</Button>
  <Button type="link" onClick={() => shell.open('https://github.com/...')}>
    {t('settings.feedback')}
  </Button>
</Space>
```

### 3.3 MCP 高级配置表单

**文件**: `src/components/McpConfig/McpServerForm.tsx` — 扩展

在现有表单底部新增可折叠的高级设置区:

```typescript
<Collapse ghost>
  <Collapse.Panel header={t('mcp.form.advancedSettings')} key="advanced">

    {/* disabled 开关 */}
    <Form.Item name="disabled" label={t('mcp.form.disabled')} valuePropName="checked">
      <Switch />
    </Form.Item>

    {/* forbidden_tools */}
    <Form.Item name="forbidden_tools" label={t('mcp.form.forbiddenTools')}>
      <Select mode="tags" placeholder={t('mcp.form.forbiddenToolsPlaceholder')}
        tokenSeparators={[',']} />
    </Form.Item>

    {/* default_tool_meta */}
    <Card size="small" title={t('mcp.form.defaultToolMeta')} style={{ marginBottom: 16 }}>
      <Form.Item name={['default_tool_meta', 'tags']} label={t('mcp.form.tags')}>
        <Select mode="tags" placeholder={t('mcp.form.tagsPlaceholder')} />
      </Form.Item>
      <Form.Item name={['default_tool_meta', 'auto_apply']} label={t('mcp.form.autoApply')}
        valuePropName="checked">
        <Switch />
      </Form.Item>
    </Card>

    {/* tool_meta (JSON 编辑器) */}
    <Form.Item name="tool_meta_json" label={t('mcp.form.toolMeta')}>
      <Input.TextArea
        rows={6}
        style={{ fontFamily: 'monospace' }}
        placeholder='{ "tool_name": { "alias": "...", "tags": [...] } }'
      />
    </Form.Item>

    {/* VRL 脚本 */}
    <Form.Item name="vrl" label={t('mcp.form.vrl')}>
      <Input.TextArea
        rows={8}
        style={{ fontFamily: 'monospace', fontSize: 13 }}
        placeholder="# VRL transformation script"
      />
    </Form.Item>

  </Collapse.Panel>
</Collapse>
```

**表单提交逻辑更新**: `handleFinish` 中解析高级字段:

```typescript
const handleFinish = async (values: FormValues) => {
  // ... existing type-specific logic ...

  // 高级字段（适用于所有类型）
  const advancedFields = {
    disabled: values.disabled || false,
    forbidden_tools: values.forbidden_tools || [],
    default_tool_meta: values.default_tool_meta?.tags || values.default_tool_meta?.auto_apply
      ? values.default_tool_meta
      : undefined,
    tool_meta: values.tool_meta_json
      ? JSON.parse(values.tool_meta_json)
      : {},
    vrl: values.vrl || undefined,
  };

  // 合并到 config 的各变体中
  if ('Stdio' in config) {
    Object.assign(config.Stdio, advancedFields);
  }
  // Http, Sse 同理
};
```

### 3.4 Header 主题切换按钮

在 `App.tsx` 的 Header 中添加:

```typescript
<Header className={styles.header}>
  <Title level={4} className={styles.title}>{t('app.name')}</Title>
  <Space>
    {/* 主题切换 */}
    <Button
      type="text"
      icon={resolved === 'dark' ? <SunOutlined /> : <MoonOutlined />}
      onClick={() => setMode(resolved === 'dark' ? 'light' : 'dark')}
    />
    {/* 语言切换 */}
    <Button
      type="text"
      onClick={() => i18n.changeLanguage(i18n.language === 'zh' ? 'en' : 'zh')}
    >
      {i18n.language === 'zh' ? 'EN' : '中'}
    </Button>
  </Space>
</Header>
```

---

## 4. i18n 新增 Key

```json
{
  "settings": {
    "appearance": "Appearance",
    "runtime": "Runtime",
    "data": "Data",
    "about": "About",
    "theme": "Theme",
    "themeLight": "Light",
    "themeDark": "Dark",
    "themeSystem": "System",
    "language": "Language",
    "runtimeName": "Runtime",
    "runtimePath": "Path",
    "runtimeVersion": "Version",
    "runtimeStatus": "Status",
    "installed": "Installed",
    "notInstalled": "Not Installed",
    "notDetected": "Not detected",
    "installGuide": "Install Guide",
    "customPaths": "Custom Paths",
    "logRetention": "Log Retention",
    "days": "days",
    "exportAll": "Export All Configurations",
    "reset": "Reset Application",
    "resetConfirm": "This will delete all data. Are you sure?",
    "appVersion": "App Version",
    "sdkVersion": "SDK Version",
    "license": "License",
    "checkUpdate": "Check for Updates",
    "feedback": "Feedback"
  },
  "mcp": {
    "form": {
      "advancedSettings": "Advanced Settings",
      "disabled": "Disabled",
      "forbiddenTools": "Forbidden Tools",
      "forbiddenToolsPlaceholder": "Enter tool names to disable",
      "defaultToolMeta": "Default Tool Metadata",
      "tags": "Tags",
      "tagsPlaceholder": "Add tags",
      "autoApply": "Auto Apply",
      "toolMeta": "Per-Tool Metadata (JSON)",
      "vrl": "VRL Transformation Script"
    }
  }
}
```

---

## 5. 文件清单

### 新建文件
```
src-tauri/src/services/settings.rs
src-tauri/src/commands/settings.rs
src/stores/themeStore.ts
src/components/Settings/index.tsx
src/components/Settings/AppearanceSettings.tsx
src/components/Settings/RuntimeSettings.tsx
src/components/Settings/DataSettings.tsx
src/components/Settings/AboutSection.tsx
```

### 修改文件
```
src-tauri/src/lib.rs                — AppState 添加 settings_service, 注册命令
src-tauri/src/commands/mod.rs       — 新增 settings 模块
src-tauri/src/services/mod.rs       — 新增 settings 模块
src/main.tsx                        — ConfigProvider 包裹主题
src/App.tsx                         — Header 添加主题/语言切换, settings 路由对接
src/styles/App.module.css           — 移除硬编码颜色
src/styles/index.css                — 暗色主题全局适配
src/components/McpConfig/McpServerForm.tsx — 新增高级设置折叠区
src/stores/mcpStore.ts              — McpServerConfig 类型补充高级字段
src/locales/en/translation.json     — 新增 key
src/locales/zh/translation.json     — 新增 key
```

---

## 6. 验收标准

1. 亮/暗/跟随系统三种主题模式可切换，Ant Design 组件全部正确响应
2. 系统主题变化时（如 macOS 自动切换），跟随系统模式实时响应
3. 主题偏好持久化，重启应用后保持上次设置
4. 语言切换即时生效，持久化到设置
5. 运行时检测页面正确显示 Node.js / Python / uv / pnpm 的路径和版本
6. 未安装的运行时显示安装引导链接
7. 自定义运行时路径可配置并持久化
8. 日志保留天数可配置
9. MCP 服务器高级配置表单（forbidden_tools / tool_meta / VRL）可编辑并正确提交
10. Header 中有快捷主题切换和语言切换按钮
