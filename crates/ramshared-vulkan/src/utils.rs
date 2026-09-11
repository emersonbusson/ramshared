use ash::vk;
use ramshared_vram::VramError;

pub(crate) const STAGING_BYTES: u64 = 1 << 20;

pub(crate) fn vk_err(ctx: &str, e: impl std::fmt::Debug) -> VramError {
    VramError::Provider(format!("vulkan {ctx}: {e:?}"))
}

pub(crate) fn pick_transfer_family(instance: &ash::Instance, phys: vk::PhysicalDevice) -> Option<u32> {
    // SAFETY: phys valid.
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

pub(crate) fn pick_memory_type(
    props: &vk::PhysicalDeviceMemoryProperties,
    type_bits: u32,
    want: vk::MemoryPropertyFlags,
) -> Option<u32> {
    (0..props.memory_type_count).find(|&i| {
        let b = 1 << i;
        (type_bits & b) != 0 && props.memory_types[i as usize].property_flags.contains(want)
    })
}

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
        // SAFETY: cleans up partially initialized resources on error.
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
