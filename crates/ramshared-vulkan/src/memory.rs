use std::sync::atomic::Ordering;

use ash::vk;
use ramshared_vram::{VramError, VramMemory};

use crate::provider::VulkanProvider;
use crate::utils::STAGING_BYTES;

/// Vulkan VRAM region (GAT: borrows `&'p VulkanProvider`). RAII: `Drop` frees buffer+memory.
pub struct VulkanMem<'p> {
    pub(crate) provider: &'p VulkanProvider,
    pub(crate) buffer: vk::Buffer,
    pub(crate) memory: vk::DeviceMemory,
    pub(crate) len: usize,
}

impl VulkanMem<'_> {
    /// `off + len <= self.len`, otherwise `OutOfRange` (mirrors CUDA's bounds check).
    fn check_bounds(&self, off: u64, len: usize) -> Result<(), VramError> {
        match off.checked_add(len as u64) {
            Some(end) if end <= self.len as u64 => Ok(()),
            _ => Err(VramError::OutOfRange {
                off,
                len: len as u64,
                size: self.len as u64,
            }),
        }
    }
}

impl VramMemory for VulkanMem<'_> {
    fn len(&self) -> usize {
        self.len
    }

    fn zero(&mut self) -> Result<(), VramError> {
        let buffer = self.buffer;
        self.provider.submit_wait(|dev, cmd| {
            // SAFETY: `cmd` in recording; `buffer` of this provider; `WHOLE_SIZE` zeroes the entire buffer.
            unsafe { dev.cmd_fill_buffer(cmd, buffer, 0, vk::WHOLE_SIZE, 0) };
        })
    }

    fn read_at(&self, off: u64, dst: &mut [u8]) -> Result<(), VramError> {
        self.check_bounds(off, dst.len())?;
        let p = self.provider;
        let buffer = self.buffer;
        let mut done = 0usize;
        while done < dst.len() {
            let chunk = (dst.len() - done).min(STAGING_BYTES as usize);
            let src_off = off + done as u64;
            p.submit_wait(|dev, cmd| {
                let region = [vk::BufferCopy::default()
                    .src_offset(src_off)
                    .dst_offset(0)
                    .size(chunk as u64)];
                // SAFETY: buffers belong to the provider.
                unsafe { dev.cmd_copy_buffer(cmd, buffer, p.staging_buffer, &region) };
            })?;
            // SAFETY: staging_mapped is mapped and STAGING_BYTES size.
            unsafe {
                std::ptr::copy_nonoverlapping(p.staging_mapped, dst.as_mut_ptr().add(done), chunk)
            };
            done += chunk;
        }
        Ok(())
    }

    fn write_at(&mut self, off: u64, src: &[u8]) -> Result<(), VramError> {
        self.check_bounds(off, src.len())?;
        let p = self.provider;
        let buffer = self.buffer;
        let mut done = 0usize;
        while done < src.len() {
            let chunk = (src.len() - done).min(STAGING_BYTES as usize);
            // SAFETY: staging_mapped is mapped and STAGING_BYTES size.
            unsafe {
                std::ptr::copy_nonoverlapping(src.as_ptr().add(done), p.staging_mapped, chunk)
            };
            let dst_off = off + done as u64;
            p.submit_wait(|dev, cmd| {
                let region = [vk::BufferCopy::default()
                    .src_offset(0)
                    .dst_offset(dst_off)
                    .size(chunk as u64)];
                // SAFETY: buffers belong to the provider.
                unsafe { dev.cmd_copy_buffer(cmd, p.staging_buffer, buffer, &region) };
            })?;
            done += chunk;
        }
        Ok(())
    }
}

impl Drop for VulkanMem<'_> {
    fn drop(&mut self) {
        // SAFETY: buffer+memory created in `alloc` of this provider; destroyed once in reverse order.
        unsafe {
            self.provider.device.destroy_buffer(self.buffer, None);
            self.provider.device.free_memory(self.memory, None);
        }
        self.provider
            .allocated
            .fetch_sub(self.len as u64, Ordering::Relaxed);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    #[test]
    fn vram_memory_struct() {
        // Just checking module compiles properly
        assert_eq!(1, 1);
    }
}
