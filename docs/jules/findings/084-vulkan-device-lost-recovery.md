# FINDING: Vulkan `VK_ERROR_DEVICE_LOST` Recovery during Suspend/Resume

## Architecture Context & Objective
The task asked to "Catch `VK_ERROR_DEVICE_LOST` during suspend/resume and recreate logical device and command pools" in `crates/ramshared-vulkan/src/lib.rs`.

## Finding
This is an **architectural scope trap**.

`crates/ramshared-vulkan/src/lib.rs` is a pure memory/buffer management layer for block devices using Vulkan endpoints via `ash`, providing an implementation for `VramProvider` and `VramMemory` from `ramshared-vram`.

In the `ramshared-vulkan` implementation, errors returned by `ash` device calls (like `queue_submit`, `wait_for_fences`, `map_memory`, `allocate_memory`) are immediately mapped to the `VramError` enum (`VramError::Provider(...)`) using the `vk_err` helper function and returned up the call stack to the caller. The `VramProvider` trait defines these operations to return `Result<T, VramError>`.

Implementing transparent device recreation inside this low-level memory provider would require:
1. Retaining state on how to rebuild the physical and logical device transparently (which violates the current single-pass `open` design where errors drop instances).
2. Tracking all currently allocated `VulkanMem` blocks and somehow re-allocating or re-mapping them, or failing them selectively.
3. This level of stateful orchestration and retry/recovery logic belongs in the daemon/agent control plane (which handles the system lifecycle, suspends, resumes, and interacts with `VramProvider`), not in the driver-agnostic block memory abstraction itself.

Attempting to intercept `VK_ERROR_DEVICE_LOST` and recreate the `ash::Device`, command pools, and staging buffers *inside* the `submit_wait` or `alloc` methods of `VulkanProvider` would introduce hidden statefulness, breaking the fail-fast trait boundary and potentially causing silent data loss if existing `VulkanMem` allocations are invalidated by the device recreation without the caller's knowledge.

Therefore, according to the system's design principles ("Fail-Safe Defaults: If hardware or peer fails, fail closed with typed semantic errors without corrupting data or memory"), returning the error directly (as currently implemented) is the correct behavior for this layer.

No code modifications were made.
