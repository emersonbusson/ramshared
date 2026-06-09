//! Wrappers seguros sobre a crate `io-uring` para a Fase B.
//!
//! O daemon `ramshared-wsl2d` fica com `#![forbid(unsafe_code)]`. Operações reais de
//! SQE que exigirem `unsafe` entram neste crate, com invariantes documentadas no
//! menor escopo possível.

#![deny(unsafe_op_in_unsafe_fn)]

use std::ffi::c_void;
use std::io;
use std::os::fd::RawFd;
use std::ptr;
use std::slice;

use io_uring::{IoUring, opcode, squeue, types};

/// Tamanho de página do sistema (`sysconf(_SC_PAGESIZE)`), com fallback 4096.
pub fn page_size() -> usize {
    // SAFETY: `sysconf` com `_SC_PAGESIZE` nao tem efeito colateral e e sempre
    // seguro de chamar; em Linux retorna um valor > 0.
    let value = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if value > 0 { value as usize } else { 4096 }
}

/// Arredonda `n` para cima ao múltiplo de página, como o `round_up(.., PAGE_SIZE)`
/// que o driver ublk usa para dimensionar o buffer de comandos por fila.
pub fn round_up_to_page(n: usize) -> usize {
    let page = page_size();
    n.div_ceil(page) * page
}

/// Mapa `mmap` somente leitura com `munmap` automático (RAII). Usado para o buffer
/// de io-desc de `/dev/ublkcN`, que o kernel expõe read-only (`VM_WRITE` -> `-EPERM`).
pub struct MmapRo {
    ptr: *mut c_void,
    len: usize,
}

impl MmapRo {
    /// Mapeia `len` bytes do `fd` em `offset`, com `PROT_READ`/`MAP_SHARED`.
    pub fn map_readonly(fd: RawFd, len: usize, offset: i64) -> io::Result<Self> {
        if len == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "mmap len must be > 0",
            ));
        }

        // SAFETY: `addr` nulo deixa o kernel escolher o endereco; mapeamos apenas
        // `PROT_READ` sobre o `fd` do char device ublk. O retorno e validado contra
        // `MAP_FAILED` logo abaixo; em sucesso o ponteiro cobre `len` bytes legiveis
        // ate o `munmap` no `Drop`.
        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                len,
                libc::PROT_READ,
                libc::MAP_SHARED,
                fd,
                offset,
            )
        };

        if ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }

        Ok(Self { ptr, len })
    }

    /// Bytes mapeados (somente leitura).
    pub fn as_bytes(&self) -> &[u8] {
        // SAFETY: `ptr` veio de um `mmap` bem-sucedido de `len` bytes legiveis
        // (`PROT_READ`) e segue mapeado enquanto `self` vive (`munmap` so no `Drop`).
        unsafe { slice::from_raw_parts(self.ptr.cast::<u8>(), self.len) }
    }
}

impl Drop for MmapRo {
    fn drop(&mut self) {
        // SAFETY: `ptr`/`len` vieram de um `mmap` bem-sucedido e ainda nao foram
        // desmapeados; `munmap` e chamado exatamente uma vez nesta queda.
        unsafe {
            libc::munmap(self.ptr, self.len);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SmokeReport {
    pub entries: u32,
    pub submitted: usize,
}

pub fn smoke(entries: u32) -> io::Result<SmokeReport> {
    let ring = io_uring::IoUring::new(entries)?;
    let submitted = ring.submit()?;

    Ok(SmokeReport { entries, submitted })
}

pub fn ublk_get_features(fd: RawFd) -> io::Result<u64> {
    const UBLK_U_CMD_GET_FEATURES: u32 = 0x8020_7513;
    const UBLK_FEATURES_LEN: u16 = 8;

    let mut features = 0u64;
    let cmd = ctrl_cmd(0, UBLK_FEATURES_LEN, (&mut features as *mut u64) as u64);

    let res = submit_uring_cmd80(fd, UBLK_U_CMD_GET_FEATURES, cmd)?;
    if res != 0 {
        return Err(io::Error::other(format!(
            "ublk GET_FEATURES returned unexpected result {res}"
        )));
    }

    Ok(features)
}

pub fn ublk_add_dev(fd: RawFd, dev_id: u32, info: &mut [u8; 64]) -> io::Result<()> {
    const UBLK_U_CMD_ADD_DEV: u32 = 0xc020_7504;
    const UBLK_CTRL_DEV_INFO_LEN: u16 = 64;

    let cmd = ctrl_cmd(dev_id, UBLK_CTRL_DEV_INFO_LEN, info.as_mut_ptr() as u64);
    expect_zero(
        submit_uring_cmd80(fd, UBLK_U_CMD_ADD_DEV, cmd)?,
        "ublk ADD_DEV",
    )
}

pub fn ublk_del_dev(fd: RawFd, dev_id: u32) -> io::Result<()> {
    const UBLK_U_CMD_DEL_DEV: u32 = 0xc020_7505;

    let cmd = ctrl_cmd(dev_id, 0, 0);
    expect_zero(
        submit_uring_cmd80(fd, UBLK_U_CMD_DEL_DEV, cmd)?,
        "ublk DEL_DEV",
    )
}

fn ctrl_cmd(dev_id: u32, len: u16, addr: u64) -> [u8; 80] {
    const UBLK_QUEUE_ID_NONE: u16 = u16::MAX;

    let mut cmd = [0u8; 80];
    cmd[0..4].copy_from_slice(&dev_id.to_ne_bytes());
    cmd[4..6].copy_from_slice(&UBLK_QUEUE_ID_NONE.to_ne_bytes());
    cmd[6..8].copy_from_slice(&len.to_ne_bytes());
    cmd[8..16].copy_from_slice(&addr.to_ne_bytes());
    cmd
}

fn expect_zero(result: i32, context: &str) -> io::Result<()> {
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{context} returned unexpected result {result}"
        )))
    }
}

fn submit_uring_cmd80(fd: RawFd, cmd_op: u32, cmd: [u8; 80]) -> io::Result<i32> {
    let mut ring = IoUring::<squeue::Entry128>::builder().build(2)?;
    let entry = opcode::UringCmd80::new(types::Fd(fd), cmd_op)
        .cmd(cmd)
        .build()
        .user_data(1);

    {
        let mut sq = ring.submission();
        // SAFETY: `cmd` e copiado para a SQE antes da submissao. Os wrappers
        // publicos deste modulo usam ponteiro nulo, ponteiro de stack local ou
        // buffer mutavel emprestado, e esta funcao espera o CQE antes de
        // retornar ao chamador.
        unsafe {
            sq.push(&entry)
                .map_err(|_| io::Error::other("io_uring submission queue is full"))?;
        }
    }

    ring.submit_and_wait(1)?;

    let cqe = ring
        .completion()
        .next()
        .ok_or_else(|| io::Error::other("io_uring completion queue is empty"))?;
    let result = cqe.result();
    if result < 0 {
        Err(io::Error::from_raw_os_error(-result))
    } else {
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_cmd80_packs_ublksrv_io_cmd_in_first_16_bytes() {
        let cmd = fetch_cmd80(0, 7, 0xdead_beef);

        assert_eq!(u16::from_ne_bytes([cmd[0], cmd[1]]), 0);
        assert_eq!(u16::from_ne_bytes([cmd[2], cmd[3]]), 7);
        assert_eq!(i32::from_ne_bytes([cmd[4], cmd[5], cmd[6], cmd[7]]), 0);
        assert_eq!(
            u64::from_ne_bytes([
                cmd[8], cmd[9], cmd[10], cmd[11], cmd[12], cmd[13], cmd[14], cmd[15],
            ]),
            0xdead_beef
        );
        assert!(cmd[16..].iter().all(|&b| b == 0));
    }
}
