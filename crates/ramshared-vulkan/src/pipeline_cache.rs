//! `pipeline_cache` — Persistent VkPipelineCache management for shader compilation.
//!
//! Provides a defensive, RAII-based wrapper around `VkPipelineCache` with disk persistence.

use ash::vk;
use ramshared_vram::VramError;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Maximum allowed size for a pipeline cache file (128 MiB).
/// Prevents memory exhaustion from malicious or corrupted cache files.
const MAX_CACHE_SIZE_BYTES: u64 = 128 * 1024 * 1024;

/// RAII wrapper for a Vulkan Pipeline Cache with atomic disk persistence.
pub struct PersistentPipelineCache {
    device: ash::Device,
    cache: vk::PipelineCache,
    path: PathBuf,
}

impl PersistentPipelineCache {
    /// Loads the pipeline cache from the given path, or creates an empty one if not found or invalid.
    pub fn new(device: ash::Device, path: impl AsRef<Path>) -> Result<Self, VramError> {
        let path = path.as_ref().to_path_buf();
        let mut data = Vec::new();

        // Attempt to read existing cache safely (defense-in-depth).
        if let Ok(metadata) = std::fs::metadata(&path) {
            #[allow(clippy::collapsible_if)]
            if metadata.len() <= MAX_CACHE_SIZE_BYTES {
                if let Ok(mut file) = File::open(&path) {
                    // Ignore read errors; we just start with an empty cache if it fails.
                    let _ = file.read_to_end(&mut data);
                }
            }
        }

        let mut ci = vk::PipelineCacheCreateInfo::default();
        if !data.is_empty() {
            ci = ci.initial_data(&data);
        }

        // SAFETY: `device` is valid. `ci.p_initial_data` points to `data` which lives until the end of this scope.
        // Vulkan driver is responsible for validating the cache header and contents.
        let cache = unsafe { device.create_pipeline_cache(&ci, None) }.map_err(|e| {
            VramError::Provider(format!("vulkan create_pipeline_cache failed: {:?}", e))
        })?;

        Ok(Self {
            device,
            cache,
            path,
        })
    }

    /// Returns the raw Vulkan pipeline cache handle.
    pub fn handle(&self) -> vk::PipelineCache {
        self.cache
    }

    /// Serializes the current pipeline cache back to disk safely using a temporary file.
    pub fn save(&self) -> Result<(), VramError> {
        // SAFETY: `cache` was created by this struct and `device` is still valid.
        let data = unsafe { self.device.get_pipeline_cache_data(self.cache) }.map_err(|e| {
            VramError::Provider(format!("vulkan get_pipeline_cache_data failed: {:?}", e))
        })?;

        if data.is_empty() {
            return Ok(()); // Nothing to save.
        }

        let temp_path = self.path.with_extension("tmp");

        {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&temp_path)
                .map_err(|e| {
                    VramError::Provider(format!("vulkan pipeline_cache temp file open: {:?}", e))
                })?;

            file.write_all(&data).map_err(|e| {
                VramError::Provider(format!("vulkan pipeline_cache write: {:?}", e))
            })?;

            file.sync_data()
                .map_err(|e| VramError::Provider(format!("vulkan pipeline_cache sync: {:?}", e)))?;
        }

        // Atomic rename to replace the old cache file.
        std::fs::rename(&temp_path, &self.path)
            .map_err(|e| VramError::Provider(format!("vulkan pipeline_cache rename: {:?}", e)))?;

        Ok(())
    }
}

impl Drop for PersistentPipelineCache {
    fn drop(&mut self) {
        // Best-effort save on drop. Errors are swallowed as we are in a destructor.
        let _ = self.save();

        // SAFETY: `cache` is valid and owned by this struct. `device` is valid.
        unsafe {
            self.device.destroy_pipeline_cache(self.cache, None);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::VulkanProvider;
    use std::env;
    use std::process;

    #[test]
    #[ignore = "requires Vulkan loader + ICD (run with --ignored)"]
    fn test_persistent_pipeline_cache_lifecycle() {
        let p = VulkanProvider::open(0).expect("opens Vulkan");
        let cache_path = env::temp_dir().join(format!("vulkan_test_cache_{}.bin", process::id()));

        let _ = std::fs::remove_file(&cache_path);

        // 1. Create a new cache
        {
            let _cache =
                PersistentPipelineCache::new(p.device.clone(), &cache_path).expect("new cache");
            // drop will trigger best-effort save
        }

        // Even if empty, it might create a file with a header, or not depending on the driver.
        // We force save and check.
        {
            let cache =
                PersistentPipelineCache::new(p.device.clone(), &cache_path).expect("new cache");
            cache.save().expect("manual save");
        }

        // 2. Open an existing cache
        {
            let cache = PersistentPipelineCache::new(p.device.clone(), &cache_path)
                .expect("existing cache");
            assert!(
                cache.handle() != vk::PipelineCache::null(),
                "cache handle should be valid"
            );
        }

        let _ = std::fs::remove_file(&cache_path);
    }
}
