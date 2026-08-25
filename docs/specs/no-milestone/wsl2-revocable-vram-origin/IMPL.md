# IMPL — Revocable VRAM cache with authoritative SSD origin

Status: **PARTIAL — P0/P1 source correction implemented; live qualification remains blocked**.

## Current boundary — disabled staging only

This implementation record authorizes no origin provisioning, VHDX/device/GPU
action, NBD/swap transition, WSL operation, VM lifecycle, formatting, or
pressure. The source/static evidence below is not live qualification. Managers
remain disabled/plan-only until every environment-bound gate closes under
separate attended approval. The legacy source-removal prerequisite is closed.

## Implemented

- Privileged lifecycle effects now keep an exact device FD and `dev_t` pinned.
  `mkswap`, `swapon`, `swapoff`, and `blkid` receive a process-FD path when
  their ABI allows it, with named-path and pinned-FD revalidation around the
  call. `zramctl` reset and NBD detach still require canonical `/dev/<name>` to
  derive sysfs; those calls retain the FD pin and require exact post-effect
  state. This narrows but cannot eliminate the external-tool/kernel boundary.
- A failed or timed-out NBD attach no longer kills the backend from command
  failure alone. Two stable global snapshots and two exact target observations
  must prove unchanged cardinality, matching node/sysfs `dev_t`, no PID, zero
  sectors, zero holders, and strict swap absence. Effect-before-timeout seals
  provisional lifecycle/path evidence and preserves the daemon; ambiguity does
  the same.
- Malformed successful zram allocation output is reconciled against exact
  before/after sets. Only one new inactive zram device may be reset, and the
  final complete set must equal the pre-call set before its record is removed.
- Origin admission holds both partition and parent FDs, sends their process-FD
  paths to `blkid`, compares node/sysfs/opened `dev_t`, re-resolves both named
  identities after the probes, and requires stable dynamically discovered
  root/active-swap identities before accepting the manifest.

- `AuthoritativeOriginBackend` acknowledges writes only after the authoritative
  origin accepts the complete range. Ordinary writes are batched; FLUSH and FUA
  force `sync_data` before acknowledgement. Cache updates are best effort after
  origin success and never define durability.
- `BoundedCacheClient` uses bounded response waits for reads and non-blocking
  mutation sends. Timeout, disconnect, or queue saturation revokes the cache
  and falls back to the origin. Disable/release travels on a dedicated bounded
  control lane, remains deliverable after local `Unavailable` and a full data
  queue, and reports zero/success only after the worker acknowledges release.
  Product origin serving currently selects
  `DisabledCache`, so it initializes no GPU/DXG provider and cannot block on a
  synchronous driver call. A real isolated GPU cache worker remains a live
  acceleration gate, not a correctness dependency.
- Daemon subprocess probes now use invocation-private process groups, a
  256 KiB stdout ceiling, concurrent drain, and Linux non-reaping `waitid`
  observation. The zombie leader pins PID/PGID until residual exact-group
  members and inherited pipes are contained, then bounded final reap completes.
  Descendant-inherited output and TERM-ignoring helpers fail closed; an injected
  unreapable target proves exit-125 fatal containment without killing the test
  process. This contract is limited to trusted helpers that do not deliberately
  change their session/group or daemonize.
- GPU base allocation precedes memory locking. The daemon permits only
  `MCL_CURRENT`; any request carrying `MCL_FUTURE` refuses before later GPU/DXG
  mappings.
- The 128 MiB chunk cache tracks 4 KiB validity and generation, reads only
  matching clean pages, promotes origin reads opportunistically, and can release
  clean least-recent chunks without whole-device swapoff.
- Target policy combines logical capacity, a sealed physical cache cap, WDDM
  budget, external GPU use, and `max(2 GiB,20% total VRAM)` headroom. Missing
  measurement targets zero. Growth is one chunk per two seconds after three
  healthy samples; three restricted samples reclaim immediately.
- The product daemon defaults to 4 GiB logical capacity, accepts 1–24 GiB, and
  accepts origin identity only through a root-owned schema-v3 manifest. The
  manifest binds its own and the host manifest hashes, PARTUUID, PTUUID,
  partition and parent `dev_t`, capacity, swap type, and expected swap UUID.
  The daemon opens one no-follow regular manifest FD, reads at most the maximum
  plus one byte, and revalidates opened/named dev+ino+type+size before accepting
  it; symlink/nonregular input, append, and path replacement refuse. It then
  revalidates the origin block FD and dynamically excludes the root and every
  active swap backing device plus their physical parents.
- Normal startup never formats storage. It verifies an already provisioned
  exact swap UUID/type. `provision-origin-swap.sh` is a separate plan-first,
  one-time path with a versioned exact approval. It enforces the exact raw host
  manifest hash and canonical configuration hash, opens and exclusively locks
  the partition once, repeats strict active-swap/root/mount and `dev_t`/sysfs/
  UUID proofs, invokes its only `mkswap` through the held `/proc/<pid>/fd/<n>`,
  and proves the same identity immediately afterwards. Source tests never run
  `mkswap` on any device.
- The legacy selector/profile chooser and full-VRAM NBD composition are absent;
  generic `VramBackend` remains available only to broker, ublk, and Windows
  consumers.
- Capacity/status schema v4 distinguishes logical capacity, cached VRAM, GPU
  headroom, authoritative SSD writes, fallback reads/invalidations, origin/cache
  state, and existing WSL fallback swap use. Broker slice accounting is
  published before its reply, making reply receipt a completion barrier; the
  bounded concurrency regression verifies 512 successive publications without
  sleeps and arms shutdown before any assertion can unwind.
- Unauthenticated TCP listeners accept only loopback, RFC1918 IPv4, IPv6 ULA,
  and exact Tailscale CGNAT `100.64.0.0/10`. Unix startup never unlinks an
  existing path. It validates parents without following links, pins the exact
  newly bound socket inode, and cleans only an unchanged owned identity; an ABA
  replacement survives old cleanup.
- Worker shutdown checks terminal state before every queue receive/dispatch, so
  a full or continuously refilled queue cannot delay stop and admitted work is
  not drained after terminal state. Broker panic/error is observed and returned
  only after bounded worker cleanup; RAII requests shutdown on unwind.
- `Manage-RamSharedOrigin.ps1` is plan-first for a separate 25 GiB origin VHDX
  on a policy-selected physical volume, a raw GPT PARTUUID manifest, and sealed
  configuration. It discovers the host volume from the VHDX path instead of
  assuming drive `I:`. The candidate host gate validates manifest hash,
  PARTUUID, and policy before any future qualification.
- Rust is pinned to exactly 1.98.0 in the workspace, toolchain file, workflows,
  and release provenance. Live DXG tests are ignored unless an attended run
  sets `RAMSHARED_ALLOW_LIVE_DXG_TESTS=1`.

## Current R5 hermetic source checkpoint

This checkpoint applies to the current dirty worktree and does not claim a live
device result.

| Path | Exact named tests or contract | Evidence |
| --- | --- | --- |
| `isolated_origin.rs` | Existing timeout/disconnect/durability tests plus `bounded_cache_client_covers_hit_miss_mutation_and_disable_protocol`, `bounded_cache_client_revokes_on_protocol_queue_and_transport_faults`, `cache_disable_remains_deliverable_after_unavailable_full_data_queue`, `release_cache_returns_zero_only_after_dedicated_control_acknowledgement`, `backend_geometry_range_and_empty_io_are_bounded`, `backend_cache_paths_preserve_origin_authority_and_telemetry`, and `origin_failure_requires_three_successful_read_sync_probes` | Hermetic unit tests cover protocol, dedicated control delivery/ack, queue, transport, geometry, cache hit/fallback/revoke, origin failure, and three-probe recovery without a device. |
| `origin_cache.rs` and request/backend code | Origin-first writes, ordinary-write batching, FLUSH, FUA, cache failure, and sticky-origin recovery cases | Included in 76/76 passing `ramshared-block --all-targets` tests. |
| `wsl2d/main.rs` | Exact manifest-FD, private-listener, owned-socket, terminal/refill, broker-panic, origin, helper-success/timeout/TERM-ignore/inherited-pipe, and fatal-seam tests named in the SPEC matrix | 86/86 binary and 114/114 library tests passed in the package all-targets gate. Hermetic tests cover max+1 manifest reads, path/append races, endpoint ownership, 512 continuous refills without sleeps, panic cleanup, and exact fixture process groups. The unsafe old drain-on-stop contract is intentionally replaced by `daemon_worker_shutdown_preempts_queued_io_at_iteration_boundary`. |
| Host-manifest enforcement | `host_manifest_hash_fields_are_enforced_end_to_end`; provisioner raw/canonical hash and FD-bound static contract | Implemented in the current remediation; fresh serial evidence pending. |
| `cascade_io.rs` and supervisor | InvocationID refusal; explicit provisioning; three NBD connect rollback cases; swapoff-before-disconnect; failed-swapoff retention | Focused one-process unit tests passed. |
| Systemd/provisioning shell | `backend_lifecycle_has_no_pre_swapoff_kill_path`; `lifecycle_recovery_requires_marker_swap_daemon_and_detach_terminal_proof`; identity/provisioning refusal fixtures | Static/manufactured tests passed; no device action occurred. |
| Windows origin/recovery/telemetry | Dynamic physical-volume discovery, plan-first lifecycle recovery, bounded host calls, and no fixed production `I:` | PowerShell parser and static tests passed; no VHDX or WSL lifecycle action occurred. |
| `ramshared-dxg` | Eight hermetic tests plus two explicit live tests | Eight passed; two live tests were ignored. |
| Product sunset | `legacy_preallocation_removed_before_day0_deadline` | **PASS** — clean active-source/current-doc scan, named checker test, thresholded checker coverage, and documentation governance close only the executable source-governance prerequisite. |

Workspace all-targets tests, rustfmt, and Clippy with `-D warnings` passed after
clean process preflights. Canonical current-worktree line coverage passed:
`isolated_origin.rs` 629/644 (97.7%) and `wsl2d/main.rs` 5860/7185 (81.6%).
The wsl2d all-targets gate discovered but did not execute 19 explicitly ignored
root/device/GPU/live-platform tests. No Guard service was started or installed,
and no live device, swap, GPU, storage, WSL, or VM action occurred.

## Legacy-preallocation source gate

The temporary full-VRAM NBD composition and
`RAMSHARED_VRAM_PREALLOC_LEGACY` selector were removed from executable source.
The clean source/current-doc scan, named
`legacy_preallocation_removed_before_day0_deadline` test, thresholded checker
coverage, and documentation-governance evidence close this source-governance
gate. Append-only `validation.md` and exact historical evidence records are
excluded from availability findings because they describe old builds; living
docs are not.

If a later change breaks the origin-cache build or any named refusal, rollback
is fail-closed: keep activation disabled, reject promotion, restore only the
exact reviewed origin-capable source snapshot needed to recover the build, and
do not restore the selector. Fresh source evidence is required before any
requalification.

## Open evidence

- A future attended rollout would need to create/attach the policy-selected
  VHDX and validate its real PARTUUID/PTUUID/dev_t and swap UUID; no storage
  mutation occurred here.
- On an isolated Windows/WSL surface with a proven functional guest WSL
  runtime, prove real NBD write→VRAM release→SSD read hash identity, GPU
  allocation failure without EIO, 4/2/1 cap transitions, logical 6/8/12/24 GiB
  rows, zram→RamShared→existing WSL swap, and terminal no-ghost teardown.
  The approved nested lab now passes bounded PowerShell Direct and WSL runtime
  readiness after a recoverable reimage. This closes access/readiness only;
  guardian, origin, pressure, GPU/WDDM, and rollout gates remain open. GPU/WDDM
  acceptance additionally requires a representative physical or explicitly
  GPU-PV-qualified surface; generic nested-VM readiness evidence cannot close
  that gate.
- Unit fault injection proves bounded timeout/disconnect fallback. The product
  remains origin-only until a process-isolated GPU worker proves that a driver
  hang cannot hold the serving process. A thread blocked forever inside a
  driver still requires process-boundary teardown qualification; no unit result
  may claim that environment-bound driver recovery is solved.
- The provisioner closes mutable-path retargeting with an open descriptor,
  exclusive lock, and immediate pre/post kernel identity proof. Physical
  hot-unplug or kernel device replacement during the formatting syscall remains
  a live-only isolated gate and is not claimed solved by source tests.
- The daemon and lifecycle controller close ordinary pathname substitution with
  open descriptors and pre/post identity checks. They cannot make a separate
  external executable or the kernel consume an already-open userspace FD when
  that executable's ABI requires a canonical device name. Those narrowly named
  zram/NBD boundaries remain live qualification gates and fail closed on any
  identity or cardinality uncertainty.

Rollback selects no RamShared activation and preserves the candidate in disabled
staging. Legacy preallocation was removed and is not a recovery path.
