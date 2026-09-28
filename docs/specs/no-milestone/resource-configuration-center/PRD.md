---
slug: resource-configuration-center
title: Guided and auditable RamShared resource configuration
milestone: —
issues: []
---

# PRD — Guided and auditable RamShared resource configuration

## 1. Summary

Provide one cross-platform `ramshared config` interface for native Linux and
WSL2. It discovers resources, explains safe capacity, lets the user choose
resource ceilings and storage locations, and records every proposal,
benchmark, apply, and refusal. On native Linux, it distinguishes system RAM,
Linux swap devices/files, RamShared's SSD origin, ZRAM, and each usable GPU
adapter, and can choose an eligible local filesystem for a managed swapfile
and a new file-backed RamShared origin. On WSL2, it distinguishes Windows
host memory, the WSL VM, the WSL fallback swap VHDX, the RamShared SSD origin,
ZRAM, and each usable GPU adapter; the Windows provider lists local Windows
volumes and owns WSL host settings. Both providers can run an attended bounded
speed comparison and recommend from fresh measurements. User settings are
ceilings; live platform, guest, disk, and GPU admission checks remain
authoritative.

## 2. Technical context

- **Confirmed in codebase:** The test `meminfo_missing_or_inconsistent_core_values_are_unavailable` in `crates/ramshared-cli/src/monitor.rs` checks missing fields and relational bounds. Its KiB values are parser fixtures, not runtime capacities, configuration defaults, or allocation ceilings; coverage is being extended across different valid RAM/swap sizes.
- **Confirmed in codebase:** `scripts/safety/wslconfig-lib.sh` currently renders defaults of a 16 GiB WSL memory ceiling and 4 GiB fallback swap. These are current script defaults, not parser limits or user-independent capacity requirements; environment variables can override them. `WSLCONFIG_SWAPFILE` preserves an existing path and does not discover all available volumes.
- **Confirmed in codebase:** `scripts/safety/wslconfig-ctl.sh apply` rewrites the canonical profile and tells the operator to restart WSL later. It does not expose an interactive resource selector.
- **Confirmed in codebase:** `scripts/windows/Manage-RamSharedOrigin.ps1` accepts an origin size/path, selects the registered distro volume or C: for the automatic new-origin path, enforces reserve checks, and preserves a sealed origin path. The WSL fallback `swapFile` is a separate setting and is not used to infer the origin volume.
- **Confirmed in codebase:** `crates/ramshared-wsl2d/src/gpu_budget.rs` has fresh adapter-bound allocator/WDDM admission and fixed host-display/runtime reserves. Existing target selection is dynamic; this PRD does not authorize allocation above that live safe target.
- **Confirmed in codebase:** `crates/ramshared-cli/src/monitor.rs` already uses Ratatui for `ramshared top`; the custom CLI parser in `crates/ramshared-cli/src/main.rs` has no `config` command. `crates/ramshared-config` currently describes broker and agent TOML, not end-user hardware policy.
- **Confirmed in codebase:** The Linux cascade creates its managed ZRAM and origin-backed swap devices through `crates/ramshared-cli/src/cascade/cascade_io.rs`; the current lifecycle does not expose a general system swapfile volume selector. Discovery must not mistake RamShared's logical SSD-backed swap device for a conventional Linux swapfile.
- **Confirmed in codebase:** `crates/ramshared-block/src/origin_cache.rs` provides `FileOrigin`, but `crates/ramshared-wsl2d/src/main.rs::open_validated_origin` currently accepts only a block device sealed by PARTUUID/PTUUID/swap UUID. Native file-backed origin selection therefore needs its own identity-sealed manifest path; an arbitrary filename cannot be passed to the existing WSL origin path.
- **Confirmed in codebase:** `docs/specs/no-milestone/wsl2-origin-capacity-policy/` covers origin size, volume reserve, and sealed-path behavior. It expressly leaves the independent WSL fallback swap path outside its scope.
- **Confirmed in docs:** `docs/BENCHMARKS.md` records one historical sync-write comparison: C: 85.4 MB/s and I: 38.0 MB/s for the tested devices/workload. This is useful historical context, not a current recommendation for a later machine or run.
- **Confirmed in official documentation:** Microsoft describes `.wslconfig` as global across WSL2 distributions, applies settings at VM start, and notes a WSL shutdown may be required before changes take effect. The config screen must disclose that scope and never shut WSL down automatically. [Microsoft WSL configuration](https://learn.microsoft.com/windows/wsl/wsl-config).
- **Confirmed in official documentation:** Linux swapfiles must satisfy filesystem-specific allocation constraints; `swapon(8)` rejects files with holes and documents XFS/Btrfs qualifications. The initial Linux provider will allow only tested ext4/XFS cases and show other filesystems as ineligible until separately qualified. [swapon(8)](https://man7.org/linux/man-pages/man8/swapon.8.html).
- **Confirmed in official documentation:** A systemd `.swap` unit is a privileged system-manager unit naming a specific swap file/device. Native persistence can therefore use an app-owned unit instead of editing `/etc/fstab`. [systemd.swap(5)](https://man7.org/linux/man-pages/man5/systemd.swap.5.html).
- **Inference:** A unified screen can reduce user mistakes only if it displays host and guest measurements as separate quantities and refuses to apply a value when its owning layer cannot verify the relevant resource.

## 3. Recommended option

Add a guided `ramshared config` TUI to the existing CLI and a typed,
platform-neutral resource profile/policy layer with Linux-native and WSL2 host
providers. Keep platform actions in their owning components:

- On native Linux, a bounded privileged provider discovers block devices,
  mounted filesystems, swap devices/files, and free space; it can create
  separate app-owned files for OS fallback swap and RamShared's durable origin
  on an eligible mounted local filesystem. It stages a systemd swap unit and
  a distinct sealed native-origin manifest. It never partitions, formats, or
  edits `/etc/fstab`.
- On WSL2, a bounded Windows helper discovers Windows volumes and host
  measurements, benchmarks eligible volumes after user consent, and stages
  only the `.wslconfig` swap settings explicitly selected by the user.
- Existing WSL origin management remains the owner of VHDX creation and
  sealed block identity. Native Linux adds a separate file-origin owner that
  reuses `FileOrigin` but seals filesystem/device/file identity before use.
  Neither path relocates or overwrites a sealed origin.
- The shared profile owns per-tier user ceilings. Runtime policy continues to
  clamp ZRAM and VRAM admission against live platform, guest, adapter, and GPU
  budget observations.

Do not add a second allocator, replace an existing origin manifest, select a
volume by drive letter or transient kernel-assigned device name, or use historical
benchmark data as live evidence. This slice does not edit the WSL memory
ceiling or native Linux host memory policy: the WSL ceiling is shown as a
maximum, while actual memory use and live admission remain separate. If a
platform provider cannot prove the selected resource identity or safe bound,
the UI can inspect and export a plan but cannot apply it.

## 4. Functional requirements

- **RF-1 — Discover:** `ramshared config` reports platform identity and the
  measurements available from its provider: physical/commit state where
  supported; WSL maximum versus current guest `MemTotal`/`MemAvailable`;
  Linux `MemTotal`/`MemAvailable`; every swap device/file; ZRAM; the RamShared
  origin; GPU adapters and live safe budgets; and every discovered local
  volume or mounted filesystem. Each value includes source, sample time/age,
  and `available`, `stale`, `unsupported`, or `unknown` state.
- **RF-2 — Explain:** Distinguish host memory, current guest memory, configured
  WSL maximum, logical swap capacity, current swap use, physical disk use,
  ZRAM, VRAM budget, and RamShared origin. Label conventional OS swap
  separately from the RamShared SSD origin. Never present these quantities as
  interchangeable or additive RAM.
- **RF-3 — Configure:** Allow per-tier ceilings for ZRAM, each selected VRAM
  adapter, and RamShared's SSD origin. Allow the platform provider to configure
  fallback swap: a managed Linux swapfile on native Linux or WSL fallback swap
  size/path on WSL2. Native Linux may select an eligible mounted filesystem
  for a new app-owned file origin; WSL2 origin placement remains owned by the
  Windows VHDX manager. Values support `automatic`, disabled when safe, or an
  explicit target; explicit values are ceilings, not reservations or promises
  of allocation. Selecting zero means “create no new managed target” and
  never removes an existing swapfile or origin. A WSL request to disable its
  configured fallback swap is allowed only when every affected distribution
  has a verified persistent non-RamShared swap alternative; otherwise it is
  refused. The feature does not set a WSL memory ceiling or reserve physical
  RAM.
- **RF-4 — GPU safety:** List every detected adapter and its stable identity,
  driver-reported budget, WDDM budget, current use, mandatory host/display
  reserve, runtime headroom, and safe target. Manual VRAM caps may lower the
  safe target but can never raise it or bypass fresh same-adapter admission.
  Ambiguous adapter identity disables manual selection.
- **RF-5 — Volume choice:** List every discovered Windows local volume and,
  on Linux, every discovered local block device plus its mounted local
  filesystems. Group each filesystem under its parent device and show the
  display letter/mount path, stable volume/filesystem/device identity,
  filesystem type, total/free bytes, and eligibility. A device without an
  eligible mounted filesystem remains visible but cannot be selected for a
  write; the feature never mounts it. Permit selection only among
  platform-eligible candidates for that platform's fallback swap and origin.
  Show every ineligible candidate and the reason. Work with one or many
  volumes without assuming a particular drive letter or transient device name.
- **RF-6 — Speed recommendation:** During the configuration flow, offer an
  automatic, bounded comparison of eligible volumes after the user approves
  its total maximum bytes written and expected duration. Compare the same
  durable workload on each eligible volume in a paired/rotated order; report
  raw per-sample durable small-write latency and sequential throughput,
  median/p99/deviation for small-write latency, median/deviation and all three
  raw values for sequential throughput, test time, bytes written, integrity
  result, and cleanup result. Recommend a measured leader only for the
  selected use case when the median of both metrics favors it by at least 10%;
  otherwise report a tie and let the user choose. Never claim one disk is
  universally fastest or run this test in the background/on startup.
- **RF-7 — Disk safety:** Refuse any proposed swap/origin size that does not
  fit the selected volume while preserving that platform's documented reserve
  floor. If swap and origin share a volume, use checked arithmetic for both
  maximum allocations and the reserve. A benchmark runs separately and must
  clean its exact temporary files before apply; uncertain cleanup blocks apply.
  Require RamShared Off before creating, activating, moving, resizing, or
  disabling a disk tier. Recheck stable volume and filesystem identity, mount
  state, available bytes, path ownership, and pressure immediately before each
  write. Never
  write to a removable, network, unmounted, read-only, ambiguous, or
  identity-changed target. Do not delete, truncate, or move user files.
- **RF-8 — Origin safety:** Display OS fallback swap and RamShared SSD origin
  as distinct resources. For a new WSL2 origin, use the existing Windows
  manager's size limits, approval, fixed-allocation, reserve, and sealed
  manifest flow. For a new native Linux origin, create a fixed-allocated file
  only inside an app-owned directory on the selected eligible mount and seal
  filesystem UUID, parent-device identity, mount identity, inode, exact path,
  and size before daemon use. If a sealed origin exists, disable path/size
  changes that replace or move it and direct the user to the existing
  migration/recovery process.
- **RF-9 — Safe resource ceilings:** Show existing host/guest memory limits
  and current availability as distinct measurements. Do not propose a new WSL
  memory ceiling. Every configurable tier cap is passed to its existing
  live-admission owner and can never raise that owner's fresh safe target. If
  that owner cannot validate the target, preserve the current profile and
  withhold a capacity increase.
- **RF-10 — Review/apply:** Before applying, display a before/after plan,
  affected WSL distributions, delayed-activation requirements, disk/GPU
  reserves, files touched, and any elevated operation. Require an explicit
  confirmation for writes. The configuration flow cannot change live tier
  state, launch a stress campaign, install a driver, or stop WSL.
- **RF-11 — Audit:** Record every discovery, benchmark start/result/refusal,
  selected profile, old/new values, safety margins, target volume/adapter
  identities, operation ID, exact config hashes, apply result, pending-restart
  state, and rollback result. Keep logs local, append-only, permission
  restricted, and free of secrets or unrelated process command lines.
- **RF-12 — Replay:** Repeating an unchanged apply creates no duplicate
  storage, VHDX, or resource activation; it reports `already-current` and
  records the request outcome.
- **RF-13 — Platform refusal:** On native Linux or WSL2, expose only
  capabilities that the active provider can identify and safely apply. A
  missing Windows host helper disables WSL host controls; missing systemd,
  unsupported filesystem semantics, or absent stable Linux volume identity
  disables the corresponding native Linux mutation. Discovery remains
  available with explicit reasons.

## 5. Non-functional requirements

- **NFR-1 (Fail closed):** Unknown, ambiguous, stale, malformed, future-dated,
  or internally inconsistent telemetry cannot authorize a capacity increase,
  disk recommendation, or write.
- **NFR-2 (Memory and tier safety):** Do not derive a new host-memory formula
  in this feature. Reuse existing platform and runtime admission policies; a
  configured cap may lower, never raise, the current safe target. Freshness,
  missing-input, overflow, and inconsistent-counter checks fail closed. No
  host RAM or VRAM is preallocated by configuration.
- **NFR-3 (Volume reserve):** For each target volume require a non-overridable
  `R_volume = 10 GiB` free after new managed allocations, matching the current
  WSL origin manager's floor. This is a conservative free-space reserve, not a
  detected capacity or target-size default. Before apply, checked-add every new
  fixed-size target on that volume (OS swapfile + RamShared origin + other
  managed files) and the reserve; refuse rather than silently shrink. A
  benchmark is a separate operation and requires `96 MiB + R_volume` free; it
  must clean its exact test file before any later apply.
- **NFR-4 (Bounded speed test):** Each sample writes at most 32 MiB of payload:
  31 MiB sequentially with a 1 MiB buffer and one durable flush, then 256
  distinct 4 KiB writes with a durable flush after each. Linux uses
  `fdatasync`; Windows uses `FlushFileBuffers`. Read back and verify the
  complete 32 MiB. Three samples per volume cap writes at 96 MiB; one run tests
  at most eight volumes (768 MiB total). Larger inventories require a selected
  batch and fresh preview. Report p99 across the 768 small-write latency
  observations per volume and raw/median/deviation for the three sequential
  throughput samples. Delete only exact files owned by that operation. Use
  one sequential child worker, one outstanding filesystem operation, and a
  120-second per-volume deadline. Abort before the next sample if memory,
  swap, filesystem, identity, or storage-pressure gates fail. On timeout,
  request worker termination and verify its process identity has exited. If
  exit or cleanup cannot be proven, persist `worker_stuck`, do not launch a
  second worker, and block disk writes to that target until attended recovery
  proves exit and exact cleanup. A deadline cannot cancel an uninterruptible
  kernel/filesystem call.
- **NFR-5 (No background pressure):** The speed test runs only after an
  explicit preview and confirmation from an open configuration session. It
  requires fresh platform/guest samples, no active RamShared tier, adequate
  reserve, and no detected storage or memory pressure. A failed gate performs
  no writes. The config feature does not start stress tests.
- **NFR-6 (Least privilege):** Discovery and planning are read-only. Native
  Linux writes use a narrowly privileged closed-action helper with bounded
  typed input. WSL host settings use a separate closed-action helper in the
  current user's Windows context; VHDX mutations remain delegated to the
  existing origin manager. Never elevate the full CLI or keep a privileged
  provider resident.
- **NFR-7 (Usability):** Use GiB/GB consistently, show bytes in detail view,
  distinguish `not detected` from `0`, and explain every disabled choice in
  plain language. Support noninteractive `show` and `plan` output for scripts.
- **NFR-8 (Configuration privacy):** Store the profile and platform-local
  event logs with owner-only read/write permissions. Each operation carries a
  shared transaction ID when host and guest are both involved. No telemetry
  leaves the machine.

## 6. Flows

### Happy flow — native Linux swapfile and RamShared origin on another SSD

1. User runs `ramshared config`; the Linux provider discovers block-device
   topology, mounted filesystems, active swaps, GPU adapters, and RamShared
   state without writing.
2. The screen shows every discovered local block device and mounted filesystem,
   grouped by parent device, with stable identity, filesystem UUID, mount path,
   filesystem type, free bytes, and an eligibility reason. Unmounted devices
   are visible but not writable selections; transient device names are for
   display only.
3. The user selects an eligible SSD mount for the managed Linux swapfile and
   optionally a different eligible mount for a new RamShared origin. The UI
   offers the bounded volume comparison after showing total write bytes,
   candidates, and duration.
4. The provider tests equal workloads, records integrity and cleanup, and
   recommends by the disclosed per-workload measurements; a tie leaves the
   choice to the user.
5. The user selects swapfile and origin capacities. The UI shows each
   app-owned path, systemd unit where applicable, fixed-allocation size,
   required reserve, combined required bytes for any shared SSD, and the fact
   that the origin file and OS swapfile serve different purposes.
6. After confirming RamShared is Off and obtaining explicit apply consent, the
   Linux helper revalidates mount and filesystem identity, space, and
   ownership; it creates only the selected managed files/unit and seals the
   native origin identity. It never edits `/etc/fstab`, repartitions, formats,
   or disables existing swap. Swap activation state is shown explicitly;
   RamShared remains Off.
7. The UI records the transaction and leaves any previous swap and sealed
   origin intact. Replacing either needs a separate migration flow.

### Happy flow — choose WSL fallback swap volume

1. User runs `ramshared config` in WSL2; the UI gathers guest state and asks
   its Windows provider for paired host observations and Windows volume
   inventory.
2. The UI distinguishes the WSL maximum, current guest use, WSL fallback swap,
   RamShared origin, ZRAM, GPU adapters, and each Windows volume.
3. The user may compare up to eight eligible volumes in one confirmed batch,
   with a preview capped at 96 MiB written per volume and 768 MiB total.
4. The user selects fallback swap size/path, then reviews the affected WSL
   distributions and exact `.wslconfig` delta.
5. The Windows helper verifies all affected distributions are Off, then
   revalidates the stable volume identity and free space,
   atomically stages only the selected `[wsl2]` swap settings, and reports
   next-start activation. It never calls `wsl --shutdown`.

### Alternate flow — one disk or unsupported provider

1. The selected platform exposes one or many volumes; the UI lists them
   without drive-letter or transient device-name assumptions.
2. Unsupported filesystems, missing systemd, unavailable WSL host interop, and
   unstable identities remain visible with specific reasons.
3. Discovery and plan export remain available; unsupported mutation is
   disabled without changing active swaps, system settings, or origin files.

### Error flow — stale telemetry or changed volume identity

1. A sample is stale, an input is inconsistent, or the selected volume,
   filesystem, mount, or adapter identity differs from the reviewed plan.
2. The UI disables the affected recommendation/apply and records the refusal.
3. It preserves current config, every active swap, and the sealed origin; no
   unrelated path is searched or substituted.

## 7. Data / state model

- **Resource profile:** schema version; platform; managed native Linux swap
  file size/mount UUID/path/priority or WSL fallback swap size/path/Windows
  volume ID; per-tier ZRAM target; per-adapter VRAM cap/adapter ID; and
  optional native file-origin or WSL VHDX-origin size/path/stable identity. No
  host RAM reservation is stored.
- **Host observation:** platform-specific total/available physical RAM and
  commit where available; WSL `vmmemWSL` working set/private bytes when
  available; observation time, source, and freshness.
- **Guest observation:** kernel identity, `MemTotal`, `MemAvailable`, PSI,
  every swap device/size/use/priority, active RamShared phase.
- **Volume observation:** stable Windows volume GUID or Linux block-device and
  filesystem identity, display mount/letter, parent relationship, filesystem,
  device kind, total/free bytes, available/reason, sample time. Unmounted
  devices have no writable mount target.
- **Native origin identity:** filesystem UUID, parent block identity, mount ID,
  app-owned relative path, inode/device number, exact allocated size, and
  manifest hash over the identity fields, verified again from the open file
  descriptor before daemon use. Do not hash origin contents: the authoritative
  file is expected to change as the daemon writes it.
- **GPU observation:** stable adapter ID, provider, allocator budget/use,
  WDDM budget/use, mandatory reserves, safe target, sample time.
- **Benchmark record:** operation ID, volume identity, workload/version,
  sample size/count, raw durations, median/p95 latency, median throughput,
  hashes, write bytes, start/end time, abort/cleanup status.
- **Audit event:** append-only event ID and transaction ID; action; source
  observations; before/after hashes and values; target IDs; safety formula
  inputs; confirmation; status; rollback/pending-restart outcome.

## 8. Interfaces

- `ramshared config`: interactive TUI; `show`, `plan`, `benchmark`, and
  `apply` have noninteractive forms. `apply` requires a fresh transaction
  token from `plan` and explicit confirmation. The platform provider is
  selected from the verified execution environment, not a user-supplied
  arbitrary command.
- Guest profile: `/etc/ramshared/resource-profile.toml`, owned by root and
  mode `0600`; all capacities encoded as unsigned bytes and all paths paired
  with stable volume identity.
- Native Linux helper: a closed-action root provider with bounded typed input
  for discovery, benchmark, managed swapfile and native file-origin creation,
  activation, verification, cleanup, and exact rollback. It never executes
  caller-provided command strings.
- WSL2 host helper: `scripts/windows/Invoke-RamSharedResourceConfig.ps1` with
  a closed action enum (`inspect`, `benchmark`, `stage-wsl-swap-config`,
  `verify-transaction`, `rollback-wsl-config`), bounded JSON input/output, no
  arbitrary command field, and create-once operation IDs.
- Platform state: Linux audit/config under `/var/lib/ramshared/` and
  `/etc/ramshared/`; WSL host audit and exact backups under the current user's
  local application state. Ownership/mode is verified before use.
- Guest state: `$XDG_STATE_HOME/ramshared/config-events.jsonl` with
  owner-only permissions. The same transaction ID joins host and guest events.
- Existing integration owners: `scripts/safety/wslconfig-lib.sh`,
  `scripts/windows/Manage-RamSharedOrigin.ps1`, native origin/lifecycle code,
  and `crates/ramshared-wsl2d/src/gpu_budget.rs` remain authoritative for
  their respective settings. The providers must not create a competing owner.

## 9. Dependencies and risks

- **Risk — global WSL effect:** `.wslconfig` applies to every WSL2 distribution
  for the Windows user. **Mitigation:** list affected distributions and require
  confirmation; preserve unknown settings; never shut down WSL.
- **Risk — RAM counters are confused:** guest capacity, Windows physical RAM,
  Windows commit, and `vmmemWSL` private bytes differ. **Mitigation:** display
  separate sources and timestamps; do not derive a new host-memory limit.
- **Risk — disk test load or residue:** test writes may cause latency or leave
  a temporary file. **Mitigation:** explicit consent, 96 MiB/write-volume and
  768 MiB/run ceilings, platform/storage admission before each sample,
  create-once exact paths, hash verification, and refusal to claim cleanup if
  exact absence is not proven. A filesystem syscall may remain uninterruptible;
  the parent UI must report a stuck worker and must not retry.
- **Risk — volume substitution or TOCTOU:** a drive letter can refer to a
  different device later. **Mitigation:** bind each path to stable volume ID,
  revalidate immediately before file creation, and block if identity changed.
- **Risk — VRAM cap is mistaken for a reservation:** WDDM budget can change
  under other processes. **Mitigation:** display it as a user ceiling clamped
  by per-allocation dynamic admission; do not preallocate memory on config.
- **Risk — sealed origin is replaced:** changing the origin path while a
  manifest or swap is active can lose data or create ghost swap. **Mitigation:**
  disable path/size edits for sealed origins; create a separate native file
  manifest for first-time native origin setup; use existing attended
  migration ownership and preserve swapoff-first semantics.
- **Risk — storage overcommit:** fallback swap and the durable origin may share
  one volume. **Mitigation:** checked-add both maximum sizes, benchmark temp
  bytes, and the required reserve before any create/stage step; recheck free
  bytes before each step and refuse if the combined plan no longer fits.
- **Numeric rollback trigger:** If a before/after config hash differs from the
  approved plan, volume identity changes, free space falls below reserve, a
  log/backup cannot be durably verified, or any required telemetry becomes
  stale before apply, abort before promotion and restore only the exact file
  bytes created/changed by that transaction. Never roll back unrelated WSL
  settings or any pre-existing origin. Never remove old swap or an origin file
  while its exact owner/reference proof is missing.

## 10. Implementation strategy

1. Complete Step 2 discovery, SPEC decisions, and Step 2.5 audit for the
   native file-origin identity and combined disk-capacity contract.
2. Implement pure resource profile parsing, validation, reserve arithmetic,
   volume ranking, and deterministic audit-event schemas with tests first.
3. Add read-only host/guest/GPU/volume discovery and `config show/plan`; make
   stale/missing sources visible without mutation.
4. Add a new native file-origin manifest/open path that validates mount,
   filesystem, parent device, inode, file size, and exact fd identity while
   leaving the sealed WSL block-origin path unchanged.
5. Add the attended bounded volume benchmark and cleanup-custody tests.
6. Add transactional profile, Linux swap/origin, and `.wslconfig` staging with
   crash/refusal/replay tests; no automatic swapoff, WSL shutdown, or tier
   activation.
7. Wire per-tier ceilings into existing live admission, keeping GPU, swap,
   origin, and cascade lifecycle ownership in the current owning crates.
8. Add separate native Linux and WSL before/action/after validation, then
   update `IMPL.md`, docs index, degradation matrix, and append validation.

## 11. Documents to update

`ARCHITECTURE.md`; `docs/reliability/GAP-REGISTER.md`;
`docs/reliability/DEGRADATION-MATRIX.md`;
`docs/specs/no-milestone/wsl2-origin-capacity-policy/SPEC.md` only if its
existing explicit-path contract changes; `docs/INDEX.md`; `IMPL.md` for this
slice; append-only `validation.md` after live evidence. Do not change public
performance claims before qualifying the new benchmark.

## 12. Out of scope

- Automatic `wsl --shutdown`, reboot, stress activation, kernel install,
  driver install, VHDX relocation, or deletion of a user-selected file.
- Force-allocating host RAM, ZRAM, or VRAM to the user's requested ceiling.
- Disabling host/display reserves or dynamic WDDM/guest admission.
- Automatic origin migration for an existing sealed manifest.
- Selecting network/removable volumes for swap or treating C: as universally
  fastest.
- Publishing user's paths or resource telemetry off-host.
- Editing `/etc/fstab`, repartitioning, formatting disks, removing existing
  swap devices, changing the WSL memory ceiling, or changing non-WSL Windows
  pagefile settings. Arbitrary native origin files outside the managed
  app-owned directory are also out of scope.

## 13. Acceptance criteria

- New `ramshared config` works on native Linux and WSL2, explains every source,
  and exposes only platform-supported volume/GPU choices without confusing
  host RAM, guest RAM, swap capacity, or VRAM budget.
- The displayed list contains all discovered Windows volumes or Linux local
  devices and mounts and gives a stable reason for every ineligible one; a
  one-volume host works without drive-letter or transient device-name
  assumptions.
- The optional benchmark is bounded, consented, integrity-checked, cleaned,
  replay-safe, and never runs when admission telemetry is stale or pressure is
  present.
- C:/I: recommendation uses the new run's comparable measurements; the
  historical 85.4/38.0 MB/s result is labeled historical only.
- No configuration write exceeds the dynamic GPU/tier or combined disk-reserve
  limits, alters unrelated `.wslconfig` keys, replaces a sealed origin, or
  activates RamShared.
- Every change/refusal/test has a local append-only record and, when a WSL2
  host/guest operation spans both OSes, linked host/guest records and an
  observable rollback or pending-restart state.
- Named Linux and Windows provider unit/refusal tests, Rust cover gate,
  PowerShell tests, and live native-Linux plus WSL2 before/action/after
  validations pass before a release claims support.

## 14. Validation plan

- Unit: variable-size memory fixtures and malformed/stale platform telemetry;
  safe-cap arithmetic; disk eligibility/ranking; volume identity mismatch;
  per-adapter VRAM caps and WDDM clamp; profile validation; event replay.
- Integration: Linux helper bounds/ownership, UUID and mount revalidation,
  filesystem eligibility, path quoting, swapfile/systemd replay, file-origin
  identity binding and fd validation, unknown action refusal, no `/etc/fstab`
  mutation, and no automatic old-swapoff;
  Windows helper JSON bounds, unknown action refusal, multi-distro scope,
  unrelated `.wslconfig` preservation, exact backup/rollback, interrupted
  apply, and operation replay.
- Benchmark: manufactured low-space, busy-disk, stale telemetry, low guest
  headroom, disk removal, fsync failure, hash mismatch, and cleanup failure;
  legitimate volume comparison on a disposable local volume.
- Live native Linux: before/action/after on a disposable local filesystem,
  exact managed-file/unit ownership, selected swap activation, previous swap
  preserved, rollback/cleanup proof, no active RamShared tier, and fresh
  memory/pressure samples.
- Live WSL2: before/action/after on a disposable host profile, installed
  `BINARY_MATCH`, no active tiers, no automatic shutdown, paired host/guest
  samples, exact `.wslconfig` backup, explicit restart-pending result, and
  verification after an operator-controlled restart.
- Environment-bound: GPU providers, WDDM budgets, multiple adapters, Windows
  volumes, Linux filesystems, and storage classes require representative
  platform runs. Until those pass, broad hardware/filesystem support remains
  partial.
