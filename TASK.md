# TASK.md — RamShared

> Live task register. Tasks are mutable records, not an append-only journal:
> every material change must advance its updated date and time. Evidence remains
> append-only
> in [`validation.md`](validation.md).

## Record schema

- **Schema:** `ramshared.task.v1`.
- **Status:** `planned`, `in_progress`, `blocked`, `completed`, or `cancelled`.
- **Time fields:** one `Date` for same-day changes, otherwise separate dates,
  plus separate registration and update times.
- **Source revision:** the Git revision observed when the record was written.
- **Diff rule:** an existing record may change only when its updated date/time advances.

<!-- task-schema-v1 -->

## TASK-0001 — Temporal governance records

**Schema:** `ramshared.task.v1`.
**Status:** `completed`.
**Owner role:** `governance`.
**Date:** `2026-08-11`.
**Registered time:** `12:15:25`.
**Updated time:** `12:16:00`.
**Source revision:** `fd5cbf2d39a026bcf737a3082ef2497d3861b257`.
**Destinations:** `TASK.md`, `validation.md`, `docs/governance/RECORD-SCHEMAS.md`, and `tools/ci/`.
**Scope:** Add RamShared-native temporal task and evidence record contracts without changing legacy validation entries or CI wiring.
**Evidence / blockers:** 26 focused Node tests and both schema checkers passed; no live hardware or destructive operation is part of this scope.

## TASK-0002 — Native governance, evidence, and cleanup parity

**Schema:** `ramshared.task.v1`.
**Status:** `completed`.
**Owner role:** `governance`.
**Date:** `2026-08-11`.
**Registered time:** `12:35:59`.
**Updated time:** `12:43:55`.
**Source revision:** `6e488df7dba5cf92a2174f59b8330d7416d68b01`.
**Destinations:** `docs/governance/`, `docs/runbooks/`, `docs/labs/`, `docs/security/`, `docs/decisions/`, `docs/reference/`, `docs/specs/no-milestone/campaign-evidence-lifecycle/`, `tools/ci/`, `scripts/docs-check.sh`, and `.github/workflows/ci.yml`.
**Scope:** Bring the native RamShared documentation, evidence custody, historical cleanup receipts, ADR registry, threat model, passive inventories, and pull-request ratchets to the same governance depth without copying another product model or operating a host/lab boundary.
**Evidence / blockers:** Documentation, evidence, cleanup, ADR, threat, and CI-ratchet gates passed locally, including the full Node suite and serial Rust workspace suite. No live Windows, WSL2, GPU, driver, VM, swap, disk, or kernel campaign is authorized by this task; such proof remains environment-bound.

## TASK-0003 — Canonical CI automatic entrypoint

**Schema:** `ramshared.task.v1`.
**Status:** `completed`.
**Owner role:** `ci-governance`.
**Date:** `2026-08-11`.
**Registered time:** `13:21:44`.
**Updated time:** `13:21:44`.
**Source revision:** `35ff48a793aaf52b6552b7b831138160a5f258e1`.
**Destinations:** `.github/workflows/`, `docs/ci/CI-TOPOLOGY.md`, `docs/governance/ci-contract.json`, `docs/specs/no-milestone/ci-trust-and-release-integrity/`, and `tools/ci/`.
**Scope:** Route automatic pull-request and main events through the single same-run CI Contract aggregate, keep canonical children reusable-only, and reject any undeclared direct trigger.
**Evidence / blockers:** The local contract, 309 Node tests, documentation gates, Actionlint, and Gitleaks passed. Hosted same-revision qualification remains an explicit promotion blocker; the prior hosted run does not qualify the changed topology.

## TASK-0004 — WSL2 NBD benchmark qualification

**Schema:** `ramshared.task.v1`.
**Status:** `in_progress`.
**Owner role:** `wsl2-reliability`.
**Date:** `2026-08-12`.
**Registered time:** `12:15:54`.
**Updated time:** `20:13:18`.
**Source revision:** `1aaeb5b3c369ffbee83b3abbd0af6e594e73e25d`.
**Destinations:** branch `feat/wsl2-nbd-benchmark-matrix`, milestone `v0.9.0-beta.1 — WSL2 NBD`, issue `#194`, `docs/specs/no-milestone/wsl2-nbd-product-readiness/`, `scripts/safety/`, `scripts/windows/`, `scripts/p0/`, and the sealed Linux bundle.
**Scope:** Correct and qualify the paired 1/2/4 GiB disk-versus-NBD benchmark with identical zram topology, cgroup-before-start containment, exact scratch identity, pair-scoped CUDA, bounded Windows-to-WSL calls, source/BINARY_MATCH binding, numeric comparison/regression records, and fail-closed cleanup. Run no live pressure until static/manufactured gates and a fresh independent Sol Gate A pass.
**Evidence / blockers:** Local cell 23/23, preflight 26/26, PowerShell static, docs, and per-file Rust coverage are green. Attempt 13 used sealed source `73e1977`: disk-only 1 GiB passed 3/3; NBD sample one reached HOLD and passed integrity, then the generic `BASELINE_REPUBLICATION_FAILED` concealed the exact post-load boundary. Cleanup left only `/dev/sdc`, with no RamShared process and no reboot or WSL shutdown. A supervised no-pressure diagnostic proved the preserved NBD device can be swapoff→mkswap→swapon with the same kernel PID. RED checkpoint `7b33d20` now has stable reasons for all eight transaction frontiers and cardinality-safe forged-output collapse; fresh independent Gate A passed. GREEN commit, rebuild/redeploy, and a fresh matrix remain before further code decisions. Root validation append, Gate B, PR checks, and merge remain open.
