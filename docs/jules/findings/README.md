# FINDING_ONLY: Sparse vRAM Page Table Compaction

## Scope Trap
The task requested to "Compact sparse page table entries to release unreferenced backing memory blocks under pressure" in `crates/ramshared-block/src/sparse_vram.rs`.

## Evidence
The `SparseVramBackend` uses a direct-mapped page table where the index of a `Chunk` in the `chunks` vector corresponds directly to the logical offset in the NBD block device:
```rust
let n = config.capacity.div_ceil(config.chunk_bytes);
// ...
let Some(chunk) = self.chunks.get(idx) else { ... };
```
Compacting or shifting entries in this vector to "release unreferenced blocks" would break the relationship between logical block addresses (LBAs) and their physical chunk mappings, causing silent data corruption for the guest OS. As a block device, the backend must preserve sparse but strictly position-dependent mappings.

## Conclusion
This is an architectural scope trap. The direct-mapped array cannot be compacted without introducing logical address shifting and breaking block device semantics. No code modifications should be made.
