# macOS background chat acceptance (TFRC-134)

This test loads the production `Chat` React page, factory, Chat Kit 0.8.0,
Socket.IO and frontend Tauri HTTP bridge in a real Tauri/WKWebView window.
The runner consumes `src-tauri/tauri.conf.json` at compile time. Authentication,
Manager data and the native HTTP relay are local fixtures, not the production
Rust session service. It does not contact production or access Keychain.

Requires macOS 14+, Rust, pnpm and a Python environment with
`python-socketio==5.13.0`, `python-engineio==4.12.2`, `uvicorn==0.31.1`, FastAPI.
The committed runner lockfile derives from the application's lockfile so Wry
and the Tauri runtime match the application, rather than resolving newer crates.
Port 18766 must be free. Do not open Web Inspector, sleep/lock the system or
interact with the test window during measurement.

```sh
pnpm exec vite build --config e2e/chat-background/vite.config.mjs
cargo build --locked --manifest-path e2e/chat-background/runner/Cargo.toml
BACKGROUND_PYTHON=/path/to/venv/bin/python \
BACKGROUND_BINARY=e2e/chat-background/runner/target/debug/chat-background-acceptance \
node e2e/chat-background/run.mjs
```

The full run takes roughly 33 minutes. `BACKGROUND_SMOKE=1` performs a short
fixture check and **does not satisfy** background duration acceptance.
`BACKGROUND_OUTPUT` optionally selects the output directory. Raw observations
and `result.json` remain in the output directory; failures return nonzero.

Acceptance:

- Hide, minimize, hide, minimize: each measured interval lasts at least eight
  minutes after the window action and animation settling. No measurement IPC,
  periodic observation fetch or synthetic traffic wakes the WebView during
  these intervals: only the actual Socket.IO heartbeat operates.
- Each PING answered within the 60-second timeout, at least five PONGs
  (the conservative bound for a 25-second interval plus a 60-second response
  budget), and no disconnect in each interval; a subsequent live
  message renders while the window is still hidden. A later show request
  checks that the text remains, but does not establish foreground visibility.
  A MutationObserver reports first render once; the controller waits for that
  report without issuing any eval/snapshot which could wake the page.
- At the end of the fourth interval, before any snapshot or window restoration,
  persist a message without broadcasting it and close the actual WebSocket
  with code 1012. Reconnect within 15 seconds while hidden, using server
  monotonic timestamps. REST recovery must request the missing message and
  render it before showing the window. Subsequent live delivery must work and
  the restored message must appear once. The last round checks live delivery
  after recovery, without first showing or inspecting the window. This is an
  absence-of-duplicates check, not a duplicate REST/socket replay test.

This validates idle heartbeat, controlled transport failure, UI delivery and
bounded history recovery together. It does not establish production routing,
production identity refresh, all history beyond the current-server rebase
bounds, older macOS behavior, lock/sleep support or long-term power consumption.

A successful native `show()` call does not guarantee the WebView becomes
visible (for example, another window can occlude it). Per-round snapshots do
not assert `visible`; final visibility is recorded separately in the report.
