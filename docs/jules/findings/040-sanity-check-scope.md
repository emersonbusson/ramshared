# Scope Integration Violation

## Finding
The new `validate_limits` method on `ResidencyConfig` correctly implements physical bounds checking (guarding against 0 and values exceeding available physical memory limits) as required by the Defensive Programming and Guard Clauses principles.

However, the invocation of `validate_limits` inside the operational execution loops (e.g., in `main.rs` daemon boot and `ublk_server.rs` initialization paths) requires modifying off-limits files. The task target scope strictly restricts code changes to `crates/ramshared-wsl2d/src/residency.rs` and associated test files.

## Resolution
To comply with the strict scope constraints, no off-limits files were modified. The `validate_limits` function has been successfully implemented using Test-Driven Development (TDD) within `residency.rs`, but a separate task within the broader scope is required to actually integrate this check during runtime startup.
