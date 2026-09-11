# Chat Kit 0.8.0 接入与适配

最新状态（2026-09-09）：已实施资源字节代理、本机流式交付及原生打开/下载；最终验证记录见文末。真实 Beta/Tauri 验收仍未完成，#76 保持 open/in-progress。

日期：2026-09-08。当前代码由 `@turingfocus/chat-kit@0.7.0` 升级到 0.8.0。
npm 六个生产包均已确认发布；上游 tag `v0.8.0` 指向 `5abc4f8d49cd5422c4fb8cb3c7993c6507087565`。

## 本轮实施状态（2026-09-08）

追踪：[#76](https://github.com/A2C-SMCP/tfrobot-client/issues/76)。本次传输与附件授权适配已实现；本地验证与最终隔离复审已完成（APPROVE，无必须修改项），真实 Beta 联调尚未执行。先前“已确认 Server 阻塞”的结论已撤回，原因见下节。

- 主包与五个 overrides 已统一为 0.8.0，lockfile 已更新。
- 默认 Chat Kit uploader 继续负责 FormData、上传返回值和附件消息映射。新增 `chatTransport.ts` 在平台边界将文件转为有界 base64 IPC，Rust 重建 multipart，使用操作级附件凭据，沿用原有 cookie 传输方式和 Robot 路由。
- 新增 `chat_prepare_transfer` / `chat_cancel_transfer` / `chat_upload_request` / `chat_resolve_resource`。服务端生成 transfer ID，先注册再执行，消除取消先到的竞态；取消、超时、lease 关闭和任务退出均清理传输。没有轮询。
- 单文件 10 MiB；文件名最多 255 UTF-8 字节；每 lease 同时最多 4 个传输预留/执行项；预留及操作总寿命 30 秒，HTTP 请求沿用 15 秒超时。超限明确失败，可重试；JSON 消息原有 1 MiB 上限保持。
- `ChatResourceProvider` 绑定 lease，`s3://` 经当前 Robot Front BFF 的 `v1/utils/cos/presign` 解析；HTTP/HTTPS/blob 继续直接展示。打开/下载沿用 Kit 浏览器默认行为，原生保存和 CORS 受限存储下载不在本轮已验证能力内。
- 五个新增公开 label 已补齐中英文。上游硬编码英文控件未打补丁修改。
- 未新增文档来源、会话管理入口、持久化草稿或 SDK 变更。

### 结论纠正：客户端授权差异，不是已证实的 Server 缺陷

用户反馈 Beta Robot 已能成功上传。此前只检查 Server 路由所需 scope 与 client 的 CHAT_SCOPE，就直接将任务标成“Server 阻塞”，证据不足；没有使用 client 的实际 Beta 凭据复现上传失败。

本轮进一步核对本地真实调用链：

- client `src-tauri/src/services/chat_session.rs` 显式请求 `chat:read chat:send`。
- TFRobotFrontPortal `src/app/api/_lib/console-sso.ts:128` 一键进入 Robot 时请求 session token，但不传 scope。
- TFRSManager `internal/user/service/oauth_token_service/oauth_token_service.go` 在请求不含 scope 时使用当前主体的可授权上限，再与 active 权限求交；显式请求则按该集合缩小。
- TFRobotFront `src/api/base/ApiClient.ts` 将门户 SSO 凭据作为 Bearer 转发；密码登录则使用不同的 AdminToken 路径。
- 已检查的 Server develop@39d62e6be2d94aeee5809aaa58e640c849b30ac7 规定上传需要 config:write、预签名需要 config:read。要求相应授权本身不构成 Server 缺陷。

这些源码差异能解释“网页成功上传”和“client 请求的 scope 不覆盖上传”并存，用户已确认入口是 Front ChatPlayer；尚未核对此次 Beta 请求的实际授予 scope 或部署版本，不能认定为真实请求的最终根因。

客户端现已按既有权限契约实现：每次上传单独请求 `config:write`，每次私有附件预签名单独请求 `config:read`，由 Manager 决定最终授权。操作级凭据不覆盖聊天 lease 缓存，也不交给 Socket；凭据交换仍复用现有 TypeScript tfrs-auth bridge，并非完全绕过 JS。增加 token bridge 的取消清理：上传取消或超时丢弃授权等待时，立即删除待处理请求，使迟到的传输调用失效。此前的聊天权限保持 `chat:read chat:send`。

无需以 Server 改造为当前前置条件；只有实际验证发现既有授权接口不能满足合理客户端场景，再提出上游需求。

### 验证记录

- 前端构建通过。
- 新增真实发布包测试：默认上传器、图片消息、大小上限、403、不影响纯文本、取消竞态、私有资源及安全 URL、公开文案、真实输入框上传和长粘贴，共 10 项通过（含邮件/相对 Markdown 链接的真实点击回归）。
- Rust services 回归：366 通过、3 项既有环境测试忽略；其中 Chat 测试 17 通过、1 个既有真实 Staging 测试忽略。新增测试通过真实本地 HTTP 完成 Manager 登录，经现有 token bridge 调用真实本地 Manager HTTP 端点 fixture，分别取得操作级附件凭据执行 multipart 上传/预签名，并断言 scope 及聊天凭据未被覆盖，并覆盖权限失败、路由限制、原始二进制、上传前/中取消和 lease 关闭。
- 本地 HTTP 服务提供受控响应，不验证已部署 Server 的 scope 或 COS 存储；真实 Tauri WebView、Manager/Server/COS 联调未执行。
- 最新完整前端回归：52 个文件、529 项测试中 528 通过；既有 McpServerForm OAuth 发现用例超过默认 5000ms，单独原配置重跑通过（2.17 秒），没有修改超时配置。此前 TypeScript 通过；ESLint 0 error、7 个既有 warning；最新 Rust Clippy（lib/tests、no-default-features、-D warnings）通过。
- 首次隔离审查发现并已修复 client 资源 port 拦截 mailto/相对链接的问题；修复后 10 项专项测试、TypeScript 和受影响 ESLint 均通过，第二次完整隔离审查结论为 APPROVE，客户端阻塞代码问题为零；随后用户反馈 Beta 已可上传，原 Server 归因已撤回；操作级授权和 token bridge 取消清理实现后，第三次重新审查完整 diff，结论仍为 APPROVE、无必须修改项。审查通过不代表真实 Beta 附件链路已验收。
- 本次权限修复后重新运行 Rust services、Clippy 及前端全量测试；结果如上，不将首次全量的超时波动隐去。


## 非阻塞审查建议

- 文件在申请 Rust 四槽之前已完成前端读取和 base64 编码；批量大文件会有额外内存峰值。后续可在读取前增加可取消的有界调度。
- 多个私有资源超出四槽会显示 Kit 的 Retry resource，需要手动重试；后续可增加排队与并发场景测试。
- 尚未专门覆盖过期预留、HTTP 超时和真实 Manager 身份切换中的传输测试；已覆盖上传前/中取消和 lease 关闭。

## 责任边界与剩余能力

Chat Kit 持有上传协议（路径、multipart 字段、响应校验）、UI 状态、附件消息映射、长文本和会话草稿、工具详情渲染。client 持有 Tauri 文件传输、取消、Manager 身份、短期凭据、Robot 路由以及资源解析。client 不建立第二套附件消息状态，不修改 A2C-SMCP rust-sdk。

当前 `current-server` profile 和有界 REST rebase 保持；不能将 degraded 展示为无损恢复。纯文本、任务中断和重连契约测试继续通过。

以下不应误报为已实现：

- Chat Kit 事件导航、References、资源与代码控件中的部分英文仍为上游硬编码；只补客户端 translation JSON 不能解决完整中文界面。
- 默认资源打开/下载依赖 WebView/浏览器行为和存储 CORS；本次没有新增原生保存 API。
- 文档引用需要宿主提供已授权纯文本来源，本次未引入新的文档服务或读取本地文档。
- compact 会话管理入口未新增；现有 BFF 没有 PATCH/DELETE 会话路由。重命名/删除是另一个旧缺口。

## 真实环境验收门槛

1. 对比 Beta 网页和 client 的实际授权与部署版本；已获相应权限的身份能上传并访问附件，无权限身份明确失败；聊天 Socket 凭据仍保持聊天权限。资源访问隔离需由真实 Server 验证。
2. 在真实 Tauri 中登录 Manager、选择 Robot、创建会话、发文本、选文件/粘贴图片、上传并发送、重开历史查看图片。
3. 检查上传失败/超限/重试、切换会话或 Robot、账号退出、旧请求晚到、interrupt 与断线重连。
4. 在目标 WebView 核验媒体解码、打开/下载和存储 CORS；没有通过的场景不扩大灰度。

上传路径联调前不能关闭 #76。回滚时原子恢复六包及 lockfile，并验证带附件历史的降级展示。

## 上游依赖结论

目前没有证据确认 Chat Kit 或 Server 存在阻塞本次接入的缺陷。先前基于“附件必须仅使用聊天 scope”的 Server 需求草案已撤回，不作为本次接入前置条件，也没有创建上游阻塞工单。若未来产品要求只有聊天权限的身份也能上传，需要另行评审服务端权限契约；这不是本轮已确认需求。

## 上游证据

- [0.8.0 npm 元数据](https://registry.npmjs.org/@turingfocus/chat-kit/0.8.0)
- [固定 tag 的 changelog](https://github.com/A2C-SMCP/tf-chat-kit/blob/5abc4f8d49cd5422c4fb8cb3c7993c6507087565/packages/chat-kit/CHANGELOG.md)
- [默认附件 uploader](https://github.com/A2C-SMCP/tf-chat-kit/blob/5abc4f8d49cd5422c4fb8cb3c7993c6507087565/packages/chat-gateway-tfrobot/src/attachment-uploader.ts)
- [资源和文档接入契约](https://github.com/A2C-SMCP/tf-chat-kit/blob/5abc4f8d49cd5422c4fb8cb3c7993c6507087565/docs/third-party-parity.md)
- [能力、验证及本地化限制](https://github.com/A2C-SMCP/tf-chat-kit/blob/5abc4f8d49cd5422c4fb8cb3c7993c6507087565/docs/baselines/parity-58/capabilities.md)

## 2026-09-09：历史图片在 Front 可见、client 不可见

用户截图提供真实验收失败证据：同一图片在 Front ChatPlayer 可见，在 client 显示 Unavailable / Retry resource，点击下载出现 Resource action failed。不能再把本地上传契约测试通过当作资源展示验收通过。

### 已确认的实现差异

1. Front `src/utils/image-loader.ts` 将 S3 图片转为 `/api/resource/s3`；该 Route Handler 在服务器端调用预签名服务，再 `fetch(presignUrl)`，流式返回图片字节并按文件名设置 MIME。浏览器访问的是 Front，不需要直接访问 COS 地址。
2. client `src-tauri/src/services/chat_session/transfers.rs:210` 只调用预签名端点，期限 600 秒。`src/components/Chat/chatResources.ts:35` 将返回 URL 直接交给 Kit，未提供二进制资源传输，也未声明有效期。
3. Kit `resource-content.tsx` 使用原生 img/audio/video 加载 URL；默认下载另外执行 `fetch(url, credentials: omit)` 再生成 Blob。预签名成功不等于 WebView 能访问该 URL；下载还需要跨域响应许可。普通 img 展示通常不需要 CORS，不能把图片失败仅归因于 CORS。
4. Server `cos_manage_service.py` 默认按存储配置的 scheme/host/port 签名，支持 public_endpoint 覆盖；当前 `/utils/cos/presign` 没传该覆盖值。因此接口不保证签出的地址是桌面端可达的公网地址，但尚未取得此次 Beta 实际返回地址，不能断言当前一定是内网地址。
5. Front 资源代理当前源码要求 adminToken；client 使用 tfUserToken。不能直接把 URL 改为 Front 代理地址就宣称修好，需核实部署版本和两种凭据的代理支持。

### 当前可下的结论与不能下的结论

已确认 client 的资源接入依赖“预签名地址可被 WebView 直接读取”，该前置条件没有验证，与 Front 的服务器代取链路不等价。这是适配与验收缺口；不是图片上传失败的证据，也不能认定 Chat Kit 缺少图片渲染功能。

截图不足以区分 config:read 交换/预签名请求失败与图片 URL 读取失败。仍需一次 Retry 对应的 IPC 错误或预签名状态、存储地址域名及图片请求错误，才能把此次真实故障归因到具体阶段。只取脱敏状态、域名和错误类别，不收集 token/Cookie/签名。

### 其他附件影响

| 场景 | 确认的实现或风险 | 验证要求 |
| --- | --- | --- |
| 图片 | 同一 S3 解析链路；地址可达性、HTTP/TLS、签名有效性影响展示 | 在真实 WebView 完成图片加载，检查实际 img 请求；不可仅断言 resolve 返回 URL |
| 音频/视频 | 同样直连存储；另依赖 WebView 编解码、正确 MIME、Range；600 秒链接可能影响延迟播放/后续分段请求 | 播放、拖动、暂停后继续，以及链接到期后的重新请求 |
| PDF | Kit 发送支持 PDF，通用资源 UI 提供 Open/Download；不等于已有内嵌 PDF 阅读器 | 分别验证打开和下载、实际格式支持 |
| Word/Excel/ZIP/普通文件 | 通用文件卡片，打开和下载仍走资源解析；上传成功不保证文件能被模型读取 | 下载字节一致性与文件名；模型读取单独验收 |
| 下载 | 默认浏览器 fetch 依赖存储 CORS，且读取完整 Blob；原生保存未接入 | 跨域下载及 Tauri 保存行为，不能用图片显示成功代替 |
| 多附件历史 | 上传/预签名共用每 lease 四槽，超额返回错误，无队列 | 五个以上资源同屏，取消/切换后重试 |
| 长时间历史展示 | client 只返回 URL，未传 expiresAt；Kit 当前 hook 不做定时有效期刷新 | 600 秒后重试/重新打开，按需重新签名 |
| MIME 为空/不准确 | Kit 按 MIME 区分 image/audio/video/pdf，空值回退 octet-stream | 同扩展名但 MIME 为空时可能变普通文件，需真实文件选择器测试 |

修复方向应由实际失败阶段决定：授权失败修正现有授权接入；预签名成功但桌面不可达，则提供当前身份可用的资源代理/字节传输；可达但下载失败，则补齐客户端下载 port。不得在 client 直接替换签名 URL 域名，亦不能向不明存储地址转发 Robot 凭据。二进制读取需匹配媒体流式/Range与有界内存需求，不宜将所有附件一律整文件 base64 化。

## 2026-09-09 实验结论更新

用户确认实验后，在 Beta 真实运行中确认：同一 PNG 的存储域名为集群内部 minio.tfrs-personal-1.svc.cluster.local，桌面 DNS 失败，Front 容器 DNS 正常且存储健康接口 200，历史图片读取 200。client 将该内部签名地址交给 WebView 是确定性资源接入缺陷。

部署 Front 0.2.12 的资源代理已支持 tfUserToken；此前本地旧源码“仅 adminToken”判断不适用于 Beta。系统方案复用该入口，补齐授权资源读取、Range/缓存/取消、Rust本地资源交付、原生下载、并发调度和错误观测。详细证据、限制及验收矩阵见 experiments/codex-chat-resource-access/REPORT.md。业务代码未在实验中修改，根因确认不等于修复完成。


## 2026-09-09：附件资源修复实现与验证

范围来源：[#76 指定验收评论](https://github.com/A2C-SMCP/tfrobot-client/issues/76#issuecomment-5596062091)。用户已确认实现方案。复用原 Issue，不新建 Front/Server 阻塞工单；未提交、推送或关闭 Issue。

### 实现

- Rust `chat_session/resources.rs` 负责资源注册、授权和独立资源队列。资源请求经当前 Robot `http_base_url` 下的 `resource/s3`，由 Front 代取内部存储。每次读取重新交换 `config:read`，保留现有聊天 Socket 凭据。签名 URL 不进入 WebView。
- `resources/local_http.rs` 仅监听随机端口 `127.0.0.1`。随机 UUID 句柄绑定 lease 与 S3 URI；校验 Host、Origin、方法和单 Range；不接受任意代理目标，不透传 Cookie、重定向或上游缓存头。响应为 `no-store, private`，限制可内嵌 MIME，禁止脚本执行。操作取消和 lease 撤销在连接层生效，即使播放器暂停读取并触发 TCP 背压也释放连接与资源槽位。
- 单进程资源注册上限 512，读取并发 4、读取中加等待总量 68；上传使用独立槽位。资源读有连接 10 秒、网络读 30 秒超时，本地连接最大 30 分钟；最大响应 1 GiB，未知长度也逐块限额。没有定时轮询。
- `resources/files.rs` 通过临时文件流式写盘，完成后原子替换原生保存对话框选定的目标；取消或失败不损坏原目标。系统打开使用允许的文件扩展名与私有临时文件，每 lease 最多保留 32 个，关闭 lease 时删除。已经交给外部应用的内容无法从该应用内存撤回。
- `chatResources.ts` 对 S3 资源提供 `resolve/open/download/dispose`，迟到注册结果也撤销句柄。公开链接保持直接访问。资源读取和原生操作错误用脱敏枚举与中英文 Alert 说明，不依赖修改 Kit DOM。
- `chatTransport.ts` 在读取文件与 base64 编码前限制并发：同 lease 最多 2 个编码/上传任务，最多排队 64 个。等待可取消，多 uploader 实例共享队列。

### 验证记录

- Rust 全部库测试：529 passed / 3 ignored（完成首轮实现时）；Clippy all-targets `-D warnings` 通过。
- 前端完整回归：539 项中 537 通过，2 个未修改的 McpServerForm Command 表单用例在 10 秒阈值超时；原阈值、单 worker 单独复验两项均通过。未修改它们或提高超时。
- 前端 Chat 专项与新资源生命周期测试通过；包含真实发布包的上传/发送/长粘贴、原生动作 IPC、迟到注册、撤销和编码前限流。构建成功，ESLint 0 error / 7 个既有 warning。
- 本地真实 HTTP 运行覆盖 Manager token HTTP bridge → Front fixture → Rust loopback → 客户端读取，断言原字节、凭据与路由、12 个资源、206/416/Range 被忽略、取消/lease 关闭、原生文件服务保存和打开、错误脱敏及 128 MiB 下载 SHA-256 一致性。
- 最新修复后 Rust Chat 专项：29 passed / 2 ignored；其中浏览器用例已另行显式执行通过，另一项为既有 Staging 凭据用例。最新 Clippy all-targets `-D warnings` 再次通过。
- 128 MiB 下载哈希专项单独运行 `/usr/bin/time -l`：maximum resident set size 为 35,389,440 字节（33.75 MiB）。这是本地测试进程的内存测量，不代表真实 Tauri 总内存。
- 首次隔离审查指出消费者背压时取消可能延迟：新增真实 HTTP 回归后复现失败，再修复连接层取消并验证通过。另补齐流式累计超过大小上限时的 UI 错误事件。
- 浏览器专项真实运行通过：Chromium 12 张真实 PNG 解码成功，12 次下载字节一致，测试端通过实际本机 Vite 导航建立正确的网络地址空间。初版使用 Playwright 合成主文档触发 Chromium loopback 权限拦截，改为真实导航后通过；没有关闭浏览器安全机制。

可重复执行的专项：

```bash
pnpm exec vitest run src/test/components/ChatResources.test.ts src/test/components/ChatAttachments080.test.ts src/test/components/Chat.test.tsx --maxWorkers=2
cargo test --manifest-path src-tauri/Cargo.toml --lib services::chat_session
# 浏览器专项需要 pnpm dev 已运行在 localhost:1420，并已安装 Playwright Chromium
cargo test --manifest-path src-tauri/Cargo.toml --lib browser_decodes_and_downloads_twelve_real_private_images -- --ignored --nocapture
```

### 尚未验收的真实环境项目

| 场景 | 当前证据与边界 |
| --- | --- |
| Beta 原 PNG、JPEG 显示 | 本地 Chromium PNG 解码已验证；Beta/macOS Tauri 未在本轮完成，JPEG 尚待实际解码 |
| 原生 Download、PDF/Office/ZIP Open | Rust 保存与临时文件服务真实写盘通过；Tauri 保存对话框和系统默认应用尚待实际操作 |
| 音视频播放和拖动 | 本地传递真实 Range 的 206/416；忽略 Range 时保留 200 并明确标记不支持，不伪造随机访问；部署 Front 0.2.12 的 Range 能力仍按已有实验记录为未透传，实际播放/拖动待验收 |
| 十分钟后再次读取 | 每次请求重新授权并访问代理，不缓存 600 秒签名；十分钟真实环境间隔未执行 |
| 切 Robot、登出、取消、旧响应、身份隔离 | 本地句柄撤销/上下文关闭、排队及背压取消已覆盖；真实 Beta 账号与跨 Robot 对象权限需最终由 Server 验证 |
| 无权限、不存在 | fixture 的 403/404 清晰分类；Front 若将上游错误折叠为 500，client 只能报告服务不可用，不能凭空恢复原始原因 |
| 大文件内存与取消 | 128 MiB 本地流式保存/哈希、断流、临时文件清理与背压释放已验证；真实 Tauri 大文件内存仍需测量 |

本地验证不等于 Beta/Tauri 验收通过。用户尚未回复当前桌面 Beta 登录态；不采集令牌、Cookie、签名或文件正文作为诊断日志。当前不扩大 Front/Server 修改范围，也不宣称 Office/PDF 已内嵌预览或文件已被模型读取。


### 本轮最终复核

- 修复后第二次 `fork_turns="none"` 隔离完整代码审查：APPROVE，无必须修改项；历史背压取消与累计超限诊断两项均已确认修复。
- 最新 Chat 前端专项：3 文件 / 33 项通过；最新 Chromium 图片字节专项再次通过；最新 Rust Chat 29 项通过，Clippy all-targets 通过。
- 非阻塞建议：浏览器用例可额外与 fixture 源 PNG 固定 SHA-256 对照；目前校验十二次读取彼此相同、PNG 签名与解码尺寸，Rust 字节服务已有固定原字节和大文件 SHA-256 校验。
- 代码审查批准不代表真实 Beta/Tauri 验收完成。未 commit/push/创建 PR；#76 保持 open/in-progress。
