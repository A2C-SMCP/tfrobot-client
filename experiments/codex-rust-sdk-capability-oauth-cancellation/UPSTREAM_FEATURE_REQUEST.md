# Feature Request: cancellable and transactional interactive OAuth lifecycle

Priority: **P0 — blocks A2C-SMCP/tfrobot-client#45**

Requester: tfrobot-client

Created upstream issue: https://github.com/A2C-SMCP/rust-sdk/issues/170

Delivered by: https://github.com/A2C-SMCP/rust-sdk/pull/172

Accepted candidate: `29b66447302a9578de21144c3b62983f5d1b7577`

Follow-up evaluated and deferred on 2026-08-07: a public, non-initializing latest OAuth status
observation with a monotonic source revision was proposed as
[rust-sdk#173](https://github.com/A2C-SMCP/rust-sdk/issues/173). The SDK review concluded that this is
not required for the current basic OAuth integration scope, so #173 was closed as not planned and
is not a blocker for tfrobot-client#45.

Evidence: `RESULTS.md` and `oauth_lifecycle_probe.patch` in this directory

## Background

tfrobot-client owns the loopback callback listener, browser launch, user-facing cancellation,
runtime replacement, and shutdown. rust-sdk owns discovery, DCR, PKCE state, token exchange,
credential persistence, and OAuth status. The current public API only returns the state after
`begin_oauth` finishes and serializes `cancel_oauth` behind an in-flight `complete_oauth`.

Consequently, the client cannot guarantee bounded cancellation during discovery or token exchange.
If a canceled reauthorization exchange commits first, the only public compensation operation is
`clear_oauth`, which also removes the valid credential that existed before reauthorization.

## Expected capability

The embedding application needs to be able to cancel an interactive OAuth attempt immediately at
any point, including discovery, registration, and token exchange. When a reauthorization or scope
step-up is canceled or fails, the previously working authorization must remain usable. Once a flow
is canceled, its late callback or late token response must not change credentials or status.

## Required contract

1. Starting a flow returns a host-visible flow handle before discovery/DCR network work completes,
   or accepts a host cancellation token that is usable before `OAuthLaunch.state` exists.
2. Complete and cancel race through one per-flow terminal decision. If cancellation wins, any late
   discovery, registration, callback, or token response is ignored and cannot persist credentials.
3. Reauthorization is transactional. The existing credential/status remains the committed value
   until a replacement token succeeds and the flow is still active. Cancellation or failure rolls
   back to that prior value without requiring `clear_oauth`.
4. Cancellation has a bounded completion path and does not wait for provider network timeout.
5. Replacing/removing a server and shutting down a Computer can cancel and drain its active flows
   without retaining a Computer read guard for the provider timeout.
6. Existing callback state/issuer validation, PKCE secrecy, resource binding, and credential-store
   isolation remain intact.

## Suggested direction (non-binding)

- Introduce an opaque `OAuthFlowHandle`/flow id and cancellation token owned by the SDK coordinator.
- Keep provider state private; the handle should permit cancellation before OAuth `state` exists.
- Give each flow a generation plus an atomic terminal outcome (`Active`, `Completing`, `Cancelled`,
  `Committed`, `Failed`). Credential commit must verify the generation/outcome after exchange.
- Snapshot or defer replacement of the committed credential for step-up flows. Do not model rollback
  as `clear_oauth`.
- Preserve the current methods as compatibility wrappers if practical, while exposing the stronger
  lifecycle contract to hosts that need bounded cancellation.

## Acceptance criteria

- Delayed protected-resource discovery and DCR can be canceled before a launch state is returned;
  cancellation completes within a documented local bound and leaves no pending flow.
- Delayed token exchange can be canceled; a late successful token response is not committed.
- Canceling a `tools.write` step-up from an existing `tools.read` authorization restores/reports the
  original authorized scopes and credential.
- Complete/cancel, duplicate callback, timeout/callback, replacement/callback, removal/callback, and
  shutdown/callback races each produce exactly one terminal outcome.
- Aborting the host caller does not leak a lifecycle lock or credential task.
- Tests cover dynamic registration, preregistered clients, PKCE, resource override, issuer checks,
  refresh, 401 invalidation, 403 insufficient-scope step-up, and durable-store restart recovery.

## Delivery requirements

- Public API and lifecycle documentation, including cancellation ownership and timing guarantees.
- Structured error/outcome definitions for canceled, superseded, expired, and failed flows.
- rust-sdk unit/integration tests plus a tfrobot-client-facing contract test or example.
- Migration and compatibility notes for existing `begin_oauth`, `complete_oauth`, `cancel_oauth`, and
  `clear_oauth` consumers.
