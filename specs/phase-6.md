# Phase 6: 系统集成与分发 — 技术执行 Spec

> **状态**: 待开发
> **对应 PRD**: PRD §8 非功能性需求, PLAN.md Phase 5/6
> **前置**: Phase 5 (所有功能开发完成)

---

## 1. 目标

实现系统托盘、最小化到托盘、Tauri updater 自动更新、应用打包（DMG / MSI / NSIS）、代码签名与公证（macOS）。完成后应用可作为生产级桌面应用分发。

---

## 2. 系统托盘

### 2.1 Tauri Tray 配置

`tauri.conf.json` 中已启用 `tray-icon` feature。需实现托盘菜单和行为。

**文件**: `src-tauri/src/tray.rs` — 新建

```rust
use tauri::{
    AppHandle, Manager,
    tray::{TrayIconBuilder, TrayIconEvent, MouseButton, MouseButtonState},
    menu::{Menu, MenuItem, PredefinedMenuItem},
};

pub fn setup_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let show = MenuItem::with_id(app, "show", "Show Window", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&show, &separator, &quit])?;

    TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .tooltip("TFRobot Client")
        .on_menu_event(move |app, event| {
            match event.id.as_ref() {
                "show" => {
                    if let Some(window) = app.get_webview_window("main") {
                        window.show().ok();
                        window.set_focus().ok();
                    }
                }
                "quit" => {
                    app.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            // 双击托盘图标显示窗口
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    window.show().ok();
                    window.set_focus().ok();
                }
            }
        })
        .build(app)?;

    Ok(())
}
```

### 2.2 最小化到托盘

**文件**: `src-tauri/src/lib.rs` — setup 闭包中

```rust
// 在 setup 中注册窗口关闭事件
let window = app.get_webview_window("main").unwrap();
window.on_window_event(move |event| {
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        // 阻止默认关闭行为，改为隐藏窗口
        api.prevent_close();
        if let Some(w) = app_handle.get_webview_window("main") {
            w.hide().ok();
        }
    }
});

// 设置托盘
crate::tray::setup_tray(app.handle())?;
```

### 2.3 托盘动态状态

在连接状态变化时更新托盘提示:

```rust
// 连接成功后
tray_icon.set_tooltip(Some("TFRobot Client - Connected")).ok();

// 断开后
tray_icon.set_tooltip(Some("TFRobot Client - Disconnected")).ok();
```

实现方式: 将 TrayIcon handle 存入 AppState，在连接命令中更新。

---

## 3. 自动更新

### 3.1 Tauri Updater 配置

**文件**: `src-tauri/tauri.conf.json`

```json
{
  "plugins": {
    "updater": {
      "active": true,
      "dialog": true,
      "endpoints": [
        "https://releases.tfrobot.example.com/{{target}}/{{arch}}/{{current_version}}"
      ],
      "pubkey": "YOUR_PUBLIC_KEY_HERE"
    }
  }
}
```

**注意**: `pubkey` 和 `endpoints` 需要在实际发布时配置。

### 3.2 前端更新检查

**文件**: `src/components/Settings/AboutSection.tsx`

```typescript
import { check } from '@tauri-apps/plugin-updater';

async function checkForUpdates() {
  try {
    const update = await check();
    if (update) {
      Modal.confirm({
        title: t('settings.updateAvailable'),
        content: `${t('settings.newVersion')}: ${update.version}`,
        onOk: async () => {
          await update.downloadAndInstall();
          // 重启应用
          await invoke('restart_app');
        },
      });
    } else {
      message.info(t('settings.upToDate'));
    }
  } catch (e) {
    message.error(String(e));
  }
}
```

### 3.3 密钥生成

```bash
# 生成更新签名密钥对
pnpm tauri signer generate -w ~/.tauri/tfrobot-client.key
```

生成的公钥填入 `tauri.conf.json` 的 `pubkey` 字段。
私钥通过环境变量 `TAURI_SIGNING_PRIVATE_KEY` 在 CI 中使用。

---

## 4. 应用打包

### 4.1 tauri.conf.json 打包配置

```json
{
  "bundle": {
    "active": true,
    "targets": "all",
    "identifier": "com.tfrobot.client",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ],
    "category": "DeveloperTool",
    "shortDescription": "A2C-SMCP Computer Management Client",
    "longDescription": "TFRobot Client provides a graphical interface for managing MCP servers and SMCP connections.",
    "copyright": "Copyright 2026 TFRobot Team",
    "macOS": {
      "minimumSystemVersion": "10.15",
      "frameworks": [],
      "entitlements": null,
      "signingIdentity": null
    },
    "windows": {
      "certificateThumbprint": null,
      "digestAlgorithm": "sha256",
      "timestampUrl": ""
    }
  }
}
```

### 4.2 macOS 打包

**DMG 构建**:
```bash
pnpm tauri build --target aarch64-apple-darwin    # Apple Silicon
pnpm tauri build --target x86_64-apple-darwin     # Intel
pnpm tauri build --target universal-apple-darwin  # Universal Binary
```

**代码签名** (需要 Apple Developer 账号):
```bash
# 设置环境变量
export APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAM_ID)"

# Tauri 会在构建时自动签名（如果设置了 signingIdentity）
```

**公证** (macOS Notarization):
```bash
# 构建后公证
xcrun notarytool submit target/release/bundle/dmg/TFRobot-Client.dmg \
  --apple-id "your@email.com" \
  --team-id "TEAM_ID" \
  --password "app-specific-password" \
  --wait

# 装订公证票据
xcrun stapler staple target/release/bundle/dmg/TFRobot-Client.dmg
```

### 4.3 Windows 打包

**MSI / NSIS 构建**:
```bash
pnpm tauri build --target x86_64-pc-windows-msvc
```

默认生成 MSI 和 NSIS 安装包。

**代码签名** (需要 Windows 代码签名证书):
```json
// tauri.conf.json
"windows": {
  "certificateThumbprint": "YOUR_CERT_THUMBPRINT",
  "digestAlgorithm": "sha256",
  "timestampUrl": "http://timestamp.digicert.com"
}
```

### 4.4 Linux 打包

```bash
pnpm tauri build --target x86_64-unknown-linux-gnu
```

生成 `.deb` 和 `.AppImage`。

---

## 5. CI/CD 流程

### 5.1 GitHub Actions 工作流

**文件**: `.github/workflows/release.yml`

```yaml
name: Release

on:
  push:
    tags:
      - 'v*'

jobs:
  build:
    strategy:
      matrix:
        include:
          - platform: macos-latest
            target: aarch64-apple-darwin
          - platform: macos-latest
            target: x86_64-apple-darwin
          - platform: ubuntu-22.04
            target: x86_64-unknown-linux-gnu
          - platform: windows-latest
            target: x86_64-pc-windows-msvc

    runs-on: ${{ matrix.platform }}

    steps:
      - uses: actions/checkout@v4

      - name: Setup Node.js
        uses: actions/setup-node@v4
        with:
          node-version: 20

      - name: Setup pnpm
        uses: pnpm/action-setup@v2

      - name: Setup Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - name: Install dependencies (Ubuntu)
        if: matrix.platform == 'ubuntu-22.04'
        run: |
          sudo apt-get update
          sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev

      - name: Install frontend dependencies
        run: pnpm install

      - name: Build Tauri app
        uses: tauri-apps/tauri-action@v0
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_KEY_PASSWORD }}
          # macOS 签名
          APPLE_CERTIFICATE: ${{ secrets.APPLE_CERTIFICATE }}
          APPLE_CERTIFICATE_PASSWORD: ${{ secrets.APPLE_CERTIFICATE_PASSWORD }}
          APPLE_SIGNING_IDENTITY: ${{ secrets.APPLE_SIGNING_IDENTITY }}
          APPLE_ID: ${{ secrets.APPLE_ID }}
          APPLE_PASSWORD: ${{ secrets.APPLE_PASSWORD }}
          APPLE_TEAM_ID: ${{ secrets.APPLE_TEAM_ID }}
        with:
          tagName: v__VERSION__
          releaseName: 'TFRobot Client v__VERSION__'
          releaseBody: 'See CHANGELOG.md for details.'
          releaseDraft: true
          prerelease: false
          args: --target ${{ matrix.target }}
```

### 5.2 更新服务器

更新端点需要提供符合 Tauri updater 格式的 JSON 响应:

```json
{
  "version": "0.2.0",
  "notes": "Bug fixes and improvements",
  "pub_date": "2026-03-01T00:00:00Z",
  "platforms": {
    "darwin-aarch64": {
      "signature": "...",
      "url": "https://releases.tfrobot.example.com/TFRobot-Client_0.2.0_aarch64.dmg.tar.gz"
    },
    "darwin-x86_64": {
      "signature": "...",
      "url": "https://releases.tfrobot.example.com/TFRobot-Client_0.2.0_x64.dmg.tar.gz"
    },
    "windows-x86_64": {
      "signature": "...",
      "url": "https://releases.tfrobot.example.com/TFRobot-Client_0.2.0_x64-setup.nsis.zip"
    },
    "linux-x86_64": {
      "signature": "...",
      "url": "https://releases.tfrobot.example.com/TFRobot-Client_0.2.0_amd64.AppImage.tar.gz"
    }
  }
}
```

可使用 GitHub Releases 作为 CDN，或自建更新服务。

---

## 6. 应用生命周期处理

### 6.1 优雅退出

**文件**: `src-tauri/src/lib.rs`

在 `Builder` 链上添加:

```rust
.on_window_event(|window, event| {
    // 窗口关闭前清理
})
.build(tauri::generate_context!())
.expect("error while running tauri application")
.run(|app_handle, event| {
    match event {
        tauri::RunEvent::ExitRequested { .. } => {
            // 在退出前执行清理
            let state = app_handle.state::<AppState>();
            // 同步关闭 — 使用 block_on 因为已在退出流程
            tauri::async_runtime::block_on(async {
                let computer = state.computer.read().await;
                computer.shutdown().await.ok();
            });
            // 写入退出日志
            state.log_service.write("INFO", "system", None,
                "Application shutting down", None).ok();
        }
        _ => {}
    }
});
```

### 6.2 单实例保护

防止多个应用实例同时运行:

```toml
# Cargo.toml
tauri-plugin-single-instance = "2"
```

```rust
// lib.rs
.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
    // 第二个实例启动时，聚焦已有窗口
    if let Some(window) = app.get_webview_window("main") {
        window.show().ok();
        window.set_focus().ok();
    }
}))
```

---

## 7. 文件清单

### 新建文件
```
src-tauri/src/tray.rs
.github/workflows/release.yml
```

### 修改文件
```
src-tauri/src/lib.rs            — 托盘初始化, 窗口关闭行为, 退出清理, 单实例
src-tauri/src/main.rs           — 可能需要调整入口（通常不变）
src-tauri/Cargo.toml            — 新增 tauri-plugin-single-instance
src-tauri/tauri.conf.json       — 更新 updater, bundle 配置
src/components/Settings/AboutSection.tsx — 更新检查按钮实现
```

---

## 8. 发布检查清单

### 构建前
- [ ] 更新 `package.json` 和 `Cargo.toml` 版本号
- [ ] 更新 CHANGELOG.md
- [ ] 确认所有 TODO 注释已解决或标记为已知问题
- [ ] 运行 `pnpm build` 验证前端构建无错误
- [ ] 运行 `cargo test` 验证后端测试通过

### 签名与安全
- [ ] macOS: Apple Developer 证书配置就绪
- [ ] macOS: 公证流程可用
- [ ] Windows: 代码签名证书配置就绪
- [ ] Tauri updater 签名密钥已生成并安全存储

### CI/CD
- [ ] GitHub Secrets 配置完成
  - `TAURI_SIGNING_PRIVATE_KEY`
  - `TAURI_SIGNING_KEY_PASSWORD`
  - `APPLE_CERTIFICATE` (macOS)
  - `APPLE_CERTIFICATE_PASSWORD` (macOS)
  - `APPLE_SIGNING_IDENTITY` (macOS)
  - `APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID` (macOS)
- [ ] 更新服务器 endpoint 已部署

### 发布后
- [ ] 各平台安装包安装测试
- [ ] 自动更新流程测试
- [ ] 卸载/重装测试

---

## 9. 验收标准

1. 关闭窗口时应用最小化到系统托盘，不退出
2. 双击托盘图标可恢复窗口
3. 托盘右键菜单包含"显示窗口"和"退出"选项
4. 通过托盘"退出"可完全关闭应用
5. 应用退出前优雅关闭 SMCP 连接和 MCP 服务器
6. 不允许多个应用实例同时运行
7. macOS 构建生成 .dmg 安装包
8. Windows 构建生成 .msi 和 .exe (NSIS) 安装包
9. Linux 构建生成 .deb 和 .AppImage
10. 自动更新检查能检测新版本并提示安装
11. CI/CD 流程在打 tag 后自动构建所有平台产物
