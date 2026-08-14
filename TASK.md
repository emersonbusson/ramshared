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
**Status:** `completed`.
**Owner role:** `wsl2-reliability`.
**Registered date:** `2026-08-12`.
**Updated date:** `2026-08-14`.
**Registered time:** `12:15:54`.
**Updated time:** `05:42:15`.
**Source revision:** `a365bda0daf89a9707159b86efca8c1ba1ac760b`.
**Destinations:** branch `feat/wsl2-nbd-benchmark-matrix`, milestone `v0.9.0-beta.1 — WSL2 NBD`, issue `#194`, `docs/specs/no-milestone/wsl2-nbd-product-readiness/`, `scripts/safety/`, `scripts/windows/`, `scripts/p0/`, `tools/ci/`, and the sealed Linux bundle.
**Scope:** Correct and qualify the paired 1/2/4 GiB disk-versus-NBD benchmark with identical zram topology, cgroup-before-start containment, exact scratch identity, pair-scoped CUDA, bounded Windows-to-WSL calls, source/BINARY_MATCH binding, numeric comparison/regression records, and fail-closed cleanup. Run no live pressure until static/manufactured gates and a fresh independent Sol Gate A pass.
**Evidence / blockers:** Attempt30 is the latest sealed live attempt, on source `a365bda0daf89a9707159b86efca8c1ba1ac760b`. The canonical matrix completed all 12 P1/P2/P4 idle/bounded disk-only/NBD cells and all 36 samples with integrity, occupancy, per-NBD-cell `BINARY_MATCH=PASS`, pair-scoped CUDA, and per-cell `PRODUCT_OFF`. Matrix SHA-256 is `42fa3e1a00dd7e7c16f0c92196f69622ac9212c9fb889e858f6e40769af292af`; its 551-entry inventory SHA-256 is `58a959fd82d29b6c503382a98d82a4bbf57bb90dc94ffaf2fdc2dfa6e985aece`, and every entry verified. All six public pair records passed the repository validator and were appended as `BASELINE`/nonpromotable because no prior canonical baseline exists. Terminal preflight returned `PRODUCT_OFF`; services are inactive/disabled, no managed swap, daemon, worker, CUDA process, benchmark cgroup, or NBD attachment remains, and `/dev/sdc` was untouched. Gate B, hosted required checks, and PR evidence/metadata review passed; PR `#202` merged as `3aa0c1d6c278b823b70d2cc638b87d7477ea1c0d` and issue `#194` closed.
**Attempt 21 integrity qualification:** The post-containment checksum PASS covered only a partial `6016/6656 MiB` allocation. It did not complete the third sample and is not a valid cell result.

Attempt22b (2026-08-13) was a live run on sealed source `63bd3be`: four P1
cells passed 3/3, then Q2 disk-only was `RED/SAMPLE_TIMEOUT`; cleanup produced
verified `PRODUCT_OFF`. Matrix and failure-receipt hashes are recorded above.
The partial HOLD/integrity artifacts are diagnostic-only and do not constitute
a completed sample or promotion evidence. The subsequent Q2 correction slice
was source-only (no live RamShared/WSL product process, NBD, swap, CUDA,
cgroup, reboot, termination, or recording action): P1/Q2/Q4 then used
`120/900`, `240/1020`, and `600/2100`; Attempt27 supersedes those single
deadline tuples, and
the named `partial_timeout_integrity_not_promoted` manufactured regression is
green. A new sealed build and complete live matrix remain required.

Attempt24 is source-only and nonpromotable: sealed preflight is the sole
`PRODUCT_OFF` authority, with two bounded 15-second observations (30 seconds
total) per lifecycle epoch. No live RamShared/WSL product process, NBD, swap,
CUDA, cgroup, reboot, termination, or recording action occurred. The
manufactured suites did intentionally create short-lived test subprocesses
and temporary evidence fixtures; those are local refusal tests, not live
product activity or promotion evidence.

The prior source-only Gate A correction was nonpromotable: cell
manufactured/refusal tests were 44/44 and the verified preflight suite was
32/32. The integrated zombie-liveness increment now makes the preflight suite
33/33. It bounds
worker TERM/KILL grace to 1–30 seconds, refuses every inherited preflight and
manufactured seam in live mode before artifact creation, keeps the first
TERM/INT status through a single bounded cleanup epoch, requires unique
allowlisted `PRODUCT_OFF` authority, and prepares/validates custody candidates
before their no-mutation publication frontier. Dynamic regressions cover
stubborn workers, nonzero `down`, repeated signals across worker/down/two
preflights, and envelope write/fsync/link/validation faults. A fresh sealed
release, independent Gate A, and the complete live 1/2/4 GiB matrix remain
required; no live RamShared/WSL product process, NBD, swap, CUDA, cgroup,
reboot, termination, or recording action occurred in this correction.

The current DT-NBD-47 liveness correction is source-only and nonpromotable:
the preflight now ignores a userspace entry only after exact `status: Z`,
`stat: Z`, `status: Z` observations for the same PID, with matching `Pid`/`Tgid`
and `Kthread: 0`; live, malformed, unreadable, and race-to-live entries fail
closed. The integrated preflight suite is 33/33. A postinstall attempt at
source `d4efe59` was blocked by 171 unrelated pre-existing zombie processes;
that installed result is not evidence, no live revalidation occurred, and a
fresh sealed release is required.

The public-pair custody correction is source-only and nonpromotable: Node
24.15.0 validates 21/21 public-evidence tests at 95.37% lines, 80.60%
branches, and 94.23% functions. The stddev-zero RED→GREEN TDD case now
requires a `null` ratio when the disk population standard deviation is zero.
Candidate and custody comparison hashes bind
the exact emitted `pair-comparison.json` bytes; fresh on-disk summaries, not
mutable cached results, supply public metrics. The repository validator
recomputes the median, p99, and population-stddev ratios with the controller's
six-decimal ToEven rule (and requires `null` when disk stddev is zero), then
enforces the exact baseline-verdict-to-public-decision mapping. YELLOW/RED
classification now keeps the immutable PASS cell summaries intact and adds a
separate nonpromotable pair decision; the focused manufactured writer path
passes for both. Cached summary, status, receipt, or published-artifact
mutation still refuses before promotion. The focused
`Test-NbdBenchmarkMatrixStatic.ps1` suite is green after the stddev-zero
correction; the complete `Test-WindowsCiStatic.ps1 -RepoRoot <repo>` wrapper
was green before that change and remains pending a same-tree rerun.
Independent Gate A is still pending for this frozen tree.

Attempt26 is source-only and remains nonpromotable. The first P1 idle
disk-only run on sealed source `4b5f554` reached the full `3584 MiB` HOLD and
was then stopped while final checksum evidence was still being emitted under
the old five-second cleanup bound. This is RED diagnostic evidence for the
integrity-finalization deadline boundary; it does not prove an NBD, WSL2, or
OOM failure, and independent cleanup returned to `PRODUCT_OFF`.

Attempt27 is source-only and nonpromotable. It records `85.149780 s` from
allocation to HOLD and a right-censored integrity-finalization observation of
`>34.044357 s`; the latter is diagnostic-only and not a performance claim.
The correction adopts Design B: HOLD containment remains P1/P2/P4
`120/240/600 s`; finalization starts only after TERM has been issued at the
observed HOLD boundary and has the same explicit tier-policy cap; outer cells
are `1020/1740/3900 s`. `CudaMaxHoldSec=7920` is an admission cap only; each
bounded P1/P2/P4 pair passes and seals its own exact CUDA hold
`2160/3600/7920 s` (`idle=0`). Runtime, controller, workload, context,
summary, internal envelope, public custody, and checker synchronize the exact
fields. A timeout is `RED`, nonpromotable, stop-first, and can claim
`PRODUCT_OFF` only after exact cleanup custody. The manufactured worker suite
is 3 checks; the cell suite is expected to be 48 checks, and the secondary
signal fixture covers 16 TERM/INT combinations per suite run. The
48-combination figure is historical total coverage from three prior suite
runs, not the canonical per-run metric. No live revalidation or sealed release
exists for this candidate. Fresh independent Gate A, resealing, and the
complete live matrix remain required.

Attempt28 (2026-08-13, sealed source `3794a30`) is the first P1 idle
disk-only RED after that policy. Its only run reached the required `3584 MiB`
`HOLD`, then the 120-second finalization deadline expired with exit `137`.
The sealed receipt records `CGROUP_OOM_KILL_BEFORE=0` and
`CGROUP_OOM_KILL_AFTER=0`, so no cgroup `oom_kill` increment was observed for
that run. It contains no NBD cell or NBD result. The worker log ends at `HOLD`, so
a slow final checksum scan while `memory.high=1200 MiB` is a strong inference,
not a calibrated duration or a proven sole cause. The source-only correction
keeps the hard `memory.max` unchanged, resets `memory.high` to 1200 MiB before
every allocation, and only after occupancy sampling validates exact non-symlink
cgroup values, temporarily sets `memory.high` to that existing hard cap,
re-reads it, then issues TERM. It records sanitized limits and monotonic OOM
receipts plus
bounded 512 MiB verification progress. Timeout remains RED and nonpromotable;
no live rerun, reseal, NBD claim, or promotion is made.

Attempt29 (2026-08-14, sealed source `a60c898`) stopped at the first P1 idle
disk-only cell. Run one completed all `3584 MiB`, HOLD, final checksum, and
occupancy; allocation-to-HOLD took `114056 ms`. Run two reached only
`2048/3584 MiB` before the 120-second HOLD deadline and correctly refused
`SAMPLE_TIMEOUT`. No NBD or bounded cell ran, so no CUDA allocation was
expected or claimed. The matrix and inventory SHA-256 values are
`3f85c9948dc8c733b06351c029bc7a2a1512574cdc1ee8fdd8abfe41b78ef33e`
and `e1d62c1c7a0d349624a8b68a309830495b67b2a2aa3c5efdd24b20a55b558fa9`.
Terminal preflight proved `PRODUCT_OFF`; no public evidence was emitted. The
source-only successor raises only the P1 allocation-to-HOLD cap to 240 seconds,
retains its independent 120-second integrity cap, and derives cell/CUDA bounds
of `1380/2880 s`. It remains nonpromotable until committed, resealed, and
exercised by a complete fresh matrix.

Attempt30 (2026-08-14, sealed source `a365bda`) completed the canonical matrix:
12/12 cells and 36/36 samples passed across P1/P2/P4, idle/bounded, and
disk-only/NBD. Every NBD sample retained `BINARY_MATCH=PASS`; bounded pairs
held one 512 MiB CUDA context across disk then NBD and released it without
force. All cells returned `PRODUCT_OFF`. The matrix and 551-entry inventory
SHA-256 values are `42fa3e1a00dd7e7c16f0c92196f69622ac9212c9fb889e858f6e40769af292af`
and `58a959fd82d29b6c503382a98d82a4bbf57bb90dc94ffaf2fdc2dfa6e985aece`;
every inventory entry verified. Six public pair records and their exact custody
and comparison artifacts pass the repository validator. They are valid
`BASELINE` candidates and remain nonpromotable because no prior canonical
baseline exists. Terminal inspection found no managed product residue and left
the pre-existing `/dev/sdc` unchanged. Gate B, hosted checks, PR review, and
merge remain open.

## TASK-0005 — Exact release manifest transition

**Schema:** `ramshared.task.v1`.
**Status:** `completed`.
**Owner role:** `ci-governance`.
**Date:** `2026-08-14`.
**Registered time:** `05:10:06`.
**Updated time:** `05:58:51`.
**Source revision:** `52b42d1083150dfe6ee823771851e428972b607b`.
**Destinations:** issue `#207`, `docs/governance/ci-contract.json`,
`docs/specs/no-milestone/release-promotion-publication/`, and
`tools/ci/check-ci-contract*`.
**Scope:** Admit only the exact Release Please manifest transition from the
historical `0.8.0` baseline to target `0.9.0-beta.1`, without accepting an
intermediate, future, malformed, or multi-package manifest.
**Evidence / blockers:** TDD reproduced the release-PR false refusal before the
checker change. The focused 61-test contract suite passes; Node coverage is
91.76% lines, 85.04% branches, and 98.67% functions. The local contract,
capability observations, documentation checks, and diff check passed. Hosted
required checks passed and PR `#208` merged as `52b42d1`; issue `#207` closed.

## TASK-0006 — Release Please machine-body recovery

**Schema:** `ramshared.task.v1`.
**Status:** `in_progress`.
**Owner role:** `ci-governance`.
**Date:** `2026-08-14`.
**Registered time:** `05:36:39`.
**Updated time:** `05:58:51`.
**Source revision:** `361427a63cbeb2a8b0ecafb224adeecb0539af9b`.
**Destinations:** issue `#211`, PR `#210`, `release-please-config.json`,
`docs/governance/ci-contract.json`,
`docs/specs/no-milestone/release-promotion-publication/`, and
`tools/ci/check-ci-contract*`.
**Scope:** Recover the App-only beta producer after release PR `#206` was
merged with its machine-readable body replaced. Bound changelog discovery to
the exact `v0.8.0` merge and generate all mandatory PR headings inside the
Release Please header without replacing its release notes.
**Evidence / blockers:** The post-merge Release workflow `31784058119` passed
at the Actions level but its log proves `Pull request body did not match`; no
tag or release was created. It opened oversized PR `#210` from 507 commits.
TDD requires exact `last-release-sha` and all seven generated headings. PR
`#212` merged the recovery as `920e7a1`; Release Please reduced PR `#210` to
the bounded changelog, preserved its machine-readable body, and created the
exact tag and draft prerelease at `361427a`. Removal of the temporary recovery
key remains pending after integrity is repaired.

## TASK-0007 — Keep read-only release integrity outside publication approval

**Schema:** `ramshared.task.v1`.
**Status:** `completed`.
**Owner role:** `ci-governance`.
**Date:** `2026-08-14`.
**Registered time:** `05:58:51`.
**Updated time:** `09:18:00`.
**Source revision:** `361427a63cbeb2a8b0ecafb224adeecb0539af9b`.
**Destinations:** issue `#213`, `.github/workflows/release-integrity.yml`,
`docs/governance/ci-contract.json`,
`docs/specs/no-milestone/release-promotion-publication/`, and
`tools/ci/check-ci-contract*`.
**Scope:** Remove the publication-only `protected-release` environment from
the exact-tag read-only integrity job and make the contract reject any future
integrity deployment environment.
**Evidence / blockers:** Tag and draft creation succeeded, but integrity run
`31785964576` was rejected before checkout because the environment branch
policy does not admit tags. TDD reproduced the stale environment contract.
PR `#214` merged the environment correction as `c47727a` after the hosted
aggregate passed. The exact-tag rerun then checked out and built the target
successfully; its later SBOM filename defect is tracked separately and does
not invalidate the completed environment-boundary correction.

## TASK-0008 — Recover immutable-tag SBOM integrity

**Schema:** `ramshared.task.v1`.
**Status:** `in_progress`.
**Owner role:** `ci-governance`.
**Date:** `2026-08-14`.
**Registered time:** `09:18:00`.
**Updated time:** `09:18:00`.
**Source revision:** `361427a63cbeb2a8b0ecafb224adeecb0539af9b`.
**Destinations:** issue `#215`, `.github/workflows/release-integrity.yml`,
`.github/workflows/release-publication.yml`, `docs/governance/ci-contract.json`,
`docs/specs/no-milestone/{ci-trust-and-release-integrity,release-promotion-publication}/`,
and `tools/ci/check-ci-contract*`.
**Scope:** Correct the pinned cargo-cyclonedx output name and recover the exact
immutable beta tag through a manual read-only tag/SHA-bound integrity run. Do
not delete or move the tag, upload an unvalidated asset, or bypass protected
publication.
**Evidence / blockers:** Integrity run `31785964576` attempt 2 checked out and
built exact source `361427a`, installed `cargo-cyclonedx 0.5.9`, then failed
because `--override-filename ramshared-sbom` emits `ramshared-sbom.json` while
the workflow required `ramshared-sbom.cdx.json`. TDD reproduces the wrong
literal and the missing exact recovery identity. Local Green, hosted checks,
merge, recovered four-file integrity artifact, protected publication, and
removal of the temporary Release Please recovery key remain pending.
