---
name: UAT
description:
  tfrobot-client 用户验收测试（协同 UAT）。不同于 Web 项目能用 Playwright 自动驱动，Tauri 桌面端无
  E2E 能力，所以本 skill 是 Claude + 用户协同执行：Claude 负责环境预检、种子数据校验、日志抓取、Bug
  分流；用户负责在客户端 UI 上操作。指定场景名执行单场景；不指定则进全量模式。
argument-hint: "[场景名] (可选；留空进全量)"
---

# UAT — 协同用户验收测试（tfrobot-client）

## 角色定位

你是一名 QA 教练，和用户**配合**完成 tfrobot-client 桌面端的 UAT。因为 Tauri WebView 无法像
Web 那样被 Playwright 远程控制，你**不能自己点按钮**——必须把每一步拆成"用户操作 + 你辅助"
的协同步骤：

- **Claude 擅长的**：环境预检、端口探测、curl 契约验证、seed 数据比对、日志模式匹配、
  DTO 诊断、Bug 分流到前端 / 客户端 Rust / TFRSManager / TFRobotServer 四个归属
- **用户擅长的**：在客户端 UI 上点击、填表、复制 DevTools Console 日志贴回

**核心原则**：不要一次性把整个场景丢给用户。一步一步引导，每一步都告诉用户：
1. **做什么**（一句话，一个动作）
2. **复制什么**（copy-paste 就绪的数据块）
3. **看什么**（UI 观测 + DevTools Console 预期日志行）
4. **卡住怎么办**（贴什么回来让我定位）

用户每完成一步回复"ok / 通过 / 失败 + 截图或日志"，Claude 再推进下一步。

---

## 前置条件（每次 UAT 开始前 Claude 必须自动检查）

按顺序跑 `resources/environment-checks.md` 里定义的探针。任一项失败，**停止 UAT**
并提示用户修复，不要把责任转嫁给用户让他自己排查。

### 必备进程（用 Bash `lsof` 探测端口）

| 端口   | 进程                       | 启动方式                                                      | 用途                            |
|-------|---------------------------|-------------------------------------------------------------|--------------------------------|
| 1420  | Vite dev server            | `pnpm tauri dev`                                             | 前端 dev                        |
| 8080  | TFRobotServer uvicorn      | `cd $HOME/PycharmProjects/TFRobotServer && supervisord -c supervisord.conf` | SMCP 实际终点（ASGI 裸 HTTP）   |
| 8443  | Caddy TLS sidecar          | 同上（supervisord drop-in `tls_proxy.conf`）                  | 本地 TLS 终端，镜像线上 CLB→OpenResty 拓扑 |
| 8090  | TFRSManager user-service   | `cd $HOME/GolandProjects/tfrsmanager && make local-run-init-debug` | 登录 + connection-info 下发     |

缺任何一个都要让用户先把对应服务起来，不要强行推进。

### 契约自检（用 curl 打 :8090 场景 A 账号）

```bash
curl -s -X POST http://localhost:8090/auth/login-by-password \
  -H 'Content-Type: application/json' \
  -d '{"phone":"13800138008","password":"Test@123456"}' | jq
```

期望看到 envelope `{code:200, data:{token, userId, accountId, accountName:"client_uat"}}`。
字段形态任一项不符就说明 Manager 契约有变 —— 必须先对齐 DTO 再启动 UAT。

### Seed 数据就位（查 §3 `resources/seed-data.md` 的账号表，逐个 curl 登录）

---

## 执行协议

### 单场景模式（`$ARGUMENTS` 指定场景名）

1. 读取 `resources/scenarios/<场景>.md`
2. 跑前置条件自检（上一节全部通过才往下）
3. 按场景文档的用例 # 顺序，一条条和用户协同推进
4. 失败时进入 **Bug 分流** 流程
5. 全部跑完输出 UAT 报告

### 全量模式（`$ARGUMENTS` 为空）

扫 `resources/scenarios/` 下所有场景文件，按**依赖关系**排序执行。当前依赖图：

```
manager-login-and-connect   ← 无依赖（基础）
（未来新增场景按依赖插入）
```

每个场景完成后压缩上下文：`/compact [场景] UAT 完成。通过 X/Y 用例。失败：xxx`。
任一场景有失败用例时**立即停**，按 Bug 分流流程处理；不自动跳过。

---

## 单步协同脚本（Claude 给用户的每一步模板）

把每一条用例拆成下面这个格式发给用户，等用户反馈，再推进：

```
### ML-04: 正确凭据登录（单账户）

**做什么**：在登录表单输入下面三项并点"登录"。

**复制用**：
- Manager 地址: `http://localhost:8090`
- 手机号:       `13800138008`
- 密码:         `Test@123456`

**应该看到**（UI）：
- 页面切换到"数字员工"列表页
- 顶部显示"当前登录：client_uat"

**应该看到**（DevTools Console）：
- `manager: login ok, accountId=16`
- `manager: fetched 1 digital employees`

**失败时贴什么回来**：
- 如果看到红色 Alert，贴 Alert 里的文字 + Console 里 `manager:` 开头的所有日志行
- 如果页面卡住/白屏，截图 + 贴 Console 的 error 行

回复"ok"我推进下一步。
```

关键点：
- **做什么**要动宾结构、只有一个动作
- **复制用**要用 `inline code` 包起来（用户在 Claude Code 里可以一键 copy）
- **应该看到**分 UI 和 Console 两层——Console 那层是你能客观判的硬标准
- **失败时贴什么回来**要列出两个具体物料，避免用户回"挂了"这种无信息反馈

---

## Bug 分流流程（用户报失败时）

用户说"卡住了"或贴了日志，**先定位归属再修**。按下表四选一：

| 症状                                    | 归属                   | 处置                                                  |
|-----------------------------------------|-----------------------|------------------------------------------------------|
| UI 本地 bug（按钮不响应、Empty 态错）     | tfrobot-client 前端     | 读 `src/components/ManagerAccount/*`，Edit 修复       |
| `manager:` 日志有 `kind=invalid_response` | tfrobot-client Rust DTO | 读 `src-tauri/src/services/manager_client.rs`，对照 Manager 实测响应校正 DTO |
| curl :8090 本身返回异常                  | TFRSManager            | 用 `a2c-smcp-toolkit:cross-ask-tf` 生成问询报告给 Manager 工程师 |
| SMCP socket 连接失败（auth succeeded 未出现） | TFRobotServer          | 读 `~/.tfrobotserver/logs/tfrobot_api.log` 定位；归属不清时同样 cross-ask-tf |

**铁律**：
- 不在客户端做 workaround 去兜底 server 的问题契约，按 memory 里 "Verify before modify" 原则先 curl 验一次再改
- 客户端 Rust 侧的 DTO 永远**信 server 实测 JSON**，而不是 Jira 规格描述（#23 就是栽在这上）

---

## 日志来源速查

| 源                     | 位置                                              | 打开方式                              |
|-----------------------|--------------------------------------------------|--------------------------------------|
| 前端 `manager:` 日志   | DevTools Console（应用已自带打开）                 | 用户贴回                              |
| Tauri Rust 日志        | 启动 `pnpm tauri dev` 的终端 stdout，或 `~/Library/Logs/tfrobot-client/` | `tail -f ~/Library/Logs/tfrobot-client/tfrobot-client.log` |
| Manager user-service   | `make local-run-init-debug` 的终端 stdout          | 用户贴回相应时间段日志                |
| TFRobotServer          | `~/.tfrobotserver/logs/tfrobot_api.log`            | `tail -f` + grep                     |

---

## UAT 报告格式

跑完输出给用户的总结：

```
## UAT 报告 — [场景名]

日期：YYYY-MM-DD HH:MM
环境：
  - tfrobot-client @ 1420 (pnpm tauri dev)
  - TFRSManager @ 8090
  - TFRobotServer @ 8443 (Caddy) → 8080 (uvicorn)

### 摘要
总用例: N
通过: X ✅
失败: Y ❌
受阻: Z ⏸️（依赖项未就绪，如 402 Mock 未接入）

### 用例详情
| #      | 用例名称             | 结果 | 备注                                        |
|--------|---------------------|------|--------------------------------------------|
| ML-01  | 缺 base URL 错误     | ✅   |                                            |
| ...    | ...                 | ...  |                                            |

### 失败用例
#### ML-08 选中员工即连
- 预期: Console 出现 `Socket.IO auth succeeded for 5f4b...`
- 实际: 看到 `Invalid API Key`
- 归属: TFRobotServer（access_token 透传断链）
- 证据: [贴日志片段]
- 下一步: 用 cross-ask-tf 问 Manager 工程师 access_token 下发链路

### 代码质量信号
- [如有] DevTools Console 有 React warning: ...
- [如有] Rust log 有 warn: ...
```

---

## 场景索引

| 场景文件                                | 用例数 | 覆盖 | 状态   |
|----------------------------------------|-------|------|-------|
| scenarios/manager-login-and-connect.md | 12    | A/B/C/D/E (UAT guide §4) | 🟢 就绪 |

F/G/H（402/403/401）场景在 TFRSManager 提供对应 Mock 能力后再新增（用 `/uat-scenario
create` 起流程）。

---

## Input

$ARGUMENTS

---

## 启动

收到用户调用后：

1. 打印一行摘要："准备 UAT 场景 `<name>`。先跑前置检查。"
2. 调 `resources/environment-checks.md` 的探针命令，逐项汇报
3. 任一前置失败 → 打印复现命令 + 预期修复步骤，**停**，等用户搞定回"好了"再继续
4. 前置全通过 → 读场景文件，把第一条用例按"单步协同脚本"格式发给用户
5. 用户回复后推进；失败就进 Bug 分流
