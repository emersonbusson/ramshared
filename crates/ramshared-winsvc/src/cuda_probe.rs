//! Bounded CUDA three-offset probe (SPEC DT-3 / ITEM-2).
//!
//! Uses `ramshared-cuda` (nvcuda.dll on Windows; libcuda on Linux/WSL). Live
//! hardware path is E2E evidence; pure offset planning lives in `ramshared_cuda::probe`.

use crate::config::WinDriveConfig;
use ramshared_cuda::probe::{pattern_for_offset, plan_probe_offsets};

/// Trait to allow mocking Cuda API in tests.
pub trait CudaApi: Sized {
    type Device: CudaDeviceApi;
    type Context<'a>: CudaContextApi<'a> where Self: 'a;

    fn load() -> Result<Self, String>;
    fn device_count(&self) -> Result<i32, String>;
    fn device(&self, ordinal: i32) -> Result<Self::Device, String>;
    fn create_context<'a>(&'a self, device: &Self::Device) -> Result<Self::Context<'a>, String>;
}

pub trait CudaDeviceApi {
    fn ordinal(&self) -> i32;
    fn name(&self) -> &str;
}

pub trait CudaContextApi<'a> {
    type Mem<'c>: CudaMemApi where Self: 'c, 'a: 'c;
    fn mem_info(&self) -> Result<(usize, usize), String>;
    fn alloc<'c>(&'c self, bytes: usize) -> Result<Self::Mem<'c>, String> where 'a: 'c;
}

pub trait CudaMemApi {
    fn zero(&mut self) -> Result<(), String>;
    fn write_at(&mut self, off: usize, src: &[u8]) -> Result<(), String>;
    fn read_at(&self, off: usize, dst: &mut [u8]) -> Result<(), String>;
}

/// The actual Cuda API implementation wrapping `ramshared_cuda::Cuda`.
pub struct RealCuda(ramshared_cuda::Cuda);
pub struct RealCudaDevice(ramshared_cuda::Device);
pub struct RealCudaContext<'a>(ramshared_cuda::Context<'a>);
pub struct RealCudaMem<'c, 'a>(ramshared_cuda::DeviceMem<'c, 'a>);

impl CudaApi for RealCuda {
    type Device = RealCudaDevice;
    type Context<'a> = RealCudaContext<'a>;

    fn load() -> Result<Self, String> {
        ramshared_cuda::Cuda::load()
            .map(RealCuda)
            .map_err(|e| e.to_string())
    }

    fn device_count(&self) -> Result<i32, String> {
        self.0.device_count().map_err(|e| e.to_string())
    }

    fn device(&self, ordinal: i32) -> Result<Self::Device, String> {
        self.0.device(ordinal).map(RealCudaDevice).map_err(|e| e.to_string())
    }

    fn create_context<'a>(&'a self, device: &Self::Device) -> Result<Self::Context<'a>, String> {
        self.0.create_context(&device.0).map(RealCudaContext).map_err(|e| e.to_string())
    }
}

impl CudaDeviceApi for RealCudaDevice {
    fn ordinal(&self) -> i32 {
        self.0.ordinal()
    }

    fn name(&self) -> &str {
        self.0.name()
    }
}

impl<'a> CudaContextApi<'a> for RealCudaContext<'a> {
    type Mem<'c> = RealCudaMem<'c, 'a> where Self: 'c, 'a: 'c;

    fn mem_info(&self) -> Result<(usize, usize), String> {
        self.0.mem_info().map_err(|e| e.to_string())
    }

    fn alloc<'c>(&'c self, bytes: usize) -> Result<Self::Mem<'c>, String> where 'a: 'c {
        self.0.alloc(bytes).map(RealCudaMem).map_err(|e| e.to_string())
    }
}

impl CudaMemApi for RealCudaMem<'_, '_> {
    fn zero(&mut self) -> Result<(), String> {
        self.0.zero().map_err(|e| e.to_string())
    }

    fn write_at(&mut self, off: usize, src: &[u8]) -> Result<(), String> {
        self.0.write_at(off, src).map_err(|e| e.to_string())
    }

    fn read_at(&self, off: usize, dst: &mut [u8]) -> Result<(), String> {
        self.0.read_at(off, dst).map_err(|e| e.to_string())
    }
}

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
    probe_cuda_internal::<RealCuda>(cfg)
}

fn probe_cuda_internal<C: CudaApi>(
    cfg: &WinDriveConfig,
) -> Result<ProbeCudaReport, ProbeCudaError> {
    cfg.validate()
        .map_err(|e| ProbeCudaError::Config(e.to_string()))?;

    let cuda = C::load().map_err(ProbeCudaError::Cuda)?;
    let count = cuda
        .device_count()
        .map_err(ProbeCudaError::Cuda)?;
    if cfg.cuda_device as i32 >= count {
        return Err(ProbeCudaError::Cuda(format!(
            "cuda_device {} >= count {count}",
            cfg.cuda_device
        )));
    }
    let dev = cuda
        .device(cfg.cuda_device as i32)
        .map_err(ProbeCudaError::Cuda)?;
    let ctx = cuda
        .create_context(&dev)
        .map_err(ProbeCudaError::Cuda)?;
    let (free, total) = ctx
        .mem_info()
        .map_err(ProbeCudaError::Cuda)?;
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
        .map_err(ProbeCudaError::Cuda)?;
    mem.zero()
        .map_err(ProbeCudaError::Cuda)?;

    let offsets = plan_probe_offsets(size).map_err(|e| ProbeCudaError::Config(e.to_string()))?;
    for &off in &offsets {
        let pat = pattern_for_offset(off);
        mem.write_at(off, &pat)
            .map_err(ProbeCudaError::Cuda)?;
        let mut got = vec![0u8; 4096];
        mem.read_at(off, &mut got)
            .map_err(ProbeCudaError::Cuda)?;
        if got != pat {
            return Err(ProbeCudaError::Mismatch { offset: off });
        }
    }

    mem.zero()
        .map_err(ProbeCudaError::Cuda)?;
    drop(mem);

    let (free_after, _) = ctx
        .mem_info()
        .map_err(ProbeCudaError::Cuda)?;
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
    use std::cell::RefCell;

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
            volume_mount_path: None,
            broker_pipe: crate::config::BrokerPipeV1::NamedPipeV1,
            broker_ready_timeout_secs: 30,
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

    thread_local! {
        static MOCK_DEVICE_COUNT: RefCell<i32> = const { RefCell::new(1) };
        static MOCK_TOTAL_MEM: RefCell<usize> = const { RefCell::new(1024 * 1024 * 1024) };
        static MOCK_FREE_MEM: RefCell<usize> = const { RefCell::new(1024 * 1024 * 1024) };
        static MOCK_MEM_WRITE: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
        static MOCK_MEM_OFFSET: RefCell<usize> = const { RefCell::new(0) };
    }

    struct MockCuda;
    struct MockCudaDevice(i32);
    struct MockCudaContext;
    struct MockCudaMem {
        size: usize,
    }

    impl CudaApi for MockCuda {
        type Device = MockCudaDevice;
        type Context<'a> = MockCudaContext;

        fn load() -> Result<Self, String> { Ok(MockCuda) }
        fn device_count(&self) -> Result<i32, String> { Ok(MOCK_DEVICE_COUNT.with(|c| *c.borrow())) }
        fn device(&self, ordinal: i32) -> Result<Self::Device, String> {
            let count = MOCK_DEVICE_COUNT.with(|c| *c.borrow());
            if ordinal >= count { Err(format!("invalid ordinal {ordinal}")) } else { Ok(MockCudaDevice(ordinal)) }
        }
        fn create_context<'a>(&'a self, _device: &Self::Device) -> Result<Self::Context<'a>, String> { Ok(MockCudaContext) }
    }

    impl CudaDeviceApi for MockCudaDevice {
        fn ordinal(&self) -> i32 { self.0 }
        fn name(&self) -> &str { "Mock GPU" }
    }

    impl<'a> CudaContextApi<'a> for MockCudaContext {
        type Mem<'c> = MockCudaMem where Self: 'c, 'a: 'c;

        fn mem_info(&self) -> Result<(usize, usize), String> {
            let free = MOCK_FREE_MEM.with(|m| *m.borrow());
            let total = MOCK_TOTAL_MEM.with(|m| *m.borrow());
            Ok((free, total))
        }

        fn alloc<'c>(&'c self, bytes: usize) -> Result<Self::Mem<'c>, String> where 'a: 'c {
            MOCK_FREE_MEM.with(|m| {
                let mut free = m.borrow_mut();
                if bytes > *free { return Err("out of memory".to_string()); }
                *free -= bytes;
                Ok(MockCudaMem { size: bytes })
            })
        }
    }

    impl Drop for MockCudaMem {
        fn drop(&mut self) {
            // Restore memory on drop to simulate free
            MOCK_FREE_MEM.with(|m| {
                let mut free = m.borrow_mut();
                *free += self.size;
            });
        }
    }

    impl CudaMemApi for MockCudaMem {
        fn zero(&mut self) -> Result<(), String> { Ok(()) }
        fn write_at(&mut self, off: usize, src: &[u8]) -> Result<(), String> {
            MOCK_MEM_OFFSET.with(|o| *o.borrow_mut() = off);
            MOCK_MEM_WRITE.with(|w| {
                let mut w = w.borrow_mut();
                w.clear();
                w.extend_from_slice(src);
            });
            Ok(())
        }
        fn read_at(&self, off: usize, dst: &mut [u8]) -> Result<(), String> {
            let written_off = MOCK_MEM_OFFSET.with(|o| *o.borrow());
            let w = MOCK_MEM_WRITE.with(|w| w.borrow().clone());
            if off == written_off && dst.len() <= w.len() {
                dst.copy_from_slice(&w[..dst.len()]);
            }
            Ok(())
        }
    }

    fn reset_mock() {
        MOCK_DEVICE_COUNT.with(|c| *c.borrow_mut() = 1);
        MOCK_TOTAL_MEM.with(|c| *c.borrow_mut() = 1024 * 1024 * 1024);
        MOCK_FREE_MEM.with(|c| *c.borrow_mut() = 1024 * 1024 * 1024);
    }

    #[test]
    fn test_cuda_probe_valid_allocation() {
        reset_mock();
        let cfg = cfg_64m();
        let report = super::probe_cuda_internal::<MockCuda>(&cfg).unwrap();
        assert_eq!(report.device_name, "Mock GPU");
        assert_eq!(report.size_bytes, 64 * 1024 * 1024);
        assert_eq!(report.allocated, 64 * 1024 * 1024);
    }

    #[test]
    fn test_cuda_probe_zero_capacity_error() {
        reset_mock();
        MOCK_FREE_MEM.with(|c| *c.borrow_mut() = 1024); // Not enough memory
        let cfg = cfg_64m();
        let err = super::probe_cuda_internal::<MockCuda>(&cfg).unwrap_err();
        assert!(matches!(err, ProbeCudaError::Capacity { .. }));
    }

    #[test]
    fn test_cuda_probe_invalid_device_index() {
        reset_mock();
        let mut cfg = cfg_64m();
        cfg.cuda_device = 5; // We only have 1 device by default
        let err = super::probe_cuda_internal::<MockCuda>(&cfg).unwrap_err();
        match err {
            ProbeCudaError::Cuda(msg) => assert!(msg.contains("cuda_device 5 >= count 1")),
            _ => panic!("Expected Cuda error"),
        }
    }

    #[test]
    fn test_cuda_probe_multiple_devices_select_second() {
        reset_mock();
        MOCK_DEVICE_COUNT.with(|c| *c.borrow_mut() = 3);
        let mut cfg = cfg_64m();
        cfg.cuda_device = 1;
        let report = super::probe_cuda_internal::<MockCuda>(&cfg).unwrap();
        assert_eq!(report.ordinal, 1);
    }
}
