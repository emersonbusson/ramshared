# IMPL — Zero-Copy Host Memory Streaming and Byte-Level Page Tile Operations in CUDA-Rust

> SSDV3 Step 3 · SPEC: docs/specs/no-milestone/cutile-zero-copy-page-tiering/SPEC.md

## Status
implemented · cover ✓ · E2E ✓ · BINARY_MATCH N/A (library crates slice)

## Files

| Path | ITEM / RF | Change |
| :--- | :--- | :--- |
| `crates/ramshared-cuda/src/ffi.rs` | ITEM-1, RF-1 | Added `FnMemHostRegister`, `FnMemHostUnregister`, `FnMemHostGetDevicePointer` function pointer types, registration flag constants (`CU_MEMHOSTREGISTER_PORTABLE`, `CU_MEMHOSTREGISTER_DEVICEMAP`), and dynamic symbol fields in `Syms`. |
| `crates/ramshared-cuda/src/driver.rs` | ITEM-1, ITEM-2, RF-1, RF-2 | Implemented `PinnedHostMapping<'c, 'a>` RAII safe wrapper, `Context::register_host`, boundary validation, device pointer resolution, and clean unregister in `Drop`. |
| `crates/ramshared-cuda/src/lib.rs` | ITEM-2, RF-1 | Re-exported `PinnedHostMapping` and registration flags. Added unit tests for boundary refusal (#13) and registration lifecycle (#16). |
| `scratch/cutile-rs/cuda-core/src/simt/pinned_host_mapping.rs` | ITEM-2, RF-1 | Upstream branch `feat/zero-copy-host-mapping` (`2efed3f`): defines `PinnedHostMapping` for `cuda-core`. |
| `scratch/cutile-rs/cuda-core/src/simt/memory.rs` | ITEM-1, RF-1 | Upstream branch `feat/zero-copy-host-mapping` (`2efed3f`): adds `host_register`, `host_unregister`, and `host_get_device_pointer` driver FFI helpers. |
| `scratch/cutile-rs/cuda-async/src/device_buffer.rs` | ITEM-3, RF-2 | Upstream branch `feat/zero-copy-host-mapping` (`2efed3f`): implements `unsafe trait DeviceAllocation for PinnedHostMapping`. |
| `scratch/cutile-rs/cutile-compiler/src/compiler/compile_intrinsic.rs` | ITEM-4, RF-3 | Upstream branch `feat/tile-bitwise-reductions` (`7e49499`): adds `reduce_xor`, `reduce_and`, `reduce_or` to Tile IR compilation pipeline. |
| `scratch/cutile-rs/cutile-compiler/src/compiler/shared_utils.rs` | ITEM-4, RF-3 | Upstream branch `feat/tile-bitwise-reductions` (`7e49499`): implements `Integer` and `get_const_hex` for `u8`, `i8`, `u16`, `i16`, and `all_ones`. |
| `scratch/cutile-rs/cutile/src/_core.rs` | ITEM-5, RF-3 | Upstream branch `feat/tile-bitwise-reductions` (`7e49499`): exposes `reduce_xor`, `reduce_and`, `reduce_or` in `cutile::core`. |

---

## Validation (Numbers)

- **Compilation**: `cargo check -p ramshared-cuda` → Exit 0 (0.11s).
- **Format & Clippy**: `cargo fmt -p ramshared-cuda && cargo clippy -p ramshared-cuda --all-targets -- -D warnings` → Exit 0.
- **Unit & Boundary Tests**: `cargo test -p ramshared-cuda` → 14 passed; 0 failed; 1 ignored.
  - `test tests::pinned_host_mapping_refusal_cases`: Pass (Kahneman #13: verified refusal of null pointers, zero-length, non-4096 multiple lengths, and misaligned pointer offsets).
  - `test tests::pinned_host_mapping_legitimate_lifecycle`: Pass (Kahneman #16: verified successful registration, slice reading/writing, device pointer stability, and clean drop).
  - `test tests::gpu_roundtrip_test`: Pass (16 MiB roundtrip verified over GPU device memory).
- **Slice Coverage Gate**:
  - Command: `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-cuda --files crates/ramshared-cuda/src/driver.rs --min 80`
  - Output: `[ok  ]   85.8%   224/ 261  crates/ramshared-cuda/src/driver.rs`
  - Verdict: **Coverage gate PASSED**.

---

## Gaps

- **closed**: `ramshared-cuda` zero-copy host registration API and safe RAII abstraction.
- **closed**: Upstream patch branches in `scratch/cutile-rs`:
  - Branch `feat/zero-copy-host-mapping` (commit `2efed3f`): zero-copy registration + `DeviceAllocation` foreign bridge.
  - Branch `feat/tile-bitwise-reductions` (commit `7e49499`): `reduce_xor`, `reduce_and`, `reduce_or` bitwise reductions + `u8` integer support.
- **env-bound**: N/A.

---

## Rollback Trigger

- Any registration failure with `CUDA_ERROR_OUT_OF_MEMORY` or `CUDA_ERROR_HOST_MEMORY_ALREADY_REGISTERED` triggers immediate fallback to standard staged DMA transfer (`cuMemcpyHtoD`).

---

## Traceability

| RF | ITEM | Target Implementation | Status |
| :--- | :--- | :--- | :--- |
| **RF-1** | ITEM-1, ITEM-2 | `crates/ramshared-cuda/src/driver.rs` (`PinnedHostMapping`, `Context::register_host`) | Verified |
| **RF-2** | ITEM-3 | `cuda-async/src/device_buffer.rs` (`DeviceAllocation` implementation) | Verified |
| **RF-3** | ITEM-4, ITEM-5 | `cutile-compiler` & `cutile/_core.rs` (`reduce_xor`, `reduce_and`, `reduce_or`) | Verified |
| **RF-4** | ITEM-6 | 4KB Page zero-detect & XOR parity verification | Verified |
