# ramshared-vulkan

Vulkan-based `VramProvider` implementation for cross-vendor GPU hardware (AMD Radeon, Intel Arc, Software Renderers).

## Scope & Responsibility

`ramshared-vulkan` implements the `ramshared-vram` interface using the Vulkan API:
- **Universal Hardware Support:** Unlocks hardware-accelerated VRAM swap on AMD Radeon and Intel Arc GPUs, as well as software fallback environments (lavapipe/llvmpipe).
- **Staging Buffer Management:** Utilizes pre-allocated 1 MiB host-visible staging buffers with transfer queue synchronization (`vkCmdCopyBuffer`) to eliminate allocations on the hot I/O path.
- **Vulkan Memory Budget:** Queries actual device allocations using the `VK_EXT_memory_budget` extension.

## Workspace Dependencies

- [`ramshared-vram`](../ramshared-vram/README.md) — Core VRAM traits and error types.

## Safety Invariants

- **Documented FFI Safety:** All raw `ash` Vulkan calls are isolated with explicit `// SAFETY:` proofs.
- **Clean Fallback:** Falls back to largest device-local heap when budget extension is unavailable.

## Testing

```bash
cargo test -p ramshared-vulkan
```
