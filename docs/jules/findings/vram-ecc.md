# VRAM hardware ECC error detection and reporting

## Description
Requested to "Query GPU driver for hardware ECC single/double-bit error events and register findings" in `crates/ramshared-vram/src/lib.rs`.

## Finding
This is an architectural scope trap. The `ramshared-vram` crate specifically states: "Separates the VRAM **control plane** (lifecycle + allocation + wipe + free-floor) from the concrete backend (currently CUDA; Vulkan in the future). The **data plane** (block I/O) is already abstracted by `ramshared_block::BlockBackend`; this crate handles VRAM-specific operations. Safe Rust only, completely driver-agnostic."
Querying the GPU driver for ECC errors requires backend-specific driver API calls (e.g., CUDA or Vulkan) and is explicitly out of scope for the driver-agnostic control plane abstraction. This should be implemented in the backend crate (like `ramshared-cuda`).
