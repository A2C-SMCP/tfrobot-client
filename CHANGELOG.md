# Changelog

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

