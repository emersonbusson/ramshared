---
slug: kernel-pci-bar-capacity-contract
title: Exact PCI BAR capacity contract for the RamShared block driver
milestone: —
issues: []
---

# PRD — Exact PCI BAR capacity contract

## 1. Summary

The Linux block driver must refuse a configured capacity that cannot be
entirely backed by PCI BAR0. It must not silently advertise a larger block
device than the write-combining mapping. The probe must also preserve the
errno reported by `pci_enable_device_mem()`.

This is a narrow correction to the in-repository kernel driver. It does not
change the supported userspace NBD transport, request a module load, or make a
hardware qualification claim.

## 2. Technical context

- **Confirmed in codebase:** `drivers/block/ramshared/main.c` calculates
  `capacity_bytes` from the `capacity_mb` module parameter and registers a
  disk at that capacity.
- **Confirmed in codebase:** `drivers/block/ramshared/dma.c` currently maps
  `min_t(size_t, bar_len, capacity_bytes)`, which can be smaller than the
  registered capacity.
- **Confirmed in codebase:** `drivers/block/ramshared/queue.c` checks both
  `dma.size` and `capacity_bytes` for each I/O, but probe must establish their
  equality rather than expose a structurally inconsistent device.
- **Confirmed in codebase:** `ramshared_dma_init()` has one caller, the PCI
  probe function in `main.c`.
- **Inference:** a target-kernel module build and a real PCI BAR are needed to
  prove the runtime paths; neither is available as a repository-contained
  test fixture.

## 3. Recommended option

Calculate the requested byte capacity before allocating device state, reject a
zero or smaller BAR0 before enabling the PCI device, and repeat the relation in
`ramshared_dma_init()` as a defence-in-depth boundary. Map exactly the accepted
capacity. Return the original PCI enable error unchanged.

Discarded alternatives:

- Keep the truncating `min_t()` mapping: it makes capacity and mapping diverge.
- Clamp the advertised block size to BAR0: it silently changes an operator's
  requested capacity and hides an invalid hardware/configuration contract.
- Rely only on I/O-time bounds checks: the device would still be registered in
  an incoherent state.

## 4. Functional requirements

| ID | Requirement | Acceptance |
| --- | --- | --- |
| RF-1 | Reject a requested capacity larger than BAR0 before device enable/registration. | Probe returns `-ERANGE`; no disk is registered. |
| RF-2 | Defend the BAR/capacity relation in DMA initialization. | A future direct caller also receives `-ERANGE` before mapping. |
| RF-3 | Map precisely the accepted capacity. | `dma.size == capacity_bytes` after successful initialization. |
| RF-4 | Preserve the error from `pci_enable_device_mem()`. | Its failure branch returns `ret`, not a replacement errno. |

Abuse case: a configuration supplies an oversized `capacity_mb` against a
small aperture. It must fail closed without mapping or registering a block
device.

## 5. Non-functional requirements

| ID | Requirement |
| --- | --- |
| NFR-1 | All capacity arithmetic is checked and no `u64` to `size_t` truncation is permitted. |
| NFR-2 | No new lock, IRQ, DMA allocation, uAPI, sysfs, or persistent-host behavior is introduced. |
| NFR-3 | Error logs expose sizes and errno class, never virtual/kernel addresses. |
| NFR-4 | The kernel target validation remains explicitly partial until checkpatch, sparse, kselftest, and a target-device drill run. |

## 6. Flows

### Happy path

1. Validate `capacity_mb` and calculate `capacity_bytes` with overflow checks.
2. Read BAR0 length and prove it is at least `capacity_bytes`.
3. Enable the PCI function, request regions, and initialize the DMA mapping.
4. Revalidate the relation in DMA initialization, convert to `size_t` only
   after checking its range, then map exactly `capacity_bytes`.
5. Register the block device.

### Error paths

| Trigger | Result | State |
| --- | --- | --- |
| BAR0 is absent or smaller than requested capacity | `-ERANGE` with a size log | No PCI enable, map, or disk. |
| Requested capacity overflows bytes | `-EOVERFLOW` | No PCI enable, map, or disk. |
| Accepted capacity cannot fit `size_t` | `-EOVERFLOW` | No mapping or disk. |
| PCI enable fails | Original `ret` | No master, region, map, or disk. |

## 7. Data / state model

`struct ramshared_device::capacity_bytes` is the logical device capacity.
`struct ramshared_dma_region::size` is the mapped BAR capacity. A successful
probe establishes the invariant:

```text
capacity_bytes == dma.size <= BAR0.length <= SIZE_MAX
```

No new state machine or persistent state is added.

## 8. Interfaces

The existing read-only `capacity_mb` module parameter remains the interface.
Invalid capacity/BAR combinations return existing kernel errno values
(`-ERANGE` or `-EOVERFLOW`); no ioctl or sysfs ABI is added.

## 9. Dependencies and risks

| Risk | Mitigation |
| --- | --- |
| A target kernel differs from the headers used by maintainers. | Require target-tree checkpatch/sparse/kselftest before promotion. |
| A narrow BAR is discovered only after a driver change. | Reject before device enable and repeat at DMA initialization. |
| 32-bit `size_t` cannot represent capacity. | Explicit `SIZE_MAX` refusal before the cast. |
| Regression blocks a formerly truncated device. | That device was unsafe; rollback is a code revert only, not a live workaround. |

Rollback trigger: any target-kernel checkpatch/sparse/kselftest failure, any
probe regression for a BAR at least as large as its configured capacity, or a
device drill reporting a mapping size different from the logical capacity.

## 10. Implementation strategy

1. Add a repository-contained static contract regression test and demonstrate
   its RED state.
2. Create the SSDV3 PRD, SPEC, and adversarial audit.
3. Make the minimal probe and DMA changes with no new policy path.
4. Run the static test GREEN, repository documentation checks, and available
   workspace checks.
5. Record target-kernel and hardware validation as environment-bound rather
   than manufacturing a completion claim.

## 11. Documents to update

- `drivers/block/ramshared/README.md` — document the exact BAR capacity rule.
- `docs/specs/no-milestone/kernel-pci-bar-capacity-contract/{PRD,SPEC,IMPL,AUDIT-2.5}.md`.
- Generated documentation index and inventory.

## 12. Out of scope

- Loading, unloading, or building a module against an external kernel tree.
- Live PCIe/VRAM, WSL2, swap, stress, or host pressure operations.
- Changing queue depth, DMA-mask fallback, disk ABI, or the NBD product path.

## 13. Acceptance criteria

- [ ] The static contract test is RED before the implementation and GREEN after it.
- [ ] Both probe and DMA initialization reject BAR0 smaller than capacity.
- [ ] Successful DMA initialization maps exactly the logical capacity.
- [ ] PCI enable failure preserves its original errno.
- [ ] The implementation is marked `partial` until target-kernel evidence exists.

## 14. Validation plan

- Unit/static: `node --test tools/ci/kernel-probe-contract.test.mjs`.
- Repository hygiene: `./scripts/docs-check.sh` and `git diff --check`.
- Target kernel (environment-bound): target-tree `checkpatch.pl --strict`,
  sparse, a focused kselftest/KUnit test, and an isolated PCI BAR probe drill.
- Live path (environment-bound): record before/action/after for one accepted
  BAR/capacity pair and one oversized-capacity refusal, without WSL2 pressure.
