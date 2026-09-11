use crate::{Cuda, CudaError};

/// A GPU device ranked by available memory capacity.
#[derive(Debug, Clone)]
pub struct OptimalDevice {
    pub ordinal: i32,
    pub name: String,
    pub free_memory: usize,
    pub total_memory: usize,
}

/// Enumerates all visible CUDA devices and ranks them by available VRAM descending.
pub fn enumerate_and_rank_devices(cuda: &Cuda) -> Result<Vec<OptimalDevice>, CudaError> {
    let count = cuda.device_count()?;
    let mut devices = Vec::with_capacity(count as usize);

    for ordinal in 0..count {
        let dev = match cuda.device(ordinal) {
            Ok(d) => d,
            Err(_) => continue, // Skip devices we can't initialize
        };

        let (free, total) = match cuda.create_context(&dev).and_then(|ctx| ctx.mem_info()) {
            Ok((f, t)) => (f, t),
            Err(_) => continue, // Skip if we can't create context or get mem info
        };

        devices.push(OptimalDevice {
            ordinal,
            name: dev.name().to_string(),
            free_memory: free,
            total_memory: total,
        });
    }

    devices.sort_by_key(|a| std::cmp::Reverse(a.free_memory));
    Ok(devices)
}

#[cfg(test)]
mod tests {
    // Unused imports are cleaned up

    #[test]
    fn test_enumerate_devices() {
        // E2E test requires a GPU, unit test compiles and ensures syntax.
    }
}
