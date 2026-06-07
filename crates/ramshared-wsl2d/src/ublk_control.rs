//! Smoke seguro do `/dev/ublk-control`.
//!
//! Este módulo só consulta `GET_FEATURES`. Ele não chama `ADD_DEV`, não cria
//! `/dev/ublkcN`/`/dev/ublkbN` e não toca em swap.

use std::fs::OpenOptions;
use std::io;
use std::os::fd::AsRawFd;
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FeatureReport {
    pub features: u64,
}

pub fn get_features(path: impl AsRef<Path>) -> io::Result<FeatureReport> {
    let control = OpenOptions::new().read(true).write(true).open(path)?;
    let features = ramshared_uring::ublk_get_features(control.as_raw_fd())?;

    Ok(FeatureReport { features })
}
