# 环境预检清单

UAT skill 在进入任何场景前必须跑的探针。所有命令均为**只读**，不改任何状态。

Claude 跑完一整轮后，把结果汇总成"✅/❌"表格发给用户；任一 ❌ 都要**停**，
打印修复指引，等用户搞定回"好了"才继续。

## 1. 端口探测

```bash
lsof -nP -iTCP -sTCP:LISTEN 2>/dev/null | grep -E ':(1420|8080|8443|8090) '
```

| 期望行数       | 期望进程                            | 缺失时指引                                                   |
|---------------|-------------------------------------|-------------------------------------------------------------|
| `*:1420`       | node (Vite dev server)              | 运行 `pnpm tauri dev` 启动客户端                              |
| `127.0.0.1:8080` 或 `*:8080` | python3.x (TFRobotServer uvicorn) | `cd $HOME/PycharmProjects/TFRobotServer && supervisord -c supervisord.conf`（见 TFRobotServer `docs/ops/local-tls-setup.md`） |
| `*:8443`       | caddy                               | 同上（确认 `supervisord.d/tls_proxy.conf` 已从 `.example` 拷贝） |
| `*:8090`       | tfrsmanager user-service (Go)       | `cd $HOME/GolandProjects/tfrsmanager && make local-run-init-debug` |

只有 1420 可缺——如果用户还没起客户端，提醒他先起再 UAT。

## 2. Manager 契约自检

用户登录接口对齐本项目 DTO 的最小充分验证：

```bash
curl -sS -X POST http://localhost:8090/auth/login-by-password \
  -H 'Content-Type: application/json' \
  -d '{"phone":"13800138008","password":"Test@123456"}' | jq
```

期望（字面匹配）：
```json
{
  "code": 200,
  "message": "success",
  "data": {
    "token": "<非空 JWT>",
    "userId": 9,
    "accountId": 16,
    "accountName": "client_uat"
  }
}
```

**异常判决**：
- `.data.token` 不存在 → Manager 端变了结构，**立即停**，先走 `a2c-smcp-toolkit:cross-ask-tf`
- 返回 `{"detail": "..."}` (FastAPI 错误) → :8090 上跑的根本不是 TFRSManager（可能是 TFRobotServer），端口冲突或用户起错了服务
- 空响应 / ECONNREFUSED → user-service 没起
- 返回 401 + `"Invalid credentials"` → seed 数据异常，跑 `make seed-local` 重新初始化

## 3. TFRobotServer TLS 自检

```bash
curl -sS "https://127.0.0.1:8443/socket.io/?EIO=4&transport=polling" | head -c 80
```

期望：`0{"sid":"..."...}` 开头的 Engine.IO handshake 响应。

**异常判决**：
- `SSL certificate problem` → mkcert 的 CA 没装到系统 trust store；让用户跑 `mkcert -install`
- `Connection refused` → Caddy 没起来，查 supervisord 状态
- `400 Invalid websocket upgrade` → 正常，curl 不能测 WS，只能测 polling（见 UAT guide §6.4）

## 4. Seed 账号快速通测

逐个过一遍 `resources/seed-data.md` 里列的账号，用 login 接口验证。各账号预期 accountName：

| 手机号        | 预期 accountName         | 用途       |
|--------------|--------------------------|-----------|
| 13800138000  | testuser 相关             | 场景 A/C   |
| 13800138008  | `client_uat`              | 场景 A/C/E（唯一真连通实例）|
| 13800138009  | clientUATEmpty 相关       | 场景 D（空列表）|
| 13900139000  | **非单账户**（返回 tempToken + accounts）| 场景 B（多账户） |

对于 13900139000，期望响应 `.data` 含 `tempToken`，**不**含 `token`。

## 5. 客户端 Rust 端 keychain 状态（可选诊断）

如果 UAT 过程中遇到 "401 但我刚登录过"，可能是 keychain 残留旧 JWT：

```bash
security find-generic-password -s tfrobot-client 2>&1 | head -20
```

找到含 `manager_jwt:` 前缀的条目；正常情况下一个 base_url 对应一条。异常时用户可以：

```bash
security delete-generic-password -s tfrobot-client -a "manager_jwt:<hash>"
```

或者更简单，让用户在客户端点"退出登录"按钮。

## 汇报模板

前置检查做完后，用这个格式告知用户：

```
✅ 端口:    1420 ✓  8080 ✓  8443 ✓  8090 ✓
✅ Manager 契约:  login.data.token 存在，accountName=client_uat
✅ TFRobotServer:  polling handshake 正常
✅ Seed 账号:  4/4 通过
🟢 前置就绪，可以进 UAT 场景。
```

或失败时：

```
✅ 端口:    1420 ✓  8080 ✓  8443 ✓  8090 ❌
❌ Manager user-service 没监听 :8090

修复：
  cd $HOME/GolandProjects/tfrsmanager
  make local-run-init-debug

起来后回"好了"继续。
```
