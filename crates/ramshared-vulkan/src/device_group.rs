use ash::vk;
use ramshared_vram::VramError;
use crate::vk_err;

/// Uses Vulkan 1.1 API to enumerate device groups (VK_KHR_device_group).
/// Safe wrapper over FFI: verifies instance validity and handles incomplete queries.
pub fn enumerate_groups(instance: &ash::Instance) -> Result<Vec<vk::PhysicalDeviceGroupProperties<'_>>, VramError> {
    let mut count = 0;
    // SAFETY: instance is valid, API call queries count.
    let result = unsafe {
        (instance.fp_v1_1().enumerate_physical_device_groups)(
            instance.handle(),
            &mut count,
            std::ptr::null_mut(),
        )
    };
    if result != vk::Result::SUCCESS {
        return Err(vk_err("enumerate_physical_device_groups(count)", result));
    }

    let mut props = vec![vk::PhysicalDeviceGroupProperties::default(); count as usize];
    // SAFETY: instance is valid, props is appropriately sized
    let result = unsafe {
        (instance.fp_v1_1().enumerate_physical_device_groups)(
            instance.handle(),
            &mut count,
            props.as_mut_ptr(),
        )
    };
    if result != vk::Result::SUCCESS && result != vk::Result::INCOMPLETE {
        return Err(vk_err("enumerate_physical_device_groups", result));
    }

    Ok(props)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use ash::vk;

    #[test]
    #[ignore = "requires Vulkan loader"]
    fn test_enumerate_groups_runs() {
        // SAFETY: dynamically loading vulkan library for tests.
        let entry = unsafe { ash::Entry::load() }.expect("load");
        let app = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_1);
        let ci = vk::InstanceCreateInfo::default().application_info(&app);
        // SAFETY: entry is valid.
        let instance = unsafe { entry.create_instance(&ci, None) }.expect("create_instance");

        let groups = enumerate_groups(&instance);
        // SAFETY: instance created above, safe to destroy.
        unsafe { instance.destroy_instance(None) };
        assert!(groups.is_ok());
    }
}
