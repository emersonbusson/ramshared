# Finding 27: QEMU ublk Teardown Bug Explanation Verification

- **Source PR:** Jules PR #501
- **Crate:** `ramshared-wsl2d`
- **Module:** `main.rs`
- **Classification:** `FINDING_ONLY`

## Observation

The comment on `BackendKind::Ram` in `crates/ramshared-wsl2d/src/main.rs` ("Ram exists to validate the lifecycle/teardown of the ublk daemon in QEMU...; the teardown bug that hung WSL2 is independent of the backend") refers to a resolved historical bug in the ublk daemon teardown lifecycle.

The `Ram` backend enables testing and verifying ublk daemon initialization, signal handling, swapoff ordering, and teardown rollback semantics in GPU-less QEMU test environments.

The teardown sequence is fully implemented and covered by unit tests in `ramshared-wsl2d`:
- `daemon_ublk_runtime_orders_lifecycle_and_rolls_back_without_device`
- `daemon_ublk_runtime_failures_delete_only_after_fresh_absence_proof`
- `daemon_ublk_shutdown_is_swapoff_first_and_preserves_on_uncertainty`

## Verdict

Accepted as documented architectural verification.
