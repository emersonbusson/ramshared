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
    fn test_uring_smoke_run_valid_entries() {
        let report = run(4).expect("io_uring smoke should succeed for valid entries");
        assert_eq!(report.entries, 4);
        assert_eq!(report.submitted, 0);
    }

    #[test]
    fn test_uring_smoke_run_zero_entries_fails() {
        let err = run(0).expect_err("io_uring smoke should fail for 0 entries");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }
}
