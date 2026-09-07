# FINDING_ONLY: PCIe bus transfer verification with readback checksumming

## Observation
The task requires computing a checksum before and after PCIe DMA transfer to detect bus transmission errors in `crates/ramshared-vram/src/lib.rs`.

## Architectural Scope Trap
`crates/ramshared-vram/src/lib.rs` is strictly an abstraction layer for the VRAM control plane (lifecycle, allocation, and wiping) and defines the `VramMemory` and `VramProvider` traits. It does not implement the data plane (block I/O) or manage concrete PCIe DMA transfers. The actual data plane operations and memory copying are handled by concrete backends (e.g., `ramshared-cuda` via `cuMemcpyHtoD_v2` and `cuMemcpyDtoH_v2`) and the block I/O layer (`ramshared_block::BlockBackend`).

Adding data plane checksumming logic in this trait definition file is an architectural mismatch and impossible to implement cleanly within the abstraction boundary. Thus, we fail closed and produce this FINDING_ONLY report to prevent violating the separation of concerns.

## Recommendation
Do not implement data-plane checksumming within the control-plane trait definitions.
