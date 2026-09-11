#![cfg(debug_assertions)]

use std::ffi::{c_void, CStr};
use ash::vk;
use tracing::{debug, error, info, warn};

/// Vulkan debug messenger callback.
/// SAFETY: `p_callback_data` must be a valid pointer to a `VkDebugUtilsMessengerCallbackDataEXT` structure as guaranteed by the Vulkan validation layers.
pub(crate) unsafe extern "system" fn vulkan_debug_callback(
    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _message_type: vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _p_user_data: *mut c_void,
) -> vk::Bool32 {
    if p_callback_data.is_null() {
        return vk::FALSE;
    }
    // SAFETY: verified non-null above. The Vulkan spec guarantees the pointer is valid for the duration of the callback.
    let data = unsafe { &*p_callback_data };

    if data.p_message.is_null() {
        return vk::FALSE;
    }

    // SAFETY: the message pointer is non-null and guaranteed to be a null-terminated UTF-8 string by the Vulkan spec.
    let msg = unsafe { CStr::from_ptr(data.p_message) }.to_string_lossy();

    match message_severity {
        vk::DebugUtilsMessageSeverityFlagsEXT::ERROR => error!("Vulkan validation: {}", msg),
        vk::DebugUtilsMessageSeverityFlagsEXT::WARNING => warn!("Vulkan validation: {}", msg),
        vk::DebugUtilsMessageSeverityFlagsEXT::INFO => info!("Vulkan validation: {}", msg),
        vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE => debug!("Vulkan validation: {}", msg),
        _ => warn!("Vulkan validation (unknown severity): {}", msg),
    }

    vk::FALSE
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    #[test]
    fn test_vulkan_debug_callback_null_pointers() {
        unsafe {
            let res = vulkan_debug_callback(
                vk::DebugUtilsMessageSeverityFlagsEXT::ERROR,
                vk::DebugUtilsMessageTypeFlagsEXT::GENERAL,
                ptr::null(),
                ptr::null_mut(),
            );
            assert_eq!(res, vk::FALSE);
        }
    }
}
