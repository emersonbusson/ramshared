---
slug: wsl2-nbd-product-readiness
title: "WSL2 NBD-only product readiness"
milestone: v0.9.0-beta.1 — WSL2 NBD
issues:
  - 194
---

# PRD — WSL2 NBD-only product readiness

## Summary

The supported WSL2 product transport is NBD. This Step 3 slice is
**source-partial**: it contains bounded pure policy, manufactured
release/preflight rollback checks, a local injected teardown contract, and a
sealed 1 GiB pilot record. It does not contain the complete paired 1/2/4 GiB
disk/NBD matrix, so it cannot call the benchmark surface `READY` or promote a
larger size. The honest product verdict remains `PARTIAL`. It deliberately
retires the legacy ublk service from the product control plane without
unloading the `ublk_drv` kernel module. A loaded module is capability evidence
only; it is never a product transport claim.

The positive state is `READY`. The intentional safe state is `PRODUCT_OFF`.
They are not aliases: an installation can be product-off and still have a
valid, read-only readiness report, while a product-off state must never be
reported as an active or ready product. `BLOCKED` is used for a refusal or an
unmet gate. No state in this PRD authorizes a reboot, WSL shutdown, host
pressure, or an external write.

The product and benchmark harness never terminate a WSL distribution, shut it
down, reboot it, or invoke any VM lifecycle operation. The Windows benchmark
watchdog bounds and may stop only its own launched host child. If that child
reaches its deadline, the result is `RED/watchdog_timeout_red` with terminal
state `unverified_unknown`: it is not a clean `PRODUCT_OFF` observation, is
never benchmark evidence, stops promotion, and has no automatic retry. CUDA
final cleanup still runs, but no cleanup is inferred from the timeout.

## Technical context

RamShared currently uses a userspace Rust daemon and the WSL2 NBD path for the
day-one cascade. The repository also contains ublk research and historical
capability drills. WSL2 teardown evidence requires swapoff-first ordering; a
daemon must not be killed while its block device remains in `/proc/swaps`.
The existing Relay safety surface is an independent WSL lifecycle gate.

The product release must be self-contained and auditable. Every installed
version is sealed under `/opt/ramshared/releases/<version>`. The release
directory is root-owned, has no in-place writes after sealing, and contains a
manifest covering every executable and configuration file used by the product.
An atomic `current` selector may point at one sealed version, but it must never
mutate a sealed version in place.

The lower-tier capacity gate is exact. Let `V` be the configured logical VRAM
NBD tier size and `L` be usable capacity available to absorb pages while the
NBD tier is demoted or swapped off:

```text
L >= V + max(ceil(0.10 * V), 512 MiB)
```

`L` is free absorbable capacity at the decision point, not nominal disk size.
The check must identify the selected lower tier and account for alignment and
already-used capacity. A stale or unreadable measurement is a refusal.

## Recommended option

Ship one NBD-only WSL2 path with these decisions:

| Decision | Contract |
| --- | --- |
| Transport | NBD is the only supported WSL2 product transport. |
| Release root | Sealed `/opt/ramshared/releases/<version>` artifacts; no in-place mutation. |
| Legacy ublk | Retire product service/autostart/control-plane references; never unload `ublk_drv`. |
| Legacy unit migration | Replace an inactive, disabled legacy `ramshared-cascade.service` only with a current approval bound to its observed SHA-256; preserve an immutable backup before the atomic replacement. |
| Status | `PRODUCT_OFF`, `READY`, or `BLOCKED`, with an independent readiness reason. |
| Capacity | Enforce `L >= V + max(ceil(10% of V), 512 MiB)` before activation and demotion. |
| Sizes | Promote in order: 1 GiB pilot, then 2 GiB, then 4 GiB. No skipped size. |
| Product host action | No product- or benchmark-triggered reboot, shutdown, WSL termination, or VM lifecycle; refuse if one is needed. |
| Harness containment | The Windows outer watchdog may stop only its bounded launched host child after the outer deadline; classify the cell `RED/watchdog_timeout_red/unverified_unknown`, never as success or `PRODUCT_OFF`. |
| Relay | `scripts/safety/wsl-relay-health.sh --check` must pass; no automatic `--reap`. |
| Identity | A daemon run must satisfy `BINARY_MATCH` against the sealed release. |

The default user-facing operation is a read-only preflight. Installation,
service retirement, activation, and deactivation are separate mutating steps
requiring a current explicit approval. Approval never expands to a reboot or
to an external communication.

## Functional requirements

| ID | Requirement | Acceptance |
| --- | --- | --- |
| RF-NBD-1 | Use NBD as the sole supported WSL2 product transport. | A ready report contains no ublk product dependency or fallback. |
| RF-NBD-2 | Keep every release immutable below `/opt/ramshared/releases/<version>`. | Post-seal write, hash, owner, and mode checks fail closed. |
| RF-NBD-3 | Retire legacy ublk product services without module unload. | No product unit starts ublk; `ublk_drv` may remain loaded and untouched. |
| RF-NBD-4 | Distinguish `PRODUCT_OFF` from `READY` and `BLOCKED`. | Status tests prove intentional off is not a readiness alias. |
| RF-NBD-5 | Enforce the lower-tier formula before start, resize, and demotion. | A shortfall, overflow, stale sample, or ambiguous sink refuses. |
| RF-NBD-6 | Gate 1 GiB, 2 GiB, and 4 GiB in that order. | A failed or partial cell prevents promotion to the next size. |
| RF-NBD-7 | Require explicit approval for mutations and prohibit reboot. | Missing approval or any reboot request returns a refusal before mutation. |
| RF-NBD-8 | Require the Relay gate. | A non-zero/read-ambiguous `--check` result yields `BLOCKED`. |
| RF-NBD-9 | Require `BINARY_MATCH` for every daemon-backed run. | `/proc/<pid>/exe` resolves to the selected sealed release and its manifest hash. |
| RF-NBD-10 | Retain swapoff-first teardown. | NBD swap is absent before daemon stop or device disconnect. |
| RF-NBD-11 | Publish public-safe evidence. | Reports contain no host identity, secret, private path, or raw log. |
| RF-NBD-12 | Make the actual cascade teardown executor fail closed after an injected `swapoff` failure. | Local action tracing proves `swapoff` completes before NBD disconnect/daemon stop and emits neither later action after refusal. |
| RF-NBD-13 | Migrate the conflicting legacy cascade unit without a blind overwrite. | The installer refuses an absent, mismatched, stale, active, enabled, symlinked, or unapproved legacy unit; an approved exact SHA-256 creates an immutable backup and atomically replaces only that unit. |
| RF-NBD-14 | Compare disk-only and NBD under one identical base topology. | Every pair starts a fresh product-owned 1 GiB zram tier at priority 200; disk-only uses one fresh exact 8 GiB scratch swapfile at priority 100, while NBD uses one exact `V`-sized NBD tier at priority 100. The pre-existing host lower sink is untouched and excluded from the second-tier comparison. |
| RF-NBD-15 | Prove that each size cell exercises its declared tier rather than merely allocating address space. | Each worker is admitted before start to a cgroup with `memory.high=1200 MiB` and emergency `memory.max=V+3072 MiB`, uses `V + 2560 MiB`, waits at a start barrier, holds the allocation, and records zram/second-tier deltas sufficient to prove occupancy. |
| RF-NBD-16 | Keep the bounded external GPU condition comparable across a pair. | One CUDA context is created and held across the disk-only and NBD cells for the same size/condition pair; the pair records the same ready-time GPU snapshot. |
| RF-NBD-17 | Bound every Windows-to-WSL process call and every campaign handshake. | Release discovery and cell execution have explicit deadlines; every ready/release artifact is fresh and create-once; a timeout is `RED/watchdog_timeout_red/unverified_unknown`, never inferred cleanup. |
| RF-NBD-18 | Capture enough context to reproduce and audit each benchmark result. | Evidence records branch/commit and dirty state, sealed release and script hashes, kernel, GPU total/free/utilization/temperature, RAM and swap baselines, lower-tier identity/free capacity, exact command parameters, pair identity, and terminal classification, with a sanitized public envelope. |
| RF-NBD-19 | Keep test seams out of approved live execution. | Fixture overrides for roots, swap files, PIDs, and lower sinks are accepted only by manufactured tests; an approved live invocation rejects or ignores them and uses the sealed product paths. |
| RF-NBD-20 | Quantify backend tradeoffs and detect compatible historical regressions. | Every pair reports NBD/disk median and p99 ratios; optional baseline comparisons require an exact environment/workload fingerprint and use the SPEC's numeric GREEN/YELLOW/RED thresholds. |

## Non-functional requirements

| ID | Requirement |
| --- | --- |
| NFR-NBD-1 | Read-only checks have no host mutation and need no elevated approval. |
| NFR-NBD-2 | Mutating operations are bounded, idempotent, and auditable. |
| NFR-NBD-3 | A missing WSL/GPU/Relay environment is `PARTIAL` or `BLOCKED`, never `DONE`. |
| NFR-NBD-4 | Benchmark cells use at least three runs and report median, p99, deviation, and units. |
| NFR-NBD-5 | No unsupervised swap pressure, GPU workload, reboot, shutdown, termination, or VM lifecycle is permitted. Product and benchmark code never terminate WSL. The outer watchdog may stop only its bounded launched host child; a deadline is `RED/watchdog_timeout_red/unverified_unknown`, with no cleanup claim, retry, or promotion. |
| NFR-NBD-6 | A release selector cannot point at an unsealed, dirty, or partially copied directory. |
| NFR-NBD-7 | Disk/NBD comparisons use one 1 GiB zram base, one second-tier backend, one workload contract, and one CUDA context per bounded pair. |
| NFR-NBD-8 | Every WSL process and campaign handshake is bounded, fresh, and independently observable. |
| NFR-NBD-9 | Live evidence cannot be produced by swapping in fixture paths or environment seams. |

## Flows

### Read-only readiness

1. Resolve the selected sealed version and verify its manifest, owner, mode,
   and executable hashes.
2. Capture `/proc/swaps`, managed process identity, NBD device identity,
   lower-tier free capacity, VRAM budget, and current product state.
3. Run the Relay `--check` gate and classify its exit and JSON result.
4. Discover legacy ublk units and active devices without loading or unloading
   modules. Any active ublk product device is a refusal.
5. Return `READY` only when every gate passes. Return `PRODUCT_OFF` for an
   intentionally quiescent product with no active managed swap; return
   `BLOCKED` for an unsafe, ambiguous, or incomplete condition.

### Activation

1. Obtain a current explicit approval for the named version and size.
2. Re-run read-only readiness; approval does not waive any gate.
3. Start only the sealed NBD daemon, verify `BINARY_MATCH`, and attach NBD.
4. Confirm lower-tier capacity, Relay health, priority ordering, and exact
   `/proc/swaps` identity before publishing the active status.
5. On any failure, execute the bounded rollback and leave `BLOCKED` or
   `PRODUCT_OFF`; never silently start ublk.

### Deactivation and legacy retirement

1. Refuse new work and record the exact managed NBD identity.
2. `swapoff` the NBD tier and wait for the postcondition that it is absent
   from `/proc/swaps`; if it fails, keep the daemon and device alive.
3. Disconnect NBD, stop the daemon, and verify no managed process or ghost
   device remains.
4. Disable/remove the legacy ublk product service references only with the
   approved package transaction. Do not run `rmmod`, `modprobe -r`, or an
   equivalent module unload.
5. Publish `PRODUCT_OFF` only after the neutral postcondition is verified.

### Size promotion

Run the complete pilot cell at 1 GiB. Promote to 2 GiB only after all named
functional, integrity, lifecycle, and benchmark gates are green. Promote to
4 GiB only after the 2 GiB cell is green. `PARTIAL`, `BLOCKED`, or an
environment-bound result stops promotion and leaves the previous proven size
as the maximum claim.

### Benchmark pair

For each tier and condition, the harness creates a fresh pair directory and
captures one baseline snapshot. It then:

1. verifies numeric GPU headroom immediately before the pair and again after a
   bounded CUDA workload reports ready;
2. starts the same fresh 1 GiB zram tier at priority 200 for both controls;
3. runs disk-only with one newly created, identity-bound 8 GiB scratch swapfile
   at priority 100, then performs swapoff-first cleanup and proves the scratch
   path is absent;
4. runs NBD with one exact `V`-sized NBD tier at priority 100, proving
   `BINARY_MATCH`, second-tier activity, and swapoff-first cleanup;
5. keeps one CUDA context alive across both cells when the condition is
   `bounded`, and releases it only after the NBD side has reached its terminal
   state; and
6. records the pair as one comparable snapshot. Any refusal, timeout,
   checksum mismatch, residual/ghost swap, failed cleanup, or watchdog deadline
   stops promotion. A watchdog timeout is recorded as
   `RED/watchdog_timeout_red/unverified_unknown`, not as a clean terminal
   state or cleanup claim.

The worker contract is deliberately size-dependent: it allocates and holds
`V + 2560 MiB` while a 1200 MiB `memory.high` forces reclaim and a
`V + 3072 MiB` `memory.max` bounds emergencies without killing valid
swapcache/writeback. A start barrier places
the worker in the cgroup before allocation begins. Each sample must prove
occupancy through observed zram/second-tier deltas and record
`allocation_to_hold_ms`, `allocation_chunk_bytes`, and `worker_threads`; the
labels `block_size` and `queue_depth` are not used for this workload contract.

## Data/state model

The state is a product contract, not a claim about an external environment.

| Field | Values | Meaning |
| --- | --- | --- |
| `product_state` | `PRODUCT_OFF`, `READY`, `BLOCKED` | Operational/readiness classification. |
| `readiness_reason` | `not_evaluated`, `all_gates_pass`, stable refusal code | Why the state was selected. |
| `release_version` | sealed version or null | `/opt/ramshared/releases/<version>` identity. |
| `transport` | `nbd`, `none` | `ublk` is never a product value in this pack. |
| `vram_bytes` | non-negative integer | Configured logical NBD tier size. |
| `lower_free_bytes` | non-negative integer or unknown | Absorbable capacity at the gate. |
| `capacity_gate` | `pass`, `fail`, `unknown` | Formula result. |
| `relay_gate` | `pass`, `fail`, `unknown` | Read-only Relay check result. |
| `binary_match` | `pass`, `fail`, `not_applicable` | Live executable identity result. |
| `approval` | `not_required`, `present`, `missing` | Scope-limited mutation approval. |
| `watchdog_outcome` | `not_used`, `completed_clean`, `watchdog_timeout_red` | Whether the separate Windows containment watchdog fired; it may stop only the bounded host child and is never a product success or cleanup state. |
| `terminal_classification` | `PRODUCT_OFF`, `RED`, `BLOCKED`, `PARTIAL` | Evidence classification after the cell/pair, independent of process exit code. |

`PRODUCT_OFF` requires no active managed swap, no active product daemon, and
no active product ublk service. A loaded kernel module is allowed. `READY`
requires all positive gates and means the NBD product may be activated; it does
not claim that a workload has run. `BLOCKED` is fail-closed and must include a
stable reason code. `RED` means the harness or product crossed a safety/error
boundary; `unverified_unknown` specifically means the watchdog deadline
stopped the bounded child before a clean postcondition was observed.

## Interfaces

| Interface | Contract |
| --- | --- |
| `scripts/safety/wsl-relay-health.sh --check` | Read-only Relay gate; no automatic reap or restart. |
| `/proc/swaps` | Exact before/after identity and swapoff-first evidence. |
| `/opt/ramshared/releases/<version>` | Immutable executable/configuration source. |
| `ramsharedd` | NBD daemon only; live path must pass `BINARY_MATCH`. |
| Legacy ublk service inventory | Discover, retire under approval, never unload module. |
| Windows outer watchdog | Separate harness authority; stops only its bounded launched host child after deadline, classified `RED/watchdog_timeout_red/unverified_unknown`, never a WSL/Windows/VM lifecycle operation. |
| Status JSON | Stable state/reason fields; unknown data is not inferred as ready. |

## Dependencies/risks

| Risk | Mitigation and rollback trigger |
| --- | --- |
| Release tree is modified after launch | Hash/owner/mode mismatch → stop, do not promote; select last sealed version. |
| Lower tier cannot absorb NBD pages | Formula shortfall, stale sample, or `swapoff` failure → keep daemon/device and report `BLOCKED`. |
| Legacy ublk service remains active | Active unit/device or unknown ownership → do not activate NBD; retire only after explicit approval. |
| Relay is stranded or unreadable | Non-zero/ambiguous check → `BLOCKED`; do not reap automatically. |
| Daemon identity is stale/deleted | `BINARY_MATCH` fail → no readiness claim; stop only after swapoff-first. |
| Legacy cascade unit conflicts with the sealed unit | Refuse by default. A separately approved SHA-256-bound migration backs up the inactive root-owned regular legacy unit before atomic replacement; any mismatch or replacement failure restores it. |
| WSL asks for reboot/shutdown | Refuse before mutation; status remains `BLOCKED` or `PRODUCT_OFF`. |
| Large size causes host pressure | Stop the cell, preserve evidence, and do not promote; no shared-host pressure is inferred. |
| Operator retries a deterministic failure | One bounded attempt; #15 refusal prevents blind retry. |
| Worker escapes its cgroup before pressure starts | Start the worker from a cgroup-resident launcher, wait for a create-once barrier, verify membership, then release it; missing proof is `RED`, not a usable sample. |
| Disk control does not match NBD topology | Start identical 1 GiB zram for both and use only the exact fresh 8 GiB scratch as disk second tier; any topology drift invalidates the pair. |
| Allocation is too small to exercise a larger tier | Use `V + 2560 MiB` and require size-specific zram/second-tier occupancy deltas; no result is promoted from a fixed-size workload. |
| Bounded CUDA load changes between controls | One CUDA context spans both cells in a pair; if it cannot be kept ready for both, classify the pair `RED`/`PARTIAL` and stop promotion. |
| WSL controller hangs before the watchdog | Bound release discovery and each cell; a timeout is `RED/watchdog_timeout_red/unverified_unknown`, never assumes `PRODUCT_OFF` or cleanup, and has no retry. |
| Test seam reaches live execution | Approved live mode rejects fixture roots, swap/PID overrides, and lower-sink overrides; a seam-bearing run is invalid evidence. |
| Evidence cannot reproduce a claim | Capture sanitized host/kernel/GPU/RAM/swap/lower-tier context, exact command and hashes, pair identity, and terminal classification before publishing. |

The rollback trigger for any non-trivial runtime change is an observable
violation of swapoff-first, release immutability, binary identity, capacity,
or no-reboot policy.

## Implementation strategy

The local Step 3 source slice implements the pure state/capacity model in
`crates/ramshared-tier/src/nbd_readiness.rs`, manufactured release/preflight
checks in `scripts/safety/test-nbd-product-preflight.sh`, installer rollback
injection after every named post-write phase, and the actual cascade teardown
executor in `crates/ramshared-cli/src/cascade/cascade_io.rs`. The teardown seam
is injected and local: it records all `swapoff` actions before any NBD
disconnect or daemon stop, and returns before either later action when a
`swapoff` is refused.

The benchmark files are a source-partial harness, not proof that the matrix is
safe to run. Before a live claim, implementation must close the pair topology,
start-barrier/cgroup, size-occupancy, one-CUDA-context, bounded-process,
observability, and live-seam gates in SPEC. Local tests may use fixture paths
and injected seams; approved live execution may not. Complete paired disk/NBD
execution, repeated per-cell `BINARY_MATCH`, the ordered 1/2/4 GiB benchmark
cells, and the Gate A corrections remain environment-bound. The sealed 1 GiB
pilot does not close those matrix gates. None may be promoted from local source
tests. `IMPL.md` records local numbers as `partial`; root `validation.md`
remains untouched without the required live evidence.

## Documents

| Document | Action |
| --- | --- |
| `PRD.md` | Maintain the source-partial decision contract. |
| `SPEC.md` | Maintain the executable source/host-bound contract. |
| `AUDIT-2.5.md` | Maintain the adversarial risk review. |
| [`wsl2-upstream-native-contribution`](../wsl2-upstream-native-contribution/CONTRIBUTION-CHECKLIST.md) | Keep config-only issue drafts and milestone mapping together. |
| `IMPL.md` | Create as a `partial` local-evidence record after tests and coverage. |
| `validation.md`, release docs | Do not change without live before/action/after evidence. |
| `docs/INDEX.md` | Regenerate as a generated index after the pack is added. |

## Out of scope

- ublk as a product transport, ublk swap, or a ublk module unload;
- custom-kernel activation, WSL shutdown, Windows reboot, or host service restart;
- changes to Microsoft repositories, GitHub issues, pull requests, or releases;
- direct host WDDM ownership or native VRAM/N3 integration;
- benchmark claims without the named before/action/after and n≥3 evidence;
- product- or benchmark-triggered WSL termination, shutdown, reboot, or VM
  lifecycle; the deadline-bound Windows watchdog may stop only its launched
  host child and is always `RED/watchdog_timeout_red/unverified_unknown`;
- CI/workflows, release documentation, validation records, or `MEMORY.md` in
  this task.

## Acceptance

| ID | Criterion |
| --- | --- |
| A-NBD-1 | The supported WSL2 product transport is unambiguously NBD-only. |
| A-NBD-2 | Immutable release, `BINARY_MATCH`, capacity, Relay, approval, and no-reboot gates are explicit. |
| A-NBD-3 | `PRODUCT_OFF` cannot be reported as `READY`; `BLOCKED` is fail-closed. |
| A-NBD-4 | Legacy ublk retirement is service/control-plane retirement with no module unload. |
| A-NBD-5 | 1 GiB → 2 GiB → 4 GiB promotion and the benchmark matrix are named. |
| A-NBD-6 | Rollback, Kahneman, security, and environment-bound partial rules are present. |
| A-NBD-7 | Source tests are explicit local evidence only; no live product readiness is claimed. |
| A-NBD-8 | The injected teardown contract and every named installer post-write rollback phase have a paired manufactured test. |
| A-NBD-9 | Disk-only and NBD are paired under identical 1 GiB zram, workload, cgroup, and (when bounded) one CUDA context; only the second tier differs. |
| A-NBD-10 | Every sample proves cgroup admission before allocation, size-dependent `V + 2560 MiB` occupancy, and uses the declared workload labels. |
| A-NBD-11 | All WSL calls and handshakes are bounded/fresh, and a watchdog deadline is `RED/watchdog_timeout_red/unverified_unknown` rather than inferred cleanup. |
| A-NBD-12 | Evidence carries sanitized execution context, exact command, release/script hashes, capacity/headroom, pair identity, and terminal state. |
| A-NBD-13 | Fixture seams are demonstrably unavailable in the approved live path. |

## Validation

Local validation is source/static/manufactured only: Rust format, clippy,
package tests, exact per-file coverage, shell syntax, the safe shell harness,
docs checks, and `git diff --check`. The live rows in the SPEC remain
environment-bound: `nbd_lifecycle_before_action_after`,
`NBD_BENCHMARK_MATRIX` is `PARTIAL`/`NO-GO` because no complete paired 1/2/4
GiB run exists. The lifecycle, Relay, and `BINARY_MATCH` rows have pilot-level
evidence only and must repeat on every matrix NBD cell. Until the corrected
matrix runs on the actual approved WSL2 NBD surface, the verdict is
`PARTIAL`/`BLOCKED`, never `DONE`.
