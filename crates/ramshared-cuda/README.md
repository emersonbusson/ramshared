# ramshared-cuda

Dynamic CUDA Driver API runtime loader and page-locked DMA allocator for Tier 2 VRAM.

## Scope & Responsibility

`ramshared-cuda` manages physical GPU memory across Linux (WSL2) and Windows environments:
- **Dynamic Runtime Loading:** Loads `libcuda.so.1` (Linux) or `nvcuda.dll` (Windows) at runtime without static linking against the CUDA Toolkit.
- **Device-Wide Occupancy (NVML):** Reads VRAM used/free/total through `nvmlDeviceGetMemoryInfo` from `libnvidia-ml.so.1` (Linux) or `nvml.dll` (Windows), also loaded at runtime from the same NVIDIA driver package.
- **VRAM Allocation & Zeroing:** Allocates device memory chunks (`cuMemAlloc`), clears pages (`cuMemsetD8`), and copies blocks with synchronous DMA.
- **Provider Implementation:** Implements `VramProvider` and `VramMemory` traits defined in `ramshared-vram`.
- **Probing Utility:** Provides diagnostic memory integrity probes (`pattern_for_offset`).

## Workspace Dependencies

- [`ramshared-vram`](../ramshared-vram/README.md) — Backend-agnostic VRAM traits and error definitions.

## Runtime Dependencies

Both are resolved at runtime from the installed NVIDIA driver; there is no
build-time CUDA Toolkit or NVML SDK dependency.

| Library | Linux candidates | Windows | Used for |
| --- | --- | --- | --- |
| CUDA Driver | `libcuda.so.1`, `/usr/lib/wsl/lib/libcuda.so.1`, `libcuda.so`, `/usr/lib/wsl/lib/libcuda.so`, `/usr/lib/x86_64-linux-gnu/libcuda.so.1` | `nvcuda.dll` | allocation, copy, context |
| NVML | `libnvidia-ml.so.1`, `/usr/lib/wsl/lib/libnvidia-ml.so.1`, `libnvidia-ml.so`, `/usr/lib/x86_64-linux-gnu/libnvidia-ml.so.1` | `nvml.dll` | device-wide budget occupancy |

### Why the budget must come from NVML

`cuMemGetInfo` is **not** a device-wide reading on WSL2 GPU-PV. The paravirtual
shim accounts the calling process's channel, so its free/total figures do not
move when another process allocates device memory. A VRAM budget built on that
number cannot yield to a GPU application, which is the opposite of the intended
containment behaviour.

`nvmlDeviceGetMemoryInfo` reports the whole adapter and does observe every
process. [`Context::budget_snapshot`](src/vram_impl.rs) therefore sources
occupancy from NVML. `Context::mem_info` still exposes the raw `cuMemGetInfo`
figures and is documented as allocator-local; it is not used for the budget.

`Cuda::load` **fails closed** when NVML is missing. A silent fallback to
`cuMemGetInfo` would reintroduce the defect, so a host without NVML gets no
VRAM budget and therefore no VRAM cache, which is the safe direction.

## Safety Invariants

- **Isolated FFI:** All raw pointer manipulations and CUDA driver ioctls are strictly contained within `driver.rs` and `ffi.rs`.
- **RAII Lifecycle:** All device allocations automatically release CUDA memory contexts upon drop.

## Testing

```bash
cargo test -p ramshared-cuda
```
