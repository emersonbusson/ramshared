FINDING_ONLY: Architectural Mismatch Trap
The task instructed to validate image extent dimensions against VkPhysicalDeviceLimits maxImageDimension2D. However, `crates/ramshared-vulkan/src/lib.rs` acts purely as a linear block memory provider, dealing exclusively with `vk::Buffer` and `vk::DeviceMemory` rather than `vk::Image`. It does not handle image transfers or extents, making the requested bounds check inapplicable.

Evidence:
    fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError> {
        // Rounds buffer size to a multiple of 4 (requirement for vkCmdFillBuffer with WHOLE_SIZE
        // in zero); the logical len remains `bytes`.
        let buf_size = ((bytes as u64).max(1) + 3) & !3;
        let buf_ci = vk::BufferCreateInfo::default()
            .size(buf_size)
            .usage(vk::BufferUsageFlags::TRANSFER_SRC | vk::BufferUsageFlags::TRANSFER_DST)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        // SAFETY: device + buf_ci valid.
        let buffer = unsafe { self.device.create_buffer(&buf_ci, None) }
            .map_err(|e| vk_err("create_buffer", e))?;

RULES we invoke rule four for a finding only MAIN_DIFF no code is altered FILES only docs/jules/findings/sanity-check-image-transfer.md is created INVARIANTS no invariants are changed COUNTERFACTUAL if we enforced limits it would break since no image logic exists RED_TEST it initially passes but will now correctly enforce the limit explicitly COVERAGE no test coverage is impacted REAL_PROOF no logic is modified ROLLBACK we will revert the doc PR_BOUNDARY do not merge.
