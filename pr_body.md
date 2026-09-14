## Summary
Implemented the `SysInfo` trait and associated safety checks (`memory_threshold_guard`, `disk_space_guard`, and `cpu_load_guard`) for the `ramshared-winsvc` crate to ensure safe operating bounds for host resources, as requested. Added 13 comprehensive unit tests using a mock system information provider to thoroughly validate standard operation, bounds checking, and handling of underlying system errors.

## Commits
| Commit | What was done | Why it was done | Details |
|--------|---------------|-----------------|---------|
| a667497 | Add unit tests for host safety guard validations | Validates safety bounds and error handling | <details><summary>Details</summary>Files: crates/ramshared-winsvc/src/host_safety.rs<br>Validation: cargo test<br>Risk/rollback: Rollback on CI failures</details> |
| 7dfd485 | Run cargo fmt | Enforce style guidelines | <details><summary>Details</summary>Files: crates/ramshared-winsvc/src/host_safety.rs<br>Validation: cargo fmt<br>Risk/rollback: Rollback on CI failures</details> |

## Issue
test-winsvc-009

## Responsavel/Owner
@jules

## Labels
type:test area:test-winsvc

## Validation
* Evaluated unit tests with `cargo test -p ramshared-winsvc` across edge cases (bounds check, invalid states, mocked external errors), validating behavior.
* Passed all linting with `cargo clippy -p ramshared-winsvc -- -D warnings`.

## Rollback trigger
test regressions on winsvc guards or CI compilation failures.
