# Phase 6: 发布准备

## 目标

完成错误上报集成、内置运行时打包，准备正式发布。

## 前置条件

- [ ] Phase 1-5 全部完成
- [ ] 功能测试通过

## 任务清单

### 6.1 错误上报

#### 6.1.1 部署 Sentry/Glitchtip

- [ ] 选择方案：自建 Sentry 或 Glitchtip
- [ ] 部署实例
- [ ] 获取 DSN

#### 6.1.2 后端集成

- [ ] 添加 `sentry` crate 依赖
- [ ] 初始化 Sentry SDK
- [ ] 捕获 panic 和错误

```toml
# Cargo.toml
sentry = "0.34"
```

```rust
// lib.rs
let _guard = sentry::init(("YOUR_DSN", sentry::ClientOptions {
    release: sentry::release_name!(),
    ..Default::default()
}));
```

#### 6.1.3 前端集成

- [ ] 添加 `@sentry/browser` 依赖
- [ ] 初始化并捕获错误

#### 6.1.4 用户授权

- [ ] 首次启动弹窗询问是否允许错误上报
- [ ] 设置页面可以开关
- [ ] 未授权时不初始化 Sentry

### 6.2 内置运行时打包

#### 6.2.1 下载运行时

创建 `scripts/download-runtimes.sh`：

```bash
#!/bin/bash
# 下载 Node.js
# macOS arm64
curl -o node-darwin-arm64.tar.gz https://nodejs.org/dist/v20.11.0/node-v20.11.0-darwin-arm64.tar.gz
# macOS x64
curl -o node-darwin-x64.tar.gz https://nodejs.org/dist/v20.11.0/node-v20.11.0-darwin-x64.tar.gz
# Windows x64
curl -o node-win-x64.zip https://nodejs.org/dist/v20.11.0/node-v20.11.0-win-x64.zip

# 下载 Python (python-build-standalone)
# ...

# 下载 uv
# ...

# 解压到 src-tauri/resources/
```

#### 6.2.2 运行时目录结构

```
src-tauri/resources/
├── node/
│   ├── darwin-arm64/
│   │   └── bin/node
│   ├── darwin-x64/
│   │   └── bin/node
│   └── win-x64/
│       └── node.exe
├── python/
│   ├── darwin-arm64/
│   │   └── bin/python3
│   ├── darwin-x64/
│   │   └── bin/python3
│   └── win-x64/
│       └── python.exe
├── uv/
│   ├── darwin-arm64/uv
│   ├── darwin-x64/uv
│   └── win-x64/uv.exe
└── pnpm/
    └── ... (通过 npm pack 获取)
```

#### 6.2.3 核心预装依赖

确定并预装常用 MCP Server 依赖：

**Python**:
- `mcp` (MCP SDK)
- `httpx`
- `pydantic`

**Node.js**:
- `@anthropic-ai/sdk`
- `zod`

### 6.3 CI/CD 配置

#### 6.3.1 GitHub Actions

创建 `.github/workflows/build.yml`：

- [ ] 多平台构建（macOS, Windows）
- [ ] 代码签名（macOS notarization, Windows signing）
- [ ] 上传到 Release

#### 6.3.2 更新服务器

- [ ] 配置静态文件服务托管更新包
- [ ] 生成 `latest.json` 文件

```json
{
  "version": "0.1.0",
  "notes": "Release notes here",
  "pub_date": "2024-01-01T00:00:00Z",
  "platforms": {
    "darwin-aarch64": {
      "signature": "...",
      "url": "https://..."
    },
    "darwin-x86_64": { ... },
    "windows-x86_64": { ... }
  }
}
```

### 6.4 文档

- [ ] README.md - 项目介绍、安装说明
- [ ] CHANGELOG.md - 版本更新日志
- [ ] LICENSE - 许可证文件

### 6.5 最终测试

#### 6.5.1 功能测试清单

- [ ] MCP Server CRUD
- [ ] MCP Server 启动/停止
- [ ] SMCP 连接/断开
- [ ] 资源浏览
- [ ] 日志查看/导出
- [ ] 设置保存/加载
- [ ] 系统托盘
- [ ] 自动更新
- [ ] 多语言切换

#### 6.5.2 平台测试

- [ ] macOS (arm64)
- [ ] macOS (x64)
- [ ] Windows (x64)

## 验收标准

1. 可以成功构建各平台安装包
2. 安装包包含完整运行时，无需用户额外安装
3. 错误能自动上报到 Sentry（用户授权后）
4. 自动更新功能正常工作

## 预计文件变更

```
.github/
└── workflows/
    └── build.yml                 # 新增

scripts/
└── download-runtimes.sh          # 新增

src-tauri/
├── Cargo.toml                    # 添加 sentry
├── resources/                    # 填充运行时
│   ├── node/
│   ├── python/
│   ├── uv/
│   └── pnpm/
└── src/
    └── lib.rs                    # 添加 sentry 初始化

src/
└── main.tsx                      # 添加 sentry 初始化

README.md                         # 新增/更新
CHANGELOG.md                      # 新增
LICENSE                           # 新增
```

## 版本号规划

- `0.1.0` - 首个公开测试版
- `0.2.0` - 基于反馈的改进版
- `1.0.0` - 正式稳定版
