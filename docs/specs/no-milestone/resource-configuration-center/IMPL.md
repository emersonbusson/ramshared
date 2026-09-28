# IMPL — Cross-platform resource configuration

> SSDV3 Step 3 · SPEC:
> `docs/specs/no-milestone/resource-configuration-center/SPEC.md`

## Status

**partial** · read-only discovery and display are implemented in the shared
CLI. A v1 typed profile model validates variable ceilings and stable storage
targets, but the CLI does not yet load or persist it. No settings are applied,
and native Linux live qualification has not been run in this WSL2 environment.

## Delivered contract

- `ramshared config` opens a read-only terminal view; `config show` prints a
  human-readable inventory and `config show --json` prints a typed snapshot.
  Unsupported mutation actions fail during parsing before resource discovery.
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
  remain visible with a refusal reason.
- Under WSL2, the view labels guest RAM separately from Windows host physical
  memory and commit headroom, and lists fixed Windows volumes with their
  current free/total bytes and stable volume IDs in JSON. Native Linux never
  queries a Windows host provider. A WSL guest filesystem is never offered as
  a write target until its exact backing Windows volume and host free capacity
  are bound to that guest filesystem; guest VHDX free space alone is not proof.
- The view explicitly says disk speed was not measured and GPU/VRAM budgets
  were not sampled. It opens no GPU context and does not modify swap, an
  origin, a profile, `.wslconfig`, a driver, or a running RamShared tier.
- `ramshared-config::resource_profile` parses a 64 KiB-bounded TOML profile
  with schema version 1, variable byte ceilings, adapter-bound GPU caps, and
  multiple platform-bound storage targets. A profile can represent swap and
  origin placements on the same or different stable volumes. Validation
  rejects schema or platform mismatch, unknown fields, unsafe or duplicate
  managed paths, unbound identities, and invalid allocation metadata. It
  computes a checked free-space requirement per stable volume, summing every
  managed allocation and adding the SPEC's 10 GiB reserve once per volume; it
  does not inspect live volume free space or authorize writes. This floor is
  not a RAM, swap, or VRAM minimum. The profile is not wired to CLI/storage
  providers yet.

## Files

| Path | Change |
| --- | --- |
| `crates/ramshared-cli/src/main.rs` | `config` parsing, dispatch, and help text; only interactive and read-only `show [--json]` modes are accepted. |
| `crates/ramshared-cli/src/resource_config.rs` | Platform detection, dynamic memory/swap snapshots, bounded Linux and Windows inventory, exact mount identity, local transport refusal, partition/whole-disk eligibility, JSON/text rendering, and read-only TUI. |
| `crates/ramshared-cli/tests/cli_dispatch.rs` | Executes the built CLI for JSON discovery and confirms mutation commands refuse before action. |
| `crates/ramshared-config/src/resource_profile.rs` | Versioned, bounded TOML policy model; platform-bound storage targets; variable tier caps; checked disk-capacity arithmetic. |
| `crates/ramshared-config/tests/resource_profile.rs` | Red/green tests for variable caps, overflow, profile round trips, platform mismatch, malformed identity, and path refusal. |
| `docs/specs/no-milestone/resource-configuration-center/SPEC.md` | Names implemented profile and inventory tests while retaining the full configuration contract as incomplete. |

## Validation

- RED/GREEN: `mounted_whole_disk_filesystem_uses_its_own_stable_identity`
  failed against the partition-only eligibility rule and passed after whole
  disks gained their own stable identity. Later,
  `network_backed_block_devices_are_ineligible_and_multiple_local_disks_remain_eligible`
  failed on an iSCSI fixture and passed after network and unproven transports
  were rejected; the test confirms independent NVMe and SATA candidates can
  both be marked eligible. It does not implement user selection.
- Full CLI tests: `cargo test -j 1 -p ramshared-cli` passed 379 unit tests and
  11 CLI integration tests.
- Profile tests: `cargo test -j 1 -p ramshared-config` passed 15 unit and 6
  integration tests. The initial integration-test run failed to compile
  because `resource_profile` did not exist; it passed after the typed module
  was added. The named profile slice gate passed at **95.5% (169/177 lines)**.
- CLI E2E: `cli_resource_config_json_discovers_platform_resources_read_only`
  executes the built binary under the current WSL2 kernel, parses its JSON,
  checks platform and resource fields, and verifies `config apply` refuses.
- Static checks: `cargo clippy -j 1 -p ramshared-cli --all-targets -- -D warnings`
  passed. `cargo fmt --all -- --check`, `git diff --check`, and
  `./scripts/docs-check.sh` passed after the final source and documentation
  updates.
- Slice coverage: `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-cli
  --files crates/ramshared-cli/src/resource_config.rs --min 80` passed at
  **88.3% (1,131/1,281 lines)** after network and unclassified-transport
  refusals were added.
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
- The test fixture `meminfo_accepts_user_sized_ram_and_swap_without_product_minima`
  accepts values from 256 MiB RAM / 128 MiB swap through 48 GiB RAM / 20 GiB
  swap. They are parser fixtures, not fixed product limits. The installed
  `/usr/local/bin/ramshared` still reports v0.14.1; this v0.15.0 source target
  has not been installed. No `ramsharedd` process was present, and the only
  active swap was the 4 GiB WSL fallback device.

## Gaps

- The UI cannot select or persist a swap volume, configure variable ZRAM/
  VRAM/origin ceilings, create a native Linux swapfile or file origin, stage
  WSL fallback swap settings, benchmark disks, or recommend a speed leader.
- The WSL memory ceiling stays read-only by SPEC. The typed profile contains
  no host-RAM setting; only ZRAM, per-adapter VRAM, origin, and platform-owned
  fallback swap targets are in its scope.
- The typed profile has no CLI loader or persistence yet. There is no
  privileged Linux helper, Windows configuration helper, transaction log, or
  apply/rollback flow.
- The profile now represents multiple swap/origin targets and groups checked
  capacity by stable volume identity. It still does not let a user select or
  save those targets from the CLI, bind a live candidate to a saved target, or
  apply the settings.
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
| RF-1..RF-2, RF-5, RF-13 | ITEM-1..ITEM-3 | Read-only discovery implemented; native live E2E remains open. |
| RF-3, RF-5, RF-7..RF-8, RF-12 | ITEM-1 | Typed schema and validation only; no profile loading, live capacity admission, or configuration write is exposed. |
| RF-3..RF-4, RF-6..RF-12 | ITEM-4..ITEM-8 | Provider integration, selection, mutation, benchmark, and live qualification remain incomplete. |
