# SPEC — Parameterized WSL2 origin capacity policy and safe storage bounds

> SSDV3 Step 2 · PRD: `docs/specs/no-milestone/wsl2-origin-capacity-policy/PRD.md`

## Closed scope

### In now
- Parameterized origin container sizing (`-OriginSizeBytes`) in `Manage-RamSharedOrigin.ps1`.
- Dynamic approval token matching the chosen size (`RAMSHARED_ORIGIN_${N}GIB_PARTUUID`).
- Mathematical physical-to-logical headroom validation (`fixed_size_bytes >= (logical + 1024) * 1024^2`).
- Policy validation update in `scripts/safety/ramshared-host-gate.sh` and `crates/ramshared-wsl2d/src/main.rs`.
- Unit tests covering 5 GiB acceptance, 25 GiB backward compatibility, and under-capacity rejection.
- Default placement on the registered WSL distro volume, falling back only to C: when the selected volume cannot preserve the fixed-origin reserve or the distro `BasePath` cannot be resolved. The independent WSL fallback swap path is never used to infer distro placement.
- Stable use of an existing sealed manifest path; no implicit movement when the distro location changes.
- Disposable guest validation of VHDX attachment, live identity-bound host gating, stale-heartbeat refusal, and idempotent swap-signature provisioning without activating the disposable partition.

### Out now
- Dynamic VHDX expansion; modifying WSL2 fallback swap device or `.wslconfig`; automated live partition resize; automatic movement of an existing origin.

### Assumed-ready dependencies
- Windows Hyper-V PowerShell module (`New-VHD`, `Mount-VHD`, `Initialize-Disk`, `New-Partition`).
- WSL2 bare VHD mount capability (`wsl.exe --mount --vhd ... --bare`).
- Sealed manifest schema 3 and SHA-256 configuration hasher.

## Traceability

| PRD | SPEC |
| --- | --- |
| RF-1 | DT-1, ITEM-1 |
| RF-2 | DT-3, ITEM-1 |
| RF-3 | DT-2, ITEM-1 |
| RF-4 | DT-2, DT-4, ITEM-2, ITEM-3 |
| RF-5 | DT-1, DT-4, ITEM-2, ITEM-3 |
| RF-6 | DT-6, DT-8, ITEM-6 |
| RF-7 | DT-7, ITEM-6 |
| NFR-1..4 | DT-1..5, ITEM-1..4 |
| NFR-5 | DT-6, DT-8, ITEM-6 |

## Technical decisions

| # | Decision | Why |
| --- | --- | --- |
| DT-1 | Add `-OriginSizeBytes` to `Manage-RamSharedOrigin.ps1` with default 5GB and valid range 5GB–64GB in whole GiB steps. | Allows operators to match physical container size to available host SSD capacity while preventing sub-5GiB partition exhaustion. |
| DT-2 | Enforce `fixed_size_bytes >= (logical_capacity_mib + 1024) * 1024^2` and `fixed_size_bytes % (1024^3) == 0`. | Guarantees that GPT header overhead and the mandatory 16 MiB MSR partition can never crowd out the requested logical swap capacity. |
| DT-3 | Derive approval token as `RAMSHARED_ORIGIN_${OriginSizeGiB}GIB_PARTUUID`. | Preserves explicit attended operator consent tied to the exact physical allocation committed to host storage. |
| DT-4 | Replace `fixed_size_bytes != 25 * GIB` in `ramshared-wsl2d` and `ramshared-host-gate.sh` with mathematical range and headroom bounds. | Establishes uniform host-guest contract validation without hardcoded arbitrary numbers. |
| DT-5 | Retain existing 25 GiB static test tokens in `Test-RamSharedOriginStatic.ps1`. | Guarantees backward compatibility with existing static test harnesses while verifying new parameterized capabilities. |
| DT-6 | For a new default origin, prefer the registered distro's `BasePath` volume when its free bytes are at least `OriginSizeBytes + 10 GiB`; otherwise try C: under the same bound and refuse if neither qualifies. If `BasePath` cannot be resolved, C: is the sole automatic candidate. Do not infer distro placement from the independent WSL fallback swap path. The selector receives observed volume records and does not scan arbitrary/removable/network volumes. | Co-locates origin I/O with Linux when practical, works on a single-volume C: host, and keeps a measurable 10 GiB host free-space floor after a fixed VHDX allocation. |
| DT-7 | If the sealed schema-3 manifest exists and no explicit path was supplied, use its absolute `origin_vhdx` as the path for plan/configure/attach/uninstall. An explicit path must canonicalize to that same path. Do not recompute or migrate a sealed origin from the current distro `BasePath`. | The origin location is persistent state; distro moves must not redirect teardown or attachment to a newly derived, unrelated path. |
| DT-8 | Require one unambiguous local volume observation for an automatically selected path and repeat the free-space check immediately before and after staging creation. Low-space outcomes are deterministic refusals, not retryable errors. | Protects against low-space host damage and narrows the check/use race around a fixed allocation. |

## Atomicity and rollback

- **Windows host:** VHDX creation uses transactional staging (`.staging.vhdx`). Any creation or formatting failure removes only current-run staging files, never touching active storage.
- **Host volume choice:** Selection and free-space checks are read-only. Before manifest publication, staging is rechecked for the 10 GiB free-space floor; a failed check uses the existing exact-current-run rollback. No existing origin is moved or deleted by selection.
- **Linux guest:** `ramshared-host-gate.sh` writes `/etc/ramshared/origin.conf` atomically via write-then-rename only after complete manifest validation passes.
- **Rollback trigger:** Any hash mismatch, invalid PARTUUID, or under-capacity failure immediately aborts with non-zero exit code without altering `/proc/swaps` or daemon state.

## Kahneman map (critical only)

| ITEM / stage | # | Question | Min evidence | Abort |
| --- | --- | --- | --- | --- |
| Boundary validation | #13 | Does accepting 5 GiB permit an under-capacity container? | `cargo test -p ramshared-wsl2d host_manifest_hash_fields` | Any acceptance of `fixed_size_bytes < (logical + 1024) * 1024^2`. |
| Replay & backward compat | #17 | Does 25 GiB legacy manifest validate identically? | `cargo test -p ramshared-wsl2d` | Rejection of standard 25 GiB manifest. |
| New volume selection | #9/#13 | Does the selector prefer the distro volume, use C: only as fallback, and refuse without reserve? | `origin_volume_prefers_distro_volume_with_reserve`; `origin_volume_falls_back_to_c_when_preferred_lacks_reserve`; `origin_volume_refuses_when_all_candidates_below_reserve` | Any choice below `OriginSizeBytes + 10 GiB`, or refusal when a valid preferred/fallback volume exists. |
| Persistent origin path | #17 | Does a distro move change the path of an already sealed origin? | `origin_existing_manifest_path_survives_distro_volume_change`; `origin_existing_manifest_override_mismatch_is_refused` | Any recomputation, implicit relocation, or acceptance of a conflicting explicit path. |
| Staging allocation | #16 | Can a concurrent volume consumer exhaust free space between plan and VHDX creation? | `origin_install_rechecks_post_create_reserve_before_manifest`; elevated disposable-host drill on C: and I: with fixed allocation, identity proof, exact-path uninstall, and a 64 GiB reserve-fallback plan | Manifest publication with less than 10 GiB remaining; any cleanup outside current-run staging. |

## Security checklist (pre-impl)

- [x] Privilege: Attended operator approval token required for host disk provisioning.
- [x] User/host copy: Manifest values parsed into typed, validated data structures before evaluation.
- [x] Flags/IOCTL codes: Only standard GPT partition styles and bare mount flags used.
- [x] Info-leak: N/A; manifest contains only storage GUIDs and capacities.
- [x] Host safety: Physical pre-allocation (`-Fixed`) preserved; dynamic VHDX expansion strictly avoided.
- [x] Shared-hardware cushion: Mathematical reserve floor enforced between physical VHDX and logical swap.
- [x] Replayable ops: Manifest validation and provisioning inspection are idempotent (#17).

## Files to CREATE / MODIFY / DELETE

### MODIFY

**`scripts/windows/Manage-RamSharedOrigin.ps1`**
- Purpose: Parameterize origin size and choose a bounded host volume without changing an existing sealed origin path.
- RF / DT: RF-1..3, RF-6..7, DT-1..3, DT-6..8.
- Before -> After:
  `$OriginSize = 25GB` -> `$OriginSize = if ($PSBoundParameters.ContainsKey("OriginSizeBytes")) { $OriginSizeBytes } else { 5GB }`
  Approval string derives `RAMSHARED_ORIGIN_${($OriginSize / 1GB)}GIB_PARTUUID` (replacing fixed `RAMSHARED_ORIGIN_25GIB_PARTUUID`)
- Tests: `scripts/windows/Test-RamSharedOriginStatic.ps1`; manufactured `Invoke-OriginManufacturedTests` selection cases.

**`scripts/safety/ramshared-host-gate.sh`**
- Purpose: Validate parameterized `fixed_size_bytes` with mathematical floor.
- RF / DT: RF-4, RF-5, DT-2, DT-4.
- Before -> After:
  `or manifest["fixed_size_bytes"] != 25 * 1024**3` ->
  `or manifest["fixed_size_bytes"] < 5 * 1024**3 or manifest["fixed_size_bytes"] > 64 * 1024**3 or manifest["fixed_size_bytes"] % (1024**3) != 0 or manifest["fixed_size_bytes"] < (logical + 1024) * 1024**2`
- Tests: `scripts/safety/test-control-plane-units.sh`.

**`crates/ramshared-wsl2d/src/main.rs`**
- Purpose: Validate parameterized `fixed_size_bytes` against logical capacity headroom.
- RF / DT: RF-4, RF-5, DT-2, DT-4.
- Before -> After:
  `|| host.fixed_size_bytes != 25 * GIB` ->
  `|| host.fixed_size_bytes < 5 * GIB || host.fixed_size_bytes > 64 * GIB || host.fixed_size_bytes % GIB != 0 || host.fixed_size_bytes < ((host.logical_capacity_mib as u64 + 1024) * MIB)`
- Tests: `host_manifest_hash_fields_are_enforced_end_to_end`, `host_manifest_accepts_five_gib_and_rejects_under_capacity`.
- Cover target: >= 80% on touched business logic.

**`scripts/windows/Test-RamSharedOriginStatic.ps1`**
- Purpose: Require executable volume-priority, fallback, low-space refusal, and manifest-path replay evidence.
- RF / DT: RF-6..7; DT-6..8.
- Tests: `Manage-RamSharedOrigin.ps1 -Action test -Run`.
- Cover target: N/A — PowerShell host orchestration.

## Implementation order

- `ITEM-1`: Update `scripts/windows/Manage-RamSharedOrigin.ps1` with `-OriginSizeBytes` and dynamic approval token.
- `ITEM-2`: Update `scripts/safety/ramshared-host-gate.sh` with mathematical floor and alignment bounds.
- `ITEM-3`: Add unit test reproducing under-capacity refusal and 5 GiB acceptance in `crates/ramshared-wsl2d/src/main.rs` (RED).
- `ITEM-4`: Implement parameterized check in `crates/ramshared-wsl2d/src/main.rs` (GREEN).
- `ITEM-5`: Update docs index and run full test suites and docs-check.
- `ITEM-6`: Add failing manufactured tests for distro-volume preference, C: fallback, no-volume refusal, and sealed-path replay; then implement the selector, explicit-path preflight, and staged-allocation reserve checks in `Manage-RamSharedOrigin.ps1`.

## Required tests matrix

| Production path | Test (`file` :: `name`) | Kind | Kahneman | Cover |
| --- | --- | --- | --- | --- |
| `crates/ramshared-wsl2d/src/main.rs` | `tests::host_manifest_hash_fields_are_enforced_end_to_end` | unit | #17 | >=80% |
| `crates/ramshared-wsl2d/src/main.rs` | `tests::host_manifest_accepts_five_gib_and_rejects_under_capacity` | unit | #13 | >=80% |
| `scripts/safety/ramshared-host-gate.sh` | `test-control-plane-units.sh` :: `fresh_schema_v2_guardian_proof_mints_boot_bound_lease` | unit | #13 | N/A — shell |
| `scripts/windows/Manage-RamSharedOrigin.ps1` | `Test-RamSharedOriginStatic.ps1` | unit | #17 | N/A — shell |
| `scripts/windows/Manage-RamSharedOrigin.ps1` | `origin_volume_prefers_distro_volume_with_reserve` | manufactured | #9/#13 | N/A — PowerShell orchestration |
| `scripts/windows/Manage-RamSharedOrigin.ps1` | `origin_volume_falls_back_to_c_when_preferred_lacks_reserve` | manufactured | #13 | N/A — PowerShell orchestration |
| `scripts/windows/Manage-RamSharedOrigin.ps1` | `origin_single_volume_c_satisfies_default` | manufactured | #13/#17 | N/A — PowerShell orchestration |
| `scripts/windows/Manage-RamSharedOrigin.ps1` | `origin_volume_refuses_when_all_candidates_below_reserve` | manufactured | #16 | N/A — PowerShell orchestration |
| `scripts/windows/Manage-RamSharedOrigin.ps1` | `origin_volume_rejects_removable_and_unsupported_filesystem` | manufactured | #13 | N/A — PowerShell orchestration |
| `scripts/windows/Manage-RamSharedOrigin.ps1` | `origin_existing_manifest_path_survives_distro_volume_change` | manufactured | #17 | N/A — PowerShell orchestration |
| `scripts/windows/Manage-RamSharedOrigin.ps1` | `origin_existing_manifest_override_mismatch_is_refused` | manufactured | #13/#17 | N/A — PowerShell orchestration |
| `scripts/windows/Manage-RamSharedOrigin.ps1` | `origin_install_rechecks_post_create_reserve_before_manifest` | manufactured | #16 | N/A — PowerShell orchestration |

## Validation checklist

- [x] `cargo fmt` / `clippy -D warnings` / `cargo test -p ramshared-wsl2d`
- [x] Cover gate: `node tools/ci/check-rust-slice-coverage.mjs`
- [x] `./scripts/safety/test-control-plane-units.sh`
- [x] `./scripts/docs-check.sh`
- [x] Every matrix row has a real test name
- [x] Kahneman critical rows have executable evidence
- [x] `Test-RamSharedOriginStatic.ps1` proves the volume policy and path replay.
- [x] Live read-only `plan` on the real host; no origin recreation for this slice.
- [x] Elevated disposable live creation, identity-proof, and exact-path rollback drill on C: fallback and the registered distro volume I:; production origin and swap left untouched.
- [x] Attach a disposable new origin to WSL, prove identity-bound host-gate acceptance and stale-heartbeat refusal, provision the 4 GiB swap signature idempotently, and detach/uninstall without activating the test swap.
- [ ] Run guarded RamShared activation, bounded stress, and swapoff-first teardown on the disposable origin; the full PRD live acceptance remains open.
