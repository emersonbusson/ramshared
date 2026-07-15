//! Bounded CUDA three-offset probe (SPEC DT-3 / ITEM-2).
//!
//! Uses `ramshared-cuda` (nvcuda.dll on Windows; libcuda on Linux/WSL). Live
//! hardware path is E2E evidence; pure offset planning lives in `ramshared_cuda::probe`.

use crate::config::WinDriveConfig;
use ramshared_cuda::Cuda;
use ramshared_cuda::probe::{pattern_for_offset, plan_probe_offsets};

/// Result of a successful probe-cuda run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeCudaReport {
    pub ordinal: i32,
    pub device_name: String,
    pub size_bytes: u64,
    pub free_before: u64,
    pub free_after: u64,
    pub allocated: u64,
    pub offsets: [usize; 3],
}

/// Errors from probe-cuda (stable classes, no pointers).
#[derive(Debug)]
pub enum ProbeCudaError {
    Config(String),
    Cuda(String),
    Mismatch { offset: usize },
    FreeRestore { delta: u64 },
    Capacity { free: u64, need: u64 },
}

impl std::fmt::Display for ProbeCudaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProbeCudaError::Config(s) => write!(f, "config: {s}"),
            ProbeCudaError::Cuda(s) => write!(f, "cuda: {s}"),
            ProbeCudaError::Mismatch { offset } => write!(f, "pattern mismatch at {offset}"),
            ProbeCudaError::FreeRestore { delta } => {
                write!(f, "free restoration outside 64 MiB: delta={delta}")
            }
            ProbeCudaError::Capacity { free, need } => {
                write!(f, "free {free} < size+reserve {need}")
            }
        }
    }
}

impl std::error::Error for ProbeCudaError {}

/// Allocate, three-offset roundtrip, zero, free, recheck capacity (DT-3).
pub fn probe_cuda_allocates_roundtrips_and_restores(
    cfg: &WinDriveConfig,
) -> Result<ProbeCudaReport, ProbeCudaError> {
    cfg.validate()
        .map_err(|e| ProbeCudaError::Config(e.to_string()))?;

    let cuda = Cuda::load().map_err(|e| ProbeCudaError::Cuda(e.to_string()))?;
    let count = cuda
        .device_count()
        .map_err(|e| ProbeCudaError::Cuda(e.to_string()))?;
    if cfg.cuda_device as i32 >= count {
        return Err(ProbeCudaError::Cuda(format!(
            "cuda_device {} >= count {count}",
            cfg.cuda_device
        )));
    }
    let dev = cuda
        .device(cfg.cuda_device as i32)
        .map_err(|e| ProbeCudaError::Cuda(e.to_string()))?;
    let ctx = cuda
        .create_context(&dev)
        .map_err(|e| ProbeCudaError::Cuda(e.to_string()))?;
    let (free, total) = ctx
        .mem_info()
        .map_err(|e| ProbeCudaError::Cuda(e.to_string()))?;
    let reserve = cfg.effective_reserve_bytes(total as u64) as usize;
    let need = (cfg.size_bytes as usize)
        .checked_add(reserve)
        .ok_or_else(|| ProbeCudaError::Config("size+reserve overflow".into()))?;
    if free < need {
        return Err(ProbeCudaError::Capacity {
            free: free as u64,
            need: need as u64,
        });
    }

    let size = cfg.size_bytes as usize;
    let mut mem = ctx
        .alloc(size)
        .map_err(|e| ProbeCudaError::Cuda(e.to_string()))?;
    mem.zero()
        .map_err(|e| ProbeCudaError::Cuda(e.to_string()))?;

    let offsets = plan_probe_offsets(size).map_err(|e| ProbeCudaError::Config(e.to_string()))?;
    for &off in &offsets {
        let pat = pattern_for_offset(off);
        mem.write_at(off, &pat)
            .map_err(|e| ProbeCudaError::Cuda(e.to_string()))?;
        let mut got = vec![0u8; 4096];
        mem.read_at(off, &mut got)
            .map_err(|e| ProbeCudaError::Cuda(e.to_string()))?;
        if got != pat {
            return Err(ProbeCudaError::Mismatch { offset: off });
        }
    }

    mem.zero()
        .map_err(|e| ProbeCudaError::Cuda(e.to_string()))?;
    drop(mem);

    let (free_after, _) = ctx
        .mem_info()
        .map_err(|e| ProbeCudaError::Cuda(e.to_string()))?;
    let delta = free_after.abs_diff(free);
    if delta > 64 * 1024 * 1024 {
        return Err(ProbeCudaError::FreeRestore {
            delta: delta as u64,
        });
    }

    Ok(ProbeCudaReport {
        ordinal: dev.ordinal(),
        device_name: dev.name().to_string(),
        size_bytes: cfg.size_bytes,
        free_before: free as u64,
        free_after: free_after as u64,
        allocated: cfg.size_bytes,
        offsets,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use std::path::PathBuf;

    fn cfg_64m() -> WinDriveConfig {
        WinDriveConfig {
            size_bytes: 64 * 1024 * 1024,
            block_size: 4096,
            cuda_device: 0,
            reserve_bytes: 512 * 1024 * 1024,
            queue_depth: 4,
            max_io_bytes: 1024 * 1024,
            evidence_path: PathBuf::from(r"C:\ProgramData\RamShared\evidence"),
            volume_letter: 'D',
            broker: "127.0.0.1:7700".into(),
            tenant: "probe".into(),
            heartbeat_secs: 5,
        }
    }

    /// Live three-offset CUDA probe (SPEC matrix name).
    ///
    /// Run: `cargo test -p ramshared-winsvc probe_cuda_allocates_roundtrips_and_restores -- --ignored --nocapture`
    #[test]
    #[ignore = "requires functional CUDA GPU (WSL2 GPU-PV or Windows nvcuda)"]
    fn probe_cuda_allocates_roundtrips_and_restores() {
        let cfg = cfg_64m();
        // Prefer small allocation for lab headroom when free is tight: re-validate
        // against actual free by letting the function fail closed on capacity.
        let report = super::probe_cuda_allocates_roundtrips_and_restores(&cfg)
            .expect("probe must pass on GPU host");
        assert_eq!(report.size_bytes, 64 * 1024 * 1024);
        assert_eq!(report.offsets[0], 0);
        assert!(report.free_after.abs_diff(report.free_before) <= 64 * 1024 * 1024);
        eprintln!(
            "PROBE_OK ordinal={} name={} free_before={} free_after={}",
            report.ordinal, report.device_name, report.free_before, report.free_after
        );
    }
}
