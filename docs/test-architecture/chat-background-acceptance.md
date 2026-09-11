# TFRC-134：macOS 后台聊天验收

## 修复与边界

主窗口通过 Tauri 配置显式设置 `backgroundThrottling: "disabled"`。
聊天 Socket 由 WebView 中的 Chat Kit/Gateway 持有；阻止 WebKit 因窗口隐藏或
最小化挂起页面，让既有心跳、重连及历史恢复继续执行。

Tauri 2.10.3 / Wry 0.54.3 将此值映射为 WebKit `inactiveSchedulingPolicy = None`。
该 API 在 macOS 14+ 可用；本项目最低系统版本仍为 10.15，本修复不保证旧系统
后台稳定性。它也不保证系统睡眠期间运行或后台毫秒级消息延迟。策略作用于整个
主窗口，可能增加后台资源消耗；此前隔离实验未发现明显 CPU/RSS 增量，但不构成
长期能耗结论。

来源：
- [TFRC-134](https://turingfocus.atlassian.net/browse/TFRC-134)
- [Tauri 官方配置说明](https://v2.tauri.app/reference/config/#backgroundthrottling)
- 本机既有实验 `experiments/codex-chat-background-heartbeat/REPORT.md`

## 可复现验收

入口与命令见 [e2e/chat-background/README.md](../../e2e/chat-background/README.md)。
使用真实 Tauri/WKWebView、生产 Chat 页面、生产 factory 与前端 Tauri HTTP 桥接、
Chat Kit 0.8.0。Manager 身份、会话租约及原生 HTTP relay 是本地夹具；不访问
生产环境，不替代生产 Rust 会话服务的鉴权/路由/续期联调。

四轮空闲后台观察每轮八分钟，分别隐藏、最小化、隐藏、最小化。观察期间无测试
IPC、轮询或合成消息流。前三轮结束后检查后台实时消息，再请求显示窗口并检查文本仍在；最后一轮在故障恢复后检查实时消息。
最后实际关闭 WebSocket，检查后台重连、未广播消息经 REST 补回、后续实时消息和去重。
历史补拉沿用 current-server 的有界 best-effort 策略，不声称恢复任意长度的全部历史。

## 本次执行（2026-09-10）

- 前端聊天回归：18/18 通过（Chat、ChatKitContract、ChatBridge070）。
- 生产 TypeScript 检查、前端构建通过。
- 验收入口独立 TypeScript 检查通过；runner Clippy `-D warnings` 通过。
- 源码 ESLint：0 error，6 个已有 warning。命令排除 `**/target/**` 编译产物；
  原始 `pnpm lint:eslint` 会把实验 target 下 Tauri 压缩资产误当 JavaScript 源码解析。
- 短测通过：后台重连 1,312 ms，漏收消息已通过历史补拉进入真实 Chat UI。
  短测输出：`e2e/chat-background/output-1789031675542/`，不计入长时间验收。
- 正式四轮验收通过，后台实际断线后 7,074 ms 重连，见下表。
- 生产 Rust `cargo check --offline --locked` 通过。
- 隔离初审：发现主动 eval 会混淆后台渲染证据，已改为 MutationObserver 首次渲染后一次性上报；
  只读复核确认修复。正式长测后已完成独立新代理的全范围复审：APPROVE，0 个阻塞项；
  可见态确认与重复投递覆盖两项非阻塞建议已在本文披露。

夹具建立时曾出现依赖解析和启动失败：最初依赖解析选中了较新的不兼容 Tauri runtime，
已改为从生产锁文件派生固定依赖；随后 REST 路径缺少 `v1/chat`，短测在初始会话
加载阶段失败，修正后通过。所有失败运行原始输出保留，未计入成功样本。

第一轮长测初次运行（`e2e/chat-background/output-1789031723651/`）在 8 分钟结束时
因脚本要求至少 18 次 PONG 而失败。窗口未发生心跳断连；17 次 PONG 中最长 RTT
约 35.6 秒。Engine.IO 在收到 PONG 后才再计时 25 秒，因此固定 18 次假设不成立。
脚本改为逐次校验 PING/PONG 小于 60 秒超时、无断连，并要求至少五次 PONG
（480 秒 / 每周期最多 85 秒的保守下界）。该次不列入正式四轮成功结果。

观察器修复后短测（`e2e/chat-background/output-1789032582064/`）通过：后台重连
603 ms，控制器在发出任何 eval 前已收到漏收消息和后续实时消息的隐藏态渲染上报。
最后一轮正式长测将在八分钟后台结束后直接关闭传输，不先恢复窗口。

## 正式验收结果

环境：macOS 14.2 arm64，Tauri 2.10.3 / Wry 0.54.3 / Chat Kit 0.8.0。
原始事件、当次脚本、配置快照与结果：`e2e/chat-background/output-1789032630476/`。

| 场景 | 连续后台时长（ms） | PING / PONG | 最大 RTT（ms） | 心跳断连 |
|---|---:|---:|---:|---:|
| round-1-hide | 480020 | 19 / 19 | 47 | 0 |
| round-2-minimize | 480008 | 19 / 19 | 87 | 0 |
| round-3-hide | 480011 | 20 / 20 | 16 | 0 |
| round-4-minimize | 480053 | 19 / 19 | 66 | 0 |

四轮总后台观察超过 32 分钟。最后一轮结束前没有窗口恢复或快照，随后发送实际
WebSocket Close(1012)。服务端单调时钟测得重连耗时 **7,074 ms**，满足 15 秒阈值。
未广播的 `m5` 消息由重新连接后的 REST 历史请求返回；页面在 hidden 状态自主渲染，
一次性上报先于任何窗口恢复或 eval。其后实时消息同样在 hidden 状态自主渲染。
最终可见态核对漏收消息出现一次，无未处理页面错误。该检查不等价于主动构造
相同 msgId 的 REST/Socket 重复投递测试；重复投递覆盖属于非阻塞后续建议。

可见态边界：原始记录中 `round-3-hide-visible` 快照实际仍为 hidden，原生 show 成功
不保证页面立刻变为 visible（可能仍被遮挡）。因此不声称每轮都完成了前台可见态
验收；最终 `final` 快照为 visible。该限制不影响此前的隐藏态自主渲染、心跳或
第四轮长期后台重连证据。
