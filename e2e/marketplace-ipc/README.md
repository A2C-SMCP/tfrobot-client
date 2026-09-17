# Marketplace IPC acceptance — GitHub #88

Runs the production skillStore, real macOS WKWebView/Tauri command parsing, production
Marketplace commands and SDK Git operations. An isolated, credential-free Git repository
is served over loopback HTTP; local Git is tested separately. No user configuration or
Keychain access is needed. Requires macOS GUI, Git and installed build dependencies.

```sh
pnpm exec tsc -p e2e/marketplace-ipc/tsconfig.json
pnpm exec vite build --config e2e/marketplace-ipc/vite.config.mjs
cargo build --manifest-path src-tauri/Cargo.toml --features marketplace-ipc-acceptance --example marketplace_ipc_acceptance
node e2e/marketplace-ipc/run.mjs
```

Checks remote/local add and catalog discovery, camelCase source summaries, and both
update source variants reaching the existing business restriction without changing
catalog/source state. Records `result.json` in the printed temporary directory and
requires actual Git HTTP requests. Completion uses an IPC result and process exit,
with a single 90-second deadline; there is no status polling. Temporary evidence and
fixture directories are retained. Set CARGO_TARGET_DIR consistently for build/run
when using a different Cargo target directory.

This exercises the production store rather than clicking the Marketplace form; the
existing MarketplaceTab component tests cover form-to-store payload construction.
It does not verify external Git authentication, public repository availability or a
signed installer. Production frontend/backend APIs are never mocked.
