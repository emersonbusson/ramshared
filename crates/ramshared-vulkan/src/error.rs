use ash::vk;
use thiserror::Error;
use ramshared_vram::VramError;

/// Vulkan backend errors.
#[derive(Debug, Error)]
pub enum VulkanError {
    #[error("Failed to load Vulkan library: {0}")]
    Load(#[from] ash::LoadingError),

    #[error("Vulkan API error during {context}: {result}")]
    Api {
        context: &'static str,
        result: vk::Result,
    },

    #[error("No Vulkan physical device available")]
    NoDevice,

    #[error("No transfer queue family available")]
    NoTransferQueue,

    #[error("Command buffer allocation returned empty")]
    EmptyCommandBuffer,

    #[error("Missing HOST_VISIBLE|HOST_COHERENT memory type for staging")]
    MissingStagingMemoryType,

    #[error("Missing DEVICE_LOCAL memory type for device buffer")]
    MissingDeviceMemoryType,
}

impl From<VulkanError> for VramError {
    fn from(err: VulkanError) -> Self {
        VramError::Provider(err.to_string())
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_conversion() {
        let err = VulkanError::NoDevice;
        let vram_err: VramError = err.into();
        assert_eq!(vram_err.to_string(), "vram provider: No Vulkan physical device available");
    }
}
