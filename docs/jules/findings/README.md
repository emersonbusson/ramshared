# Finding: Corrupted sparse page isolation and reallocation

## Summary

The request to "Quarantine corrupted sparse VRAM page and allocate fresh backing block transparently" in `crates/ramshared-block/src/sparse_vram.rs` is an architectural scope trap.

### Rationale

The NBD block device operates as a swap backend where the Linux kernel strictly relies on data durability. If a VRAM chunk becomes corrupted (e.g., bit-rot detected via hashing), allocating a *fresh* backing block transparently behind the kernel's back means the kernel will read zeroed or uninitialized memory for previously swapped-out pages. This fundamentally breaks the memory contract and results in **silent data corruption** of host processes.

According to the High-Availability architectural pillars:
- **Fail-Safe Defaults:** "If hardware or peer fails, fail closed with typed semantic errors without corrupting data or memory."

Instead of transparently reallocating and returning zeroes, the correct behavior for block storage when data is unrecoverably corrupted is to fail closed. The block device must return a hard I/O error (`IoError`) up the stack, allowing the kernel to take appropriate action (such as killing the specific process that owned the corrupted page), rather than silently handing it corrupted data that could lead to widespread system instability.
