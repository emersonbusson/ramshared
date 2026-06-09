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

// SAFETY: `MmapRo` e dono exclusivo de um mapeamento `mmap` valido em todo o
// processo; mover a posse entre threads e seguro. Nao implementa `Sync` (sem
// acesso compartilhado concorrente).
unsafe impl Send for MmapRo {}

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

/// `SET_PARAMS`: envia `struct ublk_params` (112 B) para o device `dev_id`.
pub fn ublk_set_params(fd: RawFd, dev_id: u32, params: &mut [u8; 112]) -> io::Result<()> {
    const UBLK_U_CMD_SET_PARAMS: u32 = 0xc020_7508;

    let cmd = ctrl_cmd(dev_id, 112, params.as_mut_ptr() as u64);
    expect_zero(
        submit_uring_cmd80(fd, UBLK_U_CMD_SET_PARAMS, cmd)?,
        "ublk SET_PARAMS",
    )
}

/// `GET_PARAMS`: o kernel preenche `struct ublk_params` (112 B) do device `dev_id`.
pub fn ublk_get_params(fd: RawFd, dev_id: u32, params: &mut [u8; 112]) -> io::Result<()> {
    const UBLK_U_CMD_GET_PARAMS: u32 = 0x8020_7509;

    let cmd = ctrl_cmd(dev_id, 112, params.as_mut_ptr() as u64);
    expect_zero(
        submit_uring_cmd80(fd, UBLK_U_CMD_GET_PARAMS, cmd)?,
        "ublk GET_PARAMS",
    )
}

/// `START_DEV`: cria `/dev/ublkbN` (bloqueia até as filas ready e o `add_disk`). O
/// `ublksrv_pid` vai em `data[0]` do `ublksrv_ctrl_cmd` (offset 16).
pub fn ublk_start_dev(fd: RawFd, dev_id: u32, ublksrv_pid: u32) -> io::Result<()> {
    const UBLK_U_CMD_START_DEV: u32 = 0xc020_7506;

    let mut cmd = ctrl_cmd(dev_id, 0, 0);
    cmd[16..24].copy_from_slice(&u64::from(ublksrv_pid).to_ne_bytes());
    expect_zero(
        submit_uring_cmd80(fd, UBLK_U_CMD_START_DEV, cmd)?,
        "ublk START_DEV",
    )
}

/// `STOP_DEV`: remove `/dev/ublkbN` e aborta os FETCH pendentes.
pub fn ublk_stop_dev(fd: RawFd, dev_id: u32) -> io::Result<()> {
    const UBLK_U_CMD_STOP_DEV: u32 = 0xc020_7507;

    let cmd = ctrl_cmd(dev_id, 0, 0);
    expect_zero(
        submit_uring_cmd80(fd, UBLK_U_CMD_STOP_DEV, cmd)?,
        "ublk STOP_DEV",
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

/// Conclusão de um comando ublk no ring (CQE): `tag` (do `user_data`) e `result`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UblkCompletion {
    pub tag: u16,
    pub result: i32,
}

/// Ring io_uring persistente que submete `UBLK_U_IO_FETCH_REQ` para as tags de uma
/// fila ublk **sem esperar CQE** (o driver estaciona cada comando em `-EIOCBQUEUED`
/// até haver I/O ou abort). É dono dos buffers de dados enquanto os FETCH pendem.
pub struct UblkFetchRing {
    ring: IoUring<squeue::Entry128>,
    /// Buffers de dados por tag: o `addr` de cada FETCH aponta para o respectivo
    /// buffer, que precisa permanecer vivo enquanto o comando está estacionado.
    /// Não é lido diretamente; existe só para garantir o lifetime (drop guard).
    #[allow(dead_code)]
    buffers: Vec<Vec<u8>>,
}

impl UblkFetchRing {
    /// Submete `FETCH_REQ` para as tags `[0, queue_depth)` da fila 0 do `fd`, cada
    /// uma com um buffer de `buf_size` bytes. Não espera CQE (`submit()`/want=0). O
    /// `fd` deve permanecer aberto pelo chamador enquanto este ring existir.
    pub fn submit_fetch_all(fd: RawFd, queue_depth: u16, buf_size: usize) -> io::Result<Self> {
        const UBLK_U_IO_FETCH_REQ: u32 = 0xc010_7520;
        const QUEUE_ID_ZERO: u16 = 0;

        let entries = u32::from(queue_depth).max(1).next_power_of_two();
        let mut ring = IoUring::<squeue::Entry128>::builder().build(entries)?;
        let mut buffers: Vec<Vec<u8>> = (0..queue_depth).map(|_| vec![0u8; buf_size]).collect();

        for tag in 0..queue_depth {
            let addr = buffers[usize::from(tag)].as_mut_ptr() as u64;
            let cmd = fetch_cmd80(QUEUE_ID_ZERO, tag, addr);
            let entry = opcode::UringCmd80::new(types::Fd(fd), UBLK_U_IO_FETCH_REQ)
                .cmd(cmd)
                .build()
                .user_data(u64::from(tag));

            // SAFETY: `cmd` (incluindo `addr`) é copiado para a SQE no `push`. O
            // `addr` aponta para `buffers[tag]`, que vive dentro deste struct
            // enquanto os FETCH estão estacionados; o kernel só tocaria o buffer ao
            // servir I/O, que exige `START_DEV` (não chamado neste caminho).
            unsafe {
                ring.submission()
                    .push(&entry)
                    .map_err(|_| io::Error::other("io_uring submission queue is full"))?;
            }
        }

        // Não bloqueia (want=0); os FETCH ficam pendentes no driver.
        ring.submit()?;

        Ok(Self { ring, buffers })
    }

    /// Drena os CQEs disponíveis no momento, sem bloquear.
    pub fn drain(&mut self) -> Vec<UblkCompletion> {
        self.ring
            .completion()
            .map(|cqe| UblkCompletion {
                tag: cqe.user_data() as u16,
                result: cqe.result(),
            })
            .collect()
    }
}

/// Empacota um `struct ublksrv_io_cmd` (16 B: q_id, tag, result, addr) nos
/// primeiros bytes do buffer de 80 B da SQE `UringCmd80`; o restante fica zerado.
fn io_cmd80(q_id: u16, tag: u16, result: i32, addr: u64) -> [u8; 80] {
    let mut cmd = [0u8; 80];
    cmd[0..2].copy_from_slice(&q_id.to_ne_bytes());
    cmd[2..4].copy_from_slice(&tag.to_ne_bytes());
    cmd[4..8].copy_from_slice(&result.to_ne_bytes());
    cmd[8..16].copy_from_slice(&addr.to_ne_bytes());
    cmd
}

/// `ublksrv_io_cmd` de `FETCH_REQ` (result zerado no envio).
fn fetch_cmd80(q_id: u16, tag: u16, addr: u64) -> [u8; 80] {
    io_cmd80(q_id, tag, 0, addr)
}

/// Servidor de uma fila ublk: ring `Entry128` persistente + `mmap` read-only do
/// buffer de io-desc + buffers de dados por tag. Submete `FETCH_REQ`, expõe os
/// descritores de request e completa via `COMMIT_AND_FETCH_REQ`. O `fd` do char
/// device deve permanecer aberto pelo dono enquanto o servidor vive.
pub struct UblkServer {
    fd: RawFd,
    ring: IoUring<squeue::Entry128>,
    iodesc: MmapRo,
    buffers: Vec<Vec<u8>>,
    queue_depth: u16,
}

impl UblkServer {
    /// Tamanho de `struct ublksrv_io_desc` (espelha `ublk::UBLK_IO_DESC_SIZE`).
    const IO_DESC_SIZE: usize = 24;

    /// Cria o ring e mapeia o io-desc da fila 0; NÃO submete FETCH ainda.
    pub fn new(fd: RawFd, queue_depth: u16, buf_size: usize) -> io::Result<Self> {
        let entries = u32::from(queue_depth).max(1).next_power_of_two();
        let ring = IoUring::<squeue::Entry128>::builder().build(entries)?;
        let iodesc_len = round_up_to_page(usize::from(queue_depth) * Self::IO_DESC_SIZE);
        let iodesc = MmapRo::map_readonly(fd, iodesc_len, 0)?;
        let buffers = (0..queue_depth).map(|_| vec![0u8; buf_size]).collect();
        Ok(Self {
            fd,
            ring,
            iodesc,
            buffers,
            queue_depth,
        })
    }

    /// Submete `FETCH_REQ` para todas as tags (marca a fila ready). Não espera CQE.
    pub fn submit_initial_fetch(&mut self) -> io::Result<()> {
        const UBLK_U_IO_FETCH_REQ: u32 = 0xc010_7520;
        for tag in 0..self.queue_depth {
            let addr = self.buffers[usize::from(tag)].as_mut_ptr() as u64;
            self.push(UBLK_U_IO_FETCH_REQ, tag, 0, addr)?;
        }
        self.ring.submit()?;
        Ok(())
    }

    /// Drena os CQEs disponíveis (não bloqueia).
    pub fn drain(&mut self) -> Vec<UblkCompletion> {
        self.ring
            .completion()
            .map(|cqe| UblkCompletion {
                tag: cqe.user_data() as u16,
                result: cqe.result(),
            })
            .collect()
    }

    /// Bloqueia até chegar ao menos um CQE (próximo request servido ou abort do
    /// teardown) e então drena. Não submete SQEs novos (FETCH/COMMIT já foram
    /// submetidos por `submit_initial_fetch`/`commit_and_fetch`).
    ///
    /// Re-espera em `EINTR` (`io_uring_enter` interrompido por sinal): um daemon que
    /// trata SIGINT/SIGTERM pode receber o sinal nesta thread; o EINTR não é erro, só
    /// re-bloqueia. Os SQEs já submetidos seguem armados.
    pub fn wait_and_drain(&mut self) -> io::Result<Vec<UblkCompletion>> {
        loop {
            match self.ring.submit_and_wait(1) {
                Ok(_) => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(self.drain())
    }

    /// Bytes do `ublksrv_io_desc` da `tag` (somente leitura, do `mmap`).
    pub fn io_desc_bytes(&self, tag: u16) -> &[u8] {
        let start = usize::from(tag) * Self::IO_DESC_SIZE;
        &self.iodesc.as_bytes()[start..start + Self::IO_DESC_SIZE]
    }

    /// Buffer de dados mutável da `tag` (READ preenche; WRITE já vem preenchido).
    pub fn buffer_mut(&mut self, tag: u16) -> &mut [u8] {
        &mut self.buffers[usize::from(tag)]
    }

    /// Completa a `tag` com `result` e re-arma o FETCH (re-fornecendo o buffer).
    pub fn commit_and_fetch(&mut self, tag: u16, result: i32) -> io::Result<()> {
        const UBLK_U_IO_COMMIT_AND_FETCH_REQ: u32 = 0xc010_7521;
        let addr = self.buffers[usize::from(tag)].as_mut_ptr() as u64;
        self.push(UBLK_U_IO_COMMIT_AND_FETCH_REQ, tag, result, addr)?;
        self.ring.submit()?;
        Ok(())
    }

    fn push(&mut self, cmd_op: u32, tag: u16, result: i32, addr: u64) -> io::Result<()> {
        let cmd = io_cmd80(0, tag, result, addr);
        let entry = opcode::UringCmd80::new(types::Fd(self.fd), cmd_op)
            .cmd(cmd)
            .build()
            .user_data(u64::from(tag));

        // SAFETY: `cmd` (com `addr`) é copiado para a SQE no `push`. `addr` aponta
        // para `self.buffers[tag]`, vivo enquanto o servidor existe; o `fd` segue
        // aberto pelo dono. O kernel só acessa o buffer ao servir I/O nesta thread.
        unsafe {
            self.ring
                .submission()
                .push(&entry)
                .map_err(|_| io::Error::other("io_uring submission queue is full"))?;
        }
        Ok(())
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
