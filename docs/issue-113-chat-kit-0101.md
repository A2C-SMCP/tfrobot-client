# Chat Kit 0.10.1 / 新版 Server 接入

追踪：[Client #113](https://github.com/A2C-SMCP/tfrobot-client/issues/113)。
上游：[TFRS-619](https://turingfocus.atlassian.net/browse/TFRS-619)，只阻塞私有资源预览和下载。

## 已确认范围

保留现有 Manager 登录、身份/组织选择、机器人选择、历史入口、新建会话标题窗口、Enter 发送与中文输入法保护。
使用发布版 Chat Kit 0.10.1 的会话业务、恢复、发送状态、失败草稿处理及对话窗口。
只接新版 routed Server：Bearer + JSON 会话创建（`json-v2`）；不自动重试旧聊天协议。
新 Server/Manager 先在 test 验证；beta/staging 推广新版服务后再验收，不依赖旧版聊天 BFF。

平台目录由 Chat Kit 读取。一个平台自动使用；多个平台无有效历史选择时在机器人旁选择。
原生偏好按环境/账号/组织/机器人保存平台，会话位置再按平台隔离。
切换平台销毁旧 ChatClient，由 Kit 清理订阅、上传及草稿。平台错误支持显式重试，无定时轮询。

旧偏好文件升到 schema 3，保留机器人选择及旧会话记录。没有平台归属的旧会话 ID 不映射到任一新平台，首次使用按选定平台的列表恢复；后续位置按平台保存。此约束避免多平台串会话。

## 调用边界

`chatBridge.ts` 只配置 Kit，不复制 Gateway 或会话控制器。统一 SessionProvider 取得短期 chat token。
原生服务从 Manager connection-info 解析 `/c/{robotType}/{rid}/`，验证机器人实例前缀。
HTTP/上传走原生 Bearer 和受限路由；Manager 凭据、config:write/read 凭据不交给 WebView。
Socket 仍由 Kit 在 WebView 持有，使用原有 `CURRENT_SERVER_PROFILE` 的有界 best-effort 恢复。
`GET v1/chat/platforms` 增加到原生只读白名单；任意 URL、跨机器人和写平台请求仍拒绝。
页面隐藏时，沿用现有 PageHost 的弹层作用域及事件详情处理；新版失败确认弹窗纳入回归。

私有资源仍是独立的原生通道，保留 loopback capability、来源检查、流式传输、取消、并发和大小限制。
它的 Front `/api/resource/s3` base 与新的聊天/上传 base 分开，不能随聊天路由一起改成 presign 直连。
当前 test 的 presign 指向集群内 MinIO，资源 BFF 返回 401，因此真实下载尚未验收。客户端不重写签名 URL，也不向 WebView 下发存储密钥。待 TFRS-619 给出受控接口后联调。

## 证据与验证

2026-10-17 的隔离实验及实施复验原始记录位于本机 `/tmp/tfrc-chatkit-0101-experiment/`（不是仓库依赖）。
Chat Kit 标签 v0.10.1 对应 `842564b9b27ca4e5d2aaaaf37c49c57f77f8fef1`，安装锁文件固定各聊天模块 0.10.1；auth 模块按发布依赖独立版本解析。

- 真实 test 机器人15：新建、发一条无工具测试消息、Socket 收到助手回复、fresh client 加载历史均通过。机器人16独立新建空会话，没有混入15的消息。只删除实验创建的会话。
- Socket lifecycle 为 `degraded`；不声称 current-server 能提供完整恢复。
- 单测覆盖 Kit HTTP/Socket 恢复、IME/快捷键、上传/长文本、资源与取消、平台选择/无效偏好/重试、原生按平台存盘与身份隔离。
- 新 opt-in Rust 用例 `live_test_server_chat_and_upload` 访问真实 test，使用临时文件提供短期凭据，创建/清理自己的会话和文件。它运行生产 endpoint/auth/HTTP 代码；不替代 Manager 登录 UI 或签名安装包验收。
- macOS restart fixture 运行生产 Chat/Kit/IPC/SettingsService，但 Manager/租约/服务端数据是本地夹具，不等价于完整线上原生验收。

可复验命令：

```sh
pnpm lint:ts
pnpm exec vitest run --maxWorkers=1 --testTimeout=15000 src/test/components/Chat*.test.*
cargo test --manifest-path src-tauri/Cargo.toml --lib services::chat_session --no-default-features
cargo test --manifest-path src-tauri/Cargo.toml --lib services::settings --no-default-features
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --no-default-features -- -D warnings
```

真实设施测试仅在显式授权的 test 执行；`TFRC_CHAT_ACCEPTANCE_INPUT` 指向 chmod 600 临时 JSON：environment=test、connection（真实 connection-info）、chatToken（chat:read chat:send）、uploadToken（config:write）。不要把该文件加入仓库或输出日志。

```sh
TFRC_CHAT_ACCEPTANCE_INPUT=/path/to/temporary-credentials.json cargo test --manifest-path src-tauri/Cargo.toml --lib live_test_server_chat_and_upload --no-default-features -- --ignored
```

完整 integration/E2E 和 beta/staging 服务推广后的验收由用户触发；此次不部署服务、不迁移登录系统、不自动提交推送。开发验证不等价于 UAT 通过。

### 本次实施结果

- 前端聊天回归 82/82 通过；并发编译时两项 5 秒超时，串行 15 秒测试预算复跑全部通过。
- 最新 Rust 测试二进制：聊天/上传/资源 33 通过、3 个 opt-in 用例默认忽略；设置/偏好 33 通过。
- 当次显式真实 test 原生用例 1/1 通过；Kit 真实 Socket 收发和 fresh client 历史通过；重新读取列表确认实验会话已删除。
- macOS 原生进程重启验收通过：Robot 43 / Conversation 99 经过 IPC 写盘，退出后新进程直接恢复 99，未先展示 42。记录：`/var/folders/v7/6wb5d0ks3rx3v0wg682l2m6c0000gn/T/chat-restoration-me3gEF/result.json`。
- TypeScript、相关 ESLint、原生 fixture TypeScript、生产构建和 Clippy all-targets（`-D warnings`）通过。Vite 仍提示大 chunk，未因此扩展本次范围。

发布验收仍需检查目标环境已升级 Server/Manager；私有资源下载待 TFRS-619；本地原生 fixture 不替代完整远端登录/安装包 UAT。

隔离初审发现平台保存失败会卸载当前聊天，已修复并加入真实 Kit 平台生命周期回归：写盘失败保留当前草稿和 Socket，重试成功才切换客户端，A→B→A 不串草稿且旧连接关闭。

全量浏览器 E2E 可按需执行 `pnpm test:e2e`；本次只运行了本功能的真实设施测试与原生 restart 验收，不宣称全量 E2E/UAT 完成。

隔离复审结论：APPROVE，0 个阻塞项。非阻塞后续建议：加强切平台时 pending 上传/迟到响应的组合测试；加强 opt-in 真实上传实验在异常响应解析失败时的清理。现有取消/原生边界回归与本次正常真实链路已通过。临时 Manager/session 凭据文件已删除。
