# tfrobot-client
TFRobot Client - A cross-platform desktop application for A2C-SMCP Computer

## Manager environments

The Manager sign-in screen accepts an environment instead of an arbitrary URL. The desktop client
uses these fixed mappings:

| Environment | Manager API |
|-------------|-------------|
| `test` | `https://rest.test.turingfocus.cn` |
| `staging` | `https://api-staging.turingfocus.cn` |
| `prod` (including Beta) | `https://api.turingfocus.cn` |

Start the native development client with `pnpm tauri:dev` or, on macOS when signed execution is
needed, `pnpm tauri:dev:signed`. The sign-in dialog offers Test, Staging, and Production (including
Beta), defaults to Test, and remembers the selection. Sign out before changing environments.
`pnpm dev` remains the Vite frontend server used by Tauri; native login needs the Tauri backend.

Computers are local to this machine, independent of the Manager login environment. Development
and packaged builds use the same application identifier (`com.tfrobot.client`), original app
data directory, and `tfrobot-client` keychain service. Signing in to Test, Staging, or Production
keeps the same Computer profiles, MCP configuration, local inputs, and stored Computer secrets.
Existing Computers appear directly without copying or recreating them. Manager credentials
remain scoped to their API origin, and environment changes use the existing sign-out cleanup.

Packaged builds (`pnpm tauri build`, including `--debug`) connect only to Production. Rust rejects
Test/Staging login requests and clears unsupported session hints before reading credentials or
sending authentication requests. The build policy follows Tauri's `custom-protocol` feature,
independently of debug assertions, frontend mode, or stored preferences. Release CI sets
`TFROBOT_OFFICIAL_RELEASE=1` and fails if production mode is disabled.

Beta is an alias for Production for new requests. Upgrades still discard retired, separate Beta
session hints and Beta chat preferences: old Beta credentials are never reused as Production.
The login API contract is shared across environments; the client accepts only enums, not URLs.
