with open('crates/ramshared-vram/src/lib.rs', 'r') as f:
    content = f.read()

content = content.replace(
'''pub enum VramError {
    /// Backend provider failure: initialization/driver/allocation error.
    Provider(String),
    /// Attempted access out of the allocated memory range.
    OutOfRange { off: u64, len: u64, size: u64 },
}''',
'''pub enum VramError {
    /// Backend provider failure: initialization/driver/allocation error.
    Provider(String),
    /// Attempted access out of the allocated memory range.
    OutOfRange { off: u64, len: u64, size: u64 },
}

/// Semantic Vulkan errors mapped from ash::vk::Result to domain concepts.
#[derive(Debug)]
pub enum VulkanError {
    /// Out of memory (host or device) (-ENOMEM).
    OutOfMemory,
    /// Device has been lost (-ENODEV).
    DeviceLost,
    /// Extension not present (-EINVAL).
    ExtensionNotPresent,
    /// Fallback for other Vulkan errors.
    Other(String),
}

impl std::fmt::Display for VulkanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VulkanError::OutOfMemory => write!(f, "vulkan out of memory (-ENOMEM)"),
            VulkanError::DeviceLost => write!(f, "vulkan device lost (-ENODEV)"),
            VulkanError::ExtensionNotPresent => write!(f, "vulkan extension not present (-EINVAL)"),
            VulkanError::Other(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for VulkanError {}
'''
)

with open('crates/ramshared-vram/src/lib.rs', 'w') as f:
    f.write(content)
