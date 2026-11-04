# Changelog

## 0.3.0 — 2026-10-19

- Fix Linux AppImage packaging by resolving Tcl/Tk libraries from bundled Python, and verify that the packaged tfbash runtime starts.
- Pin Tauri CLI 2.10.1 to fix empty-password updater key generation, and verify signing credentials against the configured public key before building. Preserve the original updater public key and signing identity.
- Upgrade Chat Kit to 0.10.1 and use the routed Server API with Bearer authentication and JSON conversation creation. This version requires the upgraded Server and Manager; retain the existing sign-in, identity selection, conversation navigation, and Enter/IME behavior.
- Select a conversation platform beside the Robot and remember the selection per identity and Robot. Isolate conversation positions and drafts by platform, and preserve the current draft if saving a platform change fails.
- Keep private attachment preview/download behind the native resource boundary. Real-environment download remains pending the controlled endpoint tracked in TFRS-619.
- Offer Test, Staging, and Production in development builds while restricting packaged clients to Production; resolve the advertised Robot Socket.IO route.
- Stack workbench runtime and skill cards, keep diagnostic sections visible, and load desktop resources automatically with a refresh action.

## 0.2.5 — 2026-09-17

- Upgrade Chat Kit to 0.8.2 to fix long-history redaction stalls in macOS WebViews, reuse healthy conversation connections, and improve diagnostics. Preserve Enter/Ctrl+Enter to send and Shift+Enter for a new line; add English and Chinese error and recovery notices.
- Restore the last Robot and conversation after restarting the app.
- Group MCP servers by source, expose redacted connection declarations to client-control tools, and remove user servers from both configuration and runtime.
- Preserve validation errors in tool history and resolve built-in MCP resource server names correctly.
- Coordinate macOS credential access and explain system permission prompts.
- Fix Marketplace IPC field naming and retire the Beta login environment; Staging and Production remain available.
- Upgrade the bundled offline tfbash MCP runtime to 0.2.1 while retaining Python 3.12.14 and the seven-tool stdio contract.

## 0.2.4 — 2026-09-11

- Upgrade Chat Kit to 0.8.1, with cached conversation previews, synchronization notices, and native attachment preview/download support.
- Preserve page state, drafts, selected tabs, and scroll positions across navigation; keep chat sessions active in the macOS background.
- Add bounded foreground MCP startup concurrency and expand client and Computer activity diagnostics.
- Fix workbench header navigation and scroll restoration; stabilize frontend and end-to-end regression tests.

