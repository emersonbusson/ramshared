#[cfg(debug_assertions)]
use std::ffi::{CStr, CString};
use ash::vk;
use ramshared_vram::VramError;
use crate::vk_err;

#[cfg(debug_assertions)]
use crate::debug::vulkan_debug_callback;

/// Debug resources returned alongside the instance.
pub(crate) struct DebugResources {
    #[cfg(debug_assertions)]
    pub(crate) utils: ash::ext::debug_utils::Instance,
    #[cfg(debug_assertions)]
    pub(crate) messenger: vk::DebugUtilsMessengerEXT,
}

impl DebugResources {
    pub(crate) fn destroy(&mut self) {
        #[cfg(debug_assertions)]
        unsafe {
            self.utils.destroy_debug_utils_messenger(self.messenger, None);
        }
    }
}

/// Creates a Vulkan instance, enabling validation layers and debug messenger in debug builds.
pub(crate) fn create_instance(
    entry: &ash::Entry,
) -> Result<(ash::Instance, Option<DebugResources>), VramError> {
    let app = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_1);

    #[cfg(debug_assertions)]
    let mut layer_names = Vec::new();
    #[cfg(debug_assertions)]
    let mut extension_names = Vec::new();

    #[cfg(debug_assertions)]
    let validation_layer = CString::new("VK_LAYER_KHRONOS_validation")
        .unwrap_or_else(|_| unreachable!("literal has no null bytes"));

    #[cfg(debug_assertions)]
    {
        // SAFETY: enumerate_instance_layer_properties returns valid properties.
        let available_layers = unsafe { entry.enumerate_instance_layer_properties() }
            .map_err(|e| vk_err("enumerate_instance_layer_properties", e))?;

        let has_validation = available_layers.iter().any(|layer| {
            // SAFETY: layer_name is a null-terminated C string in the properties struct.
            let name = unsafe { CStr::from_ptr(layer.layer_name.as_ptr()) };
            name == validation_layer.as_c_str()
        });

        if has_validation {
            layer_names.push(validation_layer.as_ptr());
            extension_names.push(ash::ext::debug_utils::NAME.as_ptr());
        } else {
            tracing::warn!("VK_LAYER_KHRONOS_validation not found, validation disabled");
        }
    }

    #[cfg(not(debug_assertions))]
    let layer_names: Vec<*const std::ffi::c_char> = Vec::new();
    #[cfg(not(debug_assertions))]
    let extension_names: Vec<*const std::ffi::c_char> = Vec::new();

    #[allow(unused_mut)]
    let mut ci = vk::InstanceCreateInfo::default()
        .application_info(&app)
        .enabled_layer_names(&layer_names)
        .enabled_extension_names(&extension_names);

    #[cfg(debug_assertions)]
    let mut debug_info = vk::DebugUtilsMessengerCreateInfoEXT::default()
        .message_severity(
            vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                | vk::DebugUtilsMessageSeverityFlagsEXT::INFO
                | vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE,
        )
        .message_type(
            vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
        )
        .pfn_user_callback(Some(vulkan_debug_callback));

    #[cfg(debug_assertions)]
    if !layer_names.is_empty() {
        ci = ci.push_next(&mut debug_info);
    }

    // SAFETY: ci is valid, application_info is valid, extension and layer names point to null-terminated C strings.
    let instance = unsafe { entry.create_instance(&ci, None) }
        .map_err(|e| vk_err("create_instance", e))?;

    #[allow(unused_mut)]
    let mut debug_resources = None;
    #[cfg(debug_assertions)]
    if !layer_names.is_empty() {
        // Create debug utils messenger
        let debug_utils_ext = ash::ext::debug_utils::Instance::new(entry, &instance);
        // SAFETY: debug_info is valid, instance is valid.
        match unsafe { debug_utils_ext.create_debug_utils_messenger(&debug_info, None) } {
            Ok(messenger) => {
                debug_resources = Some(DebugResources {
                    utils: debug_utils_ext,
                    messenger,
                });
            }
            Err(e) => {
                tracing::warn!("Failed to create debug utils messenger: {:?}", e);
            }
        }
    }

    Ok((instance, debug_resources))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires Vulkan loader + ICD (lavapipe is enough; run with --ignored)"]
    fn test_create_instance() {
        let entry = unsafe { ash::Entry::load() }.unwrap_or_else(|_| panic!("load entry"));
        let (instance, debug) = create_instance(&entry).unwrap_or_else(|_| panic!("create_instance"));

        let has_debug = debug.is_some();
        if let Some(mut d) = debug {
            d.destroy();
        }

        unsafe {
            instance.destroy_instance(None);
        }

        if cfg!(debug_assertions) {
            // Can't strictly assert has_debug because CI might not have the validation layers installed,
            // but we can ensure the function didn't panic and returned successfully.
            eprintln!("debug_assertions enabled, debug_utils present = {}", has_debug);
        } else {
            assert!(!has_debug, "release builds should not have debug utils");
        }
    }
}
