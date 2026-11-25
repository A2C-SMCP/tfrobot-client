# tfrobot-client
TFRobot Client - A cross-platform desktop application for A2C-SMCP Computer

## Manager environments

The desktop client offers two preset Manager API endpoints and a custom address.

| Environment | Manager API |
|-------------|-------------|
| `staging` | `https://api-staging.turingfocus.cn` |
| `prod` (including Beta) | `https://api.turingfocus.cn` |

Start the native development client with `pnpm tauri:dev` or, on macOS when signed execution is
needed, `pnpm tauri:dev:signed`. The `pnpm dev` command starts the Vite frontend only. Manager
sign-in requires the native backend.

Computers are local to this machine, independent of the Manager login environment. Development
and packaged builds use the same application identifier (`com.tfrobot.client`), original app
data directory, and `tfrobot-client` keychain service. Changing the sign-in environment
keeps the same Computer profiles, MCP configuration, local inputs, and stored Computer secrets.
Existing Computers appear directly without copying or recreating them. Manager credentials
remain scoped to the full Manager base URL, and environment changes use the existing sign-out cleanup.

Open **Settings → Sign-in environment** to select Production, Staging, or Custom environment
in either build. Enter the Manager server URL and select **Save** for a custom environment.
Existing Test settings appear under Custom environment with their previous address.
Production is the default. The choice is saved as `manager_environment` in the shared
`settings.json` and the sign-in screen shows the saved environment. Changing it while signed in
requires confirmation and a new sign-in. On startup, the client restores a session only when
it belongs to the selected environment. After upgrading from a version that saved the
development preference in the webview, select your environment once in Settings.

Beta is an alias for Production for new requests. Upgrades discard session hints and chat
preferences from the retired Beta environment. Credentials stay scoped to their original API
endpoint.
