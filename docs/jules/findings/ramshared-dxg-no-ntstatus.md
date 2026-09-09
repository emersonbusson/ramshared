# Finding: No NTSTATUS Mapping in ramshared-dxg

## Context
The task requires adding test coverage for mapping `NTSTATUS` error codes (specifically `STATUS_GRAPHICS_*`) to `DXG` error types in `crates/ramshared-dxg/src/lib.rs`.

## Investigation
I searched `crates/ramshared-dxg/src/lib.rs` for `NTSTATUS` and `STATUS_GRAPHICS` and found no results. The crate maps standard libc errors (`std::io::Error`) in its `from_sys_error` method:

```rust
    pub fn from_sys_error(error: std::io::Error) -> Self {
        match error.raw_os_error() {
            Some(libc::ENODEV) => Self::DeviceNotFound,
            Some(libc::EFAULT) => Self::BadAddress,
            Some(libc::ENOTTY) => Self::UnsupportedHardware,
            Some(libc::EOVERFLOW) => Self::BufferOverflow,
            Some(libc::EPERM) | Some(libc::EACCES) => Self::PermissionDenied,
            _ => Self::Io(error.to_string()),
        }
    }
```

Because `STATUS_GRAPHICS_*` constants (which are Windows specific NTSTATUS codes) are not used or mapped within `crates/ramshared-dxg/src/lib.rs`, the requested code modification is not possible.

## Conclusion
Code modification is not possible because the target mapping does not exist in the target file.
