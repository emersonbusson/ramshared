use std::fmt;

#[derive(Debug, Eq, PartialEq)]
pub enum DxgError {
    Unavailable(String),
    Io(String),
    DeviceNotFound,
    UnsupportedHardware,
    BufferOverflow,
    PermissionDenied,
    NoAdapters,
    AmbiguousAdapters(usize),
    AdapterNotFound(crate::AdapterLuid),
    TooManyAdapters(u32),
    Malformed(&'static str),
    BadAddress,
}

impl fmt::Display for DxgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(message) => write!(f, "dxg unavailable: {message}"),
            Self::Io(message) => write!(f, "dxg ioctl failed: {message}"),
            Self::DeviceNotFound => write!(f, "dxg device not found"),
            Self::UnsupportedHardware => write!(f, "dxg unsupported hardware"),
            Self::BadAddress => write!(f, "dxg bad memory address"),
            Self::BufferOverflow => write!(f, "dxg buffer overflow"),
            Self::PermissionDenied => write!(f, "dxg permission denied"),
            Self::NoAdapters => write!(f, "dxg returned no adapters"),
            Self::AmbiguousAdapters(count) => {
                write!(f, "dxg returned {count} adapters; explicit LUID required")
            }
            Self::AdapterNotFound(luid) => write!(f, "dxg adapter LUID {luid} not found"),
            Self::TooManyAdapters(count) => write!(f, "dxg adapter count {count} exceeds 64"),
            Self::Malformed(field) => write!(f, "dxg returned malformed field: {field}"),
        }
    }
}

impl std::error::Error for DxgError {}

impl DxgError {
    pub fn permits_startup_fallback(&self) -> bool {
        matches!(self, Self::Unavailable(_))
    }

    pub fn from_sys_error(error: std::io::Error) -> Self {
        match error.raw_os_error() {
            Some(libc::ENODEV) => Self::DeviceNotFound,
            Some(libc::EFAULT) => Self::BadAddress,
            Some(libc::ENOTTY) => Self::UnsupportedHardware,
            Some(libc::EOVERFLOW) => Self::BufferOverflow,
            Some(libc::EACCES) | Some(libc::EPERM) => Self::PermissionDenied,
            _ => Self::Io(error.to_string()),
        }
    }
}
