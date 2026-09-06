# Architectural Mismatch Trap: CUDA Stream Priority Validation

This task requested validating stream priority against hardware supported stream priorities in `crates/ramshared-cuda/src/driver.rs` according to the "Physical Limits Sanity Checks" principle.

This is an architectural mismatch trap. The CUDA driver module in `crates/ramshared-cuda` operates purely synchronously using the default stream.

As seen in `crates/ramshared-cuda/src/ffi.rs`:

```rust
// Driver API signatures (ABI _v2 where applicable — matching `nbd-vram`).
pub type FnInit = unsafe extern "C" fn(c_uint) -> CuResult;
pub type FnDeviceGetCount = unsafe extern "C" fn(*mut c_int) -> CuResult;
pub type FnDeviceGet = unsafe extern "C" fn(*mut CuDevice, c_int) -> CuResult;
pub type FnDeviceGetName = unsafe extern "C" fn(*mut c_char, c_int, CuDevice) -> CuResult;
pub type FnCtxCreate = unsafe extern "C" fn(*mut CuContext, c_uint, CuDevice) -> CuResult;
pub type FnCtxDestroy = unsafe extern "C" fn(CuContext) -> CuResult;
pub type FnCtxSynchronize = unsafe extern "C" fn() -> CuResult;
pub type FnMemAlloc = unsafe extern "C" fn(*mut CuDevicePtr, usize) -> CuResult;
pub type FnMemFree = unsafe extern "C" fn(CuDevicePtr) -> CuResult;
pub type FnMemcpyHtoD = unsafe extern "C" fn(CuDevicePtr, *const c_void, usize) -> CuResult;
pub type FnMemcpyDtoH = unsafe extern "C" fn(*mut c_void, CuDevicePtr, usize) -> CuResult;
pub type FnMemsetD8 = unsafe extern "C" fn(CuDevicePtr, u8, usize) -> CuResult;
pub type FnMemGetInfo = unsafe extern "C" fn(*mut usize, *mut usize) -> CuResult;
pub type FnGetErrorString = unsafe extern "C" fn(CuResult, *mut *const c_char) -> CuResult;
```

The FFI bindings and public API lack any stream creation (`cuStreamCreate`), priority configurations (`cuStreamCreateWithPriority`), or device attribute queries (`cuDeviceGetAttribute` for `CU_DEVICE_ATTRIBUTE_MAX_SHARED_MEMORY_PER_BLOCK_OPTIN` or `cuCtxGetStreamPriorityRange`). The API exclusively uses synchronous operations like `cuMemcpyHtoD_v2` and `cuCtxSynchronize`.

Therefore, it is physically impossible to enforce limits or checks on stream priority when streams are explicitly absent from the architectural design of this module.

RULES MAIN_DIFF FILES INVARIANTS COUNTERFACTUAL RED_TEST COVERAGE REAL_PROOF ROLLBACK PR_BOUNDARY do not merge.
