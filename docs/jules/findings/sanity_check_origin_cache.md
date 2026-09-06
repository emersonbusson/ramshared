# FINDING_ONLY: Architectural Mismatch Trap

## Rationale
The instruction to "enforce max cache size limit derived from available physical host RAM" in `crates/ramshared-block/src/origin_cache.rs` is an architectural mismatch trap.

## Evidence
1. **Domain Mismatch**: The `WriteThroughCacheBackend` uses a `VramProvider` to cache blocks in VRAM, not host RAM.
```rust
struct CacheChunk<M> {
    mem: Option<M>,
    generation: u64,
    validity_generation: u64,
    valid: Vec<bool>,
    last_access: u64,
}

pub struct WriteThroughCacheBackend<'p, P: VramProvider + 'p, O> {
    provider: &'p P,
    origin: O,
    size: u64,
    block: u32,
    chunk_bytes: u64,
    chunks: Vec<CacheChunk<P::Mem<'p>>>,
```

2. **Dynamic Allocation via Telemetry**: The cache dynamically allocates VRAM based on a `GpuSample` budget provided at runtime, rather than statically enforcing a physical host RAM limit at construction.
```rust
pub fn observe_gpu(&mut self, sample: Option<GpuSample>, now: Duration) -> CachePolicyOutcome {
    let target = physical_target_bytes(self.physical_cap_bytes, sample);
```

3. **Library Isolation**: The `ramshared-block` crate is an isolated domain library without system-level dependencies. System RAM boundary validation is the architectural responsibility of higher-level orchestrators (e.g., `crates/ramshared-tier/src/cascade.rs`), making a `/proc/meminfo` style host RAM check here inherently invalid.
