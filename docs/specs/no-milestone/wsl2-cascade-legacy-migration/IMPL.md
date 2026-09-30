# IMPL — Attended migration from a legacy WSL2 cascade

> SSDV3 Step 3 · SPEC: `docs/specs/no-milestone/wsl2-cascade-legacy-migration/SPEC.md`

## Status

Implemented (source) · cover **83.5% / 80.0%** · E2E env-bound · daemon proof pending.

This document must remain partial until the required watchdog-supervised live
path has a recorded legitimate result and refusal result. Source tests alone
do not close a privileged WSL2 cascade migration.

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `crates/ramshared-cli/src/main.rs` | ITEM-1 / RF-1 | Exact command parsing, dispatch, and help text. |
| `crates/ramshared-cli/src/cascade/mod.rs` | ITEM-2 / RF-2, RF-6 | Pure legacy eligibility plan that excludes ghosts, dirty NBD, duplicates, ublk, and fallback disk selection. |
| `crates/ramshared-cli/src/cascade/cascade_io.rs` | ITEM-3, ITEM-4 / RF-2..7 | Bound-device/pidfd transition executor, strict legacy-record validation, and sealed-origin handoff. |
| `README.md` | ITEM-5 / RF-1 | Concise attended operator guidance. |
| `docs/reliability/DEGRADATION-MATRIX.md` | ITEM-5 / RF-7 | Legacy-handoff containment entry. |

## Validation

**Re-measured 2026-09-30** after the coverage-gate denominator fix (excluding
`#[cfg(not(test))]` adapter-glue regions) and the `cascade_io` test batteries.

- RED checkpoint: `62506cf8 test(cli): reproduce legacy cascade migration command`.
- `cargo test -p ramshared-cli --bin ramshared`:
  **461 passed, 0 failed**.
- `cargo test -p ramshared-cli --bin ramshared -- legacy_migration -- --test-threads=1`:
  **8 passed, 0 failed** (named SPEC tests).
- `cargo test -p ramshared-cli --bin ramshared -- public_control_commands_parse_exactly`:
  **1 passed**.
- `cargo clippy -p ramshared-cli --all-targets -- -D warnings`: clean.
- `cargo fmt -p ramshared-cli -- --check`: clean.
- Slice cover (metric `lines`, threshold 80%):
  - `crates/ramshared-cli/src/cascade/mod.rs`: **83.5%** (1,236/1,480 lines)
  - `crates/ramshared-cli/src/cascade/cascade_io.rs`: **80.0%** (2,785/3,480 lines)
  - **Coverage gate PASSED.**
- `./scripts/docs-check.sh`: OK.
- BINARY_MATCH or replaced-daemon listener proof and live watchdog evidence:
  pending (env-bound).

## Gaps

- **Environment-bound:** supervised WSL2 migration campaign with active host
  guardian authority, daemon proof, legitimate handoff, and refusal evidence.

## Rollback trigger

Disable and revert the migration path upon foreign-device mutation, daemon
identity mismatch, uncertain teardown, watchdog action, or a kernel BUG/Oops/
panic/hung task observed during the supervised campaign.

## Traceability

| RF | ITEM | Commit |
| --- | --- | --- |
| RF-1 | ITEM-1 | `317a3e00` |
| RF-2, RF-6 | ITEM-2 | `317a3e00` |
| RF-3, RF-5 | ITEM-3 | `317a3e00`, `37ff6cd8` |
| RF-4, RF-7 | ITEM-4 | `317a3e00`, `dbcdc263`, `11baceee`, `b4a5b733` |
| NFR-1..5 | ITEM-5 | `317a3e00` |
