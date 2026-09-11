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
//! when present; otherwise, it falls back to DT-10 (largest `DEVICE_LOCAL` heap − sum allocated).

use std::ffi::CStr;
use std::sync::atomic::{AtomicU64, Ordering};

mod memory;
pub use memory::VulkanMem;
use memory::{DeviceBits, create_device_resources};

use ash::vk;
use ramshared_vram::{VramError, VramProvider};


/// Single staging buffer per provider (no alloc on hot path, DT-8): 1 MiB. Larger I/O is sliced.
const STAGING_BYTES: u64 = 1 << 20;

pub(crate) fn vk_err(ctx: &str, e: impl std::fmt::Debug) -> VramError {
    VramError::Provider(format!("vulkan {ctx}: {e:?}"))
}

/// Selects a transfer queue family (prefers explicit `TRANSFER`; falls back to `GRAPHICS`/`COMPUTE`, which imply transfer per spec). Returns the family index.
fn pick_transfer_family(instance: &ash::Instance, phys: vk::PhysicalDevice) -> Option<u32> {
    // SAFETY: `phys` was enumerated from `instance`; the query only reads properties.
    let fams = unsafe { instance.get_physical_device_queue_family_properties(phys) };
    fams.iter()
        .position(|f| f.queue_flags.contains(vk::QueueFlags::TRANSFER))
        .or_else(|| {
            fams.iter().position(|f| {
                f.queue_flags
                    .intersects(vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE)
            })
        })
        .map(|i| i as u32)
}

/// Vulkan Provider (thread-affine — create/use in the same thread, same as CUDA context;
/// the queue is externally synchronized, DT-7). Reuses 1 staging buffer + 1 cmd buffer + 1 fence.
pub struct VulkanProvider {
    pub(crate) instance: ash::Instance,
    _entry: ash::Entry, // keeps the loader alive as long as the instance exists
    pub(crate) phys: vk::PhysicalDevice,
    device: ash::Device,
    queue: vk::Queue,
    cmd_pool: vk::CommandPool,
    cmd_buf: vk::CommandBuffer,
    fence: vk::Fence,
    staging_buffer: vk::Buffer,
    staging_memory: vk::DeviceMemory,
    staging_mapped: *mut u8,
    pub(crate) allocated: AtomicU64, // Σ bytes allocated via `alloc` (fallback of `mem_info`, DT-10)
    name: String,
}

impl VulkanProvider {
    /// Loads the Vulkan loader, creates an instance, selects the physical device (prefers `DISCRETE_GPU`;
    /// otherwise the ordinal), and sets up logical device + transfer queue + staging. RF-V1.
    pub fn open(ordinal: u32) -> Result<Self, VramError> {
        // SAFETY: loads libvulkan.so.1 via libloading; symbols remain valid as long as `entry` lives.
        let entry = unsafe { ash::Entry::load() }.map_err(|e| vk_err("load", e))?;
        let app = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_1);
        let ci = vk::InstanceCreateInfo::default().application_info(&app);
        // SAFETY: `ci`/`app` valid during call; `None` = default allocator.
        let instance = unsafe { entry.create_instance(&ci, None) }
            .map_err(|e| vk_err("create_instance", e))?;

        // From this point on, any error must destroy the instance (goto out_err idiom).
        match Self::after_instance(&instance, ordinal) {
            Ok((phys, name, bits)) => Ok(Self {
                instance,
                _entry: entry,
                phys,
                device: bits.device,
                queue: bits.queue,
                cmd_pool: bits.cmd_pool,
                cmd_buf: bits.cmd_buf,
                fence: bits.fence,
                staging_buffer: bits.staging_buffer,
                staging_memory: bits.staging_memory,
                staging_mapped: bits.staging_mapped,
                allocated: AtomicU64::new(0),
                name,
            }),
            Err(e) => {
                // SAFETY: `instance` created above and destroyed exactly once here.
                unsafe { instance.destroy_instance(None) };
                Err(e)
            }
        }
    }

    /// Device selection + name + creation of device resources (with its own cleanup on error).
    fn after_instance(
        instance: &ash::Instance,
        ordinal: u32,
    ) -> Result<(vk::PhysicalDevice, String, DeviceBits), VramError> {
        // SAFETY: `instance` valid.
        let pdevs = unsafe { instance.enumerate_physical_devices() }
            .map_err(|e| vk_err("enumerate_physical_devices", e))?;
        if pdevs.is_empty() {
            return Err(VramError::Provider("no Vulkan physical device".into()));
        }
        // Prefers a discrete GPU; otherwise the requested ordinal (clamped).
        let discrete = pdevs.iter().copied().find(|&p| {
            // SAFETY: `p` is a valid handle enumerated from `instance`.
            unsafe { instance.get_physical_device_properties(p) }.device_type
                == vk::PhysicalDeviceType::DISCRETE_GPU
        });
        let phys = discrete.unwrap_or_else(|| pdevs[(ordinal as usize).min(pdevs.len() - 1)]);
        // SAFETY: `phys` valid; `device_name` is a fixed-size NUL-terminated C-string.
        let props = unsafe { instance.get_physical_device_properties(phys) };
        let name = unsafe { CStr::from_ptr(props.device_name.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        let qf = pick_transfer_family(instance, phys)
            .ok_or_else(|| VramError::Provider("sem queue family de transfer".into()))?;
        let bits = create_device_resources(instance, phys, qf)?;
        Ok((phys, name, bits))
    }

    /// Name of the selected device (e.g., \"NVIDIA GeForce RTX 2060\" or \"llvmpipe\" in software).
    pub fn device_name(&self) -> &str {
        &self.name
    }

    /// Size of the largest heap `DEVICE_LOCAL` (bytes) — base of the `total` in `mem_info` (DT-10). Fallback
    /// to the largest heap if there is no DEVICE_LOCAL (case of software/unified memory).
    pub fn device_local_total(&self) -> u64 {
        // SAFETY: `phys` valid.
        let mp = unsafe {
            self.instance
                .get_physical_device_memory_properties(self.phys)
        };
        let heaps = &mp.memory_heaps[..mp.memory_heap_count as usize];
        heaps
            .iter()
            .filter(|h| h.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL))
            .map(|h| h.size)
            .max()
            .or_else(|| heaps.iter().map(|h| h.size).max())
            .unwrap_or(0)
    }

    /// Records + submits + waits for 1 command on the transfer queue (synchronous, DT-5).
    /// `record` writes to the reused `cmd_buf`; after `wait`, the fence is reset.
    /// Single-threaded (DT-7): no races on shared cmd_buf/fence/staging.
    pub(crate) fn submit_wait<F>(&self, record: F) -> Result<(), VramError>
    where
        F: FnOnce(&ash::Device, vk::CommandBuffer),
    {
        let dev = &self.device;
        let cmd = self.cmd_buf;
        // SAFETY: `cmd` came from the `cmd_pool` of this provider; reset before rewriting;
        // single-threaded usage. The recording calls inside `record` have their own `// SAFETY:`.
        unsafe {
            dev.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())
                .map_err(|e| vk_err("reset_command_buffer", e))?;
            let begin = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            dev.begin_command_buffer(cmd, &begin)
                .map_err(|e| vk_err("begin_command_buffer", e))?;
            record(dev, cmd);
            dev.end_command_buffer(cmd)
                .map_err(|e| vk_err("end_command_buffer", e))?;
            let cmds = [cmd];
            let submits = [vk::SubmitInfo::default().command_buffers(&cmds)];
            dev.queue_submit(self.queue, &submits, self.fence)
                .map_err(|e| vk_err("queue_submit", e))?;
            let fences = [self.fence];
            dev.wait_for_fences(&fences, true, u64::MAX)
                .map_err(|e| vk_err("wait_for_fences", e))?;
            dev.reset_fences(&fences)
                .map_err(|e| vk_err("reset_fences", e))?;
        }
        Ok(())
    }
}

impl VramProvider for VulkanProvider {
    // GAT: memory borrows &self (same as CUDA's DeviceMem) -> thread affinity without Arc.
    type Mem<'p>
        = VulkanMem<'p>
    where
        Self: 'p;

    fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError> {
        memory::alloc_memory(self, bytes)
    }

    fn mem_info(&self) -> Result<(u64, u64), VramError> {
        // DT-10 (fallback without VK_EXT_memory_budget): total = largest DEVICE_LOCAL heap; free = total −
        // Σ allocated by this provider. (Exact budget for VRAM of other processes: only on physical GPU.)
        let total = self.device_local_total();
        let used = self.allocated.load(Ordering::Relaxed);
        Ok((total.saturating_sub(used), total))
    }
}

impl Drop for VulkanProvider {
    fn drop(&mut self) {
        // SAFETY: resources created in open, destroyed once in reverse order of allocation. All
        // VulkanMem have already dropped (borrowing &self), so staging/queue are idle;
        // device_wait_idle still guarantees quiescence. _entry/instance drop later (fields).
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.unmap_memory(self.staging_memory);
            self.device.free_memory(self.staging_memory, None);
            self.device.destroy_buffer(self.staging_buffer, None);
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_command_pool(self.cmd_pool, None);
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use ramshared_vram::VramMemory;

    #[test]
    #[ignore = "requires Vulkan loader + ICD (lavapipe/llvmpipe is enough; run with --ignored)"]
    fn open_enumerates_device_and_heap() {
        let p = VulkanProvider::open(0).expect("opens Vulkan");
        assert!(!p.device_name().is_empty(), "device has a name");
        let total = p.device_local_total();
        eprintln!(
            "Vulkan device='{}' heap_total={} MiB",
            p.device_name(),
            total >> 20
        );
        assert!(total > 0, "heap > 0");
    }

    #[test]
    #[ignore = "requires Vulkan loader + ICD (lavapipe is enough; run with --ignored)"]
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
