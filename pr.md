## Summary
Implemented a finding-only analysis test to explicitly document and assert that the current NBD protocol used by RamSharedServer lacks the session identity required to safely retry/replay inflight packets automatically on socket reconnect without a deeper architectural contract.

## Commits
* test(resilience): prove disconnect gap for unacknowledged tcp/unix requests

## Issue
Closes #14846098977167089094-df4423be

## Responsavel/Owner
@jules

## Labels
resilience, tests

## Validation
Passed `cargo test -p ramshared-wsl2d` and `cargo clippy -p ramshared-wsl2d --all-targets -- -D warnings`.

## Rollback trigger
Revert if the gap-finding analysis test breaks existing connectivity.
