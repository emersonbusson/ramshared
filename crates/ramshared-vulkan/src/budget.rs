use std::ffi::CStr;

use ash::vk;
use ramshared_vram::VramError;

/// Queries the memory budget using `VK_EXT_memory_budget`.
/// Returns `Some((free_bytes, total_bytes))` if successful and a DEVICE_LOCAL heap is found,
/// otherwise `None`.
pub(crate) fn query_budget(
    instance: &ash::Instance,
    phys: vk::PhysicalDevice,
) -> Result<Option<(u64, u64)>, VramError> {
    // 1. Check if VK_EXT_memory_budget is supported
    // SAFETY: `phys` is a valid physical device handle.
    let exts = unsafe { instance.enumerate_device_extension_properties(phys) }
        .map_err(|e| VramError::Provider(format!("enumerate_device_extension_properties: {:?}", e)))?;

    let mut supported = false;
    for ext in exts {
        // SAFETY: extension_name is a null-terminated C string provided by Vulkan
        let name = unsafe { CStr::from_ptr(ext.extension_name.as_ptr()) };
        if name.to_bytes() == vk::EXT_MEMORY_BUDGET_NAME.to_bytes() {
            supported = true;
            break;
        }
    }

    if !supported {
        return Ok(None);
    }

    // 2. Query budget via vkGetPhysicalDeviceMemoryProperties2
    let mut budget = vk::PhysicalDeviceMemoryBudgetPropertiesEXT::default();
    let mut props2 = vk::PhysicalDeviceMemoryProperties2::default().push_next(&mut budget);

    // SAFETY: `phys` is valid, and the extension chain is correctly initialized above.
    unsafe {
        instance.get_physical_device_memory_properties2(phys, &mut props2);
    }

    let heaps = &props2.memory_properties.memory_heaps
        [..props2.memory_properties.memory_heap_count as usize];

    let mut total_budget = 0u64;
    let mut total_usage = 0u64;
    let mut has_device_local = false;

    // A Vulkan device can have multiple DEVICE_LOCAL heaps (e.g. on RDNA GPUs).
    // Summing them gives the full physical limit.
    for (i, heap) in heaps.iter().enumerate() {
        if heap.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL) {
            total_budget += budget.heap_budget[i];
            total_usage += budget.heap_usage[i];
            has_device_local = true;
        }
    }

    if has_device_local && total_budget > 0 {
        Ok(Some((total_budget.saturating_sub(total_usage), total_budget)))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires Vulkan loader + ICD"]
    fn test_budget_query() {
        // SAFETY: Ash loads libvulkan.so.1 properly.
        let entry = unsafe { ash::Entry::load() }.unwrap();
        let app = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_1);
        let ci = vk::InstanceCreateInfo::default().application_info(&app);
        // SAFETY: app/ci live during call.
        let instance = unsafe { entry.create_instance(&ci, None) }.unwrap();

        // SAFETY: instance valid.
        let pdevs = unsafe { instance.enumerate_physical_devices() }.unwrap();
        if let Some(&phys) = pdevs.first() {
            let res = query_budget(&instance, phys).unwrap();
            eprintln!("Budget: {:?}", res);
        }

        // SAFETY: instance valid.
        unsafe { instance.destroy_instance(None) };
    }
}
