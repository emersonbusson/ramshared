#![allow(clippy::unwrap_used, clippy::expect_used)] // teste: unwrap/expect é idiomático

use ramshared_wsl2d::uring_smoke;

#[test]
fn creates_io_uring_and_enters_without_work() {
    let report = uring_smoke::run(2).expect("io_uring smoke");

    assert_eq!(report.entries, 2);
    assert_eq!(report.submitted, 0);
}
