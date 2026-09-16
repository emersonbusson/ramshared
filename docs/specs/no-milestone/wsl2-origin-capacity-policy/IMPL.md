# IMPL — Parameterized WSL2 origin capacity policy and safe storage bounds

> SSDV3 Step 3 · SPEC: `docs/specs/no-milestone/wsl2-origin-capacity-policy/SPEC.md`

## Status

Implemented (source) · 98 tests passed · TDD verified · docs-check 100% PASS.

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `scripts/windows/Manage-RamSharedOrigin.ps1` | ITEM-1 / RF-1, RF-2, RF-3 | Parameterized container sizing and dynamic approval token. |
| `scripts/safety/ramshared-host-gate.sh` | ITEM-2 / RF-4, RF-5 | Mathematical floor and alignment validation. |
| `crates/ramshared-wsl2d/src/main.rs` | ITEM-3, ITEM-4 / RF-4, RF-5 | Headroom verification and multi-capacity unit tests. |

## Validation

- RED checkpoint: `2b743a91 test(origin): reproduce 5 GiB origin acceptance and under-capacity rejection gap`.
- `cargo test -p ramshared-wsl2d --bin ramsharedd`: **98 passed, 0 failed**.
- `cargo clippy -p ramshared-wsl2d --all-targets -- -D warnings`: exit 0.
- `cargo fmt --all -- --check`: exit 0.
- `./scripts/safety/test-control-plane-units.sh`: 100% PASS (13/13).
- `./scripts/docs-check.sh`: exit 0 (`✓ docs-check OK`).

## Gaps

- None.

## Rollback trigger

Revert origin policy changes if manifest verification fails on valid containers
or if under-capacity containers are accepted.

## Traceability

| RF | ITEM | Commit |
| --- | --- | --- |
| RF-1 | ITEM-1 | `6f87ac3c` |
| RF-2 | ITEM-1 | `6f87ac3c` |
| RF-3 | ITEM-1 | `6f87ac3c` |
| RF-4 | ITEM-2, ITEM-4 | `6f87ac3c` |
| RF-5 | ITEM-2, ITEM-4 | `6f87ac3c` |
