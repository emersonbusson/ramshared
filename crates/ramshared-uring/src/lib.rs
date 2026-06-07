//! Wrappers seguros sobre a crate `io-uring` para a Fase B.
//!
//! O daemon `ramshared-wsl2d` fica com `#![forbid(unsafe_code)]`. Operações reais de
//! SQE que exigirem `unsafe` entram neste crate, com invariantes documentadas no
//! menor escopo possível.

#![deny(unsafe_op_in_unsafe_fn)]

use std::io;
use std::os::fd::RawFd;

use io_uring::{IoUring, opcode, squeue, types};

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
    const UBLK_QUEUE_ID_NONE: u16 = u16::MAX;

    let mut features = 0u64;
    let mut cmd = [0u8; 80];

    cmd[4..6].copy_from_slice(&UBLK_QUEUE_ID_NONE.to_ne_bytes());
    cmd[6..8].copy_from_slice(&UBLK_FEATURES_LEN.to_ne_bytes());
    cmd[8..16].copy_from_slice(&((&mut features as *mut u64) as u64).to_ne_bytes());

    let res = submit_uring_cmd80(fd, UBLK_U_CMD_GET_FEATURES, cmd)?;
    if res != 0 {
        return Err(io::Error::other(format!(
            "ublk GET_FEATURES returned unexpected result {res}"
        )));
    }

    Ok(features)
}

fn submit_uring_cmd80(fd: RawFd, cmd_op: u32, cmd: [u8; 80]) -> io::Result<i32> {
    let mut ring = IoUring::<squeue::Entry128>::builder().build(2)?;
    let entry = opcode::UringCmd80::new(types::Fd(fd), cmd_op)
        .cmd(cmd)
        .build()
        .user_data(1);

    {
        let mut sq = ring.submission();
        // SAFETY: `cmd` e copiado para a SQE antes da submissao. Para ublk
        // GET_FEATURES, o unico ponteiro embutido em `cmd` aponta para memoria
        // de stack de `ublk_get_features`, e esta funcao espera o CQE antes
        // desse frame poder retornar.
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
