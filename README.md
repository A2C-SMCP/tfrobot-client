# tfrobot-client
TFRobot Client - A cross-platform desktop application for A2C-SMCP Computer

## Environment Variables

| Name | Required | Purpose |
|------|----------|---------|
| `TFRS_MANAGER_BASE_URL` | See notes | Base URL of the TFRSManager instance the app logs into (e.g. `https://manager.turingfocus.cn`). No built-in default — when Manager login is triggered without an explicit `base_url` argument, this variable is the single source of truth. Requests fail with `MissingBaseUrl` if both the argument and the variable are absent. |

Set it per-shell before launching the dev app:

```bash
export TFRS_MANAGER_BASE_URL="https://manager.example.com"
pnpm tauri dev
```

Or configure it in your shell profile for persistent use. When the UI exposes a login form (issue #24), users may override this value per session.
