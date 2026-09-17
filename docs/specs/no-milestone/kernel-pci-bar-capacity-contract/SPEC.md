# SPEC — Exact PCI BAR capacity contract

> SSDV3 Step 2. The corresponding implementation is intentionally
> **partial** until it passes target-kernel and target-hardware validation.

## Closed scope

### In now

- The PCI probe path in `drivers/block/ramshared/main.c`.
- The BAR0 mapping boundary in `drivers/block/ramshared/dma.c`.
- A static contract regression test at
  `tools/ci/kernel-probe-contract.test.mjs`.
- Driver documentation of the capacity/BAR invariant.

### Out now

- Queue policy, DMA-mask negotiation, uAPI/sysfs behavior, userspace cascade,
  module installation, and live WSL2 or host testing.

### Assumed-ready dependencies

- `pci_resource_len()` returns BAR0 length before the driver claims it.
- `check_mul_overflow()` and `SIZE_MAX` are available in the target kernel.
- The target kernel tree provides the canonical checkpatch/sparse/kselftest
  gates named below.

## Traceability

| PRD | SPEC item |
| --- | --- |
| RF-1 | ITEM-2, ITEM-3 |
| RF-2 | ITEM-3 |
| RF-3 | ITEM-3 |
| RF-4 | ITEM-2 |
| NFR-1 | ITEM-2, ITEM-3 |
| NFR-2, NFR-3 | ITEM-2, ITEM-3 |
| NFR-4 | ITEM-4 |

## Technical decisions

| ID | Decision | Reason |
| --- | --- | --- |
| DT-1 | Refuse BAR0 shorter than requested capacity with `-ERANGE`; do not clamp. | A mismatched logical/mapped capacity is an invalid device contract. |
| DT-2 | Check the relation in both probe and DMA initialization. | Probe avoids side effects; DMA stays safe if another caller appears. |
| DT-3 | Reject a capacity larger than `SIZE_MAX` before casting to `size_t`. | Prevent architecture-width truncation. |
| DT-4 | Return the original `pci_enable_device_mem()` errno. | The caller retains the real PCI failure class. |
| DT-5 | Use a source-contract test only as a local regression net. | It cannot substitute for target-tree compilation or a hardware drill. |

## Atomicity and rollback

- **Atomicity frontier:** RF-1 completes before `pci_enable_device_mem()`;
  RF-2/3 complete before `devm_ioremap_wc()`. No disk is registered before
  both boundaries pass.
- **Userspace/daemon:** N/A — no userspace state changes.
- **Kernel/module:** probe either registers a fully consistent device or
  returns without enabling the device; post-enable failures follow the
  existing reverse-order cleanup labels.
- **Host/persistent:** N/A — no host state, swap, device configuration, or
  persistent data is changed.
- **Forward-only:** no. A code revert restores the previous source behavior;
  it is not a safe operational remedy for a too-small BAR.

## Kahneman map

| ITEM / stage | # | Question | Min evidence | Abort |
| --- | --- | --- | --- | --- |
| ITEM-2 | #13 — refusal + legitimate | Does probe refuse a short BAR while retaining an accepted exact-fit path? | Static contract test now; target PCI probe with one refusal and one accepted pair later. | Any registration after a short BAR. |
| ITEM-3 | #16 — exhaustion | Can a 32-bit mapping width silently truncate a valid `u64` capacity? | Static `SIZE_MAX` contract now; target build later. | Any cast without preceding range refusal. |
| ITEM-4 | #9 — numeric verification | Is a successful driver capacity exactly its BAR mapping? | Target drill reports equal byte values. | Unequal values or any kernel warning/oops. |

## Security checklist (pre-implementation)

- [x] Privilege: existing kernel PCI probe only; no new capability or node.
- [x] User/host copy: N/A — no user-copy path in this slice.
- [x] Flags/IOCTL codes: N/A — no uAPI change.
- [x] Info-leak: size-only logs; no addresses.
- [x] IRQ/atomic or IRQL: no IRQ path, sleep, allocation-context, or lock change.
- [x] Lifetime: no new mapping lifetime; existing probe/remove order remains.
- [x] Hot-unplug / device-gone: no new hot-unplug state; probe fails before map.
- [x] Host safety: no live WSL2 pressure or shared-host testing is permitted.
- [x] Shared-hardware cushion: N/A — no allocation policy or reservation change.
- [x] Bounded DMA / foreign driver calls: no new call or wait is added.
- [x] Cooperative cascade spillover: N/A — no cascade surface is in scope.
- [x] Replayable ops: probe refusal is side-effect-free before enable (#17).

## Files to CREATE / MODIFY / DELETE

### CREATE

**`tools/ci/kernel-probe-contract.test.mjs`**

- Purpose: lock the source-level invariant for this otherwise environment-bound
  driver path.
- RF / DT: RF-1..4; DT-1..5.
- Tests: `PCI probe rejects a capacity that cannot be backed by BAR0` and
  `PCI probe preserves the PCI enable error for its caller`.
- Cover target: N/A — the test observes C source contracts; kernel execution
  coverage is target-tree work.
- Kahneman: #13, #16.

### MODIFY

**`drivers/block/ramshared/main.c`**

- RF / DT: RF-1, RF-4; DT-1, DT-4.
- Symbol: `ramshared_pci_probe()`.
- Before → after: calculate bytes into a local `u64`, read BAR0 length, and
  refuse a zero/short bar before allocation/device enable; assign the checked
  capacity to `rs_dev`; return the exact PCI-enable errno.
- Tests: `tools/ci/kernel-probe-contract.test.mjs` :: both named tests.
- Cover target: N/A — target-kernel checkpatch/sparse/kselftest required.
- Kahneman: #13.

**`drivers/block/ramshared/dma.c`**

- RF / DT: RF-2, RF-3; DT-2, DT-3.
- Symbol: `ramshared_dma_init()`.
- Before → after: reject a short BAR and a capacity outside `size_t`; replace
  truncating `min_t()` assignment with exact checked capacity.
- Tests: `tools/ci/kernel-probe-contract.test.mjs` ::
  `PCI probe rejects a capacity that cannot be backed by BAR0`.
- Cover target: N/A — target-kernel checkpatch/sparse/kselftest required.
- Kahneman: #13, #16.

**`drivers/block/ramshared/README.md`**

- Purpose: state the capacity/BAR refusal contract for operators and reviewers.
- RF / DT: RF-1..3; DT-1.
- Tests: `./scripts/docs-check.sh`.

### DELETE

None.

## Observability

| Signal | Where | Level / type |
| --- | --- | --- |
| Short/absent BAR refusal | `dev_err()` during probe or DMA initialization | Error with configured and BAR byte counts |
| Width refusal | `dev_err()` during DMA initialization | Error with requested byte count |
| Successful exact map | existing DMA initialization log | Info with mapped MiB |

## Living docs

| Document | Action |
| --- | --- |
| `drivers/block/ramshared/README.md` | Alter |
| `ARCHITECTURE.md` | N/A — no architecture change |
| `docs/decisions/` | N/A — narrow invariant, not an ADR |
| `docs/reliability/DEGRADATION-MATRIX.md` | N/A — no observed degradation claim |
| `validation.md` | Append only after target-hardware close |
| `docs/BENCHMARKS.md` + results | N/A — no performance claim |
| `.claude/rules/*`, `CLAUDE.md`, `AGENTS.md` | N/A — no convention change |

## Implementation order

1. `ITEM-1`: add and execute the RED static contract reproducer.
2. `ITEM-2`: validate capacity/BAR in `ramshared_pci_probe()` and preserve
   the PCI enable errno.
3. `ITEM-3`: add DMA defence-in-depth and exact checked mapping.
4. `ITEM-4`: run local regression/docs gates; leave target-tree and hardware
   validation environment-bound.

## Required tests matrix

| Production path | Test (`file` :: `name`) | Kind | Kahneman | Cover |
| --- | --- | --- | --- | --- |
| `main.c` probe boundary | `tools/ci/kernel-probe-contract.test.mjs` :: `PCI probe rejects a capacity that cannot be backed by BAR0` | static regression | #13 | N/A — target kernel |
| `dma.c` width/mapping boundary | `tools/ci/kernel-probe-contract.test.mjs` :: `PCI probe rejects a capacity that cannot be backed by BAR0` | static regression | #13, #16 | N/A — target kernel |
| `main.c` errno propagation | `tools/ci/kernel-probe-contract.test.mjs` :: `PCI probe preserves the PCI enable error for its caller` | static regression | #13 | N/A — target kernel |
| target module | target-tree checkpatch, sparse, focused kselftest/KUnit | platform | #9 | N/A — environment-bound |
| PCI device | accepted plus refusal before/action/after drill | E2E | #9, #13 | N/A — environment-bound |

## Validation checklist

- [x] Local static test RED exists before production code.
- [x] Local static test is GREEN after production code.
- [x] `git diff --check` passes.
- [ ] `./scripts/docs-check.sh` passes.
- [ ] Target-tree checkpatch/sparse/kselftest passes (environment-bound).
- [ ] Isolated PCI device accepted/refusal drill passes (environment-bound).
- [ ] No live WSL2 pressure action is run for this slice.
