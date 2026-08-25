# Kernel and low-level code rules — RamShared

These constraints apply to every C or Rust change in the RamShared kernel and
low-level runtime surfaces, regardless of the authoring tool.

## Language and Formatting
- **C Kernel Style:** Rigorously use the standard Linux Kernel style (indentation with 8-space TABs, 80-character line limit, open brackets on the same line for `if`/`for`).
- **Rust for Linux:** If using Rust, strictly follow `alloc` and `core` standards, avoiding `std`. Use `rustfmt` with mainline configurations.
- Every `.c` or `.rs` change must be ready to pass the applicable Linux kernel
  style and static checks.

## Semantics and Memory
- Never assume virtual pointers can be passed directly to hardware. Explicitly map and unmap (e.g., `dma_map_single`, `pci_iomap`).
- Every lock (spinlock, mutex) must have a clear and justified scope. Give extreme attention to priority inversion and deadlocks in interrupt contexts.
- Do not leave memory leaks and free resources in the exact reverse order of allocation in error routines (using the `goto out_err;` idiom).

## Device and WSL kernel identity

- Bind privileged block-device effects to an already opened descriptor and
  exact `dev_t`; revalidate the named path immediately before and after every
  external effect. If an external tool requires `/dev/<name>` to derive sysfs,
  keep the descriptor pinned, revalidate cardinality and `dev_t`, fail closed,
  and document that the external-tool/kernel boundary cannot be eliminated in
  userspace.
- A failed or timed-out NBD attach never authorizes backend termination from an
  exit code alone. Require repeated stable kernel-state and exact target-status
  proof of absence, or safely detach the exact lifecycle-owned device first.
  Ambiguity preserves the backend and evidence.
- A successful allocator call with malformed zram output requires before/after
  enumeration. Reset only one exact new inactive device, prove the complete
  device set returned to the pre-call snapshot, and otherwise preserve evidence.
- WSL custom-kernel production admission uses one immutable manifest-bound
  kernel/modules/layout/QEMU pair and a versioned host-installed launcher. Both
  `kernel=` and `kernelModules=` move atomically; mutable `latest` artifacts and
  stale clean-config snapshots are not authorities. A rollback is proved only
  by a fresh boot that returns to the exact bundled-baseline kernel identity;
  removing the pair keys or observing a new boot ID alone is insufficient.

## Documentation and SPEC
- Never write direct implementations based on the PRD. Use the SSDV3 methodology (PRD -> SPEC -> IMPL).
- PRDs and SPECs must have documented Kernel Panic mitigation disciplines using the Kahneman framework.
