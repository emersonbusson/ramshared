# SPEC — wsl2-upstream-native-contribution

> **SSDV3 Step 2.** Implements [`PRD.md`](PRD.md) as a documentation and
> contribution-preparation slice only. It does not build, boot, patch, submit,
> comment on, or otherwise mutate an external kernel/WSL surface.

## Closed scope

### In now

- Lane A’s source-pinned configuration-adoption test contract.
- Lane B’s separate dxgkrnl FORTIFY release-adoption test contract.
- Lane C’s native-NUMA host-contract refusal.
- Public-safe templates and revalidation/lifetime rules.

### Out now

- External issues, comments, pull requests, email, patches, pushes, and tags.
- Kernel builds, module loading, WSL shutdowns, custom-kernel boots, GPU
  diagnostics, graphics workloads, swap, ublk, or pressure tests.
- Changes to RamShared code/CI/configuration, `validation.md`, `docs/INDEX.md`,
  claims, existing specs, or the existing WSL upstream research note.

### Assumed-ready dependencies

- A later campaign has an exact public target-tree revision and the official
  build and custom-kernel procedure for that revision.
- Any live WSL or dxg drill has current explicit authorization, an isolated
  surface, and a known-good recovery path.
- This documentation slice assumes none of those dependencies is complete or
  exercised now.

## Traceability

| PRD | SPEC item |
| --- | --- |
| RF-U1 | ITEM-1 — Lane A configuration adoption |
| RF-U2 | ITEM-1, ITEM-5 — source and branch revalidation |
| RF-U3 | ITEM-1 — official gates and three-boot contract |
| RF-U4 | ITEM-1 — non-blocking fallback boundary |
| RF-U5 | ITEM-2 — Lane B dxgkrnl FORTIFY adoption |
| RF-U6 | ITEM-2 — both-copy wire-layout audit |
| RF-U7 | ITEM-2 — causality refusal |
| RF-U8 | ITEM-3 — Lane C native-NUMA refusal |
| RF-U9, NFR-U1, NFR-U7 | ITEM-4 — public-safe packet/templates |
| RF-U10, NFR-U3, NFR-U5, NFR-U8 | ITEM-5 — target-tree CI and lifetime revalidation |
| NFR-U2, NFR-U4, NFR-U6 | ITEM-6 — host safety, rollback, and TDR stops |

## Technical decisions

| ID | Decision | Why |
| --- | --- | --- |
| **DT-U1** | Lane A is x86-only for the two requested symbols. | It matches #41054 without a configuration sweep. |
| **DT-U2** | arm64 is separately revalidated. | Its current zram-writeback setting differs from x86. |
| **DT-U3** | Lane B begins with ancestry for `1dda0bd6b031`. | The official rolling-tree commit is already the minimal flexible-array fix. |
| **DT-U4** | The fence and object-handle copies are one wire-layout audit unit. | The reported one-object warning is at the second copy; multi-object coverage is needed for the first. |
| **DT-U5** | No maintainer/list is predeclared. | `get_maintainer.pl` must run against exact source, not a remembered route. |
| **DT-U6** | Lane C is a refusal, not a future guest-side workaround. | GPU-PV does not establish guest-owned device memory or reset/migration semantics. |
| **DT-U7** | Target-kernel CI is non-mutating and separate from RamShared CI. | WSL boot/GPU checks are host-wide and environment-bound. |
| **DT-U8** | If a release contains the known dxg commit, regression evidence replaces report/patch preparation. | Avoid duplicate contribution. |

## Atomicity and rollback

| Layer | This slice | Future campaign boundary |
| --- | --- | --- |
| RamShared repository | Adds only this PRD/SPEC/checklist pack. | No runtime or CI change. |
| Target kernel worktree | No worktree is created or modified. | One isolated worktree/branch per public target revision. |
| WSL host configuration | No configuration changes. | Manual boot drill uses the existing approved custom-kernel recovery path only. |
| GPU/host state | No GPU operation. | A correlated reset/TDR/guest fault independently stops testing. |
| External collaboration | No network write. | Human approval precedes every report, patch, or submission. |

- **Lane A rollback:** an olddefconfig, build, package, capability, or boot
  failure blocks adoption evidence; do not loop/retry custom boots.
- **Lane B rollback:** any mismatch in release ancestry, header/source layout,
  diagnostic, allocation size, or wire layout drops the candidate and sets
  `NEEDS_REVALIDATION`.
- **TDR rollback:** a test-correlated reset/TDR/recovery, display instability,
  guest BUG/Oops, or unexpected dxg error stops immediately. Retain sanitized
  counters only and restore known-good kernel selection through the approved
  recovery path.

## Kahneman map

| Stage | # | Question | Minimum evidence | Abort |
| --- | --- | --- | --- | --- |
| Lane A scope | #2 | Does capability prove product transport? | `UPSTREAM_CONFIG_PRODUCT_SCOPE_REFUSAL` | Any attempt to switch RamShared policy |
| Source identity | #3 | Is branch/tag/commit exact and current? | `UPSTREAM_BRANCH_REVALIDATION` | Missing or stale identity |
| Lane B bounds | #13 | Do both object-count boundaries preserve the layout? | `DXG_FORTIFY_REPRO_OBJECT_COUNT_1`; `DXG_FORTIFY_REPRO_OBJECT_COUNT_MULTI` | Mismatch or non-reproduction |
| Measured safety | #9 | Are all three boot outcomes and counter deltas numeric? | `UPSTREAM_SAFETY_COUNTER_BASELINE` | Any nonzero reset/TDR, BUG/Oops, or dxg-error delta |
| Retry | #15 | Is failure proven transient? | `UPSTREAM_RETRY_POLICY_REFUSAL` | Blind retry of deterministic failure |
| Boot/GPU curator | #16 | Can unsafe execution stop independently? | `UPSTREAM_TDR_STOP`; `UPSTREAM_CUSTOM_KERNEL_ROLLBACK` | TDR/reset/BUG/Oops/boot failure |
| Evidence replay | #17 | Does rerun avoid external effects? | `UPSTREAM_EVIDENCE_REPLAY` | External write or host mutation |
| Native memory | #18 | Is the host contract handled by its owner? | `NATIVE_NUMA_CONTRACT_REFUSAL` | Guest-only NUMA/HMM proposal |

## Security checklist

- [x] Host safety: no unsupervised WSL pressure, swap, ublk I/O, or GPU stress.
- [x] Information safety: public evidence excludes private paths, usernames,
  machine/account IDs, IP addresses, tokens, dumps, and raw host logs.
- [x] Replayability: evidence reruns have no external side effect (#17).
- [x] Privilege boundary: host/kernel actions need current explicit approval.
- [ ] User-copy bounds: N/A for RamShared code; Lane B nonetheless checks the
  external kernel’s message size and both variable copies before a candidate.
- [ ] Flags/IOCTL: N/A — no RamShared or kernel UAPI is changed.
- [ ] IRQ/atomic/lifetime/hot-unplug: N/A in this documentation slice; Lane C
  remains refused until the host owner provides these semantics.

## Files to create / modify / delete

### Create

**`docs/specs/no-milestone/wsl2-upstream-native-contribution/PRD.md`**

- Purpose: close the contribution-route decision.
- RF / DT: RF-U1..RF-U10; DT-U1..DT-U8.
- Required tests: matrix below.
- Cover target: N/A — documentation-only.

**`docs/specs/no-milestone/wsl2-upstream-native-contribution/SPEC.md`**

- Purpose: freeze lanes, named tests, TDR/rollback, and maintenance rules.
- RF / DT: all requirements and decisions in this file.
- Required tests: matrix below.
- Cover target: N/A — documentation-only.

**`docs/specs/no-milestone/wsl2-upstream-native-contribution/CONTRIBUTION-CHECKLIST.md`**

- Purpose: prepare public-safe human review material without a network action.
- RF / DT: RF-U5, RF-U9, RF-U10; DT-U3, DT-U5, DT-U8.
- Required tests: `UPSTREAM_PUBLIC_EVIDENCE_REDACTION` and
  `UPSTREAM_TEMPLATE_SCOPE_SEPARATION`.
- Cover target: N/A — documentation-only.

### Modify / delete

None. Preserve existing specifications, `validation.md`, `docs/INDEX.md`, the
claims registry, and other agents’ work.

## Observability

| Signal | Public-safe record | PASS condition |
| --- | --- | --- |
| Target identity | branch/tag + full public commit | Recorded before every test |
| Config delta | exact symbol/value table | x86 delta is unambiguous |
| FORTIFY state | diagnostic class, function, source revision, object-count case | Exact match or `NOT_REPRODUCED` |
| Build gates | command ID, exit, warning/error count | Required gates pass or refuse |
| Boot state | ordinal, capability state, reset/TDR count | Three independent PASS rows |
| Rollback | trigger and sanitized result | Known-good selection restored or campaign stopped loud |

## Living documents

| Document | Action |
| --- | --- |
| `ARCHITECTURE.md` | N/A — no architecture change |
| `docs/decisions/ADR-…` | N/A — existing product boundary remains |
| `docs/reliability/DEGRADATION-MATRIX.md` | N/A — no live failure is claimed/changed |
| `validation.md` | N/A — excluded and no live execution occurs |
| `docs/BENCHMARKS.md` / benchmark data | N/A — no performance claim |
| `.claude/rules/*`, `CLAUDE.md`, `AGENTS.md` | N/A — no convention change |
| `docs/INDEX.md` | N/A — explicitly excluded |

## Implementation order

1. **ITEM-1:** specify the exact Lane A x86 delta, arm64 revalidation, official
   build gates, modules package, capabilities, and three boots.
2. **ITEM-2:** specify Lane B release ancestry, both copy boundaries, source
   layout, exact minimal candidate, regression, and three boots.
3. **ITEM-3:** make the host-contract requirement an executable refusal.
4. **ITEM-4:** provide public-safe and scope-separated templates.
5. **ITEM-5:** specify read-only CI and branch/release maintenance triggers.
6. **ITEM-6:** require SSDV3 Step 2.5 before any live boot, dxg reproducer, or
   external contribution decision.

## Required tests matrix

These are contractual names for a later target-kernel campaign. They have not
run in this documentation-only slice.

| Path | Test | Kind | # | Pass condition |
| --- | --- | --- | --- | --- |
| Common source identity | `UPSTREAM_BRANCH_REVALIDATION` | source/static | #3 | Public branch/tag/full commit and both architecture config states are recorded and current. |
| Lane A x86 | `UPSTREAM_CONFIG_X86_DELTA_EXACT` | config/static | #3 | Only `UBLK=m` and `ZRAM_WRITEBACK=y` differ from target x86 base. |
| Lane A arm64 | `UPSTREAM_CONFIG_ARM64_DELTA_REVALIDATION` | config/static | #3, #13 | Current state is recorded; no redundant zram-writeback request. |
| Lane A Kconfig | `UPSTREAM_CONFIG_OLDDEFCONFIG` | build | #13 | Both x86 deltas survive `olddefconfig`. |
| Lane A warning build | `UPSTREAM_CONFIG_BUILD_W1` | build | #3 | Official-tree `W=1` build succeeds. |
| Lane A sparse | `UPSTREAM_CONFIG_SPARSE_C1` | sparse | #13 | `C=1` reports no new relevant diagnostic. |
| Lane A quality | `UPSTREAM_CONFIG_CHECKPATCH` | checkpatch | #13 | Minimal config commits are checkpatch-clean or refused. |
| Lane A modules | `UPSTREAM_CONFIG_MODULES_PACKAGE` | package | #16 | Official package contains expected ublk module for built release. |
| Lane A ublk | `UPSTREAM_CONFIG_UBLK_MODULE_LOAD` | custom-kernel drill | #13 | `ublk_drv` loads and its control node exists; no swap workload. |
| Lane A zram | `UPSTREAM_CONFIG_ZRAM_WRITEBACK_CAPABILITY` | custom-kernel drill | #13 | Writeback attribute exists on disposable zram; no backing device/swap. |
| Lane A boots | `UPSTREAM_CONFIG_BOOT_1`, `UPSTREAM_CONFIG_BOOT_2`, `UPSTREAM_CONFIG_BOOT_3` | manual custom-kernel boot | #16, #17 | Expected state and no correlated reset/TDR/guest failure in all three. |
| Lane A boundary | `UPSTREAM_CONFIG_PRODUCT_SCOPE_REFUSAL` | refusal | #2 | Capability does not mark ublk product transport ready. |
| Lane A retry | `UPSTREAM_RETRY_POLICY_REFUSAL` | refusal | #15 | Deterministic mismatch/missing symbol/non-sticky delta is not retried. |
| Lane B ancestry | `DXG_FORTIFY_RELEASE_ANCESTRY` | source/static | #3 | Target includes/excludes `1dda0bd6b031`; this selects regression vs triage. |
| Lane B layout | `DXG_FORTIFY_SOURCE_LAYOUT` | source/static | #13 | Header member, `cmd_size`, and both copy regions match reviewed source. |
| Lane B one object | `DXG_FORTIFY_REPRO_OBJECT_COUNT_1` | isolated kernel drill | #13 | Exact warning is reproduced or explicitly `NOT_REPRODUCED`. |
| Lane B multi object | `DXG_FORTIFY_REPRO_OBJECT_COUNT_MULTI` | isolated kernel drill | #13 | Valid multi-object boundary covers the first variable-length copy. |
| Lane B wire contract | `DXG_FORTIFY_WIRE_LAYOUT` | source/static | #13 | Fences precede handles; allocation covers both arrays without overflow. |
| Lane B quality | `DXG_FORTIFY_CHECKPATCH` | checkpatch | #13 | Exact minimal candidate is checkpatch-clean. |
| Lane B warning build | `DXG_FORTIFY_BUILD_W1` | build | #3 | Target-tree `W=1` has no scoped FORTIFY regression. |
| Lane B sparse | `DXG_FORTIFY_SPARSE_C1` | sparse | #13 | Target-tree `C=1` has no new relevant sparse diagnostic. |
| Lane B regression | `DXG_FORTIFY_REGRESSION_OBJECT_COUNT_1`, `DXG_FORTIFY_REGRESSION_OBJECT_COUNT_MULTI` | isolated kernel drill | #13 | Same repro no longer emits warning and preserves function outcome. |
| Lane B boots | `DXG_FORTIFY_BOOT_1`, `DXG_FORTIFY_BOOT_2`, `DXG_FORTIFY_BOOT_3` | manual custom-kernel boot | #16, #17 | Three independent clean boots with no correlated reset/TDR/BUG/Oops. |
| Lane B boundary | `DXG_FORTIFY_CAUSALITY_REFUSAL` | refusal | #2 | Warning absence is never reported as a freeze/TDR cure. |
| Lane B mismatch | `DXG_FORTIFY_SOURCE_MISMATCH_REFUSAL` | refusal | #13 | Source/layout/diagnostic/ancestry mismatch stops report/patch path. |
| Safety | `UPSTREAM_SAFETY_COUNTER_BASELINE` | observation/drill | #9 | Each of three boot rows records `0` increment for reset/TDR, BUG/Oops, and unexpected dxg errors. |
| Safety | `UPSTREAM_TDR_STOP` | safety drill | #16 | Correlated reset/TDR/BUG/Oops stops campaign and records recovery state. |
| Safety | `UPSTREAM_CUSTOM_KERNEL_ROLLBACK` | safety drill | #16 | Boot failure returns through approved recovery path. |
| Process | `UPSTREAM_EVIDENCE_REPLAY` | process | #17 | Evidence rerun performs no external write or host mutation. |
| Public evidence | `UPSTREAM_PUBLIC_EVIDENCE_REDACTION` | static/manual review | #13 | No prohibited private material in packets/templates. |
| Scope separation | `UPSTREAM_TEMPLATE_SCOPE_SEPARATION` | static/manual review | #18 | #41054, Lane B, and Lane C stay separate. |
| Lane C | `NATIVE_NUMA_CONTRACT_REFUSAL` | refusal | #18 | Missing host ownership/migration/reset contract rejects guest NUMA work. |

## Validation checklist

### Official target-kernel gates, only after explicit human authorization

```bash
# Run in the exact public WSL kernel worktree; record TARGET_REV in evidence.
make KCONFIG_CONFIG=Microsoft/config-wsl olddefconfig
make -j"${JOBS}" KCONFIG_CONFIG=Microsoft/config-wsl W=1
make -j"${JOBS}" KCONFIG_CONFIG=Microsoft/config-wsl C=1

# Run on the generated, minimal candidate commit range only.
./scripts/checkpatch.pl --strict --show-types --git "${BASE_REV}..HEAD"

# Discover owners only after target source is known.
./scripts/get_maintainer.pl --norolestats --nogit \
  drivers/hv/dxgkrnl/dxgvmbus.c drivers/hv/dxgkrnl/dxgvmbus.h
```

Lane A applies only the two x86 deltas before `olddefconfig`; arm64 uses
`Microsoft/config-wsl-arm64` and records its independent result. Module
packaging follows the official WSL kernel README workflow. Capability tests do
not create swap, a backing device, or a stress workload.

Lane B starts with:

```bash
git merge-base --is-ancestor 1dda0bd6b031fec40afb3c6c9d59b8b89fd9e8db \
  "${TARGET_REV}"
```

An ancestor result means the source already has the known fix: no duplicate
patch/report is prepared. A non-ancestor result only permits source-layout and
isolated reproducer tests; it does not prove a runtime bug.

### Three-boot acceptance

The three `BOOT_*` checks are manual, never CI:

1. capture a public-safe baseline and confirm the recovery path;
2. explicitly boot the approved custom kernel once;
3. run only the named capability/diagnostic check and inspect correlated
   reset/TDR, BUG/Oops, and unexpected-dxg-error counters; and
4. return to quiescent state before the next independent boot.

For every boot, each recorded safety-counter delta must be `0`. Any nonzero
reset/TDR, BUG/Oops, or unexpected-dxg-error delta, display instability, boot
failure, or rollback failure stops the campaign at once.

### CI, lifetime, and branch revalidation

This SPEC creates no RamShared CI change. A future target-kernel CI job may run
only source/config/build gates and must never boot WSL, change host config,
drive GPU workloads, or post externally.

Set the lane to `NEEDS_REVALIDATION` and rerun all target-sensitive tests when:

- target branch head, tag, or release changes;
- Kconfig dependencies, module packaging, or `CONFIG_FORTIFY_SOURCE` changes;
- dxgkrnl source layout or known-fix ancestry changes;
- a build/checkpatch/sparse result changes; or
- 30 days elapse without source revalidation.

After public release adoption, retain source and three-boot regression checks
for each later target release while the capability/fix remains in RamShared’s
support statement.

## Out of SPEC

- Linux-mm/HMM/NUMA/GPU-PV design or implementation.
- Any claim that the known commit is absent from, or fixes, every WSL release.
- Any claim that the FORTIFY warning caused a RamShared/WSL hang, reset, or TDR.
- Direct Microsoft internal access, paths, teams, or timelines.
