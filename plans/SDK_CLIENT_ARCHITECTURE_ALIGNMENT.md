# SDK Computer Architecture Alignment

This document records how `tfrobot-client` implements the SDK-side best practices from
`plans/SDK_CLIENT_BEST_PRACTICES.md`.

## Current Boundary

`tfrobot-client` now treats SDK `Computer` as the owner of all per-computer runtime internals:

- MCP server registration and runtime status
- MCP server start/stop
- tool listing and tool execution
- resources/window reads exposed by SDK Computer
- Socket.IO connect, office join, leave, disconnect
- full runtime shutdown

The client-side boundary is:

```text
ComputerRegistry
  - owns many ComputerInstanceRuntime values
  - persists and looks up configured instances
  - never holds the registry write lock while awaiting one runtime lifecycle operation

ComputerInstanceRuntime
  - owns one SDK Computer
  - owns business connection snapshot and runtime state
  - serializes start/stop/sync/connect/reconnect/shutdown with lifecycle_lock
  - exposes narrow command-facing wrapper methods

smcp_computer::Computer
  - owns MCP clients, tools, resources/window APIs, Socket.IO and shutdown internals
```

Production code must not bypass `ComputerInstanceRuntime` to access SDK internals.

Configuration ownership follows the same boundary:

- `tfrobot-client` owns Computer profiles, connection policy, robot binding, global input
  definitions, input values, and secrets.
- `SdkConfigService` is the only client adapter for SDK-owned MCP, skill, Marketplace/plugin,
  runtime-default, revision, and provenance configuration.
- `ConfigService` exposes legacy inline MCP declarations only through the read-only,
  migration-specific `load_legacy_mcp_configs_for_migration` API; it has no general MCP write
  entry point.
- The client chooses and injects each Computer's config directory and skill-home path; SDK owns
  discovery, lifecycle, and governance within those roots.
- The UI-facing SDK config snapshot intentionally excludes SDK `inputs`, so it cannot become a
  second source of truth for client-owned input definitions.

## Runtime State

`ComputerInstanceRuntime` exposes `ComputerRuntimeState`:

- `Created`
- `Booting`
- `Booted`
- `Connecting`
- `Connected`
- `JoinedOffice`
- `Stopping`
- `Stopped`
- `Error`

`is_connected()` and `connection_status()` only report a healthy business connection when the
runtime is in `JoinedOffice` and still has a connection snapshot. Intermediate or failed states do
not present as connected to the frontend.

## Lifecycle Lock

Each runtime owns one `lifecycle_lock`. The lock serializes operations that mutate the SDK Computer
or the business connection state:

- `start`
- `shutdown`
- `sync_runtime`
- MCP server start/stop
- Socket.IO connect/disconnect/reconnect
- resources/window reads that depend on MCP clients

The registry clones the target runtime before awaiting these operations, so single-computer work
does not hold a global registry write lock.

## SDK Configuration And MCP Server Management

MCP configuration command handlers persist SDK-owned declarations through `SdkConfigService`
without requiring, rebuilding, or reloading a runtime. Runtime reload/start/preflight commands are
the explicit boundary that resolves declarations, inputs, secrets, commands, and paths into a
`ComputerInstanceRuntime`. The runtime uses the same instance-scoped config/home/env context as
`SdkConfigService`; `ConfigService` does not persist the SDK MCP source of truth.

`SdkConfigService` wraps the SDK config lifecycle (`init`, `load`, `save`, `update`, `validate`,
`migrate`, `delete`, `duplicate`, `import`, and `export`) with an isolated config/home/env context
per Computer instance.

Computer duplication keeps SDK governance isolated. `DuplicateSkillHomeMode::Copy` copies only the
source Skill Home's `user/` namespace into the target `user/` namespace. It never copies SDK-owned
Marketplace/plugin ledgers, materialized Marketplace content, or derived MCP sources. The SDK
project-config adapter duplicates only the project anchor; Marketplace and plugin lifecycle state
must be established independently for the target Computer.

Runtime behavior:

- Initial runtime construction injects the instance SDK config directory, config env, and selected
  skill home into SDK `Computer`; legacy profile MCP servers are not imported.
- `start()` calls `Computer.boot_up()`.
- `add_or_update_server()` calls `Computer.add_or_update_server(...)`.
- User-owned removal resolves the SDK bundle ID, calls `Computer.remove_server(...)`, and, while
  the runtime is running, immediately reconciles governance so an enabled same-name plugin can
  remount and start its MCP server without a restart or plugin state toggle.
- Plugin-owned MCP uses runtime-only `Computer.mount_server(...)` / `unmount_server(...)` hooks and
  never persists into user SDK config.
- `mcp_server_statuses()` calls `Computer.get_server_status()`.
- UI-facing MCP totals use the unified SDK inventory (user-configured servers plus enabled-plugin
  servers). Running/stopped counts remain derived from live runtime status rather than persisted
  configuration.
- `start_mcp_server()` calls `Computer.start_mcp_client(...)`.
- `stop_mcp_server()` calls `Computer.stop_mcp_client(...)`.
- `sync_runtime()` reconciles from the SDK-owned config projection without importing client profile
  MCP state.

Ordinary MCP config changes must not rebuild the whole SDK `Computer`, because that would stop
active MCP clients. Rebuild is reserved for structural runtime changes such as Computer name,
auto-connect policy, or skill home changes.

## Marketplace And Plugin Governance

Marketplace/plugin lifecycle and reads use SDK `Computer` high-level APIs. The client does not read
SDK settings ledgers, Marketplace manifests, or plugin directories to construct governance state.

- Lifecycle uses `add/refresh/remove_marketplace` and `install/enable/disable/uninstall_plugin`.
- Read state uses `Computer::governance_snapshot()` through `ComputerInstanceRuntime`.
- For an available plugin, installation preview comes from `PluginSnapshot.declared`.
- For an installed plugin, the client reports the SDK's actual bundled/live fields.
- `declared: None` means the catalog declaration is unknown; an empty declared list means the SDK
  inspected it and found no capability. Formal UI DTOs must preserve this distinction when it is
  user-visible.

## Tools

Debug tool listing and execution go through SDK `Computer`:

- `get_available_tools` uses `Computer.get_available_tools()`.
- `execute_tool` creates a UUID `req_id` and uses
  `Computer.execute_tool_cancellable(req_id, tool, params, timeout)`.
- UI-facing history remains backed by redacted client logs.
- SDK tool history is used to recover SDK-resolved server/tool data when available.

Sensitive parameter and error-text redaction remains a client responsibility before values are
persisted in logs.

## Connection And Refresh

Manual SMCP and Manager Robot connection paths share the same runtime wrapper:

- `Computer.connect_socketio(...)`
- `Computer.join_office(...)`

`ConnectionState` is a business snapshot. It no longer owns the primary Socket.IO client.

Manager token pre-refresh uses `reconnect_smcp_socketio_for_generation(...)` under the runtime
lifecycle lock. The generation check prevents a background refresh from overwriting a user-driven
disconnect or connection switch.

Unauthorized refresh still emits `manager:auth-expired` and exits the refresh task.

## Shutdown

Full cleanup is centralized in `ComputerInstanceRuntime::shutdown()`:

1. Mark runtime `Stopping`.
2. Clear the business connection snapshot.
3. Abort the refresh task.
4. Leave office and disconnect Socket.IO through SDK Computer state.
5. Call `Computer.shutdown()`.
6. Mark runtime `Stopped`.

App shutdown and instance deletion both call runtime shutdown instead of independently stopping
MCP clients or Socket.IO.

## Resources And Window Reads

The SDK version currently used by this project exposes the resources/window APIs needed by the
client:

- `Computer.get_resources(server, cursor)`
- `Computer.list_all_windows(window_uri)`
- `Computer.get_window_detail(server, resource)`

Therefore there is no production compatibility adapter for resources/window reads after TFRC-53.
Command handlers call runtime wrapper methods, and runtime methods call SDK `Computer`.

## Legacy MCPServerManager Status

Production source under `src-tauri/src` must not contain:

```bash
runtime.manager
MCPServerManager
```

The only remaining `MCPServerManager` references are allowed in historical or feature-gated SDK
contract material:

- `src-tauri/tests/smcp_handshake_config_test.rs` validates an old SDK builder contract behind
  `verify-smcp-0-2-2`.
- older planning documents may describe pre-alignment architecture and should be treated as
  historical context, not implementation guidance.

## Validation Matrix

The architecture alignment is guarded by:

- Rust unit and integration tests: `cargo test`
- SDK config adapter tests under `services::sdk_config`.
- MCP lifecycle integration tests: `cargo test --test mcp_integration_test`
- Marketplace governance integration tests: `cargo test --test marketplace_integration_test`
- SMCP lifecycle integration tests: `cargo test --test smcp_connection_lifecycle_test`
- frontend store/component tests:
  `pnpm test src/test/components/Dashboard.test.tsx src/test/components/Computer.test.tsx src/test/components/DesktopResources.test.tsx src/test/components/ResourceBrowser.test.tsx src/test/stores/dashboardStore.test.ts src/test/stores/runtimeStore.test.ts src/test/stores/desktopStore.test.ts src/test/stores/debugStore.test.ts`
  - The command exit status and CI output are authoritative; file and test counts are not pinned here.
- source scan: `rg -n "runtime\\.manager|MCPServerManager" src-tauri/src`
  - Result: no production-source matches.

## Follow-Up Candidates

These are not blockers for SDK Computer alignment, but they are useful future hardening
items:

- Add a regression test for updating an active server with the same name and asserting the expected
  SDK reconnect behavior.
- Add behavior-level resources/window integration tests once a stable test MCP fixture exposes
  resource/window data.
- Consider timeout or cancellation semantics for slow resources/window reads so long-running MCP
  calls cannot block lifecycle work indefinitely.
