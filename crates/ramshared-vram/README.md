# ramshared-vram

Hardware-agnostic VRAM allocator and control-plane abstraction.

## Scope & Responsibility

`ramshared-vram` decouples the high-level memory tiering logic from concrete hardware GPU drivers:
- **VRAM Provider Abstraction:** Defines the `VramProvider` trait for querying device memory budget, allocating device buffers, and managing lifecycle.
- **VRAM Memory Interface:** Defines the `VramMemory` trait for reading, writing, and zeroing GPU buffers.
- **Multi-Backend Architecture:** Serves as the shared trait boundary implemented by both `ramshared-cuda` (NVIDIA) and `ramshared-vulkan` (AMD/Intel).

## Workspace Dependencies

- Pure trait definitions; zero workspace crate dependencies.

## Safety Invariants

- **Safe Code Only:** `#![forbid(unsafe_code)]` enforced.
- **Unified Error Model:** Translates disparate driver errors into a consistent [`VramError`](src/lib.rs) enum (`OutOfRange`, `OutOfMemory`, `Busy`, `InvalidAlignment`).

## Testing

```bash
cargo test -p ramshared-vram
```
