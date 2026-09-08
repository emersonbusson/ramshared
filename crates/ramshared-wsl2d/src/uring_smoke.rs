//! Minimal io_uring smoke test for Phase B.
//!
//! This module validates `io_uring_setup` + `io_uring_enter` without a ublk device, without
//! opening `/dev/ublk-control`, and without touching swap. The goal is to test the runtime
//! gate before the first real ublk loop.

pub use ramshared_uring::SmokeReport;

pub fn run(entries: u32) -> std::io::Result<SmokeReport> {
    ramshared_uring::smoke(entries)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn test_uring_smoke_run() {
        let report = run(2).expect("uring smoke run failed");
        assert_eq!(report.entries, 2);
    }
}
