# AUDIT-2.5 — WSL2 NBD-only product readiness

> Fresh SSDV3 Step 2.5 risk audit after the independent Gate A review of the
> benchmark harness. Scope is the NBD product boundary, the pairwise WSL2
> benchmark controller/cell runner, swapoff-first teardown, Windows watchdog
> containment, evidence custody, and the named files in `SPEC.md`.

## Decision

| Path | Verdict |
| --- | --- |
| NBD-only product boundary, sealed release, state model, capacity formula, Relay, and swapoff-first source contracts | **GO for implementation** against the revised SPEC; local evidence remains source-partial. |
| Disk/NBD benchmark harness before the Gate A corrections | **NO-GO** — the first revision could race cgroup admission, compare different cascades, under-exercise larger tiers, mislabel measurements, use different CUDA snapshots, lose containment state, and accept live seams. |
| Corrected harness implementation | **GO FOR REVIEWED-RELEASE DEPLOYMENT** — the locked decisions and named local static/manufactured tests pass, and the final independent Gate A passed on the frozen pre-commit workspace. |
| Live 1/2/4 GiB matrix or product promotion | **NO-GO** until every pair has legitimate before → action → after evidence, n≥3 statistics, clean terminal observation, and no RED/PARTIAL/unverified-termination result. |
| Outer watchdog termination | Allowed only as separately approved Windows containment after a bounded deadline. It is always **`RED/unverified_terminated`**, never benchmark evidence, never `PRODUCT_OFF`, and always stops promotion. |

The product itself never calls `wsl --terminate`, `wsl --shutdown`, reboot, or
an equivalent host action. The watchdog is a harness authority outside the
product lifecycle and cannot turn an unverified termination into a clean-state
claim.

## Scope and evidence class

The existing NBD-only decisions remain valid: NBD is the sole supported WSL2
transport; `ublk_drv` is inert capability residue, not a product dependency;
release trees are sealed; `PRODUCT_OFF`, `READY`, and `BLOCKED` are distinct;
capacity is free absorbable capacity; and teardown is swapoff-first. This audit
does not approve ublk, a kernel module unload, a custom kernel, a reboot, or a
native Microsoft/WSL memory feature.

The independent Gate A review was a design/code audit, not live evidence. The
corrected implementation now has green local static/manufactured evidence, but
the current benchmark result remains `PARTIAL`/`NO-GO`; manufactured samples,
static checks, a plan, or a process exit cannot close the live gate. The fresh
implementation re-audit is still pending and the new 1/2/4 GiB matrix has not
run on the reviewed release.

## Gate A findings and mandatory disposition

| Sev | Finding | Failure class | Required correction in SPEC | Implementation status |
| --- | --- | --- | --- | --- |
| CRITICAL | The worker was started outside the cgroup and its PID was written to `cgroup.procs` only after allocation could begin. | Host pressure can escape the 1200 MiB bound. | Start a launcher already inside the cgroup; use a fresh create-once barrier; verify membership before releasing the worker; refuse if proof is missing. | **Closed in implementation/static/manufactured gates; live re-audit pending**; DT-NBD-25, `benchmark_start_barrier_and_size_occupancy_contract`. |
| CRITICAL | Disk-only omitted the common zram tier while NBD used zram → NBD → lower sink. | The controls are different cascades, so latency/occupancy comparison is invalid. | Every pair must use fresh 1 GiB zram at priority 200; disk-only second tier is only the fresh exact 8 GiB scratch; NBD second tier is only exact `V` NBD; pre-existing lower sink is untouched. | **Closed in implementation/static/manufactured gates; live re-audit pending**; DT-NBD-24. |
| HIGH | A fixed 2560 MiB worker did not exercise Q2/Q4 tier sizes. | Larger cells could pass without occupying the declared tier. | Allocate/hold `V + 2560 MiB` per size under the cgroup and require size-specific zram/second-tier occupancy deltas; no fixed-size shortcut. | **Closed in implementation/static/manufactured gates; live re-audit pending**; DT-NBD-25. |
| MEDIUM | `block_size_bytes` and `queue_depth` described allocation chunks and worker count. | Evidence labels imply an I/O protocol that is not being measured. | Use `allocation_to_hold_ms`, `allocation_chunk_bytes`, and `worker_threads=1`; retain `direction=sequential_write` separately. | **Closed in implementation/static/manufactured gates; live re-audit pending**; DT-NBD-25. |
| HIGH | Disk and NBD bounded cells started separate CUDA processes/snapshots. | External GPU state is a confounder, not a paired condition. | One CUDA context per size/condition pair, held across disk then NBD and released after NBD terminal evidence. | **Closed in implementation/static/manufactured gates; live re-audit pending**; DT-NBD-26. |
| HIGH | Results lacked full execution, identity, headroom, and lower-tier context. | A number cannot be reproduced or tied to a release/capacity decision. | Capture branch/commit/dirty state, release/manifest/script hashes, exact command, kernel/GPU/RAM/swap, lower-tier identity/free `L`, pair ID, GPU snapshots, and terminal/watchdog outcome in sanitized form. | **Closed in implementation/static/manufactured gates; live re-audit pending**; DT-NBD-27 and RF-NBD-18. |
| HIGH | NBD aggregation did not require sufficient zram activity. | A purported NBD sample could avoid the declared cascade. | Require common zram activity and NBD delta at least the declared tier threshold, plus checksum and terminal proof. | **Closed in implementation/static/manufactured gates; live re-audit pending**; DT-NBD-24/25. |
| HIGH | Selected-release discovery used an unbounded synchronous `wsl.exe` call before the cell watchdog. | The controller can hang before containment is armed. | Bound release discovery, each cell, and watchdog termination; capture start/deadline/exit/stdout/stderr and classify timeout explicitly. | **Closed in implementation/static/manufactured gates; live re-audit pending**; DT-NBD-29. |
| CRITICAL | CUDA setup/handshake failure could escape cleanup; ready/release paths could be reused. | VRAM/context leak or false readiness can poison later cells. | Put setup and wait in a `try/finally`; always release/force-clean bounded process; use fresh create-once ready/release artifacts and reject existing paths. | **Closed in implementation/static/manufactured gates; live re-audit pending**; DT-NBD-26/30. |
| HIGH | PowerShell `Start-Process -ArgumentList` could mis-handle paths with spaces or metacharacters. | Wrong command/target or unbounded shell behavior. | Use a bounded process helper with safe argument construction or restrict/validate the live path grammar before launch; record the exact argv. | **Closed in implementation/static/manufactured gates; live re-audit pending**; DT-NBD-29. |
| HIGH | Headroom was checked in the plan and before cells, but not after CUDA became ready for the paired run. | A GPU allocation can invalidate the preflight between check and action. | Recheck numeric free VRAM immediately before the pair and after the CUDA-ready receipt; refuse before WSL action on shortfall. | **Closed in implementation/static/manufactured gates; live re-audit pending**; DT-NBD-26/27. |
| CRITICAL | A timeout terminated the distro but the controller unconditionally described the campaign as `PRODUCT_OFF`. | Termination does not prove swapoff, daemon, or ghost absence. | Record `RED/unverified_terminated`; never infer `PRODUCT_OFF`, never retry/promote, and require independent revalidation before any later action. | **Closed in implementation/static/manufactured gates; live re-audit pending**; DT-NBD-9/31. |
| MEDIUM | Static tests were mostly token checks and did not prove behavioral refusal branches. | False-green CI can miss safety regressions. | Add manufactured behavior for barrier, topology, occupancy, one-context, timeout classification, seam refusal, cleanup identity, and failure paths; keep live evidence separate. | **Closed in implementation/static/manufactured gates; live re-audit pending**; full matrix below. |
| HIGH | PRD/SPEC watchdog language and prior risk statements were not aligned. | Product could be read as authorized to terminate WSL or as claiming cleanup. | Product never terminates; only separately approved outer watchdog may terminate selected distro after deadline, always `RED/unverified_terminated`. | **Closed in docs**; PRD/SPEC DT-NBD-9/31 and this audit. |
| HIGH | Live runner accepted environment-injected fixture seams for roots, swap files, PIDs, or lower sink. | Fake identities can manufacture a pass on the real surface. | Fixture seams are test-only; approved live mode rejects/ignores all such overrides and uses canonical sealed paths. | **Closed in implementation/static/manufactured gates; live re-audit pending**; DT-NBD-28. |
| MEDIUM | Raw cell numbers had no compatible historical regression contract. | A later build could look faster/slower only because the kernel, GPU driver, or workload changed. | Emit within-pair ratios now; compare history only under an exact environment/workload fingerprint and apply the DT-NBD-32 thresholds. | **Implemented in controller/static fixtures; live matrix and historical baseline remain open**; a first full campaign is only a baseline candidate. |

No finding permits weakening the lower-tier formula, using nominal disk size,
killing a daemon with active swap, or treating a timeout as cleanup.

## Locked safety and measurement contract

1. **Topology parity.** For each size/condition pair, start from no managed
   swap and create one fresh product-owned 1 GiB zram tier at priority 200.
   Disk-only adds exactly one newly created 8 GiB scratch swapfile at priority
   100. NBD adds exactly one `V`-sized NBD tier at priority 100. The existing
   host lower sink is not modified and is not the compared second tier.
2. **Bounded occupancy.** Start a launcher from inside a 1200 MiB cgroup,
   verify its cgroup membership, then release a fresh barrier. The worker
   allocates and holds `V + 2560 MiB` with `shake256-v1`, 64 MiB allocation
   chunks, and one worker thread. It records
   `allocation_to_hold_ms`, `allocation_chunk_bytes`, and `worker_threads`,
   plus zram/second-tier deltas proving the declared size was exercised.
3. **Pair condition.** Idle pairs use no external CUDA workload. Bounded pairs
   start one 512 MiB CUDA context after the numeric headroom gate and hold it
   across disk-only then NBD; the context is released only after NBD terminal
   evidence. Headroom is rechecked before the pair and after CUDA-ready.
4. **Containment.** Product code has no WSL termination operation. The outer
   Windows watchdog alone may terminate the selected distro after its deadline.
   Its result is `RED/unverified_terminated`; no `PRODUCT_OFF`, PASS, retry, or
   promotion follows.
5. **Freshness and bounds.** Selected-release discovery, every WSL cell, and
   watchdog termination are bounded. Ready/release artifacts are create-once
   and unique per pair. Existing handshake files refuse.
6. **Live seam boundary.** Approved live mode uses the sealed release and
   canonical `/proc`, `/run`, cgroup, and lower-sink paths. Fixture overrides
   are available only to manufactured tests and cannot be present in live
   evidence.
7. **Evidence.** Every pair records sanitized context, exact command/argv,
   branch/commit/dirty state, release and script hashes, kernel/GPU/RAM/swap
   state, lower-tier `L` and identity, GPU headroom snapshots, pair/cell IDs,
   checksums, occupancy deltas, WSL outcomes, watchdog outcome, and terminal
   classification. Private paths and host identity are removed from public
   evidence without changing the machine-readable private artifact used for
   audit.

## Threat and abuse model

| ID | Scenario | Severity | Required control | Abort trigger |
| --- | --- | --- | --- | --- |
| T1 | Worker allocates before cgroup admission | CRITICAL | In-cgroup launcher + barrier + membership proof | Any allocation before proof or missing cgroup record |
| T2 | Disk control omits zram or reuses host swap | CRITICAL | Fresh identical zram; exact exclusive scratch identity | Topology differs, scratch is not fresh/exclusive, or host swap changes |
| T3 | Q2/Q4 run does not occupy its tier | HIGH | `V + 2560 MiB` and per-size delta threshold | Occupancy below declared threshold or fixed allocation observed |
| T4 | Allocation labels are interpreted as I/O queue parameters | MEDIUM | Explicit measurement names and schema validation | `block_size`/`queue_depth` used for this contract |
| T5 | CUDA condition differs between controls | HIGH | One context per pair; two headroom checks | Context exits, is recreated, or GPU free shortfall occurs |
| T6 | Timeout/termination is reported clean | CRITICAL | RED/unverified-termination classification | Any `PRODUCT_OFF` claim after watchdog termination |
| T7 | WSL call hangs before watchdog | HIGH | Bounded process helper for discovery/cell/terminate | Missing deadline or uncaptured exit/output |
| T8 | CUDA/handshake cleanup leaks or reuses state | CRITICAL | `try/finally`, force cleanup, create-once fresh files | Live process/context or pre-existing handshake remains |
| T9 | Test seam produces fake live pass | HIGH | Reject fixture overrides in approved mode | Any seam-bearing live artifact |
| T10 | Evidence lacks state/identity | HIGH | Automatic context/hash/headroom/lower-capacity capture | Missing required context or unverifiable hash |
| T11 | Swapoff fails or ghost remains | CRITICAL | Identity-bound swapoff-first cleanup | Any residual managed/ghost swap or daemon mismatch |

## Kahneman map

| Discipline | Question | Evidence | Abort |
| --- | --- | --- | --- |
| #2 | Does the workload actually exercise the selected tier? | Barrier, cgroup membership, `V + 2560 MiB`, zram/second-tier deltas | Fixed-size or pre-admission allocation |
| #3 | Are numbers tied to the state and exact command? | Context schema, pair ID, hashes, lower `L`, GPU/RAM/swap snapshots | Missing unit, identity, command, or state |
| #9 | Is the comparison numeric and paired? | n=3, median, nearest-rank p99, population deviation, same pair/context | Different topology/snapshot, n<3, or mislabeled fields |
| #13 | Are legitimate and refusal paths paired? | Manufactured failures plus live before/action/after | Any fail-open state or inferred cleanup |
| #15 | Is retry justified? | One bounded attempt per deterministic cell/pair | Blind retry after timeout, identity, capacity, or cleanup failure |
| #16 | Can the host and lower tier absorb the run? | cgroup proof, GPU headroom, exact scratch, free `L`, watchdog | Shortfall, race, or unbounded process |
| #17 | Are effects idempotent and fresh? | Unique artifacts, identity rechecks, swapoff-first teardown | Reused handshake, double attach, or unsafe removal |
| #18 | Is each decision owned by the right layer? | Product has no terminate; Windows watchdog only containment; test seams local | Product kills WSL, fixture seam reaches live, or host ownership is crossed |

## Security and custody checks

| Check | Status | Required proof |
| --- | --- | --- |
| Product lifecycle cannot terminate/reboot/shutdown WSL | PASS in static/manufactured contract; live command audit pending | Static forbidden-action test plus live command audit |
| Outer watchdog authority is explicit and fail-closed | PASS in static/manufactured gates; live re-audit pending | Deadline, selected-distro identity, `RED/unverified_terminated`, no promotion |
| Worker cgroup containment | PASS in static/manufactured gates; live occupancy pending | In-cgroup launcher and start barrier, not post-start PID write |
| Disk scratch identity and deletion | PASS in manufactured gates; live pair pending | `O_EXCL|O_NOFOLLOW`, device/inode/owner/mode, exact swapoff, absence before remove |
| CUDA lifetime and handshake custody | PASS in static/manufactured gates; bounded CUDA live proof pending | Fresh create-once files, `finally` cleanup, no leaked process/context |
| WSL command bound/argument-safe | PASS in static/manufactured gates; live call receipts pending | Bounded helper, validated path grammar or safe argv, captured outputs |
| Live seam exclusion | PASS in manufactured gates; live seam audit pending | Canonical path enforcement and refusal of `RAMSHARED_*` fixture overrides |
| Public evidence hygiene | PASS in Node validator/manufactured fixture; live pair not produced | Sanitized envelope, no secrets/private host identity/raw logs |
| Release/BINARY_MATCH/swapoff-first | PASS for the historical 1 GiB pilot and local refusal contracts; repeat per new NBD cell pending | Existing named local tests plus live repetition for each NBD cell |

## Full implementation and test matrix

Every row is contractual. A static/manufactured result does not close its live
counterpart, and a missing row is a blocker for Step 3 completion.

The corrected local implementation is green for the manufactured/static rows:
the cell harness reports 13/13, the product-preflight harness reports 26/26, the
repository public-evidence validator tests report 15/15, and the focused
PowerShell matrix static harness passes. These results close the implementation
findings above but do not close the live counterparts. The complete 1/2/4 GiB
Windows/WSL2 matrix, fresh Gate A re-audit, live CUDA pair custody, live
before/action/after receipts, and BINARY_MATCH repetition for every NBD cell
remain pending.

| File/resource | Required named test/check | Kind | Gate |
| --- | --- | --- | --- |
| `crates/ramshared-tier/src/nbd_readiness.rs` | `nbd_only_transport_is_the_only_ready_value`; `lower_tier_formula_uses_ten_percent_or_512_mib`; `capacity_shortfall_refuses_before_mutation`; `product_off_is_not_ready_alias`; `deterministic_gate_failure_is_not_retried`; `activation_and_deactivation_are_idempotent` | Rust unit/refusal | Pure policy; per-file cover ≥80% |
| `crates/ramshared-tier/src/cascade.rs` | `ublk_service_is_not_a_product_dependency` | Rust integration/refusal | NBD-only boundary |
| `crates/ramshared-cli/src/cascade/cascade_io.rs` | `swapoff_completes_before_nbd_disconnect`; `failed_swapoff_keeps_daemon_and_device_alive`; `setup_new_cascade_uses_only_temp_runtime_and_direct_child_fixture` | Injected/bounded Rust | Swapoff-first/hang gate |
| `scripts/safety/nbd-product-preflight.sh` + `scripts/safety/test-nbd-product-preflight.sh` | `release_manifest_and_modes_are_verified`; `binary_match_rejects_stale_or_deleted_daemon`; `relay_check_failure_blocks_readiness`; `reboot_and_shutdown_requests_are_refused`; `legacy_ublk_retirement_never_unloads_module`; `product_off_ready_blocked_state_matrix`; `capacity_sink_identity_and_alignment_refusals`; `n3_or_ublk_capability_does_not_promote_nbd_product`; `installer_every_post_write_phase_rolls_back`; `legacy_unit_migration_requires_exact_hash_and_restores_on_failure`; `corrupt_published_legacy_backup_refuses_before_replacement`; `legacy_backup_root_symlink_is_refused`; `legacy_restore_reloads_systemd_after_daemon_reload` | Static/manufactured | Source lifecycle/refusal gates |
| `scripts/safety/cascade_pressure_integrity_worker.py` + `scripts/safety/Test-CascadePressureIntegrityWorker.sh` | `shake256_pattern_is_deterministic_and_incompressible` | Manufactured | Workload determinism/incompressibility |
| `scripts/safety/nbd-benchmark-cell.sh` + `scripts/safety/test-nbd-benchmark-cell.sh` | `benchmark_aggregation_is_exact_and_requires_three_runs`; `disk_control_and_nbd_candidate_share_one_workload_contract`; `benchmark_start_barrier_and_size_occupancy_contract`; `benchmark_cleanup_refuses_ghost_or_residual_swap`; `disk_control_scratch_is_exclusive_identity_bound_and_swapoff_first`; `benchmark_live_seams_are_unavailable_in_approved_mode` | Manufactured/refusal | Pair topology, cgroup, occupancy, cleanup, seam gates |
| `scripts/safety/nbd-benchmark-cgroup-launch.sh` | `benchmark_start_barrier_launcher_is_in_cgroup_before_exec` | Manufactured/refusal | Cgroup admission/start barrier |
| `scripts/safety/nbd-benchmark-lib.sh` | `disk_control_scratch_is_exclusive_identity_bound_and_swapoff_first` | Manufactured/refusal | Exact scratch identity, swapoff refusal preservation, and post-absence removal |
| `scripts/p0/Start-CudaVramWorkload.ps1` | `cuda_workload_uses_fresh_handshakes_and_finally_releases_context` | Static/manufactured plus bounded CUDA | CUDA lifetime gate |
| `scripts/windows/Invoke-NbdBenchmarkMatrix.ps1` | `matrix_promotes_only_after_complete_prior_pair`; `wsl_release_discovery_and_cells_are_bounded`; `one_cuda_context_covers_one_disk_nbd_pair` | Plan/manufactured/live harness | Pairing, bounds, promotion |
| `scripts/windows/Test-NbdBenchmarkMatrixStatic.ps1` | `bounded_cell_requires_numeric_gpu_headroom`; `watchdog_timeout_is_red_and_unverified_termination`; CUDA source/handshake/cleanup static checks | Manufactured/static | Headroom/containment |
| `scripts/windows/Test-WindowsCiStatic.ps1` | `windows_static_wrapper_includes_nbd_benchmark_harness` | Static wrapper | Complete PowerShell sweep |
| `scripts/package/build-linux-bundle.sh` | `sealed_bundle_contains_benchmark_runner_and_worker` | Manufactured package | Release identity/layout |
| `tools/ci/generate-capability-observations.mjs` + `tools/ci/generate-capability-observations.test.mjs` + `docs/governance/capability-observations.generated.json` | `capability_observations_are_deterministic_and_checked`; `node tools/ci/generate-capability-observations.mjs --check` | Generated/static | Observation facts are deterministic and separate from live proof |
| `scripts/safety/cascade-up.sh` + `scripts/safety/cascade-down.sh` | `nbd_lifecycle_before_action_after` | Live E2E | Approved NBD lifecycle, BINARY_MATCH, swapoff-first, no ghost |
| `scripts/safety/nbd-product-preflight.sh` | `relay_gate_before_action_after` | Live E2E | Relay clean before/after, no reap |
| Release/cascade surface | `NBD_BENCHMARK_MATRIX` | Live benchmark | P1/Q2/Q4 × idle/bounded × disk/NBD, n=3, paired context, clean terminal state |

## Re-audit gates

This audit must be reopened after implementation and before any live matrix if:

- the worker, cgroup, zram, scratch, NBD, CUDA, WSL process, watchdog, or
  evidence implementation changes;
- a test uses an environment seam in an approved live invocation;
- a timeout is classified as `PRODUCT_OFF`, PASS, or retryable;
- a new WSL/Windows termination or reboot path appears; or
- the release, lower-tier identity, Relay contract, or BINARY_MATCH semantics
  change.

The final independent implementation audit moved the corrected harness to
**GO FOR REVIEWED-RELEASE DEPLOYMENT** after confirming the named manufactured
refusal tests. The real before → action → after matrix remains a separate live
gate. Any RED, PARTIAL, or
`unverified_terminated` pair keeps product promotion **NO-GO**.

## Final verdict

**GO for clean reviewed-release deployment; NO-GO for the benchmark or
product-readiness claim now.** The remaining 1/2/4 GiB live-matrix gaps are
concrete and named above; they are not
permission to invent live proof offline or to pressure the host without the
approved Windows watchdog and bounded artifacts.
