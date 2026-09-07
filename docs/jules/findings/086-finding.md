# FINDING ONLY: VRAM backing store re-mapping on PCIe bus resume

## Analysis

The task requests implementing VRAM backing store re-mapping on PCIe bus resume in `crates/ramshared-block/src/vram_backend.rs`. However, `VramBackend` only connects the VRAM memory region (provided via `ramshared_vram::VramMemory`) to the `BlockBackend` trait (`read_at`, `write_at`, `zero`).

The `ramshared-vram` crate and its concrete implementations (like `ramshared-cuda`) define the `VramProvider` and `VramMemory` traits, representing an initialized, thread-affine memory allocation. The block backend `VramBackend<M>` holds a reference `M: VramMemory` and is entirely driver-agnostic and unaware of the physical PCIe bus. Hardware-level operations such as "re-mapping PCIe VRAM aperture after system resume" or "re-validating GPU memory handles and PCIe BAR mappings on resume/thaw" cannot be safely or correctly implemented in the `VramBackend` block abstraction layer. Those belong to the concrete driver backend (e.g. CUDA context management in `ramshared-cuda` or a theoretical `ramshared-vulkan`).

Modifying the block layer to handle driver-specific power lifecycle events (ACPI S3/S4) violates the architectural separation of the control plane (VRAM provider) from the data plane (block I/O).

Therefore, this is an architectural scope trap.
