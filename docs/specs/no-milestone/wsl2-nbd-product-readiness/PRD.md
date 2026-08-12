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
release/preflight rollback checks, and a local injected teardown contract. It
does not contain a live WSL2 before → action → after run, so it cannot call the
path `READY`. The honest product verdict remains `PARTIAL`. It deliberately
retires the legacy ublk service from the product control plane without
unloading the `ublk_drv` kernel module. A loaded module is capability evidence
only; it is never a product transport claim.

The positive state is `READY`. The intentional safe state is `PRODUCT_OFF`.
They are not aliases: an installation can be product-off and still have a
valid, read-only readiness report, while a product-off state must never be
reported as an active or ready product. `BLOCKED` is used for a refusal or an
unmet gate. No state in this PRD authorizes a reboot, WSL shutdown, host
pressure, or an external write.

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
| Status | `PRODUCT_OFF`, `READY`, or `BLOCKED`, with an independent readiness reason. |
| Capacity | Enforce `L >= V + max(ceil(10% of V), 512 MiB)` before activation and demotion. |
| Sizes | Promote in order: 1 GiB pilot, then 2 GiB, then 4 GiB. No skipped size. |
| Host action | No reboot, `wsl --shutdown`, or `wsl --terminate`; refuse if one is needed. |
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

## Non-functional requirements

| ID | Requirement |
| --- | --- |
| NFR-NBD-1 | Read-only checks have no host mutation and need no elevated approval. |
| NFR-NBD-2 | Mutating operations are bounded, idempotent, and auditable. |
| NFR-NBD-3 | A missing WSL/GPU/Relay environment is `PARTIAL` or `BLOCKED`, never `DONE`. |
| NFR-NBD-4 | Benchmark cells use at least three runs and report median, p99, deviation, and units. |
| NFR-NBD-5 | No unsupervised swap pressure, GPU workload, reboot, shutdown, or termination is permitted. |
| NFR-NBD-6 | A release selector cannot point at an unsealed, dirty, or partially copied directory. |

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

`PRODUCT_OFF` requires no active managed swap, no active product daemon, and
no active product ublk service. A loaded kernel module is allowed. `READY`
requires all positive gates and means the NBD product may be activated; it does
not claim that a workload has run. `BLOCKED` is fail-closed and must include a
stable reason code.

## Interfaces

| Interface | Contract |
| --- | --- |
| `scripts/safety/wsl-relay-health.sh --check` | Read-only Relay gate; no automatic reap or restart. |
| `/proc/swaps` | Exact before/after identity and swapoff-first evidence. |
| `/opt/ramshared/releases/<version>` | Immutable executable/configuration source. |
| `ramsharedd` | NBD daemon only; live path must pass `BINARY_MATCH`. |
| Legacy ublk service inventory | Discover, retire under approval, never unload module. |
| Status JSON | Stable state/reason fields; unknown data is not inferred as ready. |

## Dependencies/risks

| Risk | Mitigation and rollback trigger |
| --- | --- |
| Release tree is modified after launch | Hash/owner/mode mismatch → stop, do not promote; select last sealed version. |
| Lower tier cannot absorb NBD pages | Formula shortfall, stale sample, or `swapoff` failure → keep daemon/device and report `BLOCKED`. |
| Legacy ublk service remains active | Active unit/device or unknown ownership → do not activate NBD; retire only after explicit approval. |
| Relay is stranded or unreadable | Non-zero/ambiguous check → `BLOCKED`; do not reap automatically. |
| Daemon identity is stale/deleted | `BINARY_MATCH` fail → no readiness claim; stop only after swapoff-first. |
| WSL asks for reboot/shutdown | Refuse before mutation; status remains `BLOCKED` or `PRODUCT_OFF`. |
| Large size causes host pressure | Stop the cell, preserve evidence, and do not promote; no shared-host pressure is inferred. |
| Operator retries a deterministic failure | One bounded attempt; #15 refusal prevents blind retry. |

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

The following remain explicitly unimplemented/environment-bound: live sealed
installation, `BINARY_MATCH`, Relay before/action/after, a WSL2 swapoff run,
and the ordered 1/2/4 GiB benchmark cells. None may be promoted from local
source tests. `IMPL.md` records the local numbers as `partial`; root
`validation.md` remains untouched without live evidence.

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

## Validation

Local validation is source/static/manufactured only: Rust format, clippy,
package tests, exact per-file coverage, shell syntax, the safe shell harness,
docs checks, and `git diff --check`. The live rows in the SPEC remain
environment-bound: `nbd_lifecycle_before_action_after`,
`relay_gate_before_action_after`, and `NBD_BENCHMARK_MATRIX` are `PARTIAL` and
have no `BINARY_MATCH` run. Until they run on the actual approved WSL2 NBD
surface, the verdict is `PARTIAL`/`BLOCKED`, never `DONE`.
