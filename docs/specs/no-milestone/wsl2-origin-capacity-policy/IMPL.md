# IMPL — Parameterized WSL2 origin capacity policy and safe storage bounds

> SSDV3 Step 3 · SPEC: `docs/specs/no-milestone/wsl2-origin-capacity-policy/SPEC.md`

## Status

Implemented · cover **97.7%** (PASS >= 80%) · E2E **PASS** · BINARY_MATCH **OK**.

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `scripts/windows/Manage-RamSharedOrigin.ps1` | ITEM-1 / RF-1, RF-2, RF-3 | Parameterized container sizing, dynamic approval token, and GiB multiple validation. |
| `scripts/safety/ramshared-host-gate.sh` | ITEM-2 / RF-4, RF-5 | Mathematical floor, GiB alignment, and headroom validation in host gate. |
| `crates/ramshared-wsl2d/src/main.rs` | ITEM-3, ITEM-4 / RF-4, RF-5 | Fixed container headroom verification and unit test suite. |

## Validation

### Unit and static checks

- RED checkpoint: `2b743a91 test(origin): reproduce 5 GiB origin acceptance and under-capacity rejection gap`.
- `cargo test -p ramshared-wsl2d --bin ramsharedd`: **98 passed, 0 failed** (exit 0).
- `cargo clippy -p ramshared-wsl2d --all-targets -- -D warnings`: exit 0.
- `cargo fmt --all -- --check`: exit 0.
- Slice coverage: `crates/ramshared-wsl2d/src/main.rs` **97.7%** (1,113/1,139 lines covered, minimum threshold 80%).
- `./scripts/safety/test-control-plane-units.sh`: **13/13 passed** (100% PASS, exit 0).
- `./scripts/docs-check.sh`: exit 0 (`✓ docs-check OK`).

### SPEC test matrix

| Production path | Test (`file` :: `name`) | Kind | Kahneman | Result |
| --- | --- | --- | --- | --- |
| `crates/ramshared-wsl2d/src/main.rs` | `tests::host_manifest_hash_fields_are_enforced_end_to_end` | unit | #17 | PASS |
| `crates/ramshared-wsl2d/src/main.rs` | `tests::host_manifest_accepts_five_gib_and_rejects_under_capacity` | unit | #13 | PASS |
| `scripts/safety/ramshared-host-gate.sh` | `test-control-plane-units.sh` :: `fresh_schema_v2_guardian_proof_mints_boot_bound_lease` | unit | #13 | PASS |
| `scripts/windows/Manage-RamSharedOrigin.ps1` | `Test-RamSharedOriginStatic.ps1` | unit | #17 | PASS |

### Live E2E qualification (before -> action -> after)

1. **Before:**
   - Host physical disk held legacy 25 GiB VHDX (`C:\ProgramData\RamShared\ramshared-origin.vhdx`), consuming ~26 GB on Windows `C:\` partition (free space was ~97 GB).
   - WSL2 guest attached the legacy 25 GiB container as the origin block device.
   - Operator required reclaiming physical host SSD space down to 5 GiB while maintaining 4 GiB logical swap capacity.

2. **Action:**
   - Built universal Linux release bundle `v0.13.4-68-g1599655b` and atomically installed to the active release prefix.
   - Executed elevated host mutation script `C:\ProgramData\RamShared\recreate-5gb-origin.ps1`:
     - Dismounted legacy 25 GiB VHDX from WSL2.
     - Removed legacy 26 GiB VHDX and old manifest, immediately reclaiming ~21 GB of host physical SSD space (free space on `C:\` rose from 97 GB to 114 GB).
     - Provisioned new 5.1 GiB fixed VHDX with GPT layout: 16 MiB Microsoft Reserved (MSR) partition + ~4.98 GiB data partition.
     - Attached bare to WSL2 with dynamic partition and disk GUID bindings.
   - Executed `scripts/safety/ramshared-host-gate.sh`: verified `fixed_size_bytes=5368709120` (5 GiB), logical 4096 MiB, and minted sealed `/etc/ramshared/origin.conf`.
   - Executed `scripts/safety/provision-origin-swap.sh --execute`: formatted exact 4096 MiB swap with `last_page=1048575` on origin data partition (`RAMSHARED_ORIGIN_PROVISION=PROVISIONED`). Re-run verified `ALREADY_PROVISIONED` idempotency.
   - Armed 3-tier cascade via `ramshared up`:
     - `/dev/zram0`: 1024M, priority 200.
     - `/dev/nbd0`: 4096M, priority 100.
     - WSL fallback swap partition: 4096M, priority -2.
     - Verified daemon status: `phase: Armed (armed_low_vram_used)`, `protection: READY (guaranteed_vram_tier_armed)`, `topology_ok: true`, `daemon: alive pid=688034`.
   - Tested graceful teardown via `ramshared down`: verified clean swapoff-first sequence unmounting `/dev/nbd0` without hang or data loss (`phase: Off`, `daemon: dead`).
   - Re-armed via `ramshared up`: 3-tier cascade restored cleanly (`phase: Armed`).
   - Kernel ring buffer (`dmesg`): verified zero kernel panics, zero oops, and zero hung tasks (`PASS_ZERO_PANIC`).

3. **After:**
   - Physical VHDX footprint reduced to 5.1 GiB (recovering ~21 GB permanently on host SSD).
   - Authoritative SSD origin active with exact 4 GiB swap capacity.
   - Three-tier cascade fully operational: `ZRAM(200) > SSD NBD origin(100) > WSL fallback(-2)`.
   - `PASS_ZERO_PANIC` verified across all operations.

## Gaps

- None. Full E2E qualification completed on live host and WSL2 environment.

## Rollback trigger

Revert origin policy changes if manifest verification fails on valid containers,
under-capacity containers are accepted, or kernel panic/hang occurs during cascade operations.

## Traceability

| RF | ITEM | Commit |
| --- | --- | --- |
| RF-1 | ITEM-1 | `6f87ac3c` |
| RF-2 | ITEM-1 | `6f87ac3c` |
| RF-3 | ITEM-1 | `6f87ac3c` |
| RF-4 | ITEM-2, ITEM-4 | `6f87ac3c` |
| RF-5 | ITEM-2, ITEM-4 | `6f87ac3c` |
