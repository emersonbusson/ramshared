# Finding: 28-cuda-ffi-abi-coverage-trap

**Title:** Unable to add proper `rust-slice-coverage.json` entry without breaking strict CI schema

## Observation
While attempting to complete the task `map CUresult error codes (CUDA_ERROR_OUT_OF_MEMORY, CUDA_ERROR_INVALID_VALUE) to semantic enum` in `crates/ramshared-cuda`, all modifications to the codebase compiled successfully and tests passed completely. However, the CI system leverages an adversarial invariant schema (`docs/governance/rust-slice-coverage.json` validated by `tools/ci/plan-rust-slice-coverage.mjs`).

Any attempt to modify or append entries to `rust-slice-coverage.json` specifically for the test-only coverage of `crates/ramshared-cuda/src/vram_impl.rs` or `crates/ramshared-cuda/src/driver.rs` or `crates/ramshared-cuda/src/ffi.rs` failed validation. The system requires exact references to `SPEC.md` files with precise token matches, and exact cargo test invocations that also strictly include `evidence` paths like `validation.md`, but only within specifically allowed files. Attempting to add an entry triggers `test-only-spec-contract-mismatch` or `module-export-glue-differential-base-required` or `changed-rust-file-unmapped` when validating against `origin/main`.

The codebase's strict architecture prevents updating the coverage JSON easily without breaking other governance checks. I have reverted the code changes for the enums, since they cannot be merged without violating the CI `docs-check` scripts and `check-ci-contract.mjs` verification logic.

## Recommendation
Investigate the proper method to register testing/coverage boundary updates for new FFI-to-Driver semantic enum translations within `docs/governance/rust-slice-coverage.json` without failing `plan-rust-slice-coverage.mjs`. The specific tests added require exact documentation links in the JSON that correctly point to permitted `.md` specs.
