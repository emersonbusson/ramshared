# AUDIT-2.5 — kernel-pci-bar-capacity-contract

## Findings

| Sev | SPEC § | Issue | Required fix |
| --- | --- | --- | --- |
| HIGH | Closed scope, DT-1 | A `min_t()` mapping can register a logical capacity larger than the BAR-backed mapping. | Refuse the mismatch; never clamp or advertise it. |
| HIGH | Atomicity and rollback | A probe-only guard could be bypassed by a future caller of `ramshared_dma_init()`. | Keep a second boundary check in DMA initialization. |
| HIGH | Required tests matrix | A repository static test could be misrepresented as a kernel or hardware test. | Mark the implementation partial until target-tree and device evidence exists. |
| MED | DT-3 | `u64` capacity can truncate on an architecture with narrower `size_t`. | Refuse before cast with `-EOVERFLOW`. |
| MED | DT-4 | Rewriting a PCI enable failure to `-ENODEV` loses the root errno. | Return `ret` unchanged. |
| LOW | Living docs | The driver guide described arithmetic and bounds checks but not the exact BAR invariant. | Document it in the driver README. |

## Open questions

1. Which pinned target-kernel tree and commit will supply the checkpatch,
   sparse, and kselftest/KUnit validation?
2. Which isolated PCI device can provide one exact-fit success and one
   oversized-capacity refusal without touching shared-host swap?

These are environment-bound qualification questions, not reasons to weaken the
source contract.

## Verdict

### go — source implementation only

The SPEC has one narrow contract, a side-effect-free refusal frontier, explicit
kernel cleanup ownership, named local regression tests, and correct
platform-specific missing evidence. It must remain **PARTIAL** after local
validation; target-kernel and hardware proof are required before any `DONE` or
performance/stability claim.
