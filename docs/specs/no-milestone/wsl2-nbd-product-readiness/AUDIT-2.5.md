# AUDIT-2.5 — WSL2 NBD-only product readiness

This audit reviews [`SPEC.md`](SPEC.md) against the WSL2 NBD product
boundary, the existing swapoff-first and Relay safety surfaces, and SSDV3
Kahneman #2/#3/#9/#13/#15–#18. The named Step 3 source slice is implemented:
pure policy, manufactured release/preflight checks, all post-write installer
rollback injections, and the injected cascade teardown seam. It does not claim
that the product is ready, that a release is installed, or that any live WSL
evidence exists.

## Findings

| Sev | SPEC § | Finding | Disposition |
| --- | --- | --- | --- |
| HIGH | Technical decisions; ITEM-2 | A mutable or stale release could make a green status refer to the wrong daemon/config. | Sealed version directories, manifest verification, atomic selector, and `BINARY_MATCH` are mandatory before `READY`. |
| HIGH | DT-NBD-6; ITEM-4 | Nominal lower-tier size is not enough to absorb active NBD pages. | Exact free-capacity formula, sink identity, alignment, stale-sample refusal, and swapoff-first rollback are closed in SPEC. |
| HIGH | DT-NBD-13; ITEM-4 | A status-only lifecycle view could document safety while the actual teardown executor still disconnects or stops after a failed swapoff. | The real `cascade_io.rs` executor now has an injected local plan; named ordering/refusal tests require no disconnect or daemon stop after a manufactured swapoff failure. |
| HIGH | DT-NBD-4; ITEM-1 | Removing a service by unloading `ublk_drv` would cross the wrong ownership boundary and could destabilize the host. | Retirement is product service/control-plane only; no module unload is permitted or required. |
| HIGH | DT-NBD-5; ITEM-3 | Treating quiescence as readiness would hide an unavailable product. | `PRODUCT_OFF`, `READY`, and `BLOCKED` are distinct and have paired tests. |
| MEDIUM | DT-NBD-10; ITEM-6 | Relay repair could become an accidental destructive dependency. | Only read-only `--check` is a gate; candidate/ambiguity blocks and automatic `--reap` is forbidden. |
| MEDIUM | ITEM-5 | A 4 GiB result could be promoted from a single optimistic run. | Ordered 1/2/4 GiB matrix requires n≥3, p50/p99/deviation, integrity, and terminal-state evidence per cell. |
| MEDIUM | DT-NBD-8/9 | Approval wording could accidentally authorize a reboot or WSL shutdown. | Mutation approval is scope-limited; restart/shutdown/termination are hard refusals. |
| LOW | Observability | A process existence check alone is insufficient for daemon identity. | `/proc/<pid>/exe`, manifest digest, release selector, and swap identity are all recorded. |

## Open questions

No design question blocks the bounded Step 3 plan. Implementation must stop and
revise this SPEC in place if any of these assumptions is false:

- the deployed filesystem cannot enforce sealed release ownership/modes and a
  content manifest;
- the selected WSL lower tier cannot report free absorbable capacity at the
  swapoff decision point;
- the legacy ublk service identity cannot be enumerated without broadening a
  stop target; or
- the Relay script changes its read-only exit/result contract.

Those are executable implementation gates, not permission to weaken the formula,
use nominal capacity, kill a daemon with active swap, unload a module, or claim
readiness from a mock.

## Re-audit checks

| Gate | Result |
| --- | --- |
| NBD is the sole WSL2 product transport | PASS — explicit DT-NBD-1 and refusal test |
| Immutable release and live binary identity | PASS — DT-NBD-2/3/11 and ITEM-2 |
| ublk retirement preserves module lifetime | PASS — DT-NBD-4 and ITEM-1 |
| Product-off versus ready semantics | PASS — DT-NBD-5 and ITEM-3 |
| Capacity formula and lower-tier exhaustion | PASS — DT-NBD-6 and ITEM-4 |
| Ordered size/benchmark matrix | PASS — DT-NBD-7 and ITEM-5 |
| Approval/no-reboot boundary | PASS — DT-NBD-8/9 and ITEM-6 |
| Relay ownership boundary | PASS — DT-NBD-10 and paired refusal |
| Swapoff-first and layer rollback | PASS locally — injected `swapoff_completes_before_nbd_disconnect`, `failed_swapoff_keeps_daemon_and_device_alive`, and all 17 manufactured installer post-write rollback phases |
| Security/public-hygiene boundary | PASS — checklist and ITEM-7 |
| Live WSL, GPU, and host evidence | NOT RUN — environment-bound; no claim made |

## Verdict

**`go` for the bounded SSDV3 Step 3 source-partial implementation in ITEM order.**

**`no-go` for product promotion or a `READY` claim** until the release,
Relay, BINARY_MATCH, capacity, swapoff-first, 1 GiB pilot, 2 GiB, and 4 GiB
before/action/after evidence gates pass. Any implementation that introduces a
ublk product fallback, module unload, mutable release, stale binary match,
automatic Relay reap, reboot, forced teardown, nominal-only capacity, or
prestige-based size promotion changes this verdict to `no-go` and requires an
in-place SPEC correction plus re-audit.
