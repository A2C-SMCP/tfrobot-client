# macOS restart restoration acceptance — GitHub #81

Uses the production `Chat` page, Chat Kit, Tauri IPC transport and Rust
`SettingsService` in an actual macOS WKWebView. A local HTTP/Socket.IO fixture
supplies two robots and historical conversations. The fixture replaces Manager
identity, credentials and lease acquisition; it never accesses the user's account,
Keychain or application data. Production lease ordering and revocation are covered
separately by `services::chat_session::tests`.

```sh
pnpm exec vite build --config e2e/chat-restoration/vite.config.mjs
cargo build --manifest-path src-tauri/Cargo.toml \
  --features chat-restoration-acceptance --example chat_restoration_acceptance
node e2e/chat-restoration/run.mjs
```

Requires macOS, the repository's installed dependencies, an available GUI session
and a free localhost port 18767. Do not interact with the test window. The runner
uses DOM events and mutation notifications, not timed status polling.

The first process starts with no preference, opens Robot 43 through the real UI,
then selects Conversation 99 (not the first entry) without sending a message.
After native disk-write acknowledgement, it quits and waits for process exit.
A new process using the same isolated temporary directory must open only Robot 43
and render History 99 without first rendering History 42. It writes `result.json`
with observed native events in the printed temporary directory.

This proves native UI → IPC → production preference file → process restart →
history reload. It does not validate production Manager credentials or a signed
installer. Unit/integration coverage additionally exercises old-format migration,
identity/robot isolation, stale writes, closed leases, out-of-page conversation IDs,
404/403 fallback, temporary failures, retry and late responses during manual selection.
