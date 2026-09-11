//! `ramshared-vulkan` — Vulkan backend of `VramProvider` (RF-G2).
//!
//! Second implementation of the `ramshared_vram::VramProvider` trait (the first one, CUDA, remains intact),
//! unlocking "any GPU" support + a native Linux host where ublk+VRAM and eviction-under-load run e2e.
//!
//! **Complete IMPL (RF-V1..V3):** `open` initializes the loader, instance, physical device, logical device,
//! transfer queue, and staging buffer (`HOST_VISIBLE|HOST_COHERENT`). `impl VramProvider` covers `alloc`
//! (`DEVICE_LOCAL`) and `mem_info`. `impl VramMemory` covers `read_at`/`write_at` (staging +
//! `vkCmdCopyBuffer` + `VkFence`) and `zero` (`vkCmdFillBuffer`). According to
//! `docs/vulkan-backend/SPEC.md` (DT-1..DT-10).
//!
//! Validated via software rendering (lavapipe/llvmpipe) without a GPU — all unsafe blocks (FFI `ash`) are isolated here
//! with `// SAFETY:` for each block; the trait boundary is safe. `mem_info` uses `VK_EXT_memory_budget`
//! when present; otherwise, it falls back to DT-10 (largest `DEVICE_LOCAL` heap - sum allocated).

mod provider;
mod memory;
mod utils;

pub use provider::VulkanProvider;
pub use memory::VulkanMem;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use ramshared_vram::{VramMemory, VramProvider, VramError};
    use crate::utils::STAGING_BYTES;

    #[test]
    #[ignore = "requires Vulkan loader + ICD (lavapipe/llvmpipe is enough; run with --ignored)"]
    fn vulkan_roundtrip_write_then_read() {
        let p = VulkanProvider::open(0).expect("opens Vulkan");
        let (free0, total) = p.mem_info().expect("mem_info");
        assert!(total > 0, "total > 0");

        // 2 MiB region; payload > staging (1 MiB) and offset != 0 -> exercises the chunk loop.
        let size = 2 * 1024 * 1024;
        let mut m = p.alloc(size).expect("alloc 2 MiB");
        assert_eq!(m.len(), size, "reported len = requested bytes");

        let n = (STAGING_BYTES as usize) + 4096; // 1 MiB + 4 KiB -> 2 chunks
        let off = 4096u64;
        let pattern: Vec<u8> = (0..n).map(|i| (i % 251) as u8).collect();
        m.write_at(off, &pattern).expect("write");
        let mut back = vec![0u8; n];
        m.read_at(off, &mut back).expect("read");
        assert_eq!(back, pattern, "round-trip identical bytes");

        // zero zeroes the region.
        m.zero().expect("zero");
        m.read_at(off, &mut back).expect("read post-zero");
        assert!(back.iter().all(|&b| b == 0), "zero left everything as 0");

        // bounds-check.
        let mut one = [0u8; 1];
        assert!(
            matches!(
                m.read_at(size as u64, &mut one),
                Err(VramError::OutOfRange { .. })
            ),
            "read beyond the end -> OutOfRange"
        );

        // free decreased after alloc (fallback DT-10).
        let (free1, _) = p.mem_info().expect("mem_info 2");
        assert!(free1 <= free0, "free did not increase after alloc");
        eprintln!(
            "Vulkan round-trip OK device='{}' total={} MiB free0={} MiB free1={} MiB",
            p.device_name(),
            total >> 20,
            free0 >> 20,
            free1 >> 20
        );
    }
}
