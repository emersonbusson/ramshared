# IMPL — Parameterized WSL2 origin capacity policy and safe storage bounds

> SSDV3 Step 3 · SPEC: `docs/specs/no-milestone/wsl2-origin-capacity-policy/SPEC.md`

## Status

In progress · TDD checkpoint pending.

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `scripts/windows/Manage-RamSharedOrigin.ps1` | ITEM-1 / RF-1, RF-2, RF-3 | Parameterized container sizing and dynamic approval token. |
| `scripts/safety/ramshared-host-gate.sh` | ITEM-2 / RF-4, RF-5 | Mathematical floor and alignment validation. |
| `crates/ramshared-wsl2d/src/main.rs` | ITEM-3, ITEM-4 / RF-4, RF-5 | Headroom verification and multi-capacity unit tests. |

## Validation

- RED checkpoint: pending
- `cargo test -p ramshared-wsl2d`: pending
- Slice cover: pending
- `./scripts/docs-check.sh`: pending

## Gaps

- None.

## Rollback trigger

Revert origin policy changes if manifest verification fails on valid containers
or if under-capacity containers are accepted.

## Traceability

| RF | ITEM | Commit |
| --- | --- | --- |
| RF-1 | ITEM-1 | pending |
| RF-2 | ITEM-1 | pending |
| RF-3 | ITEM-1 | pending |
| RF-4 | ITEM-2, ITEM-4 | pending |
| RF-5 | ITEM-2, ITEM-4 | pending |
