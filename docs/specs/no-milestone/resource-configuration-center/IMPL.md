# IMPL — Cross-platform resource configuration

> SSDV3 Step 3 · SPEC:
> `docs/specs/no-milestone/resource-configuration-center/SPEC.md`

## Status

**partial** · read-only discovery, a v1 typed multi-target profile, and
`ramshared config plan` are implemented for native Linux and WSL2. The command
loads the root-owned default profile or an explicit bounded draft, binds its
storage targets to fresh inventory, and reports capacity/refusal reasons. It
does not persist profiles, offer target selection, benchmark disks, or apply
settings. Native Linux target qualification has not run on a native host.

## Delivered contract

- `ramshared config` opens a read-only terminal view; `config show` prints a
  human-readable inventory and `config show --json` prints a typed snapshot.
  `config plan [--json] [--profile PATH]` loads the root-owned
  `/etc/ramshared/resource-profile.toml` when present, or a user-supplied
  bounded draft. A missing default profile is reported as `not_configured`; an
  explicitly requested missing profile fails. Unsupported mutation actions
  fail during parsing before resource discovery.
- Linux reads RAM and swap counters from the active guest and enumerates block
  devices with `lsblk`. It joins devices to `/proc/self/mountinfo` by
  `MAJ:MIN` and samples filesystem total/free capacity through `statvfs`.
- A Linux filesystem is only marked as a storage candidate when it is a
  writable ext4/XFS filesystem on a partition or directly on a whole disk,
  with an exact matching mount record, non-removable status, filesystem UUID,
  stable backing identity, and measured capacity. Partitions use their
  parent's WWN/serial; whole-disk filesystems use the disk's own WWN/serial.
  Known network-backed transports and missing/unrecognized transport identity,
  removable/USB, read-only, unmounted, unsupported, or ambiguous candidates
  remain visible with a refusal reason. Bind/subtree mounts whose mountinfo
  root is not `/` also remain visible but are ineligible.
- A native Linux profile stores filesystem UUID, stable backing-device
  identity, and managed relative path; it does not store the ephemeral mount
  ID. A plan resolves exactly one current filesystem-root mount, checks its
  live mount ID and device number, writable eligibility, and fresh `statvfs`
  capacity, then reports that current mount ID in the plan. Multiple matching
  mounts refuse as ambiguous. It does not create a file, activate swap, select
  a disk, or claim native-host qualification.
- Under WSL2, the view labels guest RAM separately from Windows host physical
  memory and commit headroom, and lists fixed Windows volumes with their
  current free/total bytes and stable volume IDs in JSON. Native Linux never
  queries a Windows host provider. A WSL guest filesystem is never offered as
  a write target until its exact backing Windows volume and host free capacity
  are bound to that guest filesystem; guest VHDX free space alone is not proof.
- A WSL2 plan binds a configured target to one fresh Windows volume identity,
  confirms the drive-letter or volume-GUID path resolves under that volume,
  checks fixed NTFS/ReFS eligibility and free capacity, and rejects aliases
  where two profile entries resolve to one volume-relative path. Unknown,
  stale, ambiguous, mismatched, or inconsistent samples are refusals.
- The view explicitly says disk speed was not measured and GPU/VRAM budgets
  were not sampled. It opens no GPU context and does not modify swap, an
  origin, a profile, `.wslconfig`, a driver, or a running RamShared tier.
- Plan output states `writes_performed=false` and `apply_enabled=false` in
  both text/JSON forms. Caps are displayed as ceilings only; the plan does
  not authorize them against live GPU, ZRAM, or origin budgets.
- `ramshared-config::resource_profile` parses a 64 KiB-bounded TOML profile
  with schema version 1, variable byte ceilings, adapter-bound GPU caps, and
  multiple platform-bound storage targets. A profile can represent swap and
  origin placements on the same or different stable volumes. Validation
  rejects schema or platform mismatch, unknown fields, unsafe or duplicate
  managed paths, unbound identities, and invalid allocation metadata. It
  computes a checked free-space requirement per stable volume, summing every
  managed allocation and adding the SPEC's 10 GiB reserve once per volume; it
  does not inspect live volume free space or authorize writes. Windows paths
  reject ambiguous components, alternate data streams, reserved device names,
  and malformed volume GUIDs. The loader rejects symlinks and oversized or
  non-regular inputs; the system profile must be root-owned, single-link, mode
  0600, under a root-owned non-writable directory. This floor is not a RAM,
  swap, or VRAM minimum. No profile save or storage mutation provider exists.

## Files

| Path | Change |
| --- | --- |
| `crates/ramshared-cli/src/main.rs` | `config` parsing, dispatch, help text, and read-only `plan [--json] [--profile PATH]`; unsupported mutation actions remain rejected. |
| `crates/ramshared-cli/src/resource_config.rs` | Platform detection, memory/swap snapshots, bounded Linux and Windows inventory, read-only profile loading/planning, stable target binding, stale/inconsistent sample refusals, path alias detection, JSON/text rendering, and read-only TUI. |
| `crates/ramshared-cli/tests/cli_dispatch.rs` | Executes the built CLI for JSON discovery and explicit-profile plan; confirms plan does not apply settings and mutation commands refuse before action. |
| `crates/ramshared-config/src/resource_profile.rs` | Versioned, bounded TOML policy model; platform-bound storage targets; variable tier caps; checked capacity arithmetic; canonical Windows target paths. |
| `crates/ramshared-config/tests/resource_profile.rs` | Tests variable caps, overflow, profile round trips, platform mismatch, malformed identity, duplicate targets, and Windows path refusal. |
| `docs/specs/no-milestone/resource-configuration-center/SPEC.md` | Names implemented profile and inventory tests while retaining the full configuration contract as incomplete. |

## Validation

- RED/GREEN: `mounted_whole_disk_filesystem_uses_its_own_stable_identity`
  failed against the partition-only eligibility rule and passed after whole
  disks gained their own stable identity. Later,
  `network_backed_block_devices_are_ineligible_and_multiple_local_disks_remain_eligible`
  failed on an iSCSI fixture and passed after network and unproven transports
  were rejected; the test confirms independent NVMe and SATA candidates can
  both be marked eligible. It does not implement user selection.
- RED/GREEN: `resource_plan_rejects_drive_and_volume_guid_aliases_for_same_target`
  first exposed that the plan accepted two spellings of the same volume-relative
  file as `storage_ready`; the planner now refuses the second entry. The CLI
  parser regression also reproduced `--profile --json` being accepted as a
  profile filename; an option-looking value now fails before discovery.
- RED/GREEN: `native_linux_profile_survives_a_new_mount_namespace_id` first
  received `identity_unavailable` when the current mount ID changed from 41 to
  990. The profile no longer persists a mount ID; the planner now binds the
  fresh unique mount and reports its ephemeral ID. The new
  `resource_profile_rejects_transient_mount_id_in_persisted_targets` test
  refuses the obsolete field. `storage_candidate_rejects_filesystem_subtree_mounts`
  also reproduced the prior acceptance and now verifies a refusal.
- The full CLI suite under the coverage gate passed 389 unit tests and
  12 CLI integration tests, including
  `cli_resource_config_plan_loads_an_explicit_profile_without_applying_it`.
- Profile tests: `cargo test -j 1 -p ramshared-config` passed 15 unit and 10
  profile integration tests, including malformed/ambiguous Windows target
  paths and refusal of persisted mount IDs. The profile slice gate passed at
  **91.3% (293/321 lines)**.
- CLI E2E: `cli_resource_config_json_discovers_platform_resources_read_only`
  executes the built binary under the current WSL2 kernel, parses its JSON,
  checks platform and resource fields, and verifies `config apply` refuses.
- CLI plan E2E: `cli_resource_config_plan_loads_an_explicit_profile_without_applying_it`
  loads a temporary user-readable caps-only draft and verifies no write or
  apply permission is reported. It does not prove a live storage-target plan;
  the target identity/capacity paths are currently covered by unit fixtures.
- Static checks: `cargo clippy -j 1 -p ramshared-cli -p ramshared-config
  --all-targets --all-features -- -D warnings` passed. `cargo fmt --all -- --check`,
  `git diff --check`, and the full `./scripts/docs-check.sh` passed after the
  current source and SPEC/IMPL updates.
- Slice coverage: `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-cli
  --files crates/ramshared-cli/src/resource_config.rs --min 80` passed at
  **87.6% (1,793/2,046 lines)** for discovery and read-only planning.
- PowerShell 5.1 manufactured/static harnesses passed for
  `Test-WindowsStorageMatrixStatic.ps1`,
  `Test-RamSharedWslLifecycleRecoveryStatic.ps1`,
  `Test-HostAutonomousLifecycleStatic.ps1`, and
  `Test-RamSharedOriginStatic.ps1`. `-ExecutionPolicy Bypass` applied only to
  each test process; the Windows execution policy was not changed.
- Direct source tests for active gates passed: GPU budget 13/13, host-gate
  policy 14/14, and swapoff-first legacy migration 1/1. These prove helper
  logic only; they do not prove a live transport handshake or installed
  lifecycle.
- `mount_capacity_uses_live_available_blocks_without_writing` samples the
  current filesystem read-only. This Linux environment runs WSL2; it does not
  substitute for a native Linux machine E2E.
- A fresh `config show --json` sample at 2026-09-28 03:38 UTC identified the
  WSL root as ext4 directly on a whole virtual disk with about 1,007 GiB total
  and 834 GiB free. It is now correctly marked ineligible because the exact
  Windows volume backing its VHDX and that host volume's capacity are not
  bound to the guest filesystem. The host separately reported five fixed
  Windows volumes; labels and IDs are omitted here. No C:/I: ranking was made
  because no disk benchmark ran.
- At that sample the guest had about 8.0 GiB `MemAvailable`, 2.35 GiB
  `SwapFree`, and zero PSI. Windows physical headroom was about 1,682 MiB;
  a later 03:52 UTC sample reported 1,778 MiB and three PowerShell processes
  totaling 231 MiB private memory (largest 115 MiB). This does not reproduce
  the previously observed 11.5 GiB PowerShell process and does not identify
  its cause.
- At the recorded 03:38 UTC sample, the fixture
  `meminfo_accepts_user_sized_ram_and_swap_without_product_minima`
  accepts values from 256 MiB RAM / 128 MiB swap through 48 GiB RAM / 20 GiB
  swap. They are parser fixtures, not fixed product limits. At that sample,
  `/usr/local/bin/ramshared` reported v0.14.1; this v0.15.0 source target had
  not been installed. No `ramsharedd` process was present, and the only active
  swap was the 4 GiB WSL fallback device.

## Gaps

- The TUI cannot select or persist a swap volume, edit tier ceilings, create a
  native Linux swapfile/file origin, stage WSL fallback swap settings,
  benchmark disks, or recommend a speed leader. `config plan` validates only
  targets already present in a profile file; it does not save the draft.
- The WSL memory ceiling stays read-only by SPEC. The typed profile contains
  no host-RAM setting; only ZRAM, per-adapter VRAM, origin, and platform-owned
  fallback swap targets are in its scope.
- The typed profile loader and read-only plan are wired to live inventory.
  There is no profile writer, privileged Linux helper, Windows configuration
  helper, transaction log, or apply/rollback flow.
- The profile represents multiple swap/origin targets and the plan binds each
  configured target to its current volume/mount identity. It still does not
  let a user select candidates in the TUI or save profile changes.
- Native Linux and WSL2 mutation flows, per-filesystem allocation behavior,
  GPU adapter selection, and storage benchmark behavior remain unqualified.
- The active reliability PARTIAL gates listed in `docs/reliability/GAP-REGISTER.md`
  are unaffected by this read-only configuration slice.

## Rollback trigger

Revert or disable storage candidacy if any writable ext4/XFS filesystem is
accepted without a matching mount/device number, fresh capacity, filesystem
UUID, stable backing identity, or verified non-removable status, or if the view
changes active swap, an origin, GPU allocation, or platform configuration.

## Traceability

| RF | ITEM | Status |
| --- | --- | --- |
| RF-1..RF-2, RF-5, RF-13 | ITEM-1..ITEM-3 | Read-only discovery and target plan implemented; native live E2E remains open. |
| RF-3, RF-5, RF-7..RF-8, RF-12 | ITEM-1 | Typed schema, bounded loading, stable identity/capacity plan; no profile persistence or configuration write is exposed. |
| RF-3..RF-4, RF-6..RF-12 | ITEM-4..ITEM-8 | Provider integration, selection, mutation, benchmark, and live qualification remain incomplete. |
