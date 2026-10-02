# ramshared-vulkan

Vulkan-based `VramProvider` implementation that can expose compatible GPU adapters as cache candidates. AMD/Intel physical cache qualification remains open.

## Scope & Responsibility

`ramshared-vulkan` implements the `ramshared-vram` interface using the Vulkan API:
- **Adapter eligibility:** Vulkan devices, including AMD Radeon and Intel Arc, can be candidates when their driver provides the required transfer operations, stable identity, and usable memory-budget data. Per-adapter physical qualification is still required.
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
