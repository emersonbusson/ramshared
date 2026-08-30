1. RULES
   - Adhere to the "Specific & Semantic Errors" principle.
   - 0 unvetted unsafe code.
   - Use TDD: RED_TEST -> GREEN -> COVERAGE/REAL_PROOF -> PR_BOUNDARY.
   - Target scope: `crates/ramshared-tier/src/nbd_readiness.rs` and its tests.
2. MAIN_DIFF
   - Create `NbdReadinessError` enum with variants `ConnectionRefused` and `Timeout`.
   - Implement `core::fmt::Display` for `NbdReadinessError`.
   - Implement `core::error::Error` for `NbdReadinessError`.
   - Implement `From<std::io::ErrorKind>` to convert `ErrorKind::ConnectionRefused` and `ErrorKind::TimedOut` into `NbdReadinessError`.
3. FILES
   - Modify `crates/ramshared-tier/src/nbd_readiness.rs` to add `NbdReadinessError` and test cases.
4. INVARIANTS
   - `std::io::ErrorKind::ConnectionRefused` maps to `NbdReadinessError::ConnectionRefused`.
   - `std::io::ErrorKind::TimedOut` maps to `NbdReadinessError::Timeout`.
   - Other `ErrorKind`s can be handled by `NbdReadinessError::Other(std::io::ErrorKind)` or similar, but instructions specifically requested `ECONNREFUSED` and `ETIMEDOUT` mapping. We'll introduce `Other` to handle the `_` arm securely.
5. COUNTERFACTUAL
   - Without this custom enum, callers would have to handle generic IO errors, which contradicts the "Specific & Semantic Errors" principle and loses rich domain typing.
6. RED_TEST
   - Add test `test_nbd_readiness_error_from_io_error` verifying the conversions.
7. COVERAGE
   - Verify `cargo test -p ramshared-tier` and `cargo clippy -p ramshared-tier -- -D warnings`.
8. REAL_PROOF
   - Confirm via test success.
9. ROLLBACK
   - Any test failure or build failure.
10. PR_BOUNDARY
    - Commit with Conventional Commits, targeting `jules/inbox`.
