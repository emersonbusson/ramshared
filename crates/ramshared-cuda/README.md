# ramshared-cuda

Dynamic CUDA Driver API runtime loader and page-locked DMA allocator for Tier 2 VRAM.

## Scope & Responsibility

`ramshared-cuda` manages physical GPU memory across Linux (WSL2) and Windows environments:
- **Dynamic Runtime Loading:** Loads `libcuda.so.1` (Linux) or `nvcuda.dll` (Windows) at runtime without static linking against the CUDA Toolkit.
- **VRAM Allocation & Zeroing:** Allocates device memory chunks (`cuMemAlloc`), clears pages (`cuMemsetD8`), and copies blocks with synchronous DMA.
- **Provider Implementation:** Implements `VramProvider` and `VramMemory` traits defined in `ramshared-vram`.
- **Probing Utility:** Provides diagnostic memory integrity probes (`pattern_for_offset`).

## Workspace Dependencies

- [`ramshared-vram`](../ramshared-vram/README.md) — Backend-agnostic VRAM traits and error definitions.

## Safety Invariants

- **Isolated FFI:** All raw pointer manipulations and CUDA driver ioctls are strictly contained within `driver.rs` and `ffi.rs`.
- **RAII Lifecycle:** All device allocations automatically release CUDA memory contexts upon drop.

## Testing

```bash
cargo test -p ramshared-cuda
```
