# rust-sdk OAuth cancellation capability experiment

This isolated experiment compares the tfrobot-client pinned SDK behavior with the bounded OAuth
cancellation implementation merged by rust-sdk PR #172. It does not modify the application's Cargo
manifest, lockfile, or business code.

## Compared revisions

- Current: `b6ada4db5317b469f93aa7f5ffecf0305e51e3f9`
- Superseded candidate: `fa0d4a32dee971807e686771ab84c2c36453c68b`
- Accepted candidate: `29b66447302a9578de21144c3b62983f5d1b7577` (`develop`, merge of PR #172)

## Decision probes

1. Delay protected-resource discovery while `begin_oauth` is running. A host cannot call
   `cancel_oauth` because the SDK has not returned the required state; the only bounded operation
   available is aborting the caller future.
2. Delay the token endpoint while `complete_oauth` is running, then invoke `cancel_oauth` with the
   correct state. The cancellation must complete within 200 ms to pass the client requirement.
3. Start from a valid `tools.read` credential, run a `tools.write` step-up, cancel during exchange,
   and observe whether a host can retain the old credential. The current client compensation call
   (`clear_oauth`) is also measured explicitly.

The baseline `experiment_` tests succeed when they reproducibly observe the capability gap.
Therefore `true` values in their `EXPERIMENT` output mean the old SDK revision **fails** the desired
cancellation/rollback contract. The accepted candidate is validated with its complete deterministic
`smcp-computer` mock integration suite, whose assertions require the corrected contract.

## Commands

For the baseline, apply `oauth_lifecycle_probe.patch` and run:

```sh
cargo test -p smcp-computer --test mock_server_integration experiment_ -- --nocapture
```

For the accepted candidate at the exact SHA above, run:

```sh
cargo test -p smcp-computer --all-features --test mock_server_integration -- --nocapture
```
