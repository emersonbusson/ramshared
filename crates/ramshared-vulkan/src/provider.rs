use std::ffi::CStr;
use std::sync::atomic::{AtomicU64, Ordering};

use ash::vk;
use ramshared_vram::{VramError, VramProvider};

use crate::memory::VulkanMem;
use crate::utils::{pick_memory_type, pick_transfer_family, vk_err, DeviceBits, ResGuard, STAGING_BYTES};

/// Vulkan backend for `VramProvider`.
pub struct VulkanProvider {
    pub(crate) instance: ash::Instance,
    _entry: ash::Entry,
    pub(crate) phys: vk::PhysicalDevice,
    pub(crate) device: ash::Device,
    pub(crate) queue: vk::Queue,
    pub(crate) cmd_pool: vk::CommandPool,
    pub(crate) cmd_buf: vk::CommandBuffer,
    pub(crate) fence: vk::Fence,
    pub(crate) staging_buffer: vk::Buffer,
    pub(crate) staging_memory: vk::DeviceMemory,
    pub(crate) staging_mapped: *mut u8,
    name: String,
    pub(crate) allocated: AtomicU64,
}

impl VulkanProvider {
    /// Opens the Vulkan provider at the given ordinal.
    pub fn open(ordinal: u32) -> Result<Self, VramError> {
        // SAFETY: loader valid.
        let entry = unsafe { ash::Entry::load() }.map_err(|e| vk_err("load", e))?;
        let app = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_1);
        let ci = vk::InstanceCreateInfo::default().application_info(&app);
        // SAFETY: entry valid.
        let instance = unsafe { entry.create_instance(&ci, None) }
            .map_err(|e| vk_err("create_instance", e))?;

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
                name,
                allocated: AtomicU64::new(0),
            }),
            Err(e) => {
                // SAFETY: instance created above.
                unsafe { instance.destroy_instance(None) };
                Err(e)
            }
        }
    }

    fn after_instance(
        instance: &ash::Instance,
        ordinal: u32,
    ) -> Result<(vk::PhysicalDevice, String, DeviceBits), VramError> {
        // SAFETY: instance valid.
        let phys_list = unsafe { instance.enumerate_physical_devices() }
            .map_err(|e| vk_err("enumerate_physical_devices", e))?;

        // DT-1..DT-9 logic
        let phys = *phys_list
            .iter()
            .find(|&&p| {
                // SAFETY: p valid
                let props = unsafe { instance.get_physical_device_properties(p) };
                props.device_type == vk::PhysicalDeviceType::DISCRETE_GPU
            })
            .unwrap_or_else(|| {
                phys_list
                    .get(ordinal as usize)
                    .unwrap_or_else(|| &phys_list[0])
            });

        // SAFETY: phys valid.
        let props = unsafe { instance.get_physical_device_properties(phys) };
        // SAFETY: null-terminated C string from Vulkan.
        let name = unsafe { CStr::from_ptr(props.device_name.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        let qf = pick_transfer_family(instance, phys)
            .ok_or_else(|| VramError::Provider("sem queue family de transfer".into()))?;
        let bits = create_device_resources(instance, phys, qf)?;
        Ok((phys, name, bits))
    }

    /// Returns the name of the active Vulkan device.
    pub fn device_name(&self) -> &str {
        &self.name
    }

    /// Returns the total DEVICE_LOCAL memory available on the GPU.
    pub fn device_local_total(&self) -> u64 {
        // SAFETY: instance + phys valid.
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

    pub(crate) fn submit_wait<F>(&self, record: F) -> Result<(), VramError>
    where
        F: FnOnce(&ash::Device, vk::CommandBuffer),
    {
        let dev = &self.device;
        let cmd = self.cmd_buf;
        // SAFETY: cmd buffer + pool + queue + fence valid.
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

fn create_device_resources(
    instance: &ash::Instance,
    phys: vk::PhysicalDevice,
    qf: u32,
) -> Result<DeviceBits, VramError> {
    let prio = [1.0f32];
    let qci = [vk::DeviceQueueCreateInfo::default()
        .queue_family_index(qf)
        .queue_priorities(&prio)];

    let features = vk::PhysicalDeviceFeatures::default().sparse_binding(true);
    let dci = vk::DeviceCreateInfo::default()
        .queue_create_infos(&qci)
        .enabled_features(&features);

    // SAFETY: instance + phys + dci valid.
    let device = unsafe { instance.create_device(phys, &dci, None) }
        .map_err(|e| vk_err("create_device", e))?;

    let mut guard = ResGuard::new(device);

    // SAFETY: queue family valid from get_device_queue.
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
    // SAFETY: device + cb_ai valid.
    let cmd_buf = unsafe { guard.device.allocate_command_buffers(&cb_ai) }
        .map_err(|e| vk_err("allocate_command_buffers", e))?[0];

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

    // SAFETY: buffer + memory valid; offset 0.
    if let Err(e) = unsafe {
        guard
            .device
            .bind_buffer_memory(staging_buffer, staging_memory, 0)
    } {
        return Err(vk_err("bind_buffer_memory(staging)", e));
    }

    // SAFETY: device + memory valid; size is STAGING_BYTES.
    let staging_mapped = unsafe {
        guard
            .device
            .map_memory(staging_memory, 0, STAGING_BYTES, vk::MemoryMapFlags::empty())
    }
    .map_err(|e| vk_err("map_memory(staging)", e))? as *mut u8;
    guard.mapped = true;

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

impl VramProvider for VulkanProvider {
    type Mem<'p>
        = VulkanMem<'p>
    where
        Self: 'p;

    fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError> {
        let buf_size = ((bytes as u64).max(1) + 3) & !3;
        let buf_ci = vk::BufferCreateInfo::default()
            .flags(vk::BufferCreateFlags::SPARSE_BINDING)
            .size(buf_size)
            .usage(vk::BufferUsageFlags::TRANSFER_SRC | vk::BufferUsageFlags::TRANSFER_DST)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);

        // SAFETY: device + buf_ci valid.
        let buffer = unsafe { self.device.create_buffer(&buf_ci, None) }
            .map_err(|e| vk_err("create_buffer", e))?;

        // SAFETY: buffer valid.
        let req = unsafe { self.device.get_buffer_memory_requirements(buffer) };
        // SAFETY: phys valid.
        let mprops = unsafe {
            self.instance
                .get_physical_device_memory_properties(self.phys)
        };
        let mt = match pick_memory_type(
            &mprops,
            req.memory_type_bits,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        ) {
            Some(i) => i,
            None => {
                // SAFETY: buffer created above; destroyed before returning.
                unsafe { self.device.destroy_buffer(buffer, None) };
                return Err(VramError::Provider(
                    "no DEVICE_LOCAL memory type for the buffer".into(),
                ));
            }
        };
        let mai = vk::MemoryAllocateInfo::default()
            .allocation_size(req.size)
            .memory_type_index(mt);

        // SAFETY: device + mai valid.
        let memory = match unsafe { self.device.allocate_memory(&mai, None) } {
            Ok(m) => m,
            Err(e) => {
                // SAFETY: buffer created above; destroyed on error.
                unsafe { self.device.destroy_buffer(buffer, None) };
                return Err(vk_err("allocate_memory", e));
            }
        };

        let mem_bind = vk::SparseMemoryBind::default()
            .resource_offset(0)
            .size(req.size)
            .memory(memory)
            .memory_offset(0);

        let buf_bind = vk::SparseBufferMemoryBindInfo::default()
            .buffer(buffer)
            .binds(std::slice::from_ref(&mem_bind));

        let bind_info = vk::BindSparseInfo::default()
            .buffer_binds(std::slice::from_ref(&buf_bind));

        // SAFETY: buffer + memory valid; queue valid.
        if let Err(e) = unsafe { self.device.queue_bind_sparse(self.queue, &[bind_info], self.fence) } {
            // SAFETY: resources cleaned up on error.
            unsafe {
                self.device.free_memory(memory, None);
                self.device.destroy_buffer(buffer, None);
            }
            return Err(vk_err("queue_bind_sparse", e));
        }

        let fences = [self.fence];
        // SAFETY: fence valid.
        if let Err(e) = unsafe { self.device.wait_for_fences(&fences, true, u64::MAX) } {
            // SAFETY: resources cleaned up on error.
            unsafe {
                self.device.free_memory(memory, None);
                self.device.destroy_buffer(buffer, None);
            }
            return Err(vk_err("wait_for_fences", e));
        }

        // SAFETY: fence valid.
        if let Err(e) = unsafe { self.device.reset_fences(&fences) } {
            // SAFETY: resources cleaned up on error.
            unsafe {
                self.device.free_memory(memory, None);
                self.device.destroy_buffer(buffer, None);
            }
            return Err(vk_err("reset_fences", e));
        }

        self.allocated.fetch_add(bytes as u64, Ordering::Relaxed);
        Ok(VulkanMem {
            provider: self,
            buffer,
            memory,
            len: bytes,
        })
    }

    fn mem_info(&self) -> Result<(u64, u64), VramError> {
        let total = self.device_local_total();
        let used = self.allocated.load(Ordering::Relaxed);
        Ok((total.saturating_sub(used), total))
    }
}

impl Drop for VulkanProvider {
    fn drop(&mut self) {
        // SAFETY: resources created in open, destroyed once in reverse order of allocation.
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

    #[test]
    #[ignore = "requires Vulkan loader + ICD"]
    fn open_enumerates_device_and_heap() {
        let p = VulkanProvider::open(0).expect("opens Vulkan");
        assert!(!p.device_name().is_empty(), "device has a name");
        let total = p.device_local_total();
        assert!(total > 0, "heap > 0");
    }
}
