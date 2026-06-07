//! Wrappers seguros sobre a crate `io-uring` para a Fase B.
//!
//! O daemon `ramshared-wsl2d` fica com `#![forbid(unsafe_code)]`. Operações reais de
//! SQE que exigirem `unsafe` entram neste crate, com invariantes documentadas no
//! menor escopo possível. Este smoke inicial ainda não precisa de bloco `unsafe`.

#![deny(unsafe_op_in_unsafe_fn)]

use std::io;

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
