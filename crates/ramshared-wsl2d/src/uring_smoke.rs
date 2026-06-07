//! Smoke mínimo de io_uring para a Fase B.
//!
//! Este módulo valida `io_uring_setup` + `io_uring_enter` sem ublk device, sem
//! abrir `/dev/ublk-control` e sem tocar swap. O objetivo é testar o gate de
//! runtime antes do primeiro loop ublk real.

use std::io;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SmokeReport {
    pub entries: u32,
    pub submitted: usize,
}

pub fn run(entries: u32) -> io::Result<SmokeReport> {
    let ring = io_uring::IoUring::new(entries)?;
    let submitted = ring.submit()?;

    Ok(SmokeReport { entries, submitted })
}
