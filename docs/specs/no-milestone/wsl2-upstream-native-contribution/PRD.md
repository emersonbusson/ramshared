---
slug: wsl2-upstream-native-contribution
title: "WSL upstream-native contribution strategy — config adoption and dxgkrnl FORTIFY release path"
milestone: —
issues:
  - "microsoft/WSL#41054"
---

# PRD — WSL upstream-native contribution strategy

> **Status:** SSDV3 Step 1 — documentation and contribution preparation only.
> This PRD does not open, comment on, or submit any external issue, pull
> request, patch, or kernel build.

## 1. Summary

RamShared needs a narrow upstream-native strategy for optional WSL-kernel
capability, without treating downstream configuration adoption as a product
dependency. The strategy has three independent lanes.

| Lane | Outcome | Route | Product consequence |
| --- | --- | --- | --- |
| **A — stock configuration** | Request `CONFIG_BLK_DEV_UBLK=m` and `CONFIG_ZRAM_WRITEBACK=y` for the x86 WSL configuration. | Existing [microsoft/WSL issue #41054](https://github.com/microsoft/WSL/issues/41054) and Microsoft internal adoption. | Non-blocking; custom-kernel and NBD paths remain. |
| **B — dxgkrnl FORTIFY** | Verify release adoption of the minimal flexible-array fix for a scoped FORTIFY diagnostic. | A separate, human-reviewed WSL bug/release-adoption report. | No claim that it fixes a RamShared/WSL freeze. |
| **C — native VRAM-as-NUMA** | Refuse a guest-only NUMA/HMM implementation under WSL GPU-PV. | No contribution in this slice. | Host WDDM/GPU-PV contract work is required first. |

**Decision:** keep #41054 as the narrowly scoped configuration request, keep
the dxgkrnl fix as a separate release-adoption path, and refuse to call
GPU-PV guest VRAM a native NUMA memory tier.

## 2. Technical context

### 2.1 Primary-source facts

| Fact | Source | Classification |
| --- | --- | --- |
| #41054 is an open feature request for the two requested symbols. | [microsoft/WSL issue #41054](https://github.com/microsoft/WSL/issues/41054) | Confirmed in official Microsoft source |
| WSL kernel bugs/features belong in `microsoft/WSL`; reusable kernel code follows the Linux upstream process. | [WSL2-Linux-Kernel README](https://github.com/microsoft/WSL2-Linux-Kernel/blob/linux-msft-wsl-6.18.y/README.md) | Confirmed in official Microsoft source |
| At this PRD’s revalidation point, x86 `config-wsl` disables both requested symbols and enables `CONFIG_FORTIFY_SOURCE=y`. | [x86 config-wsl](https://github.com/microsoft/WSL2-Linux-Kernel/blob/linux-msft-wsl-6.18.y/arch/x86/configs/config-wsl) | Confirmed in official Microsoft source |
| At the same point, arm64 already enables zram writeback but disables ublk. | [arm64 config-wsl](https://github.com/microsoft/WSL2-Linux-Kernel/blob/linux-msft-wsl-6.18.y/arch/arm64/configs/config-wsl-arm64) | Confirmed in official Microsoft source |
| The released `linux-msft-wsl-6.18.35.2` source has a one-element trailing `fence_values` array; rolling source has [`1dda0bd6b031`](https://github.com/microsoft/WSL2-Linux-Kernel/commit/1dda0bd6b031fec40afb3c6c9d59b8b89fd9e8db), a one-line flexible-array FORTIFY fix. | [release tag](https://github.com/microsoft/WSL2-Linux-Kernel/tree/linux-msft-wsl-6.18.35.2), [fix commit](https://github.com/microsoft/WSL2-Linux-Kernel/commit/1dda0bd6b031fec40afb3c6c9d59b8b89fd9e8db) | Confirmed in official Microsoft source |
| dxgkrnl is a Hyper-V paravirtualized GPU driver whose hardware access occurs through host VM bus communication. | [dxgkrnl Kconfig](https://github.com/microsoft/WSL2-Linux-Kernel/blob/linux-msft-wsl-6.18.y/drivers/hv/dxgkrnl/Kconfig) | Confirmed in official Microsoft source |
| HMM/device-private migration requires device-owned memory and driver migration/lifetime work; a config switch does not create that contract. | [Linux HMM documentation](https://docs.kernel.org/6.15/mm/hmm.html) | Confirmed in official Linux source |

Microsoft documents that the WSL custom-kernel setting is host-wide and may
need an explicit WSL shutdown to take effect. Three-boot acceptance is therefore
manual and never a default RamShared or CI action.
([advanced WSL configuration](https://learn.microsoft.com/en-us/windows/wsl/wsl-config),
[WSL shutdown](https://learn.microsoft.com/en-us/windows/wsl/basic-commands))

### 2.2 RamShared context

| Existing decision | Relation here |
| --- | --- |
| [`wsl2-custom-kernel-p1`](../wsl2-custom-kernel-p1/PRD.md) owns safe custom-kernel activation and rollback. | This PRD does not change that surface. |
| [`custom-kernel-ublk-product-transport`](../custom-kernel-ublk-product-transport/PRD.md) keeps ublk product transport deferred. | Stock capability is not product transport readiness. |
| [`wsl2-native-vram-autotier`](../wsl2-native-vram-autotier/RESEARCH.md) records host WDDM/GPU-PV budget authority. | Lane C preserves its guest-PFN/NUMA refusal. |

### 2.3 dxgkrnl diagnostic boundary

A sanitized, read-only diagnostic observed
`memcpy: detected field-spanning write (size 4)` during Xwayland activity in
`dxgvmb_send_wait_sync_object_gpu` on the released `linux-msft-wsl-6.18.35.2`
source state. At that release, `dxgvmbus.c:3095` contains the relevant
variable-copy path and `dxgvmbus.h` around lines 719–727 declares
`u64 fence_values[1]`. The wire layout is:

```text
fences[object_count] followed by handles[object_count]
```

The two variable-length copies must be audited together: the fence copy and
the object-handle copy. Evidence must cover the observed `object_count == 1`
case and a valid multi-object case. The known flexible-array commit is the
preferred minimal candidate only after exact release/source/reproducer
verification.

This diagnostic is not evidence that it caused a RamShared hang, WSL hang,
GPU reset, display failure, or TDR. No causal claim is permitted here.

## 3. Recommended option

### 3.1 Lane A — configuration adoption

Keep #41054 as one WSL feature request. The x86 evidence package is restricted
to exactly:

```text
CONFIG_BLK_DEV_UBLK=m
CONFIG_ZRAM_WRITEBACK=y
```

Microsoft decides whether/when to adopt the configuration internally. A
community pull request to `microsoft/WSL2-Linux-Kernel` is neither the route
nor an acceptance gate. If Microsoft requests patch-ready material, prepare
one logical config commit per symbol while keeping one public feature request.

Arm64 is an independent revalidation. Since zram writeback is currently
enabled there, it must not receive a redundant request. Any branch/tag change
returns the result to `NEEDS_REVALIDATION`.

### 3.2 Lane B — dxgkrnl release adoption

Lane B remains separate from #41054.

1. Verify whether the exact target release contains `1dda0bd6b031`.
2. If it contains the commit, run regression verification only; do not prepare
   a duplicate report or patch.
3. If it lacks the commit, verify the header/source layout and reproduce the
   exact FORTIFY diagnostic on an explicitly approved isolated surface.
4. Only then request adoption of that exact minimal commit or an equivalently
   verified backport. Do not refactor, alter UAPI, or invent a substitute fix.
5. Run `get_maintainer.pl` only after exact source verification. Do not guess a
   maintainer, list, or Linux-upstream destination for WSL-specific code.

### 3.3 Lane C — explicit refusal

Guest-native VRAM-as-NUMA, HMM, `MEMORY_DEVICE_PRIVATE`, fake PFNs, and
guest-side `add_memory()` are refused until all of the following exist:

1. a documented host WDDM/GPU-PV ownership and capacity contract;
2. guest-visible, cooperative-driver-owned device-memory resources;
3. host/guest migration, reclaim, reset, TDR, and offline semantics; and
4. an isolated hardware lab that can test them without relying on a desktop
   WSL instance.

`/dev/dxg`, CUDA allocation, a custom kernel, or a successful config build is
not a substitute for those prerequisites.

## 4. Functional requirements

| ID | Requirement | Verifiable acceptance |
| --- | --- | --- |
| **RF-U1** | Keep Lane A restricted to the two x86 symbols through #41054/internal adoption. | Evidence contains no driver code, product feature request, or community kernel PR plan. |
| **RF-U2** | Revalidate x86 and arm64 source/configuration at each target branch or release. | Public branch/tag/commit and exact config state are recorded. |
| **RF-U3** | Require official build, checkpatch, sparse, `W=1`, module-package, capability, and three-boot gates for Lane A. | Every named Lane A test is PASS, PARTIAL, REFUSED, or NEEDS_REVALIDATION. |
| **RF-U4** | Preserve custom-kernel/NBD fallbacks while adoption is pending/rejected. | No Lane A outcome changes product transport policy. |
| **RF-U5** | Keep Lane B separate and release-specific. | It verifies exact `1dda0bd6b031` ancestry before report/patch action. |
| **RF-U6** | Treat both `dxgvmb_send_wait_sync_object_gpu` variable copies as one wire-layout contract. | One-object and multi-object tests cover both copies and allocation bounds. |
| **RF-U7** | Refuse causal claims about the FORTIFY diagnostic. | Public text uses diagnostic/release wording only. |
| **RF-U8** | Refuse Lane C until the host-contract prerequisites exist. | `NATIVE_NUMA_CONTRACT_REFUSAL` passes. |
| **RF-U9** | Produce public-safe evidence/templates only. | No private paths, usernames, machine names, account IDs, IP addresses, tokens, dumps, or raw host logs. |
| **RF-U10** | Define lifetime maintenance and branch revalidation. | Branch/release/Kconfig/build change or 30-day review forces NEEDS_REVALIDATION. |

## 5. Non-functional requirements

| ID | Requirement |
| --- | --- |
| **NFR-U1** | No external write: no issue, comment, PR, email, branch push, or tag is created. |
| **NFR-U2** | No unsupervised WSL pressure, swap, ublk I/O, or GPU stress. Runtime diagnostics require explicit approval and isolation. |
| **NFR-U3** | Evidence records public target identity, Kconfig base, toolchain identity, command result, and sanitized verdict. |
| **NFR-U4** | For each approved boot, reset/TDR, guest BUG/Oops, and unexpected dxg-error counter deltas must be zero; any nonzero delta, boot failure, or display instability stops the campaign. |
| **NFR-U5** | Three custom-kernel boots are manual; CI never shuts down WSL or changes host configuration. |
| **NFR-U6** | The dxg candidate is one minimal change; refactors, UAPI changes, and speculative fixes are rejected. |
| **NFR-U7** | Sources are official Microsoft or Linux primary sources only. |
| **NFR-U8** | Missing environment evidence is PARTIAL, never DONE. |

## 6. Flows

### Lane A

1. Record public target branch/tag/commit.
2. Revalidate x86/arm64 config facts and the exact two x86 deltas.
3. Run official-tree `olddefconfig`, `W=1`, sparse, and checkpatch gates.
4. Package modules through the official WSL workflow; prove ublk and zram
   writeback capability without creating a swap workload.
5. With explicit approval, complete three independent custom-kernel boots.
6. Prepare a human review packet for #41054/Microsoft adoption; do not post it.

### Lane B

1. Check target release ancestry for `1dda0bd6b031`.
2. Audit header/source layout, `cmd_size`, and both variable-copy bounds.
3. Reproduce the exact warning for one and multiple objects only on an approved
   isolated surface; non-reproduction stops the path.
4. If needed, test the exact one-line candidate through official gates and
   three explicit boots.
5. Prepare a separate human-reviewed release-adoption report; never append it
   to #41054.

### Lane C

1. A proposal claims guest VRAM can become a NUMA/HMM tier.
2. Check all four host-contract requirements.
3. If any is absent, return `REFUSED_NATIVE_NUMA_CONTRACT`, retain the
   RamShared cascade path, and create no patch.

## 7. Data and state model

This documentation workflow has no RamShared runtime interface.

| Field | Values | Meaning |
| --- | --- | --- |
| `lane` | `CONFIG`, `DXG_FORTIFY`, `NATIVE_NUMA` | Independent contribution boundary |
| `state` | `UNVERIFIED`, `NEEDS_REVALIDATION`, `READY_FOR_HUMAN_REVIEW`, `PARTIAL`, `REFUSED`, `ADOPTED` | Evidence status, not product release status |
| `target` | public branch/tag + commit | Exact source reviewed |
| `evidence` | named test IDs + sanitized verdicts | Reproducible public-safe proof |
| `rollback_trigger` | observable trigger | Stop/recovery condition |

`ADOPTED` requires a public released Microsoft source/release containing the
verified change. A rolling-branch commit alone is not proof for an installed
release.

## 8. Interfaces

| Interface | Contract |
| --- | --- |
| `microsoft/WSL#41054` | Lane A only; existing feature request; human-mediated updates only. |
| `Microsoft/config-wsl` | x86 candidate limited to the two requested symbols. |
| `Microsoft/config-wsl-arm64` | Independent revalidation target. |
| `drivers/hv/dxgkrnl` | Lane B only; exact release/source decides whether a fix is needed. |
| Linux submission process | Only after verified `get_maintainer.pl` ownership. |
| RamShared runtime | No change: no CLI, daemon, uAPI, sysfs, or transport switch. |

## 9. Dependencies and risks

| Risk | Mitigation |
| --- | --- |
| No Microsoft adoption timeline. | #41054 is non-blocking; custom kernel/NBD remain independent. |
| Branch/release drift. | Revalidate on every listed trigger and at least every 30 days. |
| arm64 differs from x86. | Never copy x86 evidence to arm64. |
| dxg fix is already in a target release. | Stop patch/report path; run regression verification only. |
| Warning does not reproduce. | Stop; do not infer a fault or manufacture a patch. |
| Test correlates with TDR/reset/BUG/Oops. | Stop immediately and use only the approved recovery path; retain sanitized counters. |
| GPU-PV is mistaken for guest device memory. | Enforce `NATIVE_NUMA_CONTRACT_REFUSAL`. |

## 10. Implementation strategy

1. Create this PRD, the matching SPEC, and public-safe templates.
2. Do not change RamShared code, CI, configuration, claims, validation logs, or
   existing WSL research/spec files.
3. A later human-approved campaign creates a clean target-kernel worktree and
   executes only the named SPEC tests.
4. SSDV3 Step 2.5 is required before any live dxg candidate boot or external
   patch/submission decision.
5. Three-boot validation requires current explicit approval and an isolated
   surface; unavailable infrastructure remains PARTIAL.

## 11. Documents to update

| Document | Action |
| --- | --- |
| This `PRD.md` | Create now |
| `SPEC.md` in this folder | Create now |
| `CONTRIBUTION-CHECKLIST.md` in this folder | Create now |
| `validation.md` | N/A — excluded and no live validation occurs. |
| `docs/INDEX.md` | N/A — explicitly excluded. |
| Claims registry, existing specs, and upstream research note | N/A — preserve existing ownership/worktree changes. |

## 12. Out of scope

- Any external issue/PR/comment/patch/submission.
- A community pull request to `microsoft/WSL2-Linux-Kernel`.
- Changing RamShared NBD/ublk product policy or custom-kernel activation.
- New dxgkrnl design, UAPI, refactor, or speculative FORTIFY change.
- Claims that the FORTIFY warning caused a freeze, reset, or TDR.
- Guest-native VRAM-as-NUMA/HMM implementation.
- Edits to `validation.md`, `docs/INDEX.md`, claims, or other agents’ files.

## 13. Acceptance criteria

| ID | Criterion |
| --- | --- |
| **A1** | Lane A has only the exact two x86 symbols and separate arm64 revalidation. |
| **A2** | Lane B pins `1dda0bd6b031` and release ancestry before any contribution action. |
| **A3** | Lane B covers both variable copies plus one/multi-object conditions. |
| **A4** | Lane C refuses native NUMA without host WDDM/GPU-PV contract evidence. |
| **A5** | SPEC names official build, checkpatch, sparse, `W=1`, modules, three-boot, rollback, and TDR tests. |
| **A6** | Templates are public-safe and separate #41054 from Lane B. |
| **A7** | No external action or live kernel/GPU/swap operation is performed by this documentation work. |
| **A8** | Branch/release revalidation and maintenance are explicit. |

## 14. Validation plan

The later campaign records `PASS`, `PARTIAL`, `REFUSED`, or
`NEEDS_REVALIDATION` for every named test in [`SPEC.md`](SPEC.md).

```text
before: public target identity, sanitized kernel state, no active test workload
action: one bounded, approved config/dxg step on an isolated surface
after: capability/diagnostic result, boot state, reset/TDR count, rollback state
```

Mandatory stops: custom boot failure, test-correlated guest BUG/Oops or dxg
failure, test-correlated host GPU reset/TDR/recovery/display instability, or a
FORTIFY source/diagnostic mismatch.

## 15. Kahneman map

| Discipline | Decision | Executable evidence / abort |
| --- | --- | --- |
| **#2** | Config enablement does not prove product ublk; flexible array does not prove a freeze cure. | `UPSTREAM_CONFIG_PRODUCT_SCOPE_REFUSAL`; `DXG_FORTIFY_CAUSALITY_REFUSAL` |
| **#3** | Exact target source and diagnostic are required. | `UPSTREAM_BRANCH_REVALIDATION`; `DXG_FORTIFY_RELEASE_ANCESTRY` |
| **#9** | Three boots and every safety-counter delta are measured, not described. | `UPSTREAM_SAFETY_COUNTER_BASELINE` |
| **#13** | Both legitimate object-count boundaries and mismatch refusal are mandatory. | `DXG_FORTIFY_REPRO_OBJECT_COUNT_1`; `DXG_FORTIFY_REPRO_OBJECT_COUNT_MULTI`; `DXG_FORTIFY_SOURCE_MISMATCH_REFUSAL` |
| **#15** | Missing symbols/source mismatch/non-reproduction are not blindly retried. | `UPSTREAM_RETRY_POLICY_REFUSAL` |
| **#16** | TDR/reset/boot failure stops independently of RamShared. | `UPSTREAM_TDR_STOP`; `UPSTREAM_CUSTOM_KERNEL_ROLLBACK` |
| **#17** | Evidence replay produces no external side effect. | `UPSTREAM_EVIDENCE_REPLAY` |
| **#18** | WSL config belongs to Microsoft; device memory belongs to host-contract owners. | `NATIVE_NUMA_CONTRACT_REFUSAL` |
