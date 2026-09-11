//! Vulkan device discovery and initialization logic.
//!
//! Extracted from the monolithic `lib.rs` to encapsulate `ash` instance creation,
//! physical device enumeration, and queue family selection.

use ash::vk;
use ramshared_vram::VramError;
use std::ffi::CStr;

use crate::vk_err;

/// Selects a transfer queue family (prefers explicit `TRANSFER`; falls back to `GRAPHICS`/`COMPUTE`, which imply transfer per spec). Returns the family index.
pub(crate) fn pick_transfer_family(
    instance: &ash::Instance,
    phys: vk::PhysicalDevice,
) -> Option<u32> {
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

/// Index of the first memory type that satisfies `type_bits` (bitmask of `MemoryRequirements`) and contains
/// all `want` flags. `None` if none fit.
pub(crate) fn pick_memory_type(
    props: &vk::PhysicalDeviceMemoryProperties,
    type_bits: u32,
    want: vk::MemoryPropertyFlags,
) -> Option<u32> {
    (0..props.memory_type_count).find(|&i| {
        (type_bits & (1 << i)) != 0 && props.memory_types[i as usize].property_flags.contains(want)
    })
}

/// Device selection + name extraction
pub(crate) fn discover_device(
    instance: &ash::Instance,
    ordinal: u32,
) -> Result<(vk::PhysicalDevice, String, u32), VramError> {
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
    Ok((phys, name, qf))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires Vulkan loader + ICD (lavapipe/llvmpipe is enough; run with --ignored)"]
    fn test_discover_device() {
        let entry = unsafe { ash::Entry::load() }.expect("load entry");
        let app = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_1);
        let ci = vk::InstanceCreateInfo::default().application_info(&app);
        let instance = unsafe { entry.create_instance(&ci, None) }.expect("create_instance");

        let (phys, name, qf) = discover_device(&instance, 0).expect("discover_device");
        assert!(!name.is_empty(), "device has a name");
        // Verify qf is valid
        let fams = unsafe { instance.get_physical_device_queue_family_properties(phys) };
        assert!((qf as usize) < fams.len(), "qf in bounds");

        unsafe { instance.destroy_instance(None) };
    }
}
