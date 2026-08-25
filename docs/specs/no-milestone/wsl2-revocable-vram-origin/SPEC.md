# SPEC — Revocable VRAM cache with authoritative SSD origin

## Closed scope

In now: generic write-through cache backend, origin identity/configuration,
daemon composition, schema v4 telemetry, tests, and plan-first Windows source.
Out now: real storage mutation, live NBD/swap, GPU pressure, and performance
claims. Existing NBD protocol and kernel ABI are unchanged.

## Current boundary — disabled staging only

No contract in this SPEC authorizes a live lifecycle, origin/VHDX/device/GPU
action, WSL operation, VM action, formatting, or pressure. Candidate managers
must emit disabled plans only. Qualification, release promotion, and activation
remain blocked on the environment-bound gates below. The ITEM-7 executable
source-removal prerequisite is closed and does not promote those live gates.

## Traceability

| PRD | SPEC |
| --- | --- |
| RF-1, RF-2 | ITEM-1, ITEM-2 |
| RF-3–RF-6 | ITEM-2 |
| RF-7, RF-8 | ITEM-3 |
| RF-9 | ITEM-4 |
| RF-10 | ITEM-5 |
| RF-11 | ITEM-6 |
| RF-12 | ITEM-2, ITEM-4 |
| RF-15, RF-16, RF-17 | ITEM-4, ITEM-5 |
| NFR-1, NFR-3 | ITEM-1, ITEM-2, ITEM-4 |
| NFR-2 | ITEM-2, ITEM-3 |
| NFR-4 | ITEM-5, ITEM-6 |
| NFR-5 | ITEM-7 |
| RF-14, NFR-6, NFR-7 | ITEM-4 |

## Technical decisions

| # | Decision | Why |
| --- | --- | --- |
| DT-1 | Change `BlockBackend::read_at` to `&mut self`. | Read fallback may promote cache without unsafe interior mutation. |
| DT-2 | `WriteThroughCacheBackend<P,O>` owns provider, origin, and 128 MiB chunk metadata; validity granularity equals block size. | Partial writes remain correct. |
| DT-3 | `OriginStorage` exposes exact positional read/write plus `sync_data`. Full writes join a dirty epoch without per-write sync. NBD advertises FLUSH and FUA; FLUSH syncs the epoch, and FUA syncs the completed write before reply. Unknown flags fail before mutation. A positional, FLUSH, or FUA failure returns `IoError`; durability failure invalidates cache state for the affected epoch. | Standard block durability without a sync storm or false durable acknowledgement. |
| DT-4 | The authoritative origin backend never performs a synchronous GPU call. A bounded cache client uses nonblocking writes/promotions and timeout-bounded read probes; timeout, disconnect, injected stall, and GPU/cache errors invalidate or bypass cache and return origin success. | A hung cache cannot stop swap origin progress. |
| DT-5 | A cache page is readable only when its validity bit is set for the current generation. | No stale/uninitialized reads. |
| DT-6 | Growth needs three healthy observations, respects both the measured target and a sealed physical cache cap, and is limited to one chunk per two seconds. Three restricted observations synchronously reclaim LRU clean chunks to target. The rollout default cap is 1 GiB and may be raised only up to logical capacity. | Exact elastic policy and staged rollout bound. |
| DT-7 | Headroom is `max(2GiB,ceil(total/5))`; unavailable WDDM inputs yield target zero. | GPU remains usable and unknown fails safe. |
| DT-8 | Product startup accepts only a sealed guest manifest. Before use it opens the by-PARTUUID path without following a caller override and binds configuration SHA-256, FD `st_rdev`, partition PARTUUID, parent PTUUID/`dev_t`, size, and expected swap UUID. It dynamically excludes current root, every active swap, and their parent devices. Host tooling discovers distro and configured WSL swap VHDX paths instead of assuming a drive. | Strong identity and critical-device exclusion before use. |
| DT-9 | Legacy full-VRAM NBD preallocation is absent from executable source and is not a product, laboratory, qualification, release, activation, or rollback path. | Day-0 single-path requirement. |
| DT-10 | Capacity receipt schema v4 records mode `origin-cache`, logical/cache/SSD counters and the PARTUUID, never a host drive path. The broker publishes slice byte/IO counters before sending the corresponding reply; receiving the reply is the completion barrier for that accounting. | Public status clarity without private paths or stale post-completion counters. |
| DT-11 | Origin states are `OFF/READY/DEGRADED/FAILED`; cache states are `OFF/ACTIVE/RESTRICTED/UNAVAILABLE/STUCK`. Origin I/O failure is `FAILED` and sticky until three successful read+sync probes. Missing/stale GPU measurement is cache `UNAVAILABLE`; target excess above one chunk for over two seconds is `STUCK`. Each maps to control SPEC DT-8 and forces `ok=false` except `OFF` when the product is off. | Exact recovery and false-green prevention. |
| DT-12 | The `RAMSHARED_VRAM_PREALLOC_LEGACY` selector and its full-VRAM NBD composition were removed. The named checker rejects aliases, profile selection, NBD composition, and current docs that advertise availability; append-only validation and exact historical evidence paths remain readable. | Enforces the Day-0 single-path policy without deleting historical evidence. |
| DT-13 | Runtime locking uses `MCL_CURRENT` only and rejects every request for `MCL_FUTURE`. The broker creates its base GPU allocation before that frontier; product origin mode initializes no GPU provider, and later canary/cache/DXG mappings never inherit a process-wide future-lock obligation. | Prevent later GPU mappings from inheriting an unbounded lock obligation while retaining current-page protection before serving swap. |
| DT-14 | Provisioning owns VHDX creation, partitioning, and initial `mkswap` behind attended exact approval. Normal `up` only verifies the sealed swap UUID/type and refuses missing or changed signatures. | Startup can never format a wrong device. |
| DT-15 | Daemon and provisioner independently hash the exact bounded host manifest bytes, require that hash to equal the sealed guest value, recompute the canonical host configuration hash, and require host/guest PARTUUID, disk GUID/PTUUID, swap UUID, capacities, and policy to agree. | Hash fields are enforcement, not telemetry-only decoration. |
| DT-16 | The provisioner opens the resolved partition once read-write, holds a narrowly scoped exclusive `flock`, targets `mkswap` only through `/proc/<pid>/fd/<n>`, and proves partition/parent `dev_t`, sysfs identity, PARTUUID, PTUUID, active-swap absence, mount absence, and capacity immediately before and after the write. Tests never invoke `mkswap`. | A mutable pathname cannot retarget the destructive boundary between validation and open. |
| DT-17 | Every `/proc/swaps` read used by origin exclusion is strict: exact header, exactly five typed columns, no duplicate identity, and explicit `Result`. Unreadable or malformed snapshots refuse. An active row with `Used=0` remains active. | Destructive identity checks cannot interpret parser uncertainty as absence. |
| DT-18 | Before any device effect, bind the exact block node to an open FD and `dev_t`; pass `/proc/<pid>/fd/<fd>` to tools that accept arbitrary block paths and revalidate the named path plus pinned FD before and after. Tools such as `zramctl` and NBD detach that derive sysfs from `/dev/<name>` retain the FD pin but use the canonical name, followed by exact cardinality/status proof. | Closes path retargeting where the tool ABI permits and fails closed at the remaining external-tool/kernel boundary without pretending userspace can remove it. |
| DT-19 | A failed/timed-out NBD attach permits backend termination only after two stable live-device snapshots equal the pre-effect set and two stable exact-target observations prove matching node/sysfs `dev_t`, no owner PID, zero exported sectors, zero holders, and strict swap absence. One stable new exact target seals provisional lifecycle evidence and preserves the backend; all ambiguity preserves it. | Handles effect-before-timeout without killing the server behind a live block device. |
| DT-20 | Successful malformed `zramctl --find` output triggers before/after reconciliation. Exactly one new inactive zram device is recorded, FD/`dev_t` bound, reset, and required to return the complete managed-device set to the pre-call snapshot. No-effect may try the next algorithm; ambiguity never resets a guessed device. | Prevents allocator protocol failure from leaking or resetting a foreign zram device. |
| DT-21 | Trusted, non-daemonizing daemon helpers start as leaders of invocation-private process groups and capture at most 256 KiB of stdout. Linux `waitid(WEXITED\|WNOHANG\|WNOWAIT)` observes exit without reaping, so the zombie leader pins PID/PGID while residual exact-group members and inherited pipes are contained. Timeout, overflow/disconnect, wait failure, and spawn-callback panic retain RAII custody through bounded final reap. Failure to prove reap selects daemon exit 125 through an injected fatal seam. Helpers that deliberately change session/group or daemonize are outside this contract. | Origin monitoring and teardown probes cannot become an unbounded dependency or leave a stale trusted helper behind. |
| DT-22 | Cache read/update/promotion stays on a bounded data lane, while `Disable` owns a dedicated bounded control lane and reply acknowledgement. Disable remains deliverable after local `UNAVAILABLE` and with a full data queue; release returns zero/success only after the worker acknowledgement or an already-disabled state. | Revocation cannot be starved by the data path or falsely reported complete. |
| DT-23 | The listener parser admits only loopback, RFC1918 IPv4, IPv6 ULA, and exact `100.64.0.0/10`. Unix startup opens parents component-by-component without following links and refuses every existing target, including a socket. A newly bound socket is dev+ino+type+owner captured and held by an `O_PATH` inode pin; cleanup is FD-relative and unlinks only that unchanged identity. | An unauthenticated public listener, second daemon, stale socket guess, symlink parent, or ABA replacement cannot be mistaken for an owned endpoint. |
| DT-24 | The broker worker checks terminal state at the top of every iteration, before consuming queued work, regardless of queue fullness or wake-slot availability. Shutdown preempts rather than drains admitted work. A join observer converts broker panic/error into the daemon result only after bounded worker shutdown/join, and RAII requests cleanup on unwind. | Continuous refill cannot starve stop, and a failed broker cannot report clean success or strand its worker. |
| DT-25 | Each manifest is opened once with `O_NOFOLLOW`, required to be a singly linked regular file with sealed metadata, read through that FD with a `max+1` ceiling, and accepted only if opened and named dev+ino+type+size remain identical before/after the read. Concurrent append and path replacement refuse. | Manifest size and identity cannot race an unbounded read or substitute different bytes after validation. |

## Atomicity and rollback

- Request frontier: a complete origin write precedes acknowledgement; FLUSH/FUA
  are explicit durability frontiers, and cache work is never on the origin's
  synchronous critical path.
- Cache frontier: invalidation may lose performance only; origin retains data.
- Lifecycle frontier: origin remains attached until the ordered swapoff-first
  quiescence condition and `used_kb==0`;
  cache chunks may be dropped at any time.
- Host frontier: VHDX plan/install backs up configuration; no host mutation in
  this implementation.

## Kahneman map

| ITEM / stage | # | Question | Min evidence | Abort |
| --- | --- | --- | --- | --- |
| ITEM-2 write | #13/#16 | Can GPU failure corrupt or EIO an origin-success write? | `gpu_allocation_failure_continues_on_origin` | Any EIO/cache-only write. |
| ITEM-2 origin error | #13/#16 | Can partial write or FLUSH/FUA failure acknowledge durability? | `partial_origin_write_is_completed_before_ack`; `flush_failure_invalidates_dirty_epoch`; `fua_write_syncs_before_ack`; `normal_writes_batch_until_flush` | Cache valid or durable success after failure. |
| ITEM-2 cache stall | #16 | Can a cache worker stall, unavailable client, or full data queue block origin or revocation? | `cache_timeout_falls_back_to_origin`; `cache_disconnect_falls_back_to_origin`; `cache_disable_remains_deliverable_after_unavailable_full_data_queue`; `release_cache_returns_zero_only_after_dedicated_control_acknowledgement` | Origin exceeds the bounded wait or release reports success before worker acknowledgement. |
| ITEM-4 daemon helper | #15/#16 | Can output overflow, a descendant-held pipe, callback panic, or failed SIGKILL outlive a helper deadline? | `daemon_command_contains_inherited_output_and_bounds_capture`; `daemon_command_success_reaps_all_stdio_redirected_descendant`; `daemon_command_timeout_reaps_term_ignoring_fixture`; `daemon_unreaped_group_selects_fatal_containment` | Helper succeeds with inherited pipe, exceeds storage/deadline, or resumes after failed reap. |
| ITEM-4 reply accounting | #13/#16 | Can a completed reply expose counters from before that IO or strand its worker if verification fails? | `daemon_worker_reply_is_io_accounting_barrier_and_shutdown_is_bounded` | Any reply precedes its accounting, or failure cleanup leaves the worker live. |
| ITEM-4 terminal worker | #15/#16 | Can a full or continuously refilled queue starve terminal state, or can broker panic look successful? | `daemon_worker_shutdown_preempts_queued_io_at_iteration_boundary`; `daemon_worker_terminal_flag_wins_over_512_continuous_queue_refills`; `daemon_broker_panic_propagates_after_bounded_worker_cleanup` | Post-terminal queued IO executes, shutdown waits for a slot/timer, or panic returns `Ok`. |
| ITEM-4 listener | #13/#16/#17 | Can an unauthenticated public endpoint, existing socket, unsafe parent, or ABA replacement be accepted/deleted? | `private_listener_accepts_only_documented_untrusted_network_ranges`; `second_daemon_refuses_existing_socket_and_preserves_original_listener`; `old_socket_cleanup_preserves_aba_replacement_identity`; `unix_socket_parent_symlink_is_refused_before_bind` | Public address accepted or endpoint not created by this daemon is unlinked. |
| ITEM-2 release/read | #13/#17 | Does release preserve bytes and replay? | `write_release_vram_read_origin_hash_matches` | Hash mismatch. |
| ITEM-3 controller | #9/#16 | Is growth/reclaim bounded at exact samples/time? | `cache_growth_and_reclaim_hysteresis_is_exact` | Target/rate exceeded. |
| ITEM-5 identity | #13 | Does exact manifest+FD+PARTUUID+PTUUID pass while root/swap/parent or stale `dev_t` refuses? | `origin_manifest_fd_identity_pairs_refusal_with_legitimate_fixture` | Refusal without exact pass. |
| ITEM-5 manifest bytes | #13/#16 | Can oversize input, append, symlink/nonregular input, or path replacement change bytes after validation? | `manifest_fd_reader_accepts_one_bounded_regular_file`; `manifest_fd_reader_refuses_oversize_symlink_and_nonregular_inputs`; `manifest_fd_reader_bounds_concurrent_append_at_max_plus_one`; `manifest_fd_reader_refuses_path_replacement_after_open` | Unbounded read or identity/size drift is accepted. |
| ITEM-5 provisioning | #13/#17 | Can normal startup format or silently accept a changed swap signature? | `provisioning_is_explicit_and_separate_from_normal_up`; provisioner static identity gate | `mkswap` reachable from normal `up`. |
| ITEM-5 host hash | #13/#16 | Can one-byte host-manifest or configuration drift pass because hash fields are merely recorded? | `host_manifest_hash_fields_are_enforced_end_to_end`; provisioner static hash gate | Any tamper accepted. |
| ITEM-5 retarget | #13/#16 | Can the partition path change after validation but before `mkswap`? | `provisioner_mkswap_is_fd_bound_and_never_executed_by_tests` | `mkswap` receives `$resolved` or any mutable device pathname. |
| lifecycle attach | #13/#15/#16 | Can an attach take effect before a timeout and then lose its backend? | `nbd_effect_before_timeout_preserves_backend_and_seals_exact_evidence`; `nbd_failed_attach_terminates_backend_only_after_two_absence_snapshots`; `nbd_failed_attach_preserves_backend_when_target_status_is_not_absent` | Any ambiguous target status terminates the backend. |
| zram allocation | #13/#16 | Can malformed successful allocator output leak or reset the wrong device? | `malformed_zram_success_resets_exact_new_device_without_leak` | Reset without exact one-device delta, inactive proof, FD/dev_t binding, or final cardinality equality. |
| origin admission | #13/#16 | Can partition/parent or a root/swap identity retarget during external metadata probes? | `origin_manifest_fd_identity_pairs_refusal_with_legitimate_fixture` | External probes use a mutable origin path or critical-device snapshots drift without refusal. |

## Security checklist (pre-impl)

- [x] Privilege: daemon opens only sealed origin identity.
- [x] User/host copy: size/path/UUID bounded before open.
- [x] Flags: unknown daemon arguments refuse.
- [x] Info-leak: status emits PARTUUID, not host path.
- [x] IRQ/atomic: N/A — userspace.
- [x] Lifetime: cache memory drops independently; origin outlives export.
- [x] Device-gone: origin error is stable EIO; cache gone falls back.
- [x] Host safety: no real VHDX/device action in source verification.
- [x] Replayable ops: cache release and configuration plan are idempotent.

## Files to CREATE / MODIFY / DELETE

**CREATE `crates/ramshared-block/src/origin_cache.rs`** — origin trait/file
adapter, cache backend, target formula, telemetry. Every critical named test is
listed in the required matrix below. Cover ≥80%.

**MODIFY `crates/ramshared-block/src/request.rs`, `vram_backend.rs`,
`sparse_vram.rs`, `lib.rs`** — mutable reads and exports. Every critical named
test is listed in the required matrix below. Cover ≥80% for changed business
logic.

**MODIFY `crates/ramshared-wsl2d/src/main.rs` and `backend.rs`** — parse a sealed
origin manifest through a bounded revalidated FD, enforce the host and canonical
configuration hashes, revalidate opened-FD identity, isolate cache work behind a
bounded worker client, preempt queued work on terminal state, propagate broker
failure after cleanup, bind only documented private listeners, own Unix socket
cleanup by pinned identity, use `MCL_CURRENT`, retain no product legacy selector,
and publish runtime telemetry. Every critical
named test is listed in the required matrix below. Cover ≥80%.

**MODIFY `crates/ramshared-cli/src/cascade/cascade_io.rs`, `mod.rs`, and
`lifecycle.rs`** — daemon args, origin receipt v4, status fields. Every critical
named test is listed in the required matrix below. Cover ≥80%.

**CREATE `scripts/windows/Manage-RamSharedOrigin.ps1` and
`Test-RamSharedOriginStatic.ps1`** — plan-first fixed VHDX/PARTUUID manifest and
attachment verification. N/A — PowerShell static/manufactured.

**MODIFY docs and packaging** to include new source/configuration.

ITEM-7 deletes the legacy selector and NBD full-VRAM composition while retaining
generic `VramBackend` consumers in broker, ublk, and Windows paths. No
compatibility or product fallback file is permitted.

## Observability

| Signal | Where | Type |
| --- | --- | --- |
| logical_capacity_kib | receipt/status | gauge |
| vram_cached_kib | daemon/status | gauge |
| gpu_headroom_kib | monitor | gauge/null |
| ssd_origin_written_kib | daemon/status | counter |
| cache_fallback_reads | daemon/status | counter |
| cache_invalidations/releases | daemon/status | counter |
| fallback_swap_used_kib | status | gauge |

## Living docs

| Document | Action |
| --- | --- |
| `ARCHITECTURE.md` | Alter |
| `docs/reliability/DEGRADATION-MATRIX.md` | Alter |
| `validation.md` | Append partial source evidence |
| `docs/BENCHMARKS.md` | N/A — no numbers claimed |
| `.claude/rules/*` | N/A |

## Implementation order

1. ITEM-1 mutable read contract and fake origin/provider fixtures.
2. ITEM-2 origin-first backend and fallback tests.
3. ITEM-3 target/growth/reclaim controller.
4. ITEM-4 daemon/status telemetry.
5. ITEM-5 exact manifest/FD/critical-device identity and separate provisioning.
6. ITEM-6 dynamically discovered Windows origin/WSL storage source and docs.
7. ITEM-7 remove the legacy preallocation selector/profile chooser and NBD
   full-VRAM composition; retain generic non-NBD consumers; record source-scan,
   named-test, focused Rust, and documentation-governance evidence. If evidence
   fails, keep activation disabled and roll back only the exact reviewed
   origin-capable source snapshot required to recover the build.

## Required tests matrix

Every name in this matrix is critical. A matrix row is not green merely because
another test passed; the IMPL/validation mapping names the exact available
source/static evidence and leaves unavailable live evidence `PARTIAL`.

| Production path | Test | Kind | Kahneman | Required evidence |
| --- | --- | --- | --- | --- |
| `origin_cache.rs` | `origin_write_precedes_cache_update` | unit | #13 | source test + coverage |
| `origin_cache.rs` | `write_release_vram_read_origin_hash_matches` | unit | #13/#17 | source test + coverage |
| `origin_cache.rs` | `gpu_allocation_failure_continues_on_origin` | unit | #16 | source test + coverage |
| `origin_cache.rs` | `cache_growth_and_reclaim_hysteresis_is_exact` | unit | #9/#16 | source test + coverage |
| `origin_cache.rs` | `configured_physical_cap_bounds_an_ample_gpu_budget` | unit | #9/#16 | source test + coverage |
| `origin_cache.rs` | `origin_failure_returns_io_error` | unit | #13 | source test + coverage |
| `origin_cache.rs` | `partial_origin_write_is_completed_before_ack` | unit | #13 | source test + coverage |
| `origin_cache.rs` | `zero_progress_origin_write_is_never_acknowledged` | unit | #13 | source test + coverage |
| `origin_cache.rs` | `origin_flush_failure_does_not_ack_or_validate_cache` | unit | #13/#16 | source test + coverage |
| `origin_cache.rs` | `normal_writes_batch_until_flush` | unit | #13/#17 | source test + coverage |
| `origin_cache.rs` | `fua_write_syncs_before_ack` | unit | #13 | source test + coverage |
| `origin_cache.rs` | `flush_failure_invalidates_dirty_epoch` | unit | #13/#16 | source test + coverage |
| `origin_cache.rs` | `partial_origin_failure_invalidates_cached_data_before_recovery_read` | unit | #13 | source test + coverage |
| `origin_cache.rs` | `sync_origin_failure_invalidates_cached_data_before_recovery_read` | unit | #13 | source test + coverage |
| `origin_cache.rs` | `durable_origin_write_legitimate_path_passes` | unit | #13 | source test + coverage |
| `origin_cache.rs` | `exact_target_formula_and_missing_measurement_fail_safe` | unit | #9/#16 | source test + coverage |
| `origin_cache.rs` | `cache_io_failures_invalidate_and_fall_back_without_eio` | unit | #13/#16 | source test + coverage |
| `origin_cache.rs` | `origin_failure_is_sticky_until_three_read_sync_probes` | unit | #13/#16 | source test + coverage |
| `request.rs` | `write_then_read_round_trips` | unit | #13 | source test + coverage |
| `request.rs` | `unknown_write_flags_refuse_before_mutation` | unit | #13 | source test + coverage |
| `isolated_origin.rs` | `bounded_cache_client_covers_hit_miss_mutation_and_disable_protocol` | unit/protocol | #9/#13 | source test + coverage |
| `isolated_origin.rs` | `bounded_cache_client_revokes_on_protocol_queue_and_transport_faults` | unit/fault | #13/#16 | source test + coverage |
| `isolated_origin.rs` | `cache_disable_remains_deliverable_after_unavailable_full_data_queue` | unit/control-lane | #13/#16 | source test + coverage |
| `isolated_origin.rs` | `release_cache_returns_zero_only_after_dedicated_control_acknowledgement` | unit/ack | #13/#16 | source test + coverage |
| `isolated_origin.rs` | `backend_geometry_range_and_empty_io_are_bounded` | unit/bounds | #9/#13 | source test + coverage |
| `isolated_origin.rs` | `backend_cache_paths_preserve_origin_authority_and_telemetry` | unit/fault | #13/#16 | source test + coverage |
| `isolated_origin.rs` | `origin_failure_requires_three_successful_read_sync_probes` | unit/recovery | #13/#16 | source test + coverage |
| `wsl2d/main.rs` | `product_origin_mode_does_not_preallocate_logical_capacity` | unit | #16 | source test + coverage |
| `wsl2d/main.rs` | `missing_gpu_measurement_sets_zero_cache_target` | unit | #16 | source test + coverage |
| `wsl2d/main.rs` | `critical_supervisor_request_is_consumed_and_reclaims_daemon_cache_to_zero` | unit | #13/#16 | source test + coverage |
| `wsl2d/main.rs` | `origin_and_cache_failures_are_sticky_until_exact_recovery` | unit | #13/#16 | source test + coverage |
| `wsl2d/main.rs` | `origin_args_default_to_four_gib_and_enforce_one_to_twenty_four_gib` | unit | #9 | source test + coverage |
| `wsl2d/main.rs` | `physical_cache_cap_is_explicit_bounded_and_defaults_to_one_gib` | unit | #9 | source test + coverage |
| `wsl2d/main.rs` | `origin_mode_refuses_to_start_without_a_valid_daemon_identity` | unit | #13 | source test + coverage |
| `wsl2d/main.rs` | `origin_mode_never_arms_mcl_future_before_cache_mappings` | unit | #13 | source test + coverage |
| `wsl2d/main.rs` | `origin_manifest_fd_identity_pairs_refusal_with_legitimate_fixture` | unit | #13 | source test + coverage |
| `wsl2d/main.rs` | `host_manifest_hash_fields_are_enforced_end_to_end` | unit | #13/#16 | source test + coverage |
| `wsl2d/main.rs` | `daemon_command_timeout_terminates_child_without_hang` | process/timeout | #15/#16 | source test + coverage |
| `wsl2d/main.rs` | `daemon_command_success_reaps_all_stdio_redirected_descendant` | adversarial process | #15/#16 | source test + coverage |
| `wsl2d/main.rs` | `daemon_command_timeout_reaps_term_ignoring_fixture` | adversarial process/timeout | #15/#16 | source test + coverage |
| `wsl2d/main.rs` | `daemon_command_contains_inherited_output_and_bounds_capture` | adversarial process/pipe | #15/#16 | source test + coverage |
| `wsl2d/main.rs` | `daemon_unreaped_group_selects_fatal_containment` | injected fatal seam | #15/#16 | source test + coverage |
| `wsl2d/main.rs` | `daemon_worker_reply_is_io_accounting_barrier_and_shutdown_is_bounded` | unit/concurrency | #13/#16 | source test + coverage |
| `wsl2d/main.rs` | `daemon_worker_shutdown_preempts_queued_io_at_iteration_boundary` | unit/shutdown | #15/#16 | source test + coverage |
| `wsl2d/main.rs` | `daemon_worker_terminal_flag_wins_over_512_continuous_queue_refills` | unit/concurrency | #15/#16 | source test + coverage |
| `wsl2d/main.rs` | `daemon_broker_panic_propagates_after_bounded_worker_cleanup` | unit/panic | #15/#16 | source test + coverage |
| `wsl2d/main.rs` | `private_listener_accepts_only_documented_untrusted_network_ranges` | unit/table | #13 | source test + coverage |
| `wsl2d/main.rs` | `second_daemon_refuses_existing_socket_and_preserves_original_listener` | unit/socket | #13/#17 | source test + coverage |
| `wsl2d/main.rs` | `stale_unix_socket_requires_explicit_cleanup_and_is_never_unlinked_on_startup` | unit/socket | #13/#17 | source test + coverage |
| `wsl2d/main.rs` | `old_socket_cleanup_preserves_aba_replacement_identity` | unit/ABA | #13/#16/#17 | source test + coverage |
| `wsl2d/main.rs` | `unix_socket_parent_symlink_is_refused_before_bind` | unit/security | #13/#16 | source test + coverage |
| `wsl2d/main.rs` | `owned_unix_socket_cleanup_removes_only_the_exact_bound_identity` | unit/socket | #13/#17 | source test + coverage |
| `wsl2d/main.rs` | `manifest_fd_reader_accepts_one_bounded_regular_file` | unit/legitimate | #13 | source test + coverage |
| `wsl2d/main.rs` | `manifest_fd_reader_refuses_oversize_symlink_and_nonregular_inputs` | unit/adversarial | #13/#16 | source test + coverage |
| `wsl2d/main.rs` | `manifest_fd_reader_bounds_concurrent_append_at_max_plus_one` | unit/race | #13/#16 | source test + coverage |
| `wsl2d/main.rs` | `manifest_fd_reader_refuses_path_replacement_after_open` | unit/ABA | #13/#16 | source test + coverage |
| `wsl2d/main.rs` | `daemon_ublk_runtime_failures_delete_only_after_fresh_absence_proof` | unit/refusal | #13/#16 | source test + coverage |
| `cascade_io.rs` | `product_daemon_command_requires_origin_cache` | unit | #13 | source test + coverage |
| `cascade_io.rs` | `origin_mode_refuses_missing_daemon_cache_identity_before_nbd_attach` | unit | #13 | source test + coverage |
| `lifecycle.rs` | `schema_v4_distinguishes_logical_cache_origin_and_fallback_swap` | unit | #9 | source test + coverage |
| `lifecycle.rs` | `origin_failure_and_stuck_cache_are_never_green` | unit | #13/#16 | source test + coverage |
| Windows origin | `origin_plan_is_separate_fixed_and_identity_bound` | static | #13 | static output |
| Windows origin | `origin_install_failure_rolls_back_current_run_only` | static | #13/#17 | static output |
| Windows origin | `origin_preexisting_or_foreign_vhdx_is_never_removed` | static | #13/#17 | static output |
| Windows origin | `origin_uninstall_requires_exact_sealed_ownership` | static | #13/#17 | static output |
| Windows origin | `malformed_or_foreign_origin_identity_is_refused` | static | #13 | static output |
| Cascade startup/provisioning | `provisioning_is_explicit_and_separate_from_normal_up` | unit/static | #13/#17 | source test + static scan |
| Cascade provisioning | `provisioner_mkswap_is_fd_bound_and_never_executed_by_tests` | static | #13/#16 | source-only scan; zero device execution |
| Product sunset | `legacy_preallocation_removed_before_day0_deadline` | static | #18 | clean active-source/current-doc scan + named test + governance evidence |

## Validation checklist

- [x] Source/static evidence mapped in `IMPL.md` and `validation.md` for each
  currently available named test.
- [x] Per-file source coverage evidence meets the recorded ≥80% gate.
- [x] Windows origin static contract emitted the five named static PASS markers.
- [x] `legacy_preallocation_removed_before_day0_deadline` and the clean candidate
  scan close the executable source prerequisite only.
- [ ] Live origin/NBD/GPU matrix is environment-bound and not DONE.

Rollback trigger: origin order inversion, origin-success/GPU-failure EIO, hash
mismatch after release, cache beyond target/rate, or identity-only-by-size/path.

Residual live-only limit: an already-open block handle plus `dev_t`/sysfs proofs
closes userspace pathname retargeting. It cannot turn physical hot-unplug or a
kernel-level device replacement during the `mkswap` syscall into a source-only
proof. Such behavior remains an isolated attended gate; any pre/post identity
or I/O uncertainty preserves the evidence and refuses qualification.
