//! `VramBackend` — liga `ramshared_cuda::DeviceMem` à trait `BlockBackend` do
//! `ramshared_block`. É o ponto onde "VRAM CUDA" vira "block device NBD" (SPEC §8).

use ramshared_block::{BlockBackend, IoError};
use ramshared_cuda::DeviceMem;

/// Block device respaldado por uma região de VRAM CUDA.
pub struct VramBackend<'c, 'a> {
    mem: DeviceMem<'c, 'a>,
    block_size: u32,
}

impl<'c, 'a> VramBackend<'c, 'a> {
    pub fn new(mem: DeviceMem<'c, 'a>, block_size: u32) -> Self {
        Self { mem, block_size }
    }
}

impl BlockBackend for VramBackend<'_, '_> {
    fn size_bytes(&self) -> u64 {
        self.mem.len() as u64
    }

    fn block_size(&self) -> u32 {
        self.block_size
    }

    fn read_at(&self, off: u64, buf: &mut [u8]) -> Result<(), IoError> {
        self.mem
            .read_at(off as usize, buf)
            .map_err(|e| IoError(e.to_string()))
    }

    fn write_at(&mut self, off: u64, data: &[u8]) -> Result<(), IoError> {
        self.mem
            .write_at(off as usize, data)
            .map_err(|e| IoError(e.to_string()))
    }

    fn flush(&mut self) -> Result<(), IoError> {
        // cuMemcpy*_v2 são síncronas (a referência usa o mesmo modelo); nada a drenar.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use ramshared_block::{Command, Request, serve};
    use ramshared_cuda::Cuda;

    /// Composição cuda + block em VRAM real: serve um WRITE e um READ NBD.
    /// `cargo test -p ramshared-wsl2d -- --ignored` num host com GPU.
    #[test]
    #[ignore = "requer GPU CUDA funcional (WSL2/GPU-PV)"]
    fn vram_backend_serves_nbd_write_then_read() {
        let cuda = Cuda::load().expect("libcuda");
        let dev = cuda.device(0).unwrap();
        let ctx = cuda.create_context(&dev).unwrap();
        let mut mem = ctx.alloc(1 << 20).unwrap();
        mem.zero().unwrap();
        let mut be = VramBackend::new(mem, 4096);

        let payload = vec![0x5Au8; 4096];
        let w = serve(
            &Request {
                flags: 0,
                cmd: Command::Write,
                handle: 1,
                offset: 4096,
                len: 4096,
            },
            &payload,
            &mut be,
        );
        assert_eq!(
            u32::from_be_bytes([w.reply[4], w.reply[5], w.reply[6], w.reply[7]]),
            0,
            "WRITE deve dar NBD_OK"
        );

        let r = serve(
            &Request {
                flags: 0,
                cmd: Command::Read,
                handle: 2,
                offset: 4096,
                len: 4096,
            },
            &[],
            &mut be,
        );
        assert_eq!(r.read_data, payload, "READ deve devolver o que foi escrito");
    }
}
