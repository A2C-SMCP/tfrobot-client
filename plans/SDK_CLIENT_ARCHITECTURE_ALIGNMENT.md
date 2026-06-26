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
- MCP server add/update/remove
- MCP server start/stop
- Socket.IO connect/disconnect/reconnect
- resources/window reads that depend on MCP clients

The registry clones the target runtime before awaiting these operations, so single-computer work
does not hold a global registry write lock.

## MCP Server Management

MCP command handlers persist config through `ConfigService`, then synchronize the target
`ComputerInstanceRuntime`.

Runtime behavior:

- Initial runtime construction seeds SDK `Computer` with configured MCP servers.
- `start()` calls `Computer.boot_up()`.
- `add_or_update_server()` calls `Computer.add_or_update_server(...)`.
- `remove_server()` calls `Computer.remove_server(...)`.
- `mcp_server_statuses()` calls `Computer.get_server_status()`.
- `start_mcp_server()` calls `Computer.start_mcp_client(...)`.
- `stop_mcp_server()` calls `Computer.stop_mcp_client(...)`.
- `sync_runtime()` applies ordinary MCP config differences incrementally with
  `sync_sdk_mcp_servers(...)`.

Ordinary MCP config changes must not rebuild the whole SDK `Computer`, because that would stop
active MCP clients. Rebuild is reserved for structural runtime changes such as Computer name,
auto-connect policy, or skill home changes.

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
- MCP lifecycle integration tests: `cargo test --test mcp_integration_test`
- SMCP lifecycle integration tests: `cargo test --test smcp_connection_lifecycle_test`
- frontend store/component tests:
  `pnpm test src/test/components/Dashboard.test.tsx src/test/components/ComputerOverview.test.tsx src/test/components/DesktopResources.test.tsx src/test/components/ResourceBrowser.test.tsx src/test/stores/dashboardStore.test.ts src/test/stores/computerOverviewStore.test.ts src/test/stores/desktopStore.test.ts src/test/stores/debugStore.test.ts`
  - Result: 8 files and 57 tests passed.
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
