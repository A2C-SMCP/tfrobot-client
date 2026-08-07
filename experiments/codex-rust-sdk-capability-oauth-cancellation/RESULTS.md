# OAuth lifecycle cancellation experiment results

Date: 2026-08-06

## Decision

**Go for integrating rust-sdk `develop@29b66447302a9578de21144c3b62983f5d1b7577` into
tfrobot-client.** PR [A2C-SMCP/rust-sdk#172](https://github.com/A2C-SMCP/rust-sdk/pull/172)
implements the lifecycle contract requested by
[A2C-SMCP/rust-sdk#170](https://github.com/A2C-SMCP/rust-sdk/issues/170). The old client-pinned
revision still reproduces the original gaps, while the accepted candidate passes the deterministic
OAuth cancellation, rollback, and lifecycle suite.

This Go decision is limited to the flow-lifecycle contract tested here. A later tfrobot-client
integration review identified an optional stronger status-ordering contract; after SDK scope review
on 2026-08-07, that follow-up was explicitly deferred and is not a blocker for Issue #45. See
“Client integration follow-up” below.

## Revisions

| Variant | Commit | Independent run |
| --- | --- | --- |
| tfrobot-client pinned SDK | `b6ada4db5317b469f93aa7f5ffecf0305e51e3f9` | 2 passed |
| superseded `origin/develop` | `fa0d4a32dee971807e686771ab84c2c36453c68b` | 2 passed (gap reproduced) |
| accepted `origin/develop` | `29b66447302a9578de21144c3b62983f5d1b7577` | 51 passed |

The accepted candidate is the merge commit of PR #172. It was cloned and checked out independently,
then compiled with its own target directory so the result does not depend on a shared Cargo artifact
or source-diff inference.

## Baseline observations

Both revisions emitted the same evidence:

```text
EXPERIMENT delayed_begin_requires_caller_abort=true cancel_api_has_no_state=true
EXPERIMENT cancel_waited_for_exchange=true exchange_won_cancel_race=true clear_deleted_credentials=true
```

- During delayed protected-resource discovery, `begin_oauth` does not provide a flow state within
  the 100 ms cancellation budget. Because `cancel_oauth` requires that state, the host can only
  abort its `begin_oauth` caller future.
- During a delayed step-up token exchange, `cancel_oauth` does not complete within 100 ms. It waits
  behind `complete_oauth`; the token exchange commits first and the later cancellation is rejected.
- Calling `clear_oauth` after that race changes the status to `Unauthorized`. It cannot restore the
  valid credential that existed before the step-up attempt.
- The dynamic-registration begin probe exercised protected-resource discovery, authorization-server
  discovery, DCR, resource binding, and PKCE state creation before the caller was aborted.

The tests pass when they reproduce the gap; they are not acceptance tests claiming the desired
contract is satisfied.

## Accepted-candidate observations

The exact candidate ran all 51 `smcp-computer` mock integration tests successfully. The suite
includes direct assertions that:

- an `OAuthFlow` can be cancelled before delayed discovery or DCR returns;
- cancellation preempts a delayed token exchange and a scope step-up retains the earlier
  `tools.read` credential and status;
- complete/cancel clones converge on one terminal outcome;
- server replacement, removal, and Computer shutdown cancel and drain flows without waiting for the
  provider timeout;
- failed candidate credential commits restore the previous durable credential and scopes;
- late callbacks cannot revive a retired flow.

## Verified rust-sdk contract

The candidate exposes and tests all three properties required by tfrobot-client:

1. A cancellation handle/token available before network discovery and DCR complete, so cancellation
   does not depend on the eventual `OAuthLaunch.state`.
2. A per-flow terminal decision that lets cancellation preempt or invalidate an in-flight token
   exchange instead of waiting behind it and losing the race.
3. Transactional reauthorization semantics: cancellation or failure of a scope step-up must preserve
   the previously authorized credential and status; a newly exchanged token must not be committed
   after cancellation wins.

## Client integration follow-up (2026-08-07, deferred)

The pinned SDK internally maintains the exact latest OAuth status in
`RuntimeStatus::latest_oauth_status`, but this API is crate-private and has no public source
revision. The public `Computer::oauth_status()` is not an equivalent read: it can lazily initialize
`OAuthCoordinator` and perform provider metadata discovery. Calling it from tfrobot-client's event
relay would therefore recreate uncancellable provider I/O without an `OAuthFlow` handle.

Client-side receive-order revisions cannot prove source-causal ordering across direct status
queries, queued broadcast events, credential clears, and server replacement. This stronger contract
was proposed as [rust-sdk#173](https://github.com/A2C-SMCP/rust-sdk/issues/173).

The SDK review concluded **NO-GO for the current scope**: MCP Authorization and the client's basic
OAuth product loop do not require versioned status snapshots. tfrobot-client therefore uses the
existing `OAuthStatusChanged` event stream, keeps only best-effort same-batch coalescing, and accepts
the residual extreme reordering risk. #173 was closed as not planned and should be reopened only if
a reproducible old-event-over-new-state failure appears. Provider-side RFC 7009 token revocation is
a separate capability and is also outside Issue #45.

## Commands

Baseline command after applying `oauth_lifecycle_probe.patch`:

```sh
cargo test -p smcp-computer --test mock_server_integration experiment_ -- --nocapture
```

Accepted-candidate command:

```sh
cargo test -p smcp-computer --all-features --test mock_server_integration -- --nocapture
```

## Isolation and limitations

- Only detached rust-sdk worktrees under this experiment directory were patched.
- The accepted candidate was tested from an independent temporary clone at the exact merge SHA.
- tfrobot-client manifests, lockfiles, business code, and configuration were not modified by the
  experiment.
- The experiment uses deterministic loopback mocks and does not make claims about a particular live
  OAuth provider's latency or availability.
