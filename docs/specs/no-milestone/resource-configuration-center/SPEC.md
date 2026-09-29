# SPEC — Cross-platform RamShared resource configuration

## Closed scope

One `ramshared config` interface and typed resource profile for native Linux
and WSL2. The shared planner displays RAM/commit/swap/GPU/storage as separate
resources and configures only ceilings owned by RamShared or the active
platform provider.

Native Linux may create an app-owned OS swapfile and a separate fixed-allocated
RamShared origin file on a supported mounted local filesystem. The OS swapfile
uses a systemd `.swap` unit; the origin file gets a distinct native manifest
that binds filesystem, device, inode, and open-file identity. WSL2 may stage
fallback swap size/path in the user's `.wslconfig` and an origin VHDX through
the existing Windows helper. Both may set a RamShared ZRAM/VRAM target cap
where the existing runtime owner supports it.

The feature does not set the WSL `memory` ceiling, reserve host RAM or VRAM,
partition/format a disk, edit `/etc/fstab`, remove an existing swap, relocate a
sealed origin, change the live RamShared lifecycle, run a stress campaign,
stop/restart WSL, or alter the Windows pagefile. An explicit later `activate`
action may enable a newly created Linux managed swapfile after a fresh safety
check; apply does not disable prior swap.

## Traceability

| PRD | SPEC |
| --- | --- |
| RF-1..RF-2, RF-13 | ITEM-1..ITEM-3 |
| RF-3, RF-8, RF-12 | ITEM-4, ITEM-8 |
| RF-4, RF-9 | ITEM-5 |
| RF-5..RF-7 | ITEM-6 |
| RF-10..RF-11 | ITEM-7..ITEM-8 |
| NFR-1..NFR-8 | ITEM-1..ITEM-8 |

## Technical decisions

| # | Decision | Why |
| --- | --- | --- |
| DT-1 | The shared CLI owns `config show`, `plan`, `benchmark`, and `apply`. The platform provider is selected from detected native Linux or WSL2 execution, never from an arbitrary command string. `show`/`plan` are read-only; `apply` requires a fresh plan ID and explicit confirmation. | A shared UI must not imply that Linux and WSL2 have the same host or storage controls. |
| DT-2 | `/etc/ramshared/resource-profile.toml` is a versioned root-owned `0600` profile for RamShared tier caps and selected platform targets. The profile stores byte counts and stable resource IDs, not drive letters or transient kernel-assigned device names as identity. | A profile needs portable semantics and must survive device renumbering. |
| DT-3 | Every user value is a ceiling. ZRAM, VRAM, and origin decisions remain clamped by the owning runtime policy; if no owning policy can validate a current safe maximum, an increase is refused. Config never reserves physical host memory or GPU memory. | Prevents user configuration from turning a target into an unconditional allocation. |
| DT-4 | WSL `memory=` is read-only in this slice. Do not infer a safe WSL maximum from physical-memory/commit snapshots or invent a new reserve formula. WSL memory and commit observations are explanatory only. | `FreeVirtualMemory`, physical headroom, commit headroom, and WSL working set are distinct; this feature has no independently qualified formula that turns them into a safe VM maximum. |
| DT-5 | Admission uses the freshness and identity contracts of the owning telemetry provider. A provider with no freshness bound cannot authorize an increase. Each mutation takes a fresh observation immediately before the first write and rechecks resource identity/capacity at each storage operation. | Avoids copying stale or ambiguous values into a second policy. |
| DT-6 | Linux inventory joins the complete local block-device tree from `lsblk` with `/proc/self/mountinfo`. Display unmounted devices and group mounted filesystems under their backing device. The profile persists filesystem UUID, stable backing identity (the parent WWN/serial for a partition, or the device's own WWN/serial for a filesystem directly on a whole disk), and a managed relative path; it never persists kernel-assigned mount IDs, major/minor numbers, or device names. V1 file targets require a mount exposing filesystem root `/`; bind/subtree mount roots remain visible but ineligible so the relative managed path has stable meaning. Each plan/write resolves exactly one current eligible mount and binds that operation to the fresh mount ID, major/minor, filesystem type/options, read-only state, and free-space sample. If the stable identity resolves to multiple current mounts, refuse as ambiguous. Unmounted or missing-stable-identity targets remain visible but ineligible; discovery never mounts a device. In WSL2, a guest filesystem stays ineligible for file placement until its backing Windows volume identity and current host free capacity are bound to it; guest free space alone does not satisfy that gate. | Mount IDs and device numbers can change across boots and namespaces. Keeping them only in the current operation prevents stale profiles from breaking after remount while still binding each effect to one fresh mount. Refusing subtree mount roots prevents a relative managed path from silently resolving to another underlying directory after mount changes. A machine may have one disk with a filesystem directly on the disk, so requiring a partition would hide a valid native Linux target. WSL2's expandable VHDX needs a separate host-volume capacity check. |
| DT-7 | V1 Linux managed swapfile writes are eligible only on tested ext4 and XFS mounts over a recognized local block transport. Known network-backed transports (iSCSI, NBD, RBD, DRBD, Fibre Channel, FCoE, AoE, and NVMe-oF), plus missing or unrecognized transport identity, remain visible but ineligible even when the filesystem itself is ext4/XFS. Btrfs, overlay, network filesystems, removable, read-only, unknown, and unsupported filesystems remain visible with a reason and ineligible until their filesystem-specific allocation and `swapon` contract has named tests and a separate SPEC decision. | Swapfiles have filesystem- and backing-transport-specific requirements; unsupported, ambiguous, or remote paths must not be guessed safe. |
| DT-8 | A Linux swapfile is create-once under an app-owned root-controlled directory on the selected mount; paths are opened relative to a verified directory handle with no symlink traversal. A closed-action root helper refuses disk-tier changes unless RamShared is `Off`, then validates exact size, filesystem, free-space floor, mount ID, and active swap identities. It writes a matching systemd `.swap` unit; it never edits `/etc/fstab`. Existing swaps are never disabled or removed by apply. | Makes path ownership auditable and limits root operations to one exact transaction. |
| DT-9 | Linux swap migration is forward-only until the previous swap is proven inactive and unused. Creating/enabling a replacement may leave both files present. Cleanup accepts only an app-owned exact path, inactive swap identity, `SwapUsed=0`, no unit references, and unchanged transaction hashes; any missing proof leaves the old file/unit intact and reports cleanup pending. | `swapoff` can move pages and fail under pressure; configuration must not trigger it or delete backing storage while referenced. |
| DT-10 | In WSL2, `swap` and `swapFile` are changed only by the Windows host helper, after fresh probes confirm every affected distribution is `Off`. It uses unique `[wsl2]` keys, an exact backup, stable Windows volume identity, atomic replacement, and `pending_wsl_restart=true`. Setting `swap=0` is refused unless every affected distribution has a verified persistent non-RamShared swap alternative. Changing the selected path never deletes the prior VHDX/file. No `wsl --shutdown` or `wsl --terminate` is invoked. | `.wslconfig` is global to that Windows user's WSL2 distributions and changes apply at VM start; a lost fallback or active distribution must not be hidden by a host-only configuration change. |
| DT-11 | Windows candidates are enumerated for display, including ineligible volumes. WSL swap/origin writes require a unique fixed local volume identity, NTFS/ReFS eligibility as enforced by the owning host manager, canonical target path, and the applicable free-space reserve. Drive letters are display-only and are resolved again immediately before each write. | Reuses the existing Windows origin manager's supported filesystem and identity contract. |
| DT-12 | WSL2 origin creation/path remains delegated to `Manage-RamSharedOrigin.ps1` and its sealed block manifest. Native Linux gets a separate manifest and open path for an app-owned regular file, implemented by reusing `FileOrigin`; persistent identity includes filesystem UUID, stable backing-device identity (parent WWN/serial for a partition, device WWN/serial for a whole-disk filesystem), relative managed path, inode, exact allocated size, and a manifest hash over these identity fields. At each open, resolve the unique current mount and verify its fresh mount ID/major:minor against the opened fd and stable block identity; do not persist those boot-scoped values in the manifest. Do not hash origin contents because daemon writes change them. Existing sealed origins remain immutable. | Native users can select a different SSD without repartitioning while WSL keeps its existing VHDX/block provenance contract. |
| DT-13 | GPU targets are keyed by stable adapter identity. A cap is applied as `min(user_cap, current_safe_target)` using the existing driver/WDDM provider, display reserve, runtime buffer, freshness rules, and identity checks. Missing/stale/ambiguous budgets produce no increase. | Prevents one adapter's headroom being spent against another or a stale budget. |
| DT-14 | One user-approved benchmark covers at most eight eligible volumes; each volume receives three 32 MiB samples (96 MiB writes) through a fixed 1 MiB buffer, with durable flush and read-back hash. Total write ceiling is 768 MiB per run. Larger candidate sets require an explicitly selected batch and a new preview. No background run/retry. | Keeps automatic comparison useful while bounding write volume. |
| DT-15 | Each round measures both durable small-write latency and sequential throughput; volume order rotates by round. Report raw samples and medians. Recommend a measured leader only for the selected use case when median difference is ≥10% and the other metric does not rank in the opposite direction; otherwise report a tie. Never label one volume universally fastest. | A volume may be better for swap latency and different for origin throughput; a small run is a recommendation, not a guarantee. |
| DT-16 | A benchmark refuses before any write if RamShared is active, pressure/telemetry is unhealthy, the provider is stale, or a target fails identity/reserve checks. It rechecks before each volume and sample. Integrity, flush, or cleanup uncertainty fails the run and blocks later samples. A failed run has no ranking. | Prevents a partial or unhealthy run from being presented as a valid comparison. |
| DT-17 | Host and guest operations are independent commits joined by a random transaction ID. Each append-only local event records action, source sample IDs, old/new hashes, stable target IDs, consent, result, and pending rollback/activation; no secrets or process command lines are logged. | Cross-OS files cannot participate in one atomic rename, so both sides need a durable trace. |
| DT-18 | Rollback touches only exact transaction-owned files whose current hash matches the transaction output. If any file, mount, device, or config changed after plan, preserve it and report `manual_recovery_required`; never overwrite concurrent user changes. | Prevents a rollback from becoming a second destructive write. |
| DT-19 | For each stable volume, require `free_bytes >= checked_sum(new_swapfile_bytes, new_origin_allocation_bytes, other_new_managed_bytes, 10 GiB)`. The 10 GiB floor is non-overridable and matches the existing origin reserve. Existing allocated files are already reflected in `free_bytes` and are not subtracted twice. Benchmark is separate: require `free_bytes >= 96 MiB + 10 GiB`, clean the exact test file, then remeasure before apply; uncertain cleanup refuses apply. | Makes same-disk tier configuration explicit and prevents storage overcommit. |
| DT-20 | Keep the existing v3 sealed block-origin reader/schema unchanged. New native file origins use a separate `/etc/ramshared/native-origin.toml` manifest and are selected only by the native provider; never auto-convert or write a second origin over an existing one. Block and file origins are distinct first-class storage kinds for their owning platform contracts, not an automatic compatibility fallback. If a sealed block origin already exists, show it and require its existing migration path before replacing it. | Avoids weakening or ambiguating an existing sealed origin while adding Linux SSD file selection. |
| DT-21 | Each 32 MiB sample writes 31 MiB sequentially with a 1 MiB buffer and flushes once, then performs 256 distinct 4 KiB writes with a durable flush after each write; total payload writes are exactly 32 MiB per sample. Linux uses `fdatasync`; Windows uses `FlushFileBuffers`. Read-back verifies the complete file. Three samples yield 96 MiB maximum payload writes per volume. Report p99 over 768 small-write latency observations plus all sequential-throughput results, their median, and deviation. Run one child worker sequentially with at most one outstanding filesystem operation and a 120-second per-volume deadline. Persist a lease containing process ID plus start identity and the exact owned target before launch. On timeout, request termination and verify that exact process has exited before cleanup. If exit or cleanup cannot be proven, persist `worker_stuck`, keep the lease and exact file record, do not launch another worker, and block disk mutations on that target until attended recovery proves exit and exact cleanup. | Defines a reproducible durable small-write and sequential-throughput comparison within the stated byte ceiling. A filesystem call can remain uninterruptible, so the deadline bounds the supervisor wait but cannot promise kernel-level cancellation. The persisted lease prevents a later invocation from overlooking an orphaned worker. |
| DT-22 | Represent an already-created, identity-sealed native origin as `linux_file_origin` with its inode and identity hash. Represent a not-yet-created selection as `linux_file_origin_request`, containing only the stable filesystem/device identity, managed relative path, and requested allocation. A read-only plan may validate the current mount and capacity for a request; it must not claim that the file exists, has been created, or is safe to open. The privileged creation transaction generates the inode-bound manifest only after securely creating and verifying the exact file. Duplicate-path detection treats a request and sealed origin at the same stable path as a conflict. | The prior profile shape required a real inode before the UI could express a new Linux origin, making it impossible to configure the requested target before creation. Separating intent from sealed runtime identity avoids fabricating provenance. |
| DT-23 | `ramshared config draft --output PATH` requires a foreground TTY on stdin and stdout. It lists every current candidate with eligibility reasons, lets the user select one or more eligible volumes for conventional fallback swap or a RamShared SSD-origin request, accepts positive variable sizes in MiB, and displays the read-only combined-capacity plan before a final `SAVE` confirmation. It writes only the explicitly named new file with `create_new`, `O_NOFOLLOW`, mode `0600`, and current-user ownership; the parent must already exist, be a directory owned by the current user, and not be group/world writable. Existing paths refuse; it never overwrites or creates a system profile. The writer verifies exact file length and bytes after writing and syncs the file and parent directory. Missing TTY, stale/ambiguous identity, unsupported candidates, invalid/overflowing sizes, capacity/reserve shortfall, or declined confirmation performs no write. The generated file is an untrusted draft for `config plan --profile PATH`, not an applied setting. | Allows a user to choose among detected disks and set variable swap/origin sizes while keeping all host/guest changes behind a later provider transaction. An exclusive draft cannot be confused with an already-applied system profile. |
| DT-24 | The draft wizard may also set optional positive ZRAM and SSD-origin tier ceilings in MiB; blank leaves a ceiling unset, while zero, negative, malformed, and overflowing values refuse before any file is written. The final read-only plan prints the exact configured ceilings and warns that they are policy ceilings, not reservations or proof of current safe budget. The origin-tier ceiling is distinct from the allocated SSD-origin storage target. This wizard cannot set a VRAM cap until inventory supplies a fresh budget bound to a stable adapter identity; it must explain that the GPU choice is unavailable and must not accept a manually typed adapter ID. The profile remains a draft and the existing live ZRAM/VRAM admission owners remain authoritative. | Lets users configure variable tier ceilings without turning user input into a reservation or binding a cap to an unidentified GPU. |

## Interfaces

- **CLI/TUI:** `ramshared config`; subcommands `show`, `plan`,
  `draft --output PATH`, `benchmark`,
  `apply`, `activate-linux-swap`, and `cleanup-linux-swap`. Parsing has no
  side effects. Mutations require a one-use current plan ID and explicit
  confirmation. `activate-linux-swap` and cleanup are unavailable on WSL2.
- **Shared profile:** `/etc/ramshared/resource-profile.toml`, schema version
  1, root-owned and mode `0600`. Fields include per-tier caps and a list of
  platform-bound targets: `linux_swapfile` (filesystem UUID, stable backing-device
  identity, managed relative path, bytes, priority), `linux_file_origin` (filesystem
  UUID, stable backing-device identity, managed relative path, inode,
  exact allocated bytes, identity-field manifest hash),
  `linux_file_origin_request` (filesystem UUID, stable backing-device identity,
  managed relative path, requested allocation), `wsl_fallback` (Windows volume identity,
  path, bytes), and `wsl_origin` (Windows volume identity, origin path, exact
  allocation). A profile may select swap and origin on the same or different
  stable volumes. The planner groups their allocations by stable volume and
  adds the non-overridable 10 GiB reserve once per volume. Windows paths are
  compared case-insensitively when detecting duplicate targets.
  Unknown fields, duplicate keys, overflow, stale profile hash, invalid target,
  or unsupported platform refuse apply.
- **Native origin manifest:** `/etc/ramshared/native-origin.toml`, root-owned
  and mode `0600`, separate from the WSL v3 block-origin manifest. It seals the
  stable filesystem and backing-device identity, app-owned relative path,
  inode, exact allocated size, identity-field hash, and schema version. The
  mount ID and device number are resolved for each operation and are not
  persisted. It does not hash mutable origin data. A conflict with an existing
  sealed origin refuses apply.
- **Linux provider:** closed-action root helper with bounded typed input for
  `inspect`, `benchmark`, `stage-swapfile`, `activate-swapfile`,
  `verify-transaction`, `cleanup-swapfile`, `create-file-origin`, and exact
  rollback. It uses argv-based fixed executables only; no shell command or
  user-provided executable/path is executed. Apply does not call `swapoff`.
- **WSL host provider:** `scripts/windows/Invoke-RamSharedResourceConfig.ps1`
  with closed actions for inspect, benchmark, stage/verify/rollback of WSL
  swap settings. Input/output is bounded JSON and contains no arbitrary
  command. It runs in the current user's context for `.wslconfig`; origin VHDX
  writes stay with the existing origin manager and its approval boundary. The
  helper logs to a user-local directory and uses transaction-scoped exact
  backups.
- **Audit state:** local append-only JSONL with owner-only permissions; one
  transaction ID joins WSL host and guest events. Every explicit user
  operation and refusal is recorded; dashboard repaint/poll cycles are not
  logged as new operations.
- **Benchmark worker lease:** persist one exclusive active-worker lease before
  launch. Native Linux stores it under `/var/lib/ramshared/resource-config/`;
  the Windows host provider stores it under the current user's local
  application state. The lease binds plan hash, stable volume identity,
  operation ID, process ID plus process start identity, exact temporary path,
  and byte ceiling. A later invocation may clear it only after proving the
  recorded process exited and the exact file is absent or safely cleaned.
- **Existing owners:** `scripts/safety/wslconfig-lib.sh`,
  `scripts/windows/Manage-RamSharedOrigin.ps1`, the sealed block-origin
  manifest/lifecycle, the new native file-origin owner, and existing GPU
  admission providers remain the final authority for their settings.

## Atomicity and rollback

- **Audit frontier:** before a benchmark or mutation writes, append and sync a
  transaction-intent event containing the reviewed plan hash, target identity,
  byte cap, and consent. If that durable intent cannot be verified, perform no
  write. After each side effect, append and sync its result before reporting
  success. If result logging fails after a write, preserve the owned files and
  transaction evidence, return `manual_recovery_required`, and never claim the
  operation succeeded or clean up uncertain state.

- **Profile:** write a complete validated candidate to an exclusive
  same-directory file, fsync, verify bytes/hash/mode/owner, and rename
  atomically. A changed target hash refuses; rollback only when it still
  matches this transaction's output.
- **User draft:** create only the explicitly named new profile after the user
  confirms the rendered plan. Resolve and validate the existing parent
  directory before opening the target with `create_new` and `O_NOFOLLOW`; set
  mode `0600`, verify current-user ownership and exact contents, and sync the
  file. If creation or writing fails, remove only the file proven to have the
  same opened-file identity; if that proof fails, report the exact retained
  path and do not claim a complete draft. An existing target is never replaced.
  Draft creation changes no platform setting and requires no privilege.
- **Linux swapfile:** revalidate the exact mount ID, filesystem UUID, stable
  backing-device identity, filesystem type/options, free bytes, and managed
  directory before create. Allocate only the new unique file; verify exact
  allocated size and swapfile suitability before `mkswap`. Write and verify the exact systemd
  unit. `apply` enables the unit for future boot but does not start it. The
  separate `activate-linux-swap` action repeats the admission checks and runs
  `swapon` only after confirmation. If command outcome is uncertain, retain
  file/unit and ownership evidence; do not delete or retry blindly.
- **Native RamShared origin:** create only a new create-exclusive file inside
  the app-owned directory on the reviewed mount. Use fixed allocation, verify
  allocated bytes and file type, sync the file and containing directory, then
  atomically publish a distinct native-origin manifest bound to filesystem,
  stable backing-device identity, relative path, inode, allocated size, and
  identity-field hash. Resolve the current mount ID and device number for this
  operation, but do not persist them. The daemon opens
  without symlink traversal, rechecks the fd/path identity before serving, and
  uses the existing `FileOrigin` write-through backend. Never truncate or
  reuse a pre-existing file. If manifest publication or daemon validation is
  uncertain, retain the file and transaction record for attended recovery.
- **Linux old swap:** never call `swapoff` during apply/activate/rollback.
  Cleanup is a later explicit action and requires exact ownership, inactive
  state, zero use, no systemd references, and fresh identity proof. Otherwise
  keep the old target and report cleanup pending.
- **WSL host:** preserve exact `.wslconfig` bytes, update only requested
  unique `[wsl2].swap`/`swapFile` keys, verify the staged hash, and use atomic
  same-directory replacement. Report pending next WSL start; no shutdown.
- **Benchmark:** operation-scoped create-new files, maximum 32 MiB of payload
  writes each: 31 MiB sequential with a 1 MiB buffer and one durable flush,
  then 256 distinct 4 KiB writes, each followed by the platform's durable
  flush (`fdatasync` on Linux, `FlushFileBuffers` on Windows). Read back and
  verify the entire file.
  Maximum is 96 MiB per volume and 768 MiB per run. Close every handle before
  exact cleanup. The exclusive worker lease prevents concurrent benchmarks
  and survives parent-process exit. If worker exit or file absence cannot be
  proven, keep the lease and ownership record and refuse further disk writes
  to that target.
- **Origin/GPU/ZRAM:** native file-origin creation is performed only by the
  closed-action helper and its dedicated manifest owner; WSL VHDX/block origin
  stays with the current Windows manager. GPU/ZRAM caps are consumed by their
  existing runtime owners and clamped again at every allocation/admission.
- **Cross-platform:** each side commits and records independently. If the
  second side fails, restore only the first side's exact transaction output
  when its hash is unchanged; otherwise preserve both states and report
  `manual_recovery_required`.

## Kahneman map (critical only)

| ITEM / stage | # | Question | Min evidence | Abort |
| --- | --- | --- | --- | --- |
| Resource and volume admission | #3/#13 | Can missing/stale/inconsistent memory, mount, or volume data allow an increase? | `resource_policy_rejects_unknown_stale_and_inconsistent_samples`; `volume_plan_refuses_unstable_identity` | Any increase without current owner-approved measurements and exact stable identity. |
| Linux swap create/activate | #13/#16 | Can a path race, failed `mkswap`, or uncertain `swapon` cause foreign-file mutation or false ownership? | `linux_swap_apply_creates_only_owned_file_and_unit`; `linux_swap_activation_refuses_changed_mount_or_active_ramshared` | Any write through symlink/changed mount, ambiguous outcome marked success, or existing swap disabled. |
| Native RamShared origin | #13/#16/#17 | Does a file-backed origin bind the path and open fd to the reviewed filesystem/device/inode and survive rollback without replacing existing data? | `native_origin_manifest_binds_open_fd_identity`; `native_origin_creation_refuses_existing_file_or_capacity_shortfall` | Any daemon start with path/fd identity mismatch, sparse/short allocation, or replacement of a sealed origin. |
| Linux swap cleanup | #16/#17 | Does cleanup require exact ownership and zero references, and do repeated calls preserve the same result? | `linux_swap_cleanup_refuses_foreign_active_or_used_target`; `linux_swap_cleanup_replay_is_idempotent` | Delete while active/used, owner/hash mismatch, or non-idempotent replay. |
| Volume benchmark | #9/#13/#16 | Does each result bind to its exact volume, obey the write cap, and prove integrity/cleanup? | `benchmark_binds_volume_caps_bytes_and_verifies_cleanup` | Bytes exceed preview, target drifts, or incomplete cleanup is green. |
| GPU cap | #2/#13/#16 | Can a user cap raise budget or bind another adapter? | `user_gpu_cap_never_exceeds_fresh_same_adapter_safe_target`; `user_gpu_cap_refuses_stale_or_ambiguous_adapter` | Any allocation above current safe target or adapter mismatch. |
| Multi-provider transaction | #13/#17 | Can a replay or rollback overwrite a later user edit across OSes? | `config_apply_is_idempotent_and_refuses_changed_rollback_target`; `wsl_config_apply_only_stages_next_start` | Any rollback after target hash drift or any automatic WSL shutdown. |

## Security checklist (pre-implementation)

- [x] Privilege boundary is provider-specific; discovery/plan are read-only;
  mutations use closed action sets and explicit consent.
- [x] User/provider input is bounded, typed, overflow checked, and copied into
  owned data before validation; no arbitrary command field.
- [x] Flags/IOCTL codes: N/A — no new IOCTL in this userspace slice.
- [x] Info leak: local logs omit secrets and process argv; paths/IDs remain
  local and permissions are owner-only.
- [x] IRQ/atomic/IRQL: N/A — userspace providers only; foreign driver queries
  must keep their existing deadline.
- [x] Lifetime: exact file/handle ownership, no symlink traversal, and no
  cleanup while a swapfile may be referenced.
- [x] Hot-unplug/device-gone: re-resolve mount/volume and adapter identity
  before every write/admission; identity drift refuses without retry.
- [x] Host safety: no WSL shutdown, stress, RamShared activation, or RAM/VRAM
  reservation; volume benchmark is visible, consented, and bounded.
- [x] Shared-hardware cushion: all GPU caps remain below existing live
  reserves and all memory caps are ceilings; no new host reserve formula.
- [x] Bounded foreign calls: use existing provider deadlines; missing deadline
  makes the operation unavailable.
- [x] Cooperative cascade: config cannot remove required fallback/origin while
  a RamShared tier is active.
- [x] Replayable operations: transaction hashes and one-use plan IDs prevent
  stale apply; repeated identical apply is a no-op.

## Files to CREATE / MODIFY / DELETE

### CREATE / MODIFY — partial implementation

**`crates/ramshared-config/src/resource_profile.rs`**
- Purpose: parse and validate versioned user ceilings and platform-bound
  storage targets without performing host or guest mutations.
- RF / DT: RF-3, RF-5, RF-7..RF-9, RF-12; DT-2..DT-3, DT-6..DT-8,
  DT-10..DT-13, DT-19..DT-20.
- Types / fns: `ResourceProfile`, `PlannedTierCaps`, `ResourceTarget`,
  `ResourcePlatform`, `StorageVolumeIdentity`, `validate_for()`, and checked
  per-volume capacity arithmetic.
- Implemented tests in `crates/ramshared-config/tests/resource_profile.rs`:
  `resource_profile_accepts_variable_caps_and_rejects_overflow`,
  `resource_profile_roundtrips_stable_volume_and_adapter_ids`,
  `resource_profile_supports_multiple_targets_on_one_and_multiple_volumes`,
  `resource_profile_rejects_duplicate_managed_paths_and_capacity_overflow`,
  `resource_profile_rejects_transient_mount_id_in_persisted_targets`,
  `resource_profile_rejects_platform_mismatch_unknown_fields_and_unsafe_paths`,
  `resource_profile_rejects_zero_or_unbound_storage_identity`,
  `resource_profile_rejects_oversized_or_controlled_identity_and_paths`, and
  `resource_profile_rejects_ambiguous_windows_target_paths`,
  `resource_profile_accepts_a_new_linux_origin_request_without_a_preexisting_inode`.
- Read-only CLI profile loading and capacity planning are implemented. Remaining:
  interactive target selection, profile persistence, providers, mutation, and
  live native Linux/WSL2 target qualification.
- Cover: profile slice passed at 93.1% (312/335 lines).

**`crates/ramshared-cli/src/resource_config.rs`**
- Purpose: shared read-only resource observations, native Linux/WSL2 platform
  detection, verified filesystem inventory, a read-only TUI, and a typed
  read-only plan for an existing profile. Provider actions and mutation
  orchestration remain unimplemented.
- RF / DT: RF-1..RF-3, RF-9..RF-13; DT-1..DT-7, DT-17..DT-18.
- Types / fns currently present: `ResourceSnapshot`, `RuntimePlatform`,
  `MountInfo`, `collect_snapshot()`, `parse_mountinfo()`, and `run()`.
- Reference: `crates/ramshared-cli/src/monitor.rs` for Ratatui/test backend;
  `crates/ramshared-cli/src/cascade/` for platform boundary patterns.
- Implemented tests: `platform_detection_distinguishes_native_linux_from_wsl2`,
  `linux_block_inventory_preserves_mounted_and_unmounted_devices`,
  `mountinfo_parser_decodes_paths_and_records_mount_identity_and_access`,
  `storage_candidate_requires_current_writable_mount_capacity_and_stable_identity`,
  `network_backed_block_devices_are_ineligible_and_multiple_local_disks_remain_eligible`,
  `mounted_whole_disk_filesystem_uses_its_own_stable_identity`,
  `mount_capacity_uses_live_available_blocks_without_writing`,
  `config_plan_never_mutates_host_or_guest`,
  `native_linux_plan_resolves_current_mount_from_stable_filesystem_identity`,
  `native_linux_origin_request_plan_binds_volume_without_claiming_creation`,
  `native_linux_profile_survives_a_new_mount_namespace_id`,
  `native_linux_plan_refuses_multiple_current_mounts_for_one_profile_identity`,
  `storage_candidate_rejects_filesystem_subtree_mounts`,
  `resource_policy_rejects_unknown_stale_and_inconsistent_samples`,
  `resource_plan_without_profile_reports_not_configured_and_read_only`,
  `resource_plan_rejects_drive_and_volume_guid_aliases_for_same_target`, and
  `profile_loader_rejects_symlinks_oversized_files_and_untrusted_system_profiles`,
  `windows_inventory_lists_every_volume_and_explains_ineligible_targets`, and
  `windows_inventory_probe_does_not_filter_volumes_by_drive_type`.
- Remaining required tests: `config_apply_is_idempotent_and_refuses_changed_rollback_target`,
  `config_apply_requires_durable_intent_before_mutation`, and provider/E2E
  tests listed below.
- Cover: the current read-only planning slice passed at 88.7%; apply/provider
  policy remains unimplemented and uncovered.
- Kahneman: #13/#17.

**`crates/ramshared-cli/src/resource_config/linux.rs`**
- Purpose: native Linux inventory and closed-action request/response client for
  the privileged provider; distinguish OS swapfile from the RamShared origin
  file.
- RF / DT: RF-1, RF-5..RF-8, RF-13; DT-5..DT-9.
- Required tests: `volume_plan_refuses_unstable_identity`,
  `linux_swap_activation_refuses_changed_mount_or_active_ramshared`,
  `native_origin_plan_accounts_combined_volume_capacity`.
- Cover: ≥80% policy; OS helper interaction uses manufactured integration.
- Kahneman: #13/#16.

**`crates/ramshared-wsl2d/src/native_origin.rs`**
- Purpose: parse the native file-origin manifest and open/verify its
  file-backed `FileOrigin` against mount, filesystem, stable backing-device
  identity, inode, size, and fd identity.
- RF / DT: RF-1, RF-3, RF-8; DT-12, DT-19..DT-20.
- Required tests: `native_origin_manifest_binds_open_fd_identity`,
  `native_origin_manifest_rejects_path_mount_and_inode_drift`.
- Cover: ≥80% identity/policy logic.
- Kahneman: #13/#16/#17.

**`crates/ramshared-cli/src/resource_config/wsl2.rs`**
- Purpose: bounded Windows-provider client and WSL-only plan rendering.
- RF / DT: RF-1, RF-5..RF-8, RF-13; DT-10..DT-12.
- Required tests: `wsl_provider_missing_refuses_host_mutation`,
  `wsl_config_plan_distinguishes_swap_and_origin`.
- Cover: ≥80% pure policy.
- Kahneman: #13/#17.

**`scripts/linux/ramshared-resource-config-helper`**
- Purpose: privileged native Linux operations for inspect, benchmark,
  create/activate/verify/cleanup of app-owned swapfiles and fixed-allocated
  native origin files, plus exact rollback.
- RF / DT: RF-1, RF-5..RF-8, RF-10..RF-12; DT-6..DT-9, DT-12, DT-17..DT-20.
- Required tests: `linux_swap_apply_creates_only_owned_file_and_unit`,
  `linux_swap_apply_preserves_old_active_swap_on_failure`,
  `linux_swap_cleanup_refuses_foreign_active_or_used_target`,
  `native_origin_creation_refuses_existing_file_or_capacity_shortfall`.
- Cover: N/A — privileged OS orchestration; manufactured FS/command matrix.
- Kahneman: #13/#16/#17.

**`scripts/windows/Invoke-RamSharedResourceConfig.ps1`**
- Purpose: Windows volume/host discovery, bounded benchmark, exact
  `.wslconfig` WSL swap edit, verify/rollback, and local audit.
- RF / DT: RF-1, RF-5..RF-7, RF-10..RF-12; DT-10..DT-11, DT-14..DT-18.
- Required tests: `volume_inventory_includes_all_candidates_and_reasons`,
  `wsl_config_apply_only_stages_next_start`,
  `benchmark_binds_volume_caps_bytes_and_verifies_cleanup`.
- Cover: N/A — PowerShell host orchestration; manufactured matrix.
- Kahneman: #9/#13/#16.

### MODIFY

**`crates/ramshared-config/src/lib.rs`**
- Purpose: export the resource-profile module while preserving the existing
  broker/agent `Config` schema and behavior.
- RF / DT: RF-3, RF-8, RF-12; DT-2..DT-3, DT-12..DT-13.
- Tests: existing broker/agent unit suite plus the dedicated profile test
  target.
- Cover: N/A — module declaration.
- Kahneman: #13/#17.

**`crates/ramshared-cli/src/main.rs`**
- Purpose: add config command parse/dispatch; parse alone has no side effects.
- RF / DT: RF-1, RF-10; DT-1.
- Required tests: `config_cli_parses_show_plan_apply_and_confirmation`,
  `config_cli_rejects_unknown_actions`.
- Cover: N/A — dispatch.

**`crates/ramshared-wsl2d/src/main.rs`, `crates/ramshared-cli/src/cascade/mod.rs`,
`crates/ramshared-cli/src/cascade/cascade_io.rs`**
- Purpose: add native file-origin selection to the native lifecycle while
  preserving the current sealed block-origin path and its validation.
- RF / DT: RF-3, RF-8, RF-10, RF-12; DT-12, DT-18, DT-20.
- Required tests: `native_lifecycle_opens_only_manifest_bound_file_origin`,
  `native_origin_apply_does_not_change_existing_block_manifest`.
- Cover: ≥80% identity/selection logic.
- Kahneman: #13/#16/#17.

**Existing origin and GPU owners**
- Purpose: consume user ceilings without changing sealed origin identity or
  exceeding current live GPU safe target.
- RF / DT: RF-3..RF-4, RF-8; DT-3, DT-12..DT-13.
- Required tests: `user_gpu_cap_never_exceeds_fresh_same_adapter_safe_target`,
  `user_gpu_cap_refuses_stale_or_ambiguous_adapter`, plus the existing sealed
  origin/path and fixed-size suites.
- Cover: ≥80% new business logic.
- Kahneman: #2/#13/#16.

**`scripts/safety/wslconfig-lib.sh`**
- Purpose: preserve existing profile defaults but expose explicit selected
  `swap`/`swapFile` staging to the Windows host owner; never change `memory=`.
- RF / DT: RF-3, RF-9..RF-10; DT-4, DT-10.
- Required tests: `wslconfig_render_preserves_unselected_values`,
  `wslconfig_rejects_unsafe_selected_path_and_over_budget_swap`.
- Cover: N/A — shell configuration renderer.
- Kahneman: #13/#17.

**`docs/reliability/DEGRADATION-MATRIX.md`, `ARCHITECTURE.md`,
`docs/INDEX.md`**
- Purpose: document supported/unsupported providers, refusal states, and
  host/guest data flow after implementation.
- RF / DT: RF-1, RF-10..RF-13; DT-1..DT-18.
- Tests: `./scripts/docs-check.sh` and `node tools/generate-docs-index.mjs --check`.
- Cover: N/A — documentation.

### DELETE

None. Existing Linux cascade, WSL configuration, origin, and GPU owners remain
in place.

## Observability

| Signal | Where | Level / type |
| --- | --- | --- |
| Platform/resource observations | `ramshared config show` and `plan --json` | source + timestamp + freshness + stable identity + available/unsupported reason |
| Benchmark | TUI and local JSONL | raw per-sample latency/throughput, bytes, hash, target identity, abort/cleanup state |
| Apply/rollback | Linux and Windows local JSONL | transaction ID, before/after hashes, owner, status, recovery action |
| Linux swap lifecycle | `ramshared config show --json`, `swapon --show`, systemd unit state | active/disabled/pending-cleanup with exact managed path and usage |
| WSL delayed change | interactive result and `show` | `pending_wsl_restart=true`; no automatic shutdown |
| GPU cap | existing status JSON | configured cap and current same-adapter safe clamp |

## Living docs

| Document | Action |
| --- | --- |
| `ARCHITECTURE.md` | Add shared config and native/WSL provider data flow after implementation. |
| `docs/decisions/ADR-resource-configuration-center.md` | Create if provider or helper boundary changes during implementation. |
| `docs/reliability/DEGRADATION-MATRIX.md` | Add stale telemetry, mount/volume drift, unsupported filesystem, benchmark cleanup failure, partial apply, and old-swap cleanup pending. |
| `docs/reliability/GAP-REGISTER.md` | Keep implementation and platform qualification statuses distinct. |
| `docs/INDEX.md` | Regenerate after add/remove under `docs/specs/`. |
| `validation.md` | Append only after actual tests/live proof; never mark external platform proof from manufactured tests. |
| `docs/BENCHMARKS.md` | Update only after a qualified fresh workload result. |

## Implementation order

1. Add failing pure profile/policy tests for variable caps, overflow, platform
   mismatch, unknown telemetry, stable resource IDs, and combined storage
   capacity before any write.
2. Implement read-only Linux/WSL inventory and `show`/`plan`; no helper writes.
3. Implement interactive volume/role/size draft creation and safe no-overwrite
   profile output; prove native Linux and Windows volume selections with
   manufactured candidate inventories and keep the generated plan read-only.
4. Implement Linux helper request bounds and manufactured volume/mount/filesystem
   identity tests; enable only ext4/XFS.
5. Add the native file-origin manifest parser/open path and lifecycle selector;
   keep the current v3 sealed block-origin reader intact and test platform
   selection, identity drift, allocation, and manifest replay.
6. Implement create-once managed Linux swapfile and systemd unit; test all
   partial failures and preserve existing active swaps. Add explicit activate
   and zero-use cleanup commands only after the ownership tests pass.
7. Implement bounded per-platform volume benchmark with global byte cap,
   volume identity pinning, supervised worker, measured timeout, and exact
   cleanup custody.
8. Implement the Windows host helper for WSL `swap`/`swapFile`, retaining all
   unrelated `.wslconfig` bytes and exact rollback. Do not modify `memory=`.
9. Wire resource profile caps into existing ZRAM/GPU/origin owners; revalidate
   caps at every live admission, not only at apply.
10. Run Rust coverage, Linux manufactured-helper tests, Windows PowerShell
   manufactured tests, and docs checks. Then run separate disposable native
   Linux and WSL2 before/action/after drills. Record absent hardware/filesystem
   classes as partial, not supported.

## Required tests matrix

| Production path | Test (`file` :: `test_name`) | Kind | Kahneman | Cover |
| --- | --- | --- | --- | --- |
| `crates/ramshared-cli/src/resource_config.rs` | `platform_detection_distinguishes_native_linux_from_wsl2` | unit | #13 | slice gate |
| `crates/ramshared-cli/src/resource_config.rs` | `linux_block_inventory_preserves_mounted_and_unmounted_devices` | unit | #13 | slice gate |
| `crates/ramshared-cli/src/resource_config.rs` | `mountinfo_parser_decodes_paths_and_records_mount_identity_and_access` | unit | #13 | slice gate |
| `crates/ramshared-cli/src/resource_config.rs` | `storage_candidate_requires_current_writable_mount_capacity_and_stable_identity` | unit | #13/#16 | slice gate |
| `crates/ramshared-cli/src/resource_config.rs` | `network_backed_block_devices_are_ineligible_and_multiple_local_disks_remain_eligible` | unit | #13/#16 | slice gate |
| `crates/ramshared-cli/src/resource_config.rs` | `mounted_whole_disk_filesystem_uses_its_own_stable_identity` | unit | #13/#16 | slice gate |
| `crates/ramshared-cli/src/resource_config.rs` | `wsl2_guest_storage_requires_host_volume_identity_and_capacity_binding` | unit | #13/#16 | slice gate |
| `crates/ramshared-cli/src/resource_config.rs` | `meminfo_accepts_user_sized_ram_and_swap_without_product_minima` | unit | #13 | slice gate |
| `crates/ramshared-cli/src/resource_config.rs` | `mount_capacity_uses_live_available_blocks_without_writing` | read-only live unit | #13 | slice gate |
| `crates/ramshared-cli/tests/cli_dispatch.rs` | `cli_resource_config_json_discovers_platform_resources_read_only` | CLI E2E | #13 | N/A — dispatch |
| `crates/ramshared-cli/src/main.rs` | `config_command_accepts_draft_mode_and_requires_output_path` | unit | #13 | N/A — parser |
| `crates/ramshared-cli/src/resource_config.rs` | `config_draft_builds_native_targets_from_eligible_mounts` | unit | #13/#16 | ≥80% |
| `crates/ramshared-cli/src/resource_config.rs` | `config_draft_builds_wsl_targets_from_unique_eligible_volumes` | unit | #13/#16 | ≥80% |
| `crates/ramshared-cli/src/resource_config.rs` | `config_draft_builds_wsl_volume_guid_target_without_drive_letter` | unit | #13/#16 | ≥80% |
| `crates/ramshared-cli/src/resource_config.rs` | `config_draft_refuses_ineligible_ambiguous_stale_and_overflowed_targets` | unit | #13/#16 | ≥80% |
| `crates/ramshared-cli/src/resource_config.rs` | `config_draft_save_requires_owned_parent_uses_mode_0600_and_never_overwrites` | unit | #13/#17 | ≥80% |
| `crates/ramshared-cli/src/resource_config.rs` | `config_draft_wizard_saves_planned_caps_as_unenforced_draft_policy` | unit | #13/#17 | ≥80% |
| `crates/ramshared-cli/tests/cli_dispatch.rs` | `cli_resource_config_draft_refuses_non_tty_before_writing` | CLI refusal E2E | #13/#16 | N/A — dispatch |
| `crates/ramshared-cli/src/resource_config.rs` | `resource_plan_aggregates_case_aliases_before_capacity_check` | unit | #9/#13/#16 | ≥80% |
| `crates/ramshared-config/tests/resource_profile.rs` | `resource_profile_groups_windows_volume_ids_case_insensitively_for_capacity` | unit | #9/#13 | ≥80% |
| `crates/ramshared-config/src/resource_profile.rs` | `resource_profile_accepts_variable_caps_and_rejects_overflow` | unit | #9/#13 | ≥80% |
| `crates/ramshared-config/src/resource_profile.rs` | `resource_profile_roundtrips_stable_volume_and_adapter_ids` | unit | #17 | ≥80% |
| `crates/ramshared-config/src/resource_profile.rs` | `resource_profile_supports_multiple_targets_on_one_and_multiple_volumes` | unit | #9/#13/#17 | ≥80% |
| `crates/ramshared-config/src/resource_profile.rs` | `resource_profile_rejects_duplicate_managed_paths_and_capacity_overflow` | unit | #13/#16 | ≥80% |
| `crates/ramshared-config/src/resource_profile.rs` | `resource_profile_rejects_transient_mount_id_in_persisted_targets` | unit | #13/#17 | ≥80% |
| `crates/ramshared-config/tests/resource_profile.rs` | `resource_profile_accepts_a_new_linux_origin_request_without_a_preexisting_inode` | unit | #13/#17 | ≥80% |
| `crates/ramshared-cli/src/resource_config.rs` | `config_plan_never_mutates_host_or_guest` | unit | #13 | ≥80% |
| `crates/ramshared-cli/src/resource_config.rs` | `native_linux_plan_resolves_current_mount_from_stable_filesystem_identity` | unit | #13/#16 | ≥80% |
| `crates/ramshared-cli/src/resource_config.rs` | `native_linux_origin_request_plan_binds_volume_without_claiming_creation` | unit | #13/#16 | ≥80% |
| `crates/ramshared-cli/src/resource_config.rs` | `native_linux_profile_survives_a_new_mount_namespace_id` | unit | #13/#17 | ≥80% |
| `crates/ramshared-cli/src/resource_config.rs` | `native_linux_plan_refuses_multiple_current_mounts_for_one_profile_identity` | unit | #13/#16 | ≥80% |
| `crates/ramshared-cli/src/resource_config.rs` | `storage_candidate_rejects_filesystem_subtree_mounts` | unit | #13/#16 | ≥80% |
| `crates/ramshared-cli/src/resource_config.rs` | `resource_policy_rejects_unknown_stale_and_inconsistent_samples` | unit | #13 | ≥80% |
| `crates/ramshared-cli/src/resource_config.rs` | `resource_plan_without_profile_reports_not_configured_and_read_only` | unit | #13 | ≥80% |
| `crates/ramshared-cli/src/resource_config.rs` | `resource_plan_rejects_drive_and_volume_guid_aliases_for_same_target` | unit | #13/#16 | ≥80% |
| `crates/ramshared-cli/src/resource_config.rs` | `profile_loader_rejects_symlinks_oversized_files_and_untrusted_system_profiles` | unit | #13/#16 | ≥80% |
| `crates/ramshared-cli/src/resource_config.rs` | `windows_inventory_lists_every_volume_and_explains_ineligible_targets` | unit | #13/#16 | ≥80% |
| `crates/ramshared-cli/src/resource_config.rs` | `windows_inventory_probe_does_not_filter_volumes_by_drive_type` | source contract | #13 | ≥80% |
| `crates/ramshared-cli/tests/cli_dispatch.rs` | `cli_resource_config_plan_loads_an_explicit_profile_without_applying_it` | CLI E2E | #13 | N/A — dispatch |
| `crates/ramshared-cli/src/main.rs` | `config_command_accepts_interactive_show_and_read_only_plan_modes` | unit | #13 | N/A — parser |
| `crates/ramshared-cli/src/resource_config.rs` | `config_apply_is_idempotent_and_refuses_changed_rollback_target` | unit | #17 | ≥80% |
| `crates/ramshared-cli/src/resource_config.rs` | `config_apply_requires_durable_intent_before_mutation` | unit | #13/#16 | ≥80% |
| `crates/ramshared-cli/src/resource_config.rs` | `native_origin_plan_accounts_combined_volume_capacity` | unit | #9/#13 | ≥80% |
| `crates/ramshared-cli/src/resource_config/linux.rs` | `volume_plan_refuses_unstable_identity` | unit | #13 | ≥80% |
| `crates/ramshared-cli/src/resource_config/linux.rs` | `volume_inventory_lists_unmounted_device_as_ineligible` | unit | #13 | ≥80% |
| `crates/ramshared-cli/src/resource_config/linux.rs` | `linux_swap_activation_refuses_changed_mount_or_active_ramshared` | unit | #13/#16 | ≥80% |
| `crates/ramshared-cli/src/resource_config/linux.rs` | `linux_swap_cleanup_refuses_last_persistent_fallback` | unit | #13/#16 | ≥80% |
| `scripts/linux/ramshared-resource-config-helper` | `linux_swap_apply_creates_only_owned_file_and_unit` | manufactured | #13 | N/A — privileged helper |
| `scripts/linux/ramshared-resource-config-helper` | `linux_swap_apply_preserves_old_active_swap_on_failure` | manufactured | #16 | N/A — privileged helper |
| `scripts/linux/ramshared-resource-config-helper` | `linux_swap_cleanup_refuses_foreign_active_or_used_target` | manufactured | #16/#17 | N/A — privileged helper |
| `scripts/linux/ramshared-resource-config-helper` | `benchmark_binds_volume_caps_bytes_and_verifies_cleanup` | manufactured | #9/#13 | N/A — privileged helper |
| `scripts/linux/ramshared-resource-config-helper` | `benchmark_lease_refuses_concurrent_or_unreaped_worker` | manufactured | #13/#16/#17 | N/A — privileged helper |
| `scripts/linux/ramshared-resource-config-helper` | `benchmark_workload_obeys_32mib_payload_ceiling` | manufactured | #9/#16 | N/A — privileged helper |
| `scripts/windows/Invoke-RamSharedResourceConfig.ps1` | `volume_inventory_includes_all_candidates_and_reasons` | manufactured | #13 | N/A — PowerShell |
| `scripts/windows/Invoke-RamSharedResourceConfig.ps1` | `wsl_config_apply_only_stages_next_start` | manufactured | #13/#17 | N/A — PowerShell |
| `scripts/windows/Invoke-RamSharedResourceConfig.ps1` | `wsl_config_refuses_zero_without_persistent_guest_fallback` | manufactured | #13/#16 | N/A — PowerShell |
| `scripts/windows/Invoke-RamSharedResourceConfig.ps1` | `benchmark_binds_volume_caps_bytes_and_verifies_cleanup` | manufactured | #9/#16 | N/A — PowerShell |
| `scripts/windows/Invoke-RamSharedResourceConfig.ps1` | `benchmark_lease_refuses_concurrent_or_unreaped_worker` | manufactured | #13/#16/#17 | N/A — PowerShell |
| `scripts/windows/Invoke-RamSharedResourceConfig.ps1` | `benchmark_workload_obeys_32mib_payload_ceiling` | manufactured | #9/#16 | N/A — PowerShell |
| `scripts/safety/wslconfig-lib.sh` | `wslconfig_render_preserves_unselected_values` | shell | #17 | N/A — shell |
| existing GPU owner | `user_gpu_cap_never_exceeds_fresh_same_adapter_safe_target` | unit | #2/#13 | ≥80% |
| `crates/ramshared-wsl2d/src/main.rs` | disposable WSL2 before/action/after with paired host/guest state and `BINARY_MATCH` | E2E | #13/#16 | platform-bound |
| `crates/ramshared-wsl2d/src/native_origin.rs` | `native_origin_manifest_binds_open_fd_identity` | unit | #13/#17 | ≥80% |
| Linux provider | disposable ext4/XFS before/action/after, native file origin + swap + systemd + exact cleanup | E2E | #13/#16 | platform-bound |

## Validation checklist

- [ ] `cargo fmt --all -- --check`; `cargo clippy -p ramshared-config -p ramshared-cli -- -D warnings`; focused and workspace tests.
- [ ] Cover gate: `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-cli,ramshared-config --files crates/ramshared-cli/src/resource_config.rs,crates/ramshared-config/src/resource_profile.rs --min 80`. GPU budget policy coverage is owned by the GPU worker specification.
- [ ] Linux provider manufactured tests prove exact ownership, supported filesystem rules, active-swap preservation, and cleanup refusal.
- [ ] Windows provider manufactured tests prove volume identity, `.wslconfig` preservation, restart pending, and exact rollback.
- [ ] `bash scripts/safety/wslconfig-ctl.sh selftest` and existing origin/GPU suites pass.
- [ ] `./scripts/docs-check.sh` passes and the generated docs index is current.
- [ ] Native Linux and WSL2 live E2E each have a before/action/after evidence set; tests on one platform do not qualify the other.
- [ ] No `DONE` or release claim before both platform E2E and representative filesystem/GPU hardware evidence.

## Rollback trigger

Abort the transaction if any profile/config hash changes, any selected volume,
filesystem, mount, or adapter identity changes, free space crosses its
displayed floor, telemetry becomes unavailable/stale, or a backup/audit record
cannot be verified. Roll back only exact transaction-owned files whose live
hash still equals the recorded transaction output; otherwise preserve state
and report `manual_recovery_required`. Never call `swapoff` as part of apply or
rollback, delete an active/used swapfile, alter `/etc/fstab`, stop WSL, disable
an active cascade's fallback, or reclaim a live sealed origin.
