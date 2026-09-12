use ash::vk;
use ramshared_vram::{VramError, VramMemory};

use crate::{STAGING_BYTES, VulkanProvider, pick_memory_type, vk_err};
use std::sync::atomic::Ordering;

/// Logical device resources created in `open` (loaded into `VulkanProvider` on success).
pub(crate) struct DeviceBits {
    pub(crate) device: ash::Device,
    pub(crate) queue: vk::Queue,
    pub(crate) cmd_pool: vk::CommandPool,
    pub(crate) cmd_buf: vk::CommandBuffer,
    pub(crate) fence: vk::Fence,
    pub(crate) staging_buffer: vk::Buffer,
    pub(crate) staging_memory: vk::DeviceMemory,
    pub(crate) staging_mapped: *mut u8,
}

/// RAII guard for the `goto out_err` (kernel idiom) in device creation: on error (any `?`),
/// destroys the already created resources in reverse order **and** the device. On success, `disarm()` prevents
/// cleanup and the handles are passed to the `VulkanProvider`.
pub(crate) struct ResGuard {
    pub(crate) device: ash::Device,
    pub(crate) cmd_pool: Option<vk::CommandPool>,
    pub(crate) fence: Option<vk::Fence>,
    pub(crate) staging_buffer: Option<vk::Buffer>,
    pub(crate) staging_memory: Option<vk::DeviceMemory>,
    pub(crate) mapped: bool,
    pub(crate) armed: bool,
}

impl ResGuard {
    pub(crate) fn new(device: ash::Device) -> Self {
        Self {
            device,
            cmd_pool: None,
            fence: None,
            staging_buffer: None,
            staging_memory: None,
            mapped: false,
            armed: true,
        }
    }
}

impl Drop for ResGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        // SAFETY: all Some handles were created from self.device in this flow and are destroyed
        // exactly once (in reverse order of allocation). device_wait_idle guarantees nothing is
        // in-flight before freeing.
        unsafe {
            let _ = self.device.device_wait_idle();
            if let Some(m) = self.staging_memory {
                if self.mapped {
                    self.device.unmap_memory(m);
                }
                self.device.free_memory(m, None);
            }
            if let Some(b) = self.staging_buffer {
                self.device.destroy_buffer(b, None);
            }
            if let Some(f) = self.fence {
                self.device.destroy_fence(f, None);
            }
            if let Some(p) = self.cmd_pool {
                self.device.destroy_command_pool(p, None);
            }
            self.device.destroy_device(None);
        }
    }
}

/// Vulkan Provider (thread-affine — create/use in the same thread, same as CUDA context;
/// the queue is externally synchronized, DT-7). Reuses 1 staging buffer + 1 cmd buffer + 1 fence.
/// Creates logical device + queue + cmd pool/buffer + fence + mapped staging buffer, with RAII cleanup on error.
pub(crate) fn create_device_resources(
    instance: &ash::Instance,
    phys: vk::PhysicalDevice,
    qf: u32,
) -> Result<DeviceBits, VramError> {
    let prio = [1.0f32];
    let qci = [vk::DeviceQueueCreateInfo::default()
        .queue_family_index(qf)
        .queue_priorities(&prio)];
    let dci = vk::DeviceCreateInfo::default().queue_create_infos(&qci);
    // SAFETY: `dci`/`qci`/`prio` valid during call; `phys` enumerated from `instance`. Before
    // device creation, there are no resources to clean up (returns directly on failure).
    let device = unsafe { instance.create_device(phys, &dci, None) }
        .map_err(|e| vk_err("create_device", e))?;

    // From here on, every `?` is covered by `guard` (destroys children + device on error).
    let mut guard = ResGuard::new(device);

    // SAFETY: `guard.device`/`qf` valid.
    let queue = unsafe { guard.device.get_device_queue(qf, 0) };

    let pool_ci = vk::CommandPoolCreateInfo::default()
        .queue_family_index(qf)
        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
    // SAFETY: device + pool_ci valid.
    let cmd_pool = unsafe { guard.device.create_command_pool(&pool_ci, None) }
        .map_err(|e| vk_err("create_command_pool", e))?;
    guard.cmd_pool = Some(cmd_pool);

    let cb_ai = vk::CommandBufferAllocateInfo::default()
        .command_pool(cmd_pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(1);
    // SAFETY: device + cb_ai valid; the cmd buffer(s) are freed together with the pool.
    let cbs = unsafe { guard.device.allocate_command_buffers(&cb_ai) }
        .map_err(|e| vk_err("allocate_command_buffers", e))?;
    let cmd_buf = cbs
        .first()
        .copied()
        .ok_or_else(|| VramError::Provider("allocate_command_buffers returned empty".into()))?;

    // SAFETY: device valid.
    let fence = unsafe {
        guard
            .device
            .create_fence(&vk::FenceCreateInfo::default(), None)
    }
    .map_err(|e| vk_err("create_fence", e))?;
    guard.fence = Some(fence);

    let buf_ci = vk::BufferCreateInfo::default()
        .size(STAGING_BYTES)
        .usage(vk::BufferUsageFlags::TRANSFER_SRC | vk::BufferUsageFlags::TRANSFER_DST)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    // SAFETY: device + buf_ci valid.
    let staging_buffer = unsafe { guard.device.create_buffer(&buf_ci, None) }
        .map_err(|e| vk_err("create_buffer(staging)", e))?;
    guard.staging_buffer = Some(staging_buffer);

    // SAFETY: buffer valid.
    let req = unsafe { guard.device.get_buffer_memory_requirements(staging_buffer) };
    // SAFETY: phys valid.
    let mprops = unsafe { instance.get_physical_device_memory_properties(phys) };
    let mt = pick_memory_type(
        &mprops,
        req.memory_type_bits,
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
    )
    .ok_or_else(|| {
        VramError::Provider("sem memory type HOST_VISIBLE|COHERENT p/ staging".into())
    })?;
    let mai = vk::MemoryAllocateInfo::default()
        .allocation_size(req.size)
        .memory_type_index(mt);
    // SAFETY: device + mai valid.
    let staging_memory = unsafe { guard.device.allocate_memory(&mai, None) }
        .map_err(|e| vk_err("allocate_memory(staging)", e))?;
    guard.staging_memory = Some(staging_memory);

    // SAFETY: buffer + memory valid; offset 0 satisfies the alignment of `req`.
    unsafe {
        guard
            .device
            .bind_buffer_memory(staging_buffer, staging_memory, 0)
    }
    .map_err(|e| vk_err("bind_buffer_memory(staging)", e))?;

    // SAFETY: newly allocated HOST_VISIBLE memory; maps the entire range.
    let raw = unsafe {
        guard.device.map_memory(
            staging_memory,
            0,
            STAGING_BYTES,
            vk::MemoryMapFlags::empty(),
        )
    }
    .map_err(|e| vk_err("map_memory(staging)", e))?;
    guard.mapped = true;
    let staging_mapped = raw.cast::<u8>();

    // Success: disarms the guard and extracts the handles (the device is cloned — lightweight handle from ash;
    // the actual destroy is done in Drop of VulkanProvider).
    guard.armed = false;
    Ok(DeviceBits {
        device: guard.device.clone(),
        queue,
        cmd_pool,
        cmd_buf,
        fence,
        staging_buffer,
        staging_memory,
        staging_mapped,
    })
}

pub(crate) fn alloc_buffer<'p>(
    provider: &'p VulkanProvider,
    bytes: usize,
) -> Result<VulkanMem<'p>, VramError> {
    // Rounds buffer size to a multiple of 4 (requirement for vkCmdFillBuffer with WHOLE_SIZE
    // in zero); the logical len remains `bytes`.
    let buf_size = ((bytes as u64).max(1) + 3) & !3;
    let buf_ci = vk::BufferCreateInfo::default()
        .size(buf_size)
        .usage(vk::BufferUsageFlags::TRANSFER_SRC | vk::BufferUsageFlags::TRANSFER_DST)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    // SAFETY: device + buf_ci valid.
    let buffer = unsafe { provider.device.create_buffer(&buf_ci, None) }
        .map_err(|e| vk_err("create_buffer", e))?;

    // SAFETY: buffer valid.
    let req = unsafe { provider.device.get_buffer_memory_requirements(buffer) };
    // SAFETY: phys valid.
    let mprops = unsafe {
        provider
            .instance
            .get_physical_device_memory_properties(provider.phys)
    };
    let mt = match pick_memory_type(
        &mprops,
        req.memory_type_bits,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    ) {
        Some(i) => i,
        None => {
            // SAFETY: buffer created above; destroyed before returning (no leak).
            unsafe { provider.device.destroy_buffer(buffer, None) };
            return Err(VramError::Provider(
                "no DEVICE_LOCAL memory type for the buffer".into(),
            ));
        }
    };
    let mai = vk::MemoryAllocateInfo::default()
        .allocation_size(req.size)
        .memory_type_index(mt);
    // SAFETY: device + mai valid.
    let memory = match unsafe { provider.device.allocate_memory(&mai, None) } {
        Ok(m) => m,
        Err(e) => {
            // SAFETY: buffer created above; destroyed on error.
            unsafe { provider.device.destroy_buffer(buffer, None) };
            return Err(vk_err("allocate_memory", e));
        }
    };
    // SAFETY: buffer + memory valid; offset 0.
    if let Err(e) = unsafe { provider.device.bind_buffer_memory(buffer, memory, 0) } {
        // SAFETY: buffer + memory created above; freed in reverse order on error.
        unsafe {
            provider.device.free_memory(memory, None);
            provider.device.destroy_buffer(buffer, None);
        }
        return Err(vk_err("bind_buffer_memory", e));
    }
    provider
        .allocated
        .fetch_add(bytes as u64, Ordering::Relaxed);
    Ok(VulkanMem {
        provider,
        buffer,
        memory,
        len: bytes,
    })
}

/// Vulkan VRAM region (GAT: borrows `&'p VulkanProvider`). RAII: `Drop` frees buffer+memory.
pub struct VulkanMem<'p> {
    pub(crate) provider: &'p VulkanProvider,
    pub(crate) buffer: vk::Buffer,
    pub(crate) memory: vk::DeviceMemory,
    pub(crate) len: usize,
}

impl VulkanMem<'_> {
    /// `off + len <= self.len`, otherwise `OutOfRange` (mirrors CUDA's bounds check).
    fn check_bounds(&self, off: u64, len: usize) -> Result<(), VramError> {
        match off.checked_add(len as u64) {
            Some(end) if end <= self.len as u64 => Ok(()),
            _ => Err(VramError::OutOfRange {
                off,
                len: len as u64,
                size: self.len as u64,
            }),
        }
    }
}

impl VramMemory for VulkanMem<'_> {
    fn len(&self) -> usize {
        self.len
    }

    fn zero(&mut self) -> Result<(), VramError> {
        let buffer = self.buffer;
        self.provider.submit_wait(|dev, cmd| {
            // SAFETY: `cmd` in recording; `buffer` of this provider; `WHOLE_SIZE` zeroes the
            // entire buffer (allocated as a multiple of 4 to satisfy `vkCmdFillBuffer`).
            unsafe { dev.cmd_fill_buffer(cmd, buffer, 0, vk::WHOLE_SIZE, 0) };
        })
    }

    fn read_at(&self, off: u64, dst: &mut [u8]) -> Result<(), VramError> {
        self.check_bounds(off, dst.len())?;
        let p = self.provider;
        let buffer = self.buffer;
        let mut done = 0usize;
        while done < dst.len() {
            let chunk = (dst.len() - done).min(STAGING_BYTES as usize);
            let src_off = off + done as u64;
            // GPU: copies `[src_off, src_off + chunk)` from the `DEVICE_LOCAL` buffer -> staging.
            p.submit_wait(|dev, cmd| {
                let region = [vk::BufferCopy::default()
                    .src_offset(src_off)
                    .dst_offset(0)
                    .size(chunk as u64)];
                // SAFETY: buffers belong to the provider; `chunk <= STAGING_BYTES` and bounds-checked on the buffer.
                unsafe { dev.cmd_copy_buffer(cmd, buffer, p.staging_buffer, &region) };
            })?;
            // Host: staging.mapped -> dst[done..].
            // SAFETY: `staging_mapped` has `STAGING_BYTES` bytes (`HOST_VISIBLE|HOST_COHERENT`, no flush);
            // `chunk <= STAGING_BYTES`; `dst[done..done+chunk]` is valid (slice bounds).
            unsafe {
                std::ptr::copy_nonoverlapping(p.staging_mapped, dst.as_mut_ptr().add(done), chunk)
            };
            done += chunk;
        }
        Ok(())
    }

    fn write_at(&mut self, off: u64, src: &[u8]) -> Result<(), VramError> {
        self.check_bounds(off, src.len())?;
        let p = self.provider;
        let buffer = self.buffer;
        let mut done = 0usize;
        while done < src.len() {
            let chunk = (src.len() - done).min(STAGING_BYTES as usize);
            // Host: src[done..] -> staging.mapped.
            // SAFETY: `staging_mapped` has `STAGING_BYTES` bytes; `chunk <= STAGING_BYTES`;
            // `src[done..done+chunk]` is valid (slice bounds). `HOST_COHERENT`: no flush.
            unsafe {
                std::ptr::copy_nonoverlapping(src.as_ptr().add(done), p.staging_mapped, chunk)
            };
            let dst_off = off + done as u64;
            // GPU: copies staging -> `[dst_off, dst_off + chunk)` on the `DEVICE_LOCAL` buffer.
            p.submit_wait(|dev, cmd| {
                let region = [vk::BufferCopy::default()
                    .src_offset(0)
                    .dst_offset(dst_off)
                    .size(chunk as u64)];
                // SAFETY: buffers belong to the provider; `chunk <= STAGING_BYTES` and bounds-checked on the buffer.
                unsafe { dev.cmd_copy_buffer(cmd, p.staging_buffer, buffer, &region) };
            })?;
            done += chunk;
        }
        Ok(())
    }
}

impl Drop for VulkanMem<'_> {
    fn drop(&mut self) {
        // SAFETY: buffer+memory created in `alloc` of this provider; destroyed once in reverse
        // order. The device remains alive (borrowing `&'p provider`).
        unsafe {
            self.provider.device.destroy_buffer(self.buffer, None);
            self.provider.device.free_memory(self.memory, None);
        }
        self.provider
            .allocated
            .fetch_sub(self.len as u64, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn transfer_module_compiles() {}
}
