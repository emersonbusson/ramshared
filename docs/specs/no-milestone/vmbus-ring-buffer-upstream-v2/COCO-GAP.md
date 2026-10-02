# COCO-GAP — formal block on Confidential Computing evidence

**Status:** BLOCKING — the v2 series stays **PARTIAL and unsendable**
**Recorded:** 2026-09-30
**Authority:** [`PRD.md`](PRD.md) acceptance criteria
**Related:** [`IMPL.md`](IMPL.md) · [`AUDIT-source.md`](AUDIT-source.md) ·
[`GAP-REGISTER.md`](../../../reliability/GAP-REGISTER.md)

---

## The rule

The PRD acceptance criteria state, verbatim:

> isolated live normal and failure paths pass; **CoCo evidence exists or the
> mainline patch remains a draft rather than a sendable v2.**

This file records that the CoCo half of that disjunction is **not satisfied**,
and that the consequence is therefore the second branch: the mainline patch
**remains a draft and must not be sent**.

This is a deliberate choice to keep the claim honest rather than weaken it. No
document, cover letter, commit message, or review reply may assert CoCo
correctness, CoCo safety, or universal confidential-guest support for this
series until the evidence below exists.

---

## Why this blocks

Michael Kelley's 2026-09-22 review rejected the original `vzalloc()` ring
fallback specifically on Confidential Computing grounds: `set_memory_decrypted()`
operates on physically contiguous direct-mapped memory and is not valid for a
`vmalloc()` virtual range. On arm64 CCA and on Intel TDX without a paravisor,
a virtual-address decryption attempt is a guest-fatal operation.

The v2 redesign answers that objection at the **design** level. It decrypts on
direct-map chunk addresses *before* joining them with `vmap()`, and it retains
pages whose page-state transition cannot be proven. That is the correct
approach and it is what the maintainer asked for.

It is still **unproven on hardware**. A design argument that matches the
maintainer's reasoning is necessary but not sufficient: the failing mode he
named is a memory-encryption transition, and only a memory-encryption
transition can confirm it.

---

## Required evidence (none of it exists)

| ID | Platform | What must be shown |
| --- | --- | --- |
| COCO-1 | AMD SEV-SNP | Ring and UIO buffers allocated through the chunked path decrypt on direct-map chunk addresses and remain valid across `vmap()`; teardown re-encrypts or retains. No `#GP` / `#VC` / guest panic. |
| COCO-2 | Intel TDX, **no paravisor** | Same as COCO-1. The no-paravisor path is the one Kelley named; a paravisor result does **not** close this row. |
| COCO-3 | Arm CCA | Same as COCO-1. CCA is called out in the PRD as its own acceptance line. |
| COCO-4 | All of the above | Failed or unknown page-state transitions retain the buffer (`gpadl.leak`) rather than freeing or re-encrypting unproven pages. Observed, not only read from source. |
| COCO-5 | All of the above | Intentionally injected page-state failure leaves the guest alive and the buffer retained; no oops, no leak-unbounded under a counted owner. |

Each row requires a **guest boot of the exact candidate** on that platform, a
before→action→after record, and a dmesg containing no BUG/Oops/panic. Source
inspection cannot substitute for any of them.

---

## Why the local host cannot supply this

`EVD-0056` (2026-09-25) audited the Windows host inventory:

- Windows 11 Pro build 26200, **AMD Ryzen 5 3600**, ~32 GiB RAM.
- AMD documents SEV-SNP for **EPYC 7003-series-and-newer**. The Ryzen 5 3600
  is Zen 2 (Matisse); SEV-SNP is not present.
- Intel TDX requires an Intel CPU. This is an AMD host.
- Arm CCA requires Arm hardware. This is x86-64.
- **No dedicated Linux Hyper-V guest** existed for another exact-series
  runtime drill.

Therefore **no local operation can close COCO-1..5**. This is a hardware
boundary, not an effort gap. Running more QEMU, more KUnit, more builds, or
more WSL boots does not move these rows.

Generic QEMU without CoCo passthrough cannot emulate these transitions either.

### Why CI cannot fake this

Two things look like they would help in GitHub Actions and would not:

1. **A QEMU boot of the candidate kernel proves nothing about VMBus.** VMBus
   is the Hyper-V guest bus. QEMU (TCG or KVM) presents virtio, not VMBus, so
   a QEMU boot exercises **zero** lines of `drivers/hv/`. A green
   "boot-smoke" job would be evidence theatre and must not be added as if it
   qualified the series. Do not add it.
2. **A QEMU or TCG CoCo machine without passthrough does not perform a page
   state transition.** The guest-fatal operation is a real firmware/GPA
   sequence. Emulation that skips it proves only that the code did not crash
   on a machine that could not have crashed it.

What hosted CI *does* legitimately cover, and does: applying the exact
pinned bytes to a pinned mainline base, `git diff --check`, cumulative
`scripts/checkpatch.pl --strict --no-tree` at every stage, `W=1` plus Sparse
on x86_64 and arm64, the VMBus and UIO KUnit suites, and the machine-checked
CoCo static invariants with their self-test. That is real evidence about the
source. It is not evidence about a memory-encryption transition.

The nearest real VMBus runtime available without new hardware is a
**disposable WSL2 guest on a Windows CI runner** — WSL2 is a genuine Hyper-V
guest. That is no longer hypothetical: `EVD-0131` measured
`VERDICT=WSL2_RUNTIME_AVAILABLE` on `windows-latest` and `windows-2025`
(run 36767912983), where a 3.7 MB Alpine rootfs imports as a scratch WSL
distro and the guest reports `VMBUS_PRESENT` with **30 VMBus devices** on
`6.18.33.2-microsoft-standard-WSL2`, then unregisters cleanly.

A WSL2 guest alone is **WSL-backport evidence**, not mainline evidence, and
must be labelled as such; the six-patch mainline series does not apply to
the WSL 6.18 tree. `EVD-0132` measures the answer to that objection: the
same runners expose the Hyper-V management stack
(`HYPERV_MANAGEMENT_AVAILABLE`, run 36768828972) and reach `Running` for a
Gen2 VM we define, with no reboot. The mainline candidate can therefore be
booted as an ordinary x86_64 Hyper-V guest in Actions and drilled there.
That is ordinary Hyper-V evidence for the exact mainline bytes.

Neither route closes COCO-1..5. A stock WSL2 VM is not a confidential guest,
and neither is a Gen2 Hyper-V VM on a hosted runner.

---

## What this does *not* say

To avoid the opposite error — treating an unclosable gap as if nothing is
qualified — the following are **true and separately evidenced**:

- The series **design** satisfies all five of Kelley's refactor points
  (`vmbus_alloc_buffer()` for all rings, `struct vmbus_buffer` grouping,
  `struct vmbus_gpadl` folded in, `gpadl.leak` on unproven transitions,
  `HV_GPADL_BUFFER_DECRYPTED` removed). See [`IMPL.md`](IMPL.md).
- A **static negative proof** shows the guest-fatal pattern has no code path
  in any allocation this series introduces, and CI machine-checks that on
  every run including a gate self-test. See
  [`COCO-STATIC-PROOF.md`](COCO-STATIC-PROOF.md). This is stronger than a
  design argument and **still does not close any row of the table above**.
- Hosted build/Sparse/checkpatch/KUnit gates pass for the current
  **nine-patch** pinned bytes: runs **36970357736** and **36970357645**
  on `f2713da10608`, against mainline base `93f51579e7df`, all jobs
  green including both CoCo gate self-tests and 20/20 named KUnit
  cases. Runs 36763981097, 36574925363 and 36590352003 qualified
  earlier candidates and are not evidence for this one.
- Ordinary x86_64 Hyper-V runtime on the **current** candidate is
  qualified for GPADL/UIO lifecycle and forced order-7 fragmentation:
  eight drill runs across sixteen guest-runs, 100 rebind cycles back to
  the exact map baseline, hold-in-mmap across teardown, and
  `high_order_7plus` driven to zero on fourteen of sixteen guest-runs
  with `accept4_failures=0 oops=0`. That is still not a CoCo platform,
  and it does not exercise the chunked order-N → order-0 degrade.
- Ordinary x86_64 Hyper-V runtime on an earlier four-commit snapshot
  (`EVD-0054`) is retained as history and is superseded by the runs
  above.
- The code **refuses to claim** universal CoCo support. Removing the
  unsupported claim is already part of the series.

So the series is a well-founded draft. It is not a sendable v2.

---

## Decision

**Do not send.** Until COCO-1..5 are satisfied on real hardware, the series
stays a draft and the send gate in the PRD remains closed.

Three acceptable resolutions, in order of preference:

1. **Obtain a CoCo lab** — AMD SEV-SNP (EPYC 7003+), Intel TDX without a
   paravisor, or Arm CCA — and run COCO-1..5 on the exact candidate. This is
   the only route that satisfies the PRD's first branch.
2. **Rent one, do not buy one.** Buying the silicon is not required and is
   not realistic for this work. Azure Confidential VMs expose exactly the
   target platforms on an hourly rate: SEV-SNP on `DCasv5` / `ECasv5`, Intel
   TDX on `DCesv5` / `ECesv5`. A few hours of a confidential guest booting
   the exact candidate would close COCO-1, COCO-2, COCO-4 and COCO-5. Arm
   CCA (COCO-3) has no equivalent cloud SKU at the time of writing and
   remains the one row that likely needs partner or lab hardware. This path
   needs an Azure subscription and someone to run the guest-side drills
   (`scripts/kernel/vmbus-lifecycle-drill.sh` already refuses to run on the
   daily WSL2 host). The guest-side procedure — provisioning, byte-pinning
   the nine patches, what to capture per row, and the rollback trigger on a
   `#GP`/`#VC`/panic — is written down in
   [`COCO-LAB-RUNBOOK.md`](COCO-LAB-RUNBOOK.md).
3. **Keep it a draft indefinitely** — the PRD explicitly permits this. The
   work is not lost: the design, the source fixes, and the non-CoCo evidence
   all remain valid and reusable the moment a lab appears.

What is **not** permitted is a third route: sending the series while asserting
CoCo correctness from source reading alone, or quietly relaxing the acceptance
criterion to remove the CoCo requirement. Both would replace a true "PARTIAL"
with a false "DONE".

---

## Rollback trigger

If the series is ever sent before COCO-1..5 are satisfied, treat that as a
process defect: retract the send, restore the draft status here and in
[`GAP-REGISTER.md`](../../../reliability/GAP-REGISTER.md), and record the
retraction in [`validation.md`](../../../../validation.md).
