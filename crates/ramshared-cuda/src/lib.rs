//! ramshared-cuda — wrapper seguro sobre a CUDA Driver API (`libcuda`) carregada
//! em runtime, para o tier VRAM da cascata (SPECv3-WSL2.md §4, §8).
//!
//! Lib reutilizável: `Cuda::load()` → `device()` → `create_context()` → `alloc()`
//! com cópias Host↔Device síncronas e liberação por RAII. Portado do design da
//! referência `c0deJedi/nbd-vram` (C, MIT), não copiado — ver SPECv3 §0.2.
//!
//! ```no_run
//! use ramshared_cuda::Cuda;
//! let cuda = Cuda::load()?;
//! let dev = cuda.device(0)?;
//! let ctx = cuda.create_context(&dev)?;
//! let (free, total) = ctx.mem_info()?;
//! let mut mem = ctx.alloc(256 * 1024 * 1024)?; // 256 MiB de VRAM
//! mem.zero()?;
//! mem.write_at(0, b"ping")?;
//! let mut out = [0u8; 4];
//! mem.read_at(0, &mut out)?;
//! assert_eq!(&out, b"ping");
//! # Ok::<(), ramshared_cuda::CudaError>(())
//! ```

#[cfg(unix)]
mod loader_unix;
#[cfg(unix)]
use loader_unix as loader;

#[cfg(windows)]
mod loader_win;
#[cfg(windows)]
use loader_win as loader;

mod driver;
mod ffi;
mod vram_impl; // impl VramProvider/VramMemory p/ os tipos CUDA (RF-G1)

pub use driver::{Context, Cuda, CudaError, Device, DeviceMem};

#[cfg(test)]
mod tests {
    // unwrap/expect liberados só em teste (regra coding.md), apesar do deny no crate.
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn error_display_is_descriptive() {
        let e = CudaError::OutOfRange {
            off: 4096,
            len: 8192,
            size: 8192,
        };
        let s = e.to_string();
        assert!(s.contains("off=4096"));
        assert!(s.contains("size=8192"));
    }

    #[test]
    fn driver_error_carries_op_and_code() {
        let e = CudaError::Driver {
            op: "cuMemAlloc",
            code: 2,
            msg: "out of memory".to_string(),
        };
        let s = e.to_string();
        assert!(s.contains("cuMemAlloc"));
        assert!(s.contains("CUresult=2"));
    }

    /// Roundtrip real Host→VRAM→Host. Exige GPU CUDA funcional (WSL2/GPU-PV).
    /// Rodar com: `cargo test -p ramshared-cuda -- --ignored`
    #[test]
    #[ignore = "requer GPU CUDA funcional (rodar com --ignored num host com GPU)"]
    fn gpu_roundtrip_256mib() {
        let cuda = Cuda::load().expect("libcuda deve carregar");
        assert!(cuda.device_count().unwrap() >= 1);
        let dev = cuda.device(0).unwrap();
        let ctx = cuda.create_context(&dev).unwrap();

        let (free_before, total) = ctx.mem_info().unwrap();
        assert!(total > 0 && free_before > 0);

        let size = 256 * 1024 * 1024;
        let mut mem = ctx.alloc(size).unwrap();
        mem.zero().unwrap();

        // padrão conhecido em três offsets
        let pat: Vec<u8> = (0..4096).map(|i| (i % 251) as u8).collect();
        for off in [0usize, size / 2, size - pat.len()] {
            mem.write_at(off, &pat).unwrap();
            let mut out = vec![0u8; pat.len()];
            mem.read_at(off, &mut out).unwrap();
            assert_eq!(out, pat, "roundtrip divergiu em off={off}");
        }

        // fora da faixa é erro, não corrupção
        let mut tiny = [0u8; 16];
        assert!(matches!(
            mem.read_at(size - 8, &mut tiny),
            Err(CudaError::OutOfRange { .. })
        ));
    }
}
