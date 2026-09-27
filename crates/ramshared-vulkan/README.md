# ramshared-vulkan

Vulkan-based `VramProvider` implementation for cross-vendor GPU hardware (AMD Radeon, Intel Arc, Software Renderers).

## Scope & Responsibility

`ramshared-vulkan` implements the `ramshared-vram` interface using the Vulkan API:
- **Cross-vendor backend:** Supports Vulkan devices including AMD Radeon, Intel Arc, and software renderers when their driver and memory type support the required transfer operations. Hardware support still requires per-adapter qualification.
- **Staging Buffer Management:** Utilizes pre-allocated 1 MiB host-visible staging buffers with transfer queue synchronization (`vkCmdCopyBuffer`) to eliminate allocations on the hot I/O path.
- **Adapter-bound budget:** Uses `VK_EXT_memory_budget` and a physical-device UUID or valid Windows adapter LUID when available. Vulkan's budget and usage follow the extension's heap-level estimates; they are not presented as a universal count of every application's allocations. If the extension is unavailable, the provider-local estimate cannot authorize automatic allocation. A driver sample without stable adapter identity remains informational and is also rejected by automatic admission.

## Workspace Dependencies

- [`ramshared-vram`](../ramshared-vram/README.md) — Core VRAM traits and error types.

## Safety Invariants

- **Documented FFI Safety:** All raw `ash` Vulkan calls are isolated with explicit `// SAFETY:` proofs.
- **Fail-closed fallback:** Without external memory-budget data, reported heap size is informational only and cannot authorize cache admission.

## Testing

```bash
cargo test -p ramshared-vulkan
```
