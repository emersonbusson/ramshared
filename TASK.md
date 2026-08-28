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
**Status:** `completed`.
**Owner role:** `ci-governance`.
**Date:** `2026-08-14`.
**Registered time:** `05:36:39`.
**Updated time:** `10:54:33`.
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
exact tag and draft prerelease at `361427a`. Protected publication run
`31793790581` made that exact beta public, and issue `#229` removes both
one-shot recovery keys after the published state was independently verified.

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
**Status:** `completed`.
**Owner role:** `ci-governance`.
**Date:** `2026-08-14`.
**Registered time:** `09:18:00`.
**Updated time:** `10:54:33`.
**Source revision:** `361427a63cbeb2a8b0ecafb224adeecb0539af9b`.
**Destinations:** issues `#215`, `#217`, `#219`, `#221`, `#223`, `#225`,
`#227`, and `#229`,
`.github/workflows/release-integrity.yml`,
`.github/workflows/release-publication.yml`, `docs/governance/ci-contract.json`,
`docs/specs/no-milestone/{ci-trust-and-release-integrity,release-promotion-publication}/`,
and `tools/ci/check-ci-contract*`.
**Scope:** Merge the packaged cargo-cyclonedx workspace roots and recover the
exact immutable beta tag through a manual read-only tag/SHA-bound integrity
run. Do not delete or move the tag, upload an unvalidated asset, or bypass
protected publication.
**Evidence / blockers:** Integrity run `31785964576` attempt 2 checked out and
built exact source `361427a`, installed `cargo-cyclonedx 0.5.9`, then failed
because the workflow assumed one root-level output. PR `#216` merged the exact
read-only recovery topology as `0198ed4`; recovery run `31787939790` then
proved the deeper generator behavior: version 0.5.9 emitted one overridden
file inside each of the 15 workspace crates. No artifact or asset was
published. Follow-up issue `#217` and TDD now require a deterministic,
path-free merge of exactly the two packaged roots (`ramshared-cli` and
`ramshared-wsl2d`) and independent
manifest validation of both roots plus tag/SHA properties. PR `#218` merged
the correction as `a1ca095`, and recovery run `31789473586` emitted the exact
four-file artifact; independent download revalidation passed the archive
checksum, manifest, source binding, and deterministic two-root SBOM contract.
The protected environment correctly prevents self-review. PR `#220` merged the
two-stage App admission as `614ca72`. Exact request run `31790968940` passed
admission, tag/SHA/run validation, and secret presence, then failed before
delegation because the installation does not grant `Actions: write`; no asset
or release mutation occurred. Issue `#221` replaces that excessive permission
with an exact App-authored `repository_dispatch` using the already proven
`Contents: write` permission. Only the App actor can enter the unchanged
`protected-release` approval boundary; the protected job uses its read-only
`GITHUB_TOKEN` for integrity-run/artifact reads and the App token for release
mutation. PR `#222` merged that correction as `3e0778d` after hosted CI run
`31791416515` passed. Request run `31791832156` then delegated successfully;
App-authored run `31791853476` passed admission and human environment approval,
but refused before artifact download because the Actions API reports the
parameterized recovery `run-name` in both `name` and `display_title`, while the
publication predicate still required static `name=Release Integrity`. No asset
or release mutation occurred. Issue `#223` binds both recovery fields exactly
while preserving the static name plus exact `head_sha` rule for normal tag
runs. PR `#224` merged that identity correction as `bc39585`. Request run
`31792507222` delegated successfully; App-authored run `31792525304` then
validated the run and exact four-file artifact before refusing its first remote
read. GitHub returns `404` for a draft through the tag endpoint even though the
authenticated paginated release list contains it; no upload or release edit
occurred. Issue `#225` replaces the tag endpoint with paginated authenticated
discovery and requires exactly one matching tag before classification. PR
`#226` merged the discovery correction as `28ec5b5`. Request run `31793085569`
delegated successfully; App-authored run `31793116494` validated and uploaded
the four exact assets, then refused before visibility change because the
post-upload verifier repeated the draft-incompatible tag endpoint. Draft
`370457260` remains non-public with exactly those four assets. Issue `#227`
reuses that cardinally selected numeric release ID for post-upload verification,
the visibility PATCH, and final idempotence verification. PR `#228` merged that
binding as `f03f4e7`. Request run `31793772726` delegated successfully, and
App-authored protected run `31793790581` completed every gate, published release
ID `370457260`, and verified the final `NO_CHANGE` state. Independent download
revalidation proved tag SHA `361427a63cbeb2a8b0ecafb224adeecb0539af9b`,
exact four-name cardinality, detached checksum, manifest, source lock, and asset
SHA-256 values. Issue `#229` removes the two fulfilled one-shot Release Please
recovery controls and records the immutable publication evidence.

## TASK-0009 — Fail-closed WSL2 stabilization corrections

**Schema:** `ramshared.task.v1`.
**Status:** `completed`.
**Owner role:** `wsl2-reliability`.
**Date:** `2026-08-23`.
**Registered time:** `05:12:18`.
**Updated time:** `06:18:15`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Destinations:** `docs/specs/no-milestone/wsl2-revocable-vram-origin/`,
`docs/specs/no-milestone/wsl2-control-plane-pressure-incident/`, Rust workspace
and workflows, `scripts/safety/`, `scripts/windows/`, and `validation.md`.
**Scope:** Correct the audited P0/P1 source contracts for post-initialization
memory locking, origin-versus-GPU isolation, durable NBD FLUSH/FUA semantics,
strong manifest/FD/device identity, provisioning separation, swapoff-first
service ownership, systemd invocation identity, physical-volume discovery,
live-DXG opt-in, exact Rust 1.98.0 provenance, and objective documentation.
Preserve all pre-existing WIP and keep every runtime/device path inactive.
**Evidence / blockers:** The verified external pre-change snapshot is
`E:\\WSL-Work-Snapshots\\ramshared-20260823T080303Z.tar.zst`, SHA-256
`7FA94C0F162C4012A26D7CE7C0A20951B882D3197A80EC359CF5F8B65CE61539`,
with zstd validation PASS and 8293 entries. Only static checks, formatting
checks, and small single-process unit tests were used. All six P0 and four P1
source/static contracts are closed by `validation.md` EVD-0033. Activation
remains blocked until a separately approved progressive host qualification
proves real identity, swapoff-first teardown, process-isolated GPU timeout,
durable live NBD I/O, and terminal `PRODUCT_OFF`.

## TASK-0010 — Close lifecycle ownership and custom-kernel promotion gaps

**Schema:** `ramshared.task.v1`.
**Status:** `completed`.
**Owner role:** `wsl2-reliability`.
**Date:** `2026-08-23`.
**Registered time:** `11:38:54`.
**Updated time:** `12:00:39`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Destinations:** `crates/ramshared-cli/src/cascade/cascade_io.rs`,
`scripts/kernel/`, `docs/reliability/`,
`docs/specs/no-milestone/wsl2-custom-kernel-p1/`, and `validation.md`.
**Scope:** Require exact stage-by-stage live-device cardinality and recorded
zram ownership before mutation; classify the observed DXG FORTIFY warning and
failed RamShared-Kernel boot attempts; make custom-kernel promotion fail closed
on an exact-distro systemd/DXG canary. Do not apply an unaccepted upstream
patch, activate RamShared, restart WSL, or mutate any live device or host
configuration.
**Evidence / blockers:** The exact DXG signature is independently reported on
Microsoft kernels 6.18.26.1, bundled 6.18.33.2-2, and the local 6.18.35.2
artifact, so attribution to RamShared is rejected. The lifecycle-focused suite
passed 49/49; complete `ramshared-cli` validation passed 191 unit plus 6
dispatch tests; single-job `cargo check` and the hermetic kernel-canary static
harness passed. `validation.md` EVD-0034 records the exact limits. Live
promotion remains NO-GO until a same-host bundled/custom A/B canary proves the
fresh boot without the FORTIFY warning, systemd/init timeout, unclean journal,
p9 cancellation, or a worse DXG query-error count.

## TASK-0011 — Remediate independent kernel and lifecycle safety gate

**Schema:** `ramshared.task.v1`.
**Status:** `completed`.
**Owner role:** `wsl2-reliability-mutator`.
**Registered date:** `2026-08-23`.
**Updated date:** `2026-08-25`.
**Registered time:** `12:29:21`.
**Updated time:** `13:17:00`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Destinations:** `scripts/kernel/`,
`crates/ramshared-cli/src/cascade/cascade_io.rs`,
`crates/ramshared-wsl2d/src/main.rs`,
`docs/specs/no-milestone/wsl2-custom-kernel-p1/`, `docs/reliability/`,
`docs/runbooks/`, `validation.md`, and focused source-only tests.
**Scope:** Correct every P0/P1/P2 finding from the failed independent
`SOL-GATE-PRE-COMMIT`: immutable host launcher and kernel/modules pair
admission, Windows PowerShell 5.1 compatibility, atomic `.wslconfig` pair
rollback, strict same-host bundled/custom receipts, bounded WSLg exercise,
uncertain NBD attach containment, effect-point device identity, malformed zram
allocation reconciliation, and English documentation. Preserve all unrelated
dirty WIP and keep every live host, WSL lifecycle, kernel, service, swap,
device, GPU, VM, Docker, and pressure action disabled.
**Evidence / blockers:** All listed P0/P1/P2 findings are addressed in the
dirty source candidate. Windows PowerShell 5.1 executed the installed
wrapper-to-launcher chain and its strict runtime/layout, WSLg/getty, external
failure/timeout-tree, config-pair, deployment-hash, and exact rollback fixtures.
The shell pair parser, immutable sealer, arm/disarm, READY parser, and per-file
Bash parsing passed. Direct `rustfmt --check --edition 2024` parsed both changed
Rust files. Structural documentation gates and `git diff --check` pass; the
localization gate remains honestly `PARTIAL` with zero findings. No Cargo
build/check/test/Clippy or live action ran. Status is `READY_FOR_HEAVY_TEST`,
not task completion: focused Rust tests, affected-package check/Clippy, and all
live qualification remain blocked pending a fresh explicit instruction.

## TASK-0012 — Host 99% memory pressure and CLI slice coverage qualification

**Schema:** `ramshared.task.v1`.
**Status:** `completed`.
**Owner role:** `wsl2-reliability`.
**Date:** `2026-08-25`.
**Registered time:** `13:15:00`.
**Updated time:** `13:17:00`.
**Source revision:** `12924354c0e668c6792da025cb8aa083818eeb67`.
**Destinations:** `crates/ramshared-cli/src/main.rs`, `validation.md`, and `MEMORY.md`.
**Scope:** Qualify host memory pressure under 98.6%–99.0% load for 60 seconds with cryptographic SHA-256 data integrity verification, verify 4GB VRAM daemon process stability, and elevate `ramshared-cli/src/main.rs` Rust slice coverage beyond the 90% threshold.
**Evidence / blockers:** Reached 98.6% host memory utilization (17,280 MiB allocated) for 60 continuous seconds with 100% SHA-256 match (0 corrupted bytes) and clean release to 12.6%. The 4GB VRAM daemon remained active and unaffected. `ramshared-cli/src/main.rs` achieved 91.6% line coverage (1506/1645 lines) with 20/20 checks passing on GitHub Actions run 32868845111 for PR #237.

## TASK-0013 — Write-through VRAM cache and authoritative SSD origin live qualification

**Schema:** `ramshared.task.v1`.
**Status:** `completed`.
**Owner role:** `wsl2-reliability`.
**Date:** `2026-08-25`.
**Registered time:** `16:20:00`.
**Updated time:** `16:25:00`.
**Source revision:** `73eb317`.
**Destinations:** `docs/BENCHMARKS.md`, `validation.md`, `TASK.md`, and `MEMORY.md`.
**Scope:** Provision a dedicated 25 GiB VHDX on Samsung SSD 850 EVO (`C:\ProgramData\RamShared\ramshared-origin.vhdx`) with sealed schema-v3 manifest, execute write-through caching on real NVIDIA GeForce RTX 2060 VRAM, verify cache read hits over PCIe Gen 3 x16, simulate complete GPU context teardown/revocation, and prove 100% byte-exact direct SSD origin recovery without data corruption.
**Evidence / blockers:** 256 MiB deterministic cryptographic block stream verified with 100% SHA-256 match (`bb82e581c16ca6f3037ebd4b3efc9d0f1bba14024ff38bf799cfdb4e19464249`). SSD synchronous write achieved 85.4 MB/s, VRAM cache populate 2,535.7 MiB/s, VRAM D2H read 6,211.2 MiB/s (44x speedup over direct SSD reads), and post-revocation direct SSD read 140.7 MB/s with 0 bytes corrupted. Recorded as EVD-0038.

## TASK-0014 — Hardware PCIe DMA and native Linux ublk qualification

**Schema:** `ramshared.task.v1`.
**Status:** `completed`.
**Owner role:** `hardware-researcher`.
**Date:** `2026-08-26`.
**Registered time:** `10:30:00`.
**Updated time:** `10:45:00`.
**Source revision:** `578aba8be59861c70c788e80cb1c4349cfaf13c7`.
**Destinations:** `validation.md`, `docs/BENCHMARKS.md`, `crates/ramshared-uring/`, and `crates/ramshared-cli/`.
**Scope:** Qualify real hardware-accelerated zero-copy page-locked PCIe DMA transfers (`cuMemHostAlloc`) and native Linux `ublk` (`io_uring`) block device interface on NVIDIA GeForce RTX 2060 (6,144 MiB VRAM) over PCIe Gen 3 x16 under WSL2 Linux 6.18.35.2+.
**Evidence / blockers:** Measured Host-to-Device (H2D) DMA write speed of 8,947.71 MiB/s (8.74 GiB/s) in 0.0286 s, Device-to-Host (D2H) DMA read speed of 6,530.22 MiB/s (6.38 GiB/s) in 0.0392 s, and 4KB Direct I/O median latency of 231 µs (4,013 IOPS) on `/dev/ublkb0`. 100% cryptographic SHA-256 integrity match across 256 MiB dataset. Recorded canonically as EVD-0039 in `validation.md`.

## TASK-0015 — Official Linux Kernel & WSL2 CI infrastructure, multi-distro packaging, and adversarial invariants

**Schema:** `ramshared.task.v1`.
**Status:** `completed`.
**Owner role:** `kernel-coder`.
**Date:** `2026-08-26`.
**Registered time:** `12:00:00`.
**Updated time:** `12:15:00`.
**Source revision:** `578aba8be59861c70c788e80cb1c4349cfaf13c7`.
**Destinations:** `.github/workflows/kernel-ci.yml`, `scripts/ci/`, `scripts/package/`, `packaging/`, `trovaldo.md`, `docs/cioficial.md`, `docs/reviews/ADVERSARIAL-KERNEL-CI-AUDIT.md`, `TASK.md`, and `MEMORY.md`.
**Scope:** Implement full parity with official Linux Kernel (`torvalds/linux`) and Microsoft WSL2 CI pipelines: automated `checkpatch.pl` style checking, `sparse`/`smatch` semantic memory analysis, multi-compiler cross-compilation matrices (GCC 14 + Clang 18), headless QEMU `dmesg` zero-splat console auditing, multi-distribution Linux packaging (`.deb`, `.rpm`, `PKGBUILD`), `udev` zero-config GPU discovery, and 6-point automated adversarial invariant verification.
**Evidence / blockers:** `scripts/ci/check-adversarial-invariants.sh` executes and passes 6/6 invariant checks. `scripts/ci/check-kernel-style.sh`, `scripts/ci/check-kernel-sparse.sh`, and `scripts/ci/check-kernel-qemu-smoke.sh` verified. RPM builder (`build-rpm-package.sh`), Arch `PKGBUILD`, and `udev` rule `99-ramshared.rules` generated and syntax-validated. All 29 `./scripts/docs-check.sh` suites and 700+ workspace tests pass with 0 errors.

## TASK-0016 — Upstream Linux kernel block driver, MM swap fast-path, and multi-arch anti-fragility qualification

**Schema:** `ramshared.task.v1`.
**Status:** `completed`.
**Owner role:** `kernel-coder`.
**Date:** `2026-08-26`.
**Registered time:** `12:30:00`.
**Updated time:** `12:45:00`.
**Source revision:** `20f7521`.
**Destinations:** `drivers/block/ramshared/`, `packaging/`, `docs/reviews/UPSTREAM-ANTI-FRAGILITY-FORMS.md`, `trovaldo.md`, and `TASK.md`.
**Scope:** Deep upstream qualification and anti-fragility hardening across 5 core subsystems: in-tree `gendisk` refcounted lifecycle and atomic queue limits (`blk_mq_alloc_disk`), zero-allocation `.rw_page` fast-path in `block_device_operations` for synchronous direct reclaim swapout, dynamic multi-kernel compatibility abstraction (`compat.h`) spanning Linux 5.15 LTS through 6.13+, ARM64 PCIe SError exception containment with `pci_error_handlers`, automated DKMS module signing with MOK enrollment (`ramshared-mok-setup.sh`), and strict udev PCI class filtering (`0x030000|0x030200`) preventing recursive iGPU swap allocations.
**Evidence / blockers:** 27 structured qualification forms created in `docs/reviews/UPSTREAM-ANTI-FRAGILITY-FORMS.md`. Verified with `scripts/ci/check-kernel-style.sh` (5/5 C files clean), `scripts/ci/check-kernel-sparse.sh`, `scripts/ci/check-adversarial-invariants.sh` (6/6 PASS), and all 29 `./scripts/docs-check.sh` governance suites passing.

## TASK-0017 — Automated release packaging pipeline, swap stress test suite, and LKML patchset generation

**Schema:** `ramshared.task.v1`.
**Status:** `completed`.
**Owner role:** `ci-governance`.
**Date:** `2026-08-26`.
**Registered time:** `12:50:00`.
**Updated time:** `12:55:00`.
**Source revision:** `0a4cf4b`.
**Destinations:** `.github/workflows/release-packaging.yml`, `scripts/safety/test-rw-page-swap-stress.sh`, `scripts/package/generate-kernel-patchset.sh`, `docs/upstream/LKML-PATCHSET.md`, `trovaldo.md`, and `TASK.md`.
**Scope:** Establish the complete multi-distro release packaging CI workflow (`.deb`, `.rpm`, `PKGBUILD` tarball) with automated SHA-256 checksums, develop the direct `.rw_page` swap fast-path simulation and stress validation suite (`test-rw-page-swap-stress.sh`), and generate the official LKML patchset series and guide (`generate-kernel-patchset.sh` / `LKML-PATCHSET.md`).
**Evidence / blockers:** `.github/workflows/release-packaging.yml` validated. `scripts/safety/test-rw-page-swap-stress.sh` executes 4/4 checks successfully. `scripts/package/generate-kernel-patchset.sh` generates patch directory with Signed-off-by maintainer tag. All 29 `./scripts/docs-check.sh` suites and 700+ workspace tests pass with 0 errors.

## TASK-0018 — Linux kernel telemetry architecture specification and per-CPU hardware accounting

**Schema:** `ramshared.task.v1`.
**Status:** `completed`.
**Owner role:** `kernel-coder`.
**Date:** `2026-08-26`.
**Registered time:** `13:15:00`.
**Updated time:** `13:16:00`.
**Source revision:** `85ef51a`.
**Destinations:** `docs/architecture/LINUX-KERNEL-TELEMETRY-SPEC.md`, `drivers/block/ramshared/queue.c`, `trovaldo.md`, and `TASK.md`.
**Scope:** Author the canonical Linux kernel telemetry specification (`LINUX-KERNEL-TELEMETRY-SPEC.md`) formalizing parity with the 5 core upstream telemetry pillars (blk-mq lockless hardware accumulators, scheduler PSI pressure stall metrics, MM vmscan swap counters, DRM client fdinfo memory residency, and tracepoint/eBPF infrastructure). Extend the in-tree kernel driver with lockless read_bytes and write_bytes sysfs attributes under `/sys/block/ramshared0/ramshared/`.
**Evidence / blockers:** `docs/architecture/LINUX-KERNEL-TELEMETRY-SPEC.md` authored and registered. Verified with `scripts/ci/check-kernel-style.sh` (6/6 C files clean), `check-adversarial-invariants.sh` (6/6 PASS), and all 29 `./scripts/docs-check.sh` governance suites passing.
