# TFRobot Client 开发阶段

本目录包含项目的分阶段实施计划。每个 Phase 是一个可独立完成的工作单元。

## 阶段总览

| Phase | 名称 | 状态 | 说明 |
|-------|------|------|------|
| [Phase 1](./PHASE-1-MCP-SERVER.md) | MCP Server 管理 | 🟢 已完成 | 核心功能，集成 smcp-computer |
| [Phase 2](./PHASE-2-SMCP-CONNECTION.md) | SMCP Server 连接 | 🔴 未开始 | 远程连接和钥匙串 |
| [Phase 3](./PHASE-3-RESOURCES.md) | Desktop 资源浏览器 | 🔴 未开始 | 资源查看功能 |
| [Phase 4](./PHASE-4-LOGGING.md) | 日志系统 | 🔴 未开始 | 双层日志架构 |
| [Phase 5](./PHASE-5-SYSTEM-INTEGRATION.md) | 系统集成 | 🔴 未开始 | 托盘、更新、设置 |
| [Phase 6](./PHASE-6-RELEASE.md) | 发布准备 | 🔴 未开始 | 错误上报、打包、CI/CD |

**状态说明**: 🔴 未开始 | 🟡 进行中 | 🟢 已完成

## 依赖关系

```
Phase 1 (MCP Server)
    │
    └──► Phase 2 (SMCP 连接)
            │
            ├──► Phase 3 (资源浏览器)
            │
            └──► Phase 4 (日志系统)
                    │
                    └──► Phase 5 (系统集成)
                            │
                            └──► Phase 6 (发布)
```

## 已完成的基础工作

- ✅ Tauri 2.x + React + TypeScript 项目脚手架
- ✅ Ant Design 5.x UI 框架集成
- ✅ i18next 国际化配置（中/英）
- ✅ 基础布局和导航菜单
- ✅ Rust 后端命令骨架
- ✅ 类型定义（MCP 配置、日志等）

## 如何开始

1. 确认 `smcp-computer` 已发布到 crates.io
2. 从 [Phase 1](./PHASE-1-MCP-SERVER.md) 开始
3. 完成每个 Phase 后更新此文档的状态

## 开发建议

- 每个 Phase 完成后建议创建一个 Git tag
- 复杂任务可以拆分为多个 PR
- 保持前后端同步开发，避免接口不匹配
