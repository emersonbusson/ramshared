with open('crates/ramshared-vulkan/src/lib.rs', 'r') as f:
    content = f.read()

content = content.replace(
'''fn vk_err(ctx: &str, e: impl std::fmt::Debug) -> VramError {
    VramError::Provider(format!("vulkan {ctx}: {e:?}"))
}''',
'''use ramshared_vram::VulkanError;

fn vk_err(ctx: &str, e: ash::vk::Result) -> VramError {
    let err = match e {
        ash::vk::Result::ERROR_OUT_OF_HOST_MEMORY | ash::vk::Result::ERROR_OUT_OF_DEVICE_MEMORY => {
            VulkanError::OutOfMemory
        }
        ash::vk::Result::ERROR_DEVICE_LOST => VulkanError::DeviceLost,
        ash::vk::Result::ERROR_EXTENSION_NOT_PRESENT => VulkanError::ExtensionNotPresent,
        _ => VulkanError::Other(format!("vulkan {ctx}: {e:?}")),
    };
    VramError::Provider(err.to_string())
}'''
)

content = content.replace(
'''        let entry = unsafe { ash::Entry::load() }.map_err(|e| vk_err("load", e))?;''',
'''        let entry = unsafe { ash::Entry::load() }.map_err(|e| VramError::Provider(format!("vulkan load: {e:?}")))?;'''
)

content = content.replace(
'''mod tests {
    use super::*;

    #[test]
    #[ignore = "requires Vulkan loader + ICD (lavapipe/llvmpipe is enough; run with --ignored)"]
    fn open_enumerates_device_and_heap() {''',
'''mod tests {
    use super::*;

    #[test]
    fn test_vk_err_semantic_mapping() {
        let err = vk_err("alloc", ash::vk::Result::ERROR_OUT_OF_HOST_MEMORY);
        assert!(matches!(err, VramError::Provider(s) if s.contains("-ENOMEM")));

        let err2 = vk_err("alloc", ash::vk::Result::ERROR_OUT_OF_DEVICE_MEMORY);
        assert!(matches!(err2, VramError::Provider(s) if s.contains("-ENOMEM")));

        let err3 = vk_err("queue", ash::vk::Result::ERROR_DEVICE_LOST);
        assert!(matches!(err3, VramError::Provider(s) if s.contains("-ENODEV")));

        let err4 = vk_err("ext", ash::vk::Result::ERROR_EXTENSION_NOT_PRESENT);
        assert!(matches!(err4, VramError::Provider(s) if s.contains("-EINVAL")));

        let err5 = vk_err("unknown", ash::vk::Result::ERROR_UNKNOWN);
        assert!(matches!(err5, VramError::Provider(s) if s.contains("ERROR_UNKNOWN")));
    }

    #[test]
    #[ignore = "requires Vulkan loader + ICD (lavapipe/llvmpipe is enough; run with --ignored)"]
    fn open_enumerates_device_and_heap() {'''
)

with open('crates/ramshared-vulkan/src/lib.rs', 'w') as f:
    f.write(content)
