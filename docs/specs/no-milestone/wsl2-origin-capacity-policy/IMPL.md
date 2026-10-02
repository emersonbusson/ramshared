# IMPL — Parameterized WSL2 origin capacity policy and safe storage bounds

> SSDV3 Step 3 · SPEC: `docs/specs/no-milestone/wsl2-origin-capacity-policy/SPEC.md`

## Status

Historical capacity slice implemented · prior cover **97.7%** and host E2E recorded below. New-origin selection and the reversible fixed-VHDX lifecycle passed an elevated disposable-host drill on C: and the distro volume I:. EVD-0067 also passed guest attachment, identity-bound host-gate checks, stale-heartbeat refusal, and idempotent 4 GiB swap-signature provisioning on the disposable I: VHDX. Step 3 remains **PARTIAL** because no RamShared cascade, stress, or teardown was run on that origin.

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `scripts/windows/Manage-RamSharedOrigin.ps1` | ITEM-1 / RF-1, RF-2, RF-3 | Parameterized container sizing, dynamic approval token, and GiB multiple validation. |
| `scripts/safety/ramshared-host-gate.sh` | ITEM-2 / RF-4, RF-5 | Mathematical floor, GiB alignment, and headroom validation in host gate. |
| `crates/ramshared-wsl2d/src/main.rs` | ITEM-3, ITEM-4 / RF-4, RF-5 | Fixed container headroom verification and unit test suite. |
| `scripts/windows/Manage-RamSharedOrigin.ps1` | ITEM-6 / RF-6, RF-7 | Prefers the distro `BasePath` volume, uses C: as the only fallback, enforces a 10 GiB post-allocation reserve, validates explicit destinations, preserves sealed paths, and isolates manufactured tests from live host discovery. |

## Validation

### Unit and static checks

- RED checkpoint: `2b743a91 test(origin): reproduce 5 GiB origin acceptance and under-capacity rejection gap`.
- `cargo test -p ramshared-wsl2d --bin ramsharedd`: **98 passed, 0 failed** (exit 0).
- `cargo clippy -p ramshared-wsl2d --all-targets -- -D warnings`: exit 0.
- `cargo fmt --all -- --check`: exit 0.
- Slice coverage: `crates/ramshared-wsl2d/src/main.rs` **97.7%** (1,113/1,139 lines covered, minimum threshold 80%).
- `./scripts/safety/test-control-plane-units.sh`: **13/13 passed** (100% PASS, exit 0).
- `./scripts/docs-check.sh`: exit 0 (`✓ docs-check OK`).
- `powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File scripts/windows/Manage-RamSharedOrigin.ps1 -Action test -Run`: **17 named checks passed**, including distro-volume preference, C: fallback, one-volume C: at the 15 GiB boundary, low-space and unsupported-storage refusal, replayed sealed path, conflicting override refusal, and post-allocation reserve refusal.
- `scripts/windows/Test-RamSharedOriginStatic.ps1`: exit 0; verifies the host-discovery bypass for manufactured tests, explicit-path reserve preflight, and reserve-check order before VHDX promotion and manifest publication.
- `powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File scripts/windows/Manage-RamSharedOrigin.ps1 -Action plan`: exit 0; read-only output selected `C:\ProgramData\RamShared\ramshared-origin.vhdx` from `sealed_manifest`, with `fixed_size_bytes=5368709120` and the independent WSL fallback swap at `C:\wsl\swap.vhdx`. Since a sealed manifest already exists, this run did not exercise new-origin volume selection or report current free-space headroom.
- `git diff --check`: exit 0.

### SPEC test matrix

| Production path | Test (`file` :: `name`) | Kind | Kahneman | Result |
| --- | --- | --- | --- | --- |
| `crates/ramshared-wsl2d/src/main.rs` | `tests::host_manifest_hash_fields_are_enforced_end_to_end` | unit | #17 | PASS |
| `crates/ramshared-wsl2d/src/main.rs` | `tests::host_manifest_accepts_five_gib_and_rejects_under_capacity` | unit | #13 | PASS |
| `scripts/safety/ramshared-host-gate.sh` | `test-control-plane-units.sh` :: `fresh_schema_v2_guardian_proof_mints_boot_bound_lease` | unit | #13 | PASS |
| `scripts/windows/Manage-RamSharedOrigin.ps1` | `Test-RamSharedOriginStatic.ps1` | unit | #17 | PASS |
| `scripts/windows/Manage-RamSharedOrigin.ps1` | `origin_volume_prefers_distro_volume_with_reserve` | manufactured | #9/#13 | PASS |
| `scripts/windows/Manage-RamSharedOrigin.ps1` | `origin_volume_falls_back_to_c_when_preferred_lacks_reserve` | manufactured | #13 | PASS |
| `scripts/windows/Manage-RamSharedOrigin.ps1` | `origin_single_volume_c_satisfies_default` | manufactured | #13/#17 | PASS |
| `scripts/windows/Manage-RamSharedOrigin.ps1` | `origin_volume_refuses_when_all_candidates_below_reserve` | manufactured | #16 | PASS |
| `scripts/windows/Manage-RamSharedOrigin.ps1` | `origin_volume_rejects_removable_and_unsupported_filesystem` | manufactured | #13 | PASS |
| `scripts/windows/Manage-RamSharedOrigin.ps1` | `origin_existing_manifest_path_survives_distro_volume_change` | manufactured | #17 | PASS |
| `scripts/windows/Manage-RamSharedOrigin.ps1` | `origin_existing_manifest_override_mismatch_is_refused` | manufactured | #13/#17 | PASS |
| `scripts/windows/Manage-RamSharedOrigin.ps1` | `origin_install_rechecks_post_create_reserve_before_manifest` | manufactured | #16 | PASS |

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

### Disposable host E2E for the volume-selection extension (2026-09-26)

This was a separate, reversible Windows storage-manager drill. It did not reuse,
replace, or attach the production origin. The temporary manager template had the
same SHA-256 as `scripts/windows/Manage-RamSharedOrigin.ps1`
(`827344c8e7372717f95a5036b1ed5e854c97382a9d2fdbf8a08e2f86dff37725`); each
case copy changed only its manifest and backup roots to isolate lab state.

| Case | Plan result before write | Fixed allocation and verification | Cleanup result |
| --- | --- | --- | --- |
| Unregistered disposable distro; C: is the only candidate | `c_default`, `C:\`, free `115876167680` bytes, required `16106127360` | 5 GiB (`5368709120` bytes), `Fixed`; free after allocation `110502268928`; `configure=VERIFIED` | Test VHDX and manifest absent; free after removal `115875147776` |
| Registered `Ubuntu-24.04` distro on I: | `distro_basepath`, `I:\`, free `61655785472` bytes, required `16106127360` | 5 GiB (`5368709120` bytes), `Fixed`; free after allocation `56281833472`; `configure=VERIFIED` | Test VHDX and manifest absent; free after removal `61654736896` |

Additional read-only plans proved that a 64 GiB request requires
`79456894976` free bytes: I: had `61655785472`, so selection fell back to C:
with `115879870464` free. An explicit C: path was also accepted with the
`16106127360`-byte reserve. Manufactured tests continue to cover the literal
single-volume C: boundary; the live C: run used an unregistered lab distro on a
host that also has I:.

After the drill, a separate cleanup check found both lab targets and manifests
absent, zero backup files, and the production manifest still pointing to
`C:\ProgramData\RamShared\ramshared-origin.vhdx` at `5368709120` bytes. The
existing `.wslconfig` still specified `swapFile=C:/wsl/swap.vhdx`. No WSL
attachment, guest host-gate, cascade activation, stress, or CoCo qualification
was performed in this drill.

## Gaps

- The new volume-selection extension now has live, reversible 5 GiB fixed-VHDX creation, manifest proof, and exact-path uninstall evidence on C: and the actual distro volume I:. A read-only 64 GiB plan also proved fallback when I: lacked the required reserve. The production manifest and independent WSL swap path remained unchanged.
- EVD-0067 attached the isolated I: VHDX to WSL and proved live identity matching, valid host-gate acceptance, stale guardian-proof refusal, and 4 GiB swap-signature provisioning with idempotent replay. The test partition was never activated as swap; it was detached and uninstalled, and the production origin configuration hash remained unchanged.
- The C:-only selection scenario was exercised with an unregistered disposable distro on a multi-volume host, and the literal single-volume C: boundary remains covered by its manufactured test. No RamShared cascade or stress used the new VHDX. Keep the full PRD/Step 3 **PARTIAL** until guarded cascade activation, teardown, and bounded stress are proven.

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
| RF-6 | ITEM-6 | uncommitted worktree |
| RF-7 | ITEM-6 | uncommitted worktree |

## 2026-09-26 — Volume placement extension

- New default origins prefer the registered distro `BasePath` volume when it has `OriginSizeBytes + 10 GiB` free; C: is the only fallback. When `BasePath` is unavailable, the manager uses C: and does not infer distro placement from `.wslconfig`'s separate fallback swap path. Non-fixed, non-NTFS/ReFS, ambiguous, or under-reserve destinations fail closed.
- Explicit new-origin paths now receive the same read-only volume and reserve preflight. Installation checks again before creating the fixed VHDX and after staging allocation; the 10 GiB reserve must remain before proof, promotion, or manifest publication. Existing schema-3 manifests keep their exact absolute origin path and reject a conflicting explicit path.
- Manufactured tests passed for preference, C: fallback, the single-volume C: boundary, refusal below reserve, removable/unsupported-storage refusal, sealed-path replay, explicit mismatch, and post-allocation exhaustion. Static checks confirm unique local-volume/filesystem gates, that tests bypass live host discovery, and that reserve checks precede promotion and manifest publication.
- The read-only host plan confirmed that the current sealed origin remains on `C:\ProgramData\RamShared\ramshared-origin.vhdx` and the independent WSL fallback swap remains `C:\wsl\swap.vhdx`. Because the sealed manifest is present, the plan did not run the new-origin selector or sample free-space headroom. No storage mutation or new live qualification is claimed.
