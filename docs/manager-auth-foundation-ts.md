# Manager auth foundation integration

TFRC-114 delegates Manager User JWT token exchange to `@turingfocus/tfrs-auth` while Rust retains
Keychain, Manager Context generation, connection/Chat lifecycles, and refresh scheduling.

## Dependency status

The client pins the public npm artifact `@turingfocus/tfrs-auth@0.1.0` exactly. Its npm `gitHead` is
`0eceb65d5d563f8efa050e0c749b74156c495b83`; compared with the originally audited foundation commit
`cd7e551a80927e50ae0cbaa4514cf41a8fc7dfd0`, only license and publication metadata changed. Package
source code is identical, and the published tarball contains `dist/index.js`, declarations, and a
source map. The lockfile records npm integrity
`sha512-7lcMdOiIzjFH0CkaP5Y4GMQL/21AUQkwcoCjGzoCOhqM61lsXit9hPb22S+aDYyVvnR/PqWo6U4L7GSVfj8Ceg==`.

## Publication gate

Before TFRC-114 is delivered:

1. Run `pnpm install --frozen-lockfile`, `pnpm build`, `pnpm lint`, `pnpm test`, and the Rust suite
   from a clean checkout without a local link.
2. Run the staging/beta login → token exchange → Robot connection and Chat acceptance loop.

Rollback removes the exact `@turingfocus/tfrs-auth` dependency and reverts the TFRC-114 bridge
changes as one change set. Do not fall back to the discarded Rust foundation implementation.

## Security boundary

The authoritative User JWT remains in Rust memory and the OS Keychain. A dedicated event supplies
a transient copy to TypeScript `UserJwtCredential`; it is never placed in React/Zustand state,
browser storage, settings, activity records, or logs. Context changes invalidate all TokenSources
and release their references. JavaScript strings cannot be deterministically zeroed. The trust
boundary is the entire main WebView, not ES module privacy: every script executing there must be
treated as credential-capable. CSP and remote-content policy must be reviewed before release.

The native transport accepts only a pending request ID and its exact Manager generation. Native
code derives the URL, method, and `application/x-www-form-urlencoded` content type. It parses the
form only to enforce that the RFC 8693 grant type, User JWT, token type, audience, scope, and field
cardinality match the pending request; `@turingfocus/tfrs-auth` still owns construction and response
parsing.
Each request permits at most three serial transport attempts (initial call plus two foundation
retries), and completion is rejected while an attempt is active. Bridge readiness uses an instance
lease so stale React cleanup cannot revoke a newer bridge. Error completion fields are redacted in
both TypeScript and Rust before they can enter UI state.
