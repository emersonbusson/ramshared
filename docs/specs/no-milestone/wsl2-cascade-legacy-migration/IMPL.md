# IMPL — Attended migration from a legacy WSL2 cascade

> SSDV3 Step 3 · SPEC: `docs/specs/no-milestone/wsl2-cascade-legacy-migration/SPEC.md`

## Status

Implemented (source) · cover **85.4%** · E2E env-bound · daemon proof pending.

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

- RED checkpoint: `62506cf8 test(cli): reproduce legacy cascade migration command`.
- `cargo test -p ramshared-cli --bin ramshared -- --test-threads=1`:
  **275 passed, 0 failed** (36.39 s).
- `cargo clippy -p ramshared-cli --all-targets -- -D warnings`: exit 0.
- `cargo fmt --all -- --check`: exit 0.
- Slice cover: `crates/ramshared-cli/src/cascade/mod.rs` **85.4%**
  (1,724/2,018 lines), threshold 80%.
- `./scripts/docs-check.sh`: exit 0.
- BINARY_MATCH or replaced-daemon listener proof and live watchdog evidence:
  pending.

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
| RF-1 | ITEM-1 | pending |
| RF-2, RF-6 | ITEM-2 | pending |
| RF-3, RF-5 | ITEM-3 | pending |
| RF-4, RF-7 | ITEM-4 | pending |
| NFR-1..5 | ITEM-5 | pending |
