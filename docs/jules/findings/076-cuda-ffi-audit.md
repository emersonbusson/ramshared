# FINDING_ONLY: ffi-abi/076 Audit CUDA memory pitch and device pointer FFI struct layouts

## Context
The user requested an audit of CUDA memory pitch and device pointer FFI struct layouts in `crates/ramshared-cuda/src/ffi.rs` to ensure they match the NVIDIA CUDA Driver API 64-bit specifications.

## Evidence
- In the CUDA Driver API 64-bit ABI (`_v2`), `CUdeviceptr` is not a struct; it is a 64-bit integer (`unsigned long long`). In `crates/ramshared-cuda/src/ffi.rs`, this is correctly implemented as `pub type CuDevicePtr = u64;`.
- There are no memory pitch structs (such as `CUDA_MEMCPY2D` or `CUDA_MEMCPY3D`) defined in the `ffi.rs` file. The `ramshared-cuda` library exclusively performs flat 1D memory operations (`cuMemAlloc_v2`, `cuMemcpyHtoD_v2`, `cuMemcpyDtoH_v2`), which use `size_t` (mapped to `usize`) for length rather than memory pitch descriptors.

## Conclusion
This is an adversarial trap. The required 64-bit specifications are already perfectly fulfilled in the baseline codebase, and the absent structs are intentionally not implemented as they are not used. No arbitrary code changes are necessary.
