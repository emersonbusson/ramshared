---
slug: wsl2-origin-capacity-policy
title: Parameterized WSL2 origin capacity policy and safe storage bounds
milestone: —
issues: []
---

# PRD — Parameterized WSL2 origin capacity policy and safe storage bounds

## 1. Summary

This document defines the parameterized capacity policy for the RamShared WSL2
origin VHDX container. It replaces the rigid, hardcoded 25 GiB fixed-size invariant
with a parameter-driven, mathematically bounded policy (`5 GiB <= fixed_size_bytes <= 64 GiB`
in whole GiB increments, with `fixed_size_bytes >= (logical_capacity_mib + 1024) * 1024^2`).
This enables operators with constrained host SSD capacity to provision exact-size
origin containers (such as 5 GiB for a 4096 MiB logical swap, releasing ~20 GiB
of host disk space) while preserving complete cryptographic integrity, fixed-extent
allocation performance, and strict backward compatibility with existing 25 GiB
deployments.

For a new origin, the host manager prefers the storage volume containing the
registered WSL distro, provided that the fixed VHDX plus a 10 GiB free-space
reserve fit. It then tries C: under the same bound. If neither fits, it refuses
before writing. Once sealed, the manifest's origin path remains authoritative;
moving the distro does not silently move or replace the origin.

## 2. Technical context

- **`crates/ramshared-wsl2d/src/main.rs`**: Daemon manifest validation
  (`validate_host_origin_manifest_bytes`) explicitly enforces
  `host.fixed_size_bytes != 25 * GIB` (`Confirmed in codebase`).
- **`scripts/safety/ramshared-host-gate.sh`**: Python host gate enforces
  `manifest["fixed_size_bytes"] != 25 * 1024**3` (`Confirmed in codebase`).
- **`scripts/windows/Manage-RamSharedOrigin.ps1`**: Origin provisioning script sets
  `$OriginSize = 25GB`, approval string `RAMSHARED_ORIGIN_25GIB_PARTUUID`,
  and validates `$fixedSize -ne [uint64]$OriginSize` and
  `[uint64]$vhd.Size -ne [uint64]$OriginSize` (`Confirmed in codebase`).
- **`scripts/windows/Test-RamSharedOriginStatic.ps1`**: Static contract test checks
  for `'25GB'` token presence in `Manage-RamSharedOrigin.ps1` (`Confirmed in codebase`).
- **`scripts/safety/test-control-plane-units.sh`**: Control plane unit suite
  includes test fixtures using `25 * 1024**3` (`Confirmed in codebase`).
- **Storage geometry**: Windows `Initialize-Disk -PartitionStyle GPT` allocates a
  mandatory 16 MiB Microsoft Reserved (MSR) partition (partition 1), followed by
  the data/swap partition (partition 2) created via `New-Partition -UseMaximumSize`.
  For a 4096 MiB (4 GiB) swap partition, a 5 GiB fixed VHDX yields ~4.98 GiB for
  partition 2, providing ample headroom for alignment and metadata (`Confirmed in codebase`).
- **Origin placement (pre-slice baseline)**: `Get-WslDistroStorageRoot` read the
  distro's registered WSL `BasePath` volume, but did not check free space or
  fall back to C:. It recomputed the origin path on each invocation, so an
  existing sealed manifest at a different path failed the exact-path comparison
  (`Confirmed in codebase at discovery`). The new policy uses C: when the
  registry does not provide a usable distro `BasePath`; it does not infer the
  distro's volume from the independent WSL fallback swap path.
- **WSL fallback swap**: `wslconfig-lib.sh` preserves an existing `swapFile` and
  otherwise leaves path selection to WSL; this feature does not rewrite that
  independent fallback path (`Confirmed in codebase`).

## 3. Recommended option

### Option 1 (Recommended): Parameter-driven with mathematical headroom bounds
- `Manage-RamSharedOrigin.ps1` accepts an optional `-OriginSizeBytes` parameter
  (defaulting to `5GB`, with valid range `5GB..64GB` in whole GiB steps).
- The required attended approval token dynamically reflects the selected size:
  `RAMSHARED_ORIGIN_${OriginSizeGiB}GIB_PARTUUID`.
- Both Windows and Linux control plane gates validate:
  1. `fixed_size_bytes % (1024^3) == 0` (whole GiB alignment).
  2. `5 GiB <= fixed_size_bytes <= 64 GiB`.
  3. `fixed_size_bytes >= (logical_capacity_mib + 1024) * 1024^2`.
  4. Cryptographic sealing via `configuration_sha256`.

### Discarded alternatives
- **Hardcoding 5 GiB**: Replacing 25 GiB with 5 GiB reproduces the exact same
  architectural inflexibility, preventing users with larger workloads (e.g. 16 GiB
  swap) from provisioning adequate origin capacity.
- **Dynamic VHDX**: Dynamic expansion creates NTFS block allocation latency,
  severe fragmentation, and host out-of-space pause hazards during kernel memory
  pressure spikes. Fixed allocation must remain mandatory.
- **Always use C:** This ignores a valid distro volume and can force origin I/O
  onto a separate device even when the user has one suitable volume. C: remains
  a deterministic fallback.
- **Scan arbitrary volumes:** This can select removable, network, or unrelated
  storage. Only the distro volume and C: participate in automatic selection.

## 4. Functional requirements (RF-N)

- **`RF-1`**: The operator can specify the physical origin container size via
  `-OriginSizeBytes` in `Manage-RamSharedOrigin.ps1` in whole GiB increments
  between 5 GiB and 64 GiB (default: 5 GiB).
- **`RF-2`**: Attended origin provisioning requires an explicit approval token
  corresponding to the selected container size
  (`RAMSHARED_ORIGIN_${OriginSizeGiB}GIB_PARTUUID`).
- **`RF-3`**: The origin container manifest (`ramshared-origin-manifest.json`)
  records the exact `fixed_size_bytes` and incorporates it into the tamper-proof
  `configuration_sha256` hash.
- **`RF-4`**: The host gate script (`ramshared-host-gate.sh`) and the daemon
  (`ramshared-wsl2d`) validate that `fixed_size_bytes` is a whole GiB multiple
  in the range `[5 GiB, 64 GiB]` and satisfies
  `fixed_size_bytes >= (logical_capacity_mib + 1024) * 1024^2`.
- **`RF-5`**: Existing 25 GiB origin containers and manifests remain fully valid
  and accepted without requiring migration or recreation.
- **`RF-6`**: For a new default origin, the manager selects the registered WSL
  distro's volume when it has at least `OriginSizeBytes + 10 GiB` free; otherwise
  it selects C: only when that volume meets the same bound. If the registered
  `BasePath` cannot be resolved, C: is the sole automatic candidate. The manager
  never infers distro placement from the separate WSL fallback swap path. If no
  allowed volume qualifies, it refuses before VHDX creation and reports required
  and observed free bytes. An explicit `-OriginVhdxPath` is honored only when
  its volume has the same reserve.
- **`RF-7`**: When a sealed schema-3 manifest already exists, its validated
  `origin_vhdx` path remains the selected path when the distro's `BasePath`
  changes. A conflicting explicit path is refused; no automatic migration,
  recreation, or cleanup is permitted.

## 5. Non-functional requirements (NFR-N)

- **`NFR-1` (Storage efficiency)**: A 4096 MiB logical swap deployment uses an exact
  5 GiB origin container, freeing ~20 GiB (21.47 GB) on the Windows host SSD.
- **`NFR-2` (Performance & determinism)**: Origin VHDX files remain strictly
  pre-allocated (`-Fixed`), preventing runtime block allocation stalls under swap pressure.
- **`NFR-3` (Cryptographic integrity)**: All capacity and geometry parameters are
  bound into `configuration_sha256`; any external modification fails closed.
- **`NFR-4` (Safety & fail-closed)**: Insufficient container size relative to
  logical swap capacity fails validation immediately before disk mounting or swapoff/swapon.
- **`NFR-5` (Host free space)**: New fixed allocation preserves at least 10 GiB
  free on the selected volume after allocation. The manager checks before
  creation and again after creating the staging VHDX; a lost reserve rolls back
  only the current run's staging file.

## 6. Flows

### Happy flow: 5 GiB origin deployment
1. Operator runs `Manage-RamSharedOrigin.ps1 -Action install -OriginSizeBytes 5GB -LogicalCapacityMiB 4096 -ApproveOriginProvision RAMSHARED_ORIGIN_5GIB_PARTUUID`.
2. Windows initializes 5 GiB fixed VHDX, creates GPT layout (16 MiB MSR + ~4.98 GiB data partition), and seals manifest.
3. Operator runs `Manage-RamSharedOrigin.ps1 -Action attach`.
4. Linux runs `ramshared-host-gate.sh`: verifies 5 GiB fixed size >= 5120 MiB, validates `configuration_sha256`, and writes `/etc/ramshared/origin.conf`.
5. Linux runs `provision-origin-swap.sh`: formats exactly 4096 MiB swap on partition 2.
6. Cascade activates cleanly (`ramshared up`).

### Alternate flow: 25 GiB legacy deployment
1. Operator provisions with `-OriginSizeBytes 25GB` and approval token `RAMSHARED_ORIGIN_25GIB_PARTUUID`.
2. Gate and daemon accept `fixed_size_bytes = 26843545600`.

### Alternate flow: distro volume lacks reserve
1. The registered distro volume has less than `OriginSizeBytes + 10 GiB` free.
2. The manager checks C:; if it meets the same bound, the plan selects C:.
3. Install seals that one selected path, and later actions read it from the
   existing manifest rather than recomputing it from the distro location.

### Error flow: Insufficient physical container capacity
1. Manifest indicates `fixed_size_bytes = 5 * 1024^3` but `logical_capacity_mib = 8192`.
2. Gate and daemon evaluate `fixed_size_bytes < (8192 + 1024) * 1024^2`.
3. Validation fails closed with error `host origin manifest policy mismatch`.

## 7. Data / state model

- Manifest field `fixed_size_bytes`: `uint64` in bytes, multiple of `1024^3`.
- Policy constraints:
  - `fixed_size_bytes >= 5 * 1024^3`
  - `fixed_size_bytes <= 64 * 1024^3`
  - `fixed_size_bytes >= (logical_capacity_mib + 1024) * 1024^2`
  - `fixed_size_bytes % 1024^3 == 0`

## 8. Interfaces

- PowerShell: `Manage-RamSharedOrigin.ps1` with `-OriginSizeBytes` and optional
  explicit `-OriginVhdxPath`; default placement is distro volume, then C:.
- Shell: `ramshared-host-gate.sh` manifest parser.
- Rust: `validate_host_origin_manifest_bytes` in `crates/ramshared-wsl2d/src/main.rs`.

## 9. Dependencies and risks

- **Risk**: User specifies container smaller than MSR + swap partition.
  **Mitigation**: Enforce mathematical floor `(logical_capacity_mib + 1024) * 1024^2`.
- **Risk**: Free space changes between plan and fixed allocation.
  **Mitigation**: Check the exact selected volume immediately before creation and
  after staging allocation; never retry a deterministic low-space refusal.
- **Numeric rollback trigger**: A failed identity/hash check, or less than 10 GiB
  free after staging allocation, aborts and removes only the current-run staging
  VHDX. Any existing sealed VHDX or manifest remains untouched.

## 10. Implementation strategy

1. Define PRD and SPEC with formal technical decisions.
2. Complete Step 2.5 safety audit.
3. Implement TDD: add unit tests for size boundaries, volume priority/reserve,
   sealed-path replay, and fail-closed refusal.
4. Preserve the existing daemon and gate capacity contract; change only host
   provisioning path selection and pre-allocation checks.
5. Run PowerShell and docs checks. Host-path creation remains attended and
   environment-bound; do not recreate a sealed origin for this slice.

## 11. Documents to update

- `docs/INDEX.md` (via `node tools/generate-docs-index.mjs`)
- `docs/specs/no-milestone/wsl2-origin-capacity-policy/{PRD,SPEC,AUDIT-2.5,IMPL}.md`

## 12. Out of scope

- Dynamic VHDX conversion or runtime compacting.
- Altering the WSL2 root disk or fallback swap device / `.wslconfig` path.
- Live in-place resizing of an attached origin VHDX without re-provisioning.
- Automatically relocating, resizing, deleting, or replacing any sealed origin.

## 13. Acceptance criteria

- `Manage-RamSharedOrigin.ps1` successfully creates and validates a 5 GiB fixed VHDX.
- Plan selection prefers the distro volume with sufficient reserve, falls back
  to C: with sufficient reserve, accepts a single C: volume when the distro is
  stored there, and refuses if no allowed volume qualifies; existing manifest
  paths remain stable after the distro volume changes.
- `ramshared-host-gate.sh` and `ramshared-wsl2d` accept 5 GiB and 25 GiB manifests, and reject <5 GiB or under-sized manifests.
- Slice coverage on modified code >= 80%.
- `./scripts/docs-check.sh` passes with zero findings.

## 14. Validation plan

- Unit: Rust unit tests in `ramshared-wsl2d` covering multi-capacity acceptance and boundary refusals.
- Shell: `scripts/safety/test-control-plane-units.sh` verifying gate validation.
- Live: End-to-end recreation of the origin VHDX with 5 GiB and verified cascade activation (`ramshared up`).
