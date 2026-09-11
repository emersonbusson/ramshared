//! CUDA Error types and conversions.

use core::fmt;

/// Strongly typed representation of a CUDA Driver API error code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverError {
    InvalidValue,
    OutOfMemory,
    NotInitialized,
    Deinitialized,
    ProfilerDisabled,
    ProfilerNotInitialized,
    ProfilerAlreadyStarted,
    ProfilerAlreadyStopped,
    StubLibrary,
    DeviceUnavailable,
    NoDevice,
    InvalidDevice,
    DeviceNotLicensed,
    InvalidImage,
    InvalidContext,
    ContextAlreadyCurrent,
    MapFailed,
    UnmapFailed,
    ArrayIsMapped,
    AlreadyMapped,
    NoBinaryForGpu,
    AlreadyAcquired,
    NotMapped,
    NotMappedAsArray,
    NotMappedAsPointer,
    EccUncorrectable,
    UnsupportedLimit,
    ContextAlreadyInUse,
    PeerAccessUnsupported,
    InvalidPtx,
    InvalidGraphicsContext,
    NvlinkUncorrectable,
    JitCompilerNotFound,
    UnsupportedPtxVersion,
    JitCompilationDisabled,
    UnsupportedExecAffinity,
    UnsupportedDevsideSync,
    InvalidSource,
    FileNotFound,
    SharedObjectSymbolNotFound,
    SharedObjectInitFailed,
    OperatingSystem,
    InvalidHandle,
    IllegalState,
    NotFound,
    NotReady,
    IllegalAddress,
    LaunchOutOfResources,
    LaunchTimeout,
    LaunchIncompatibleTexturing,
    PeerAccessAlreadyEnabled,
    PeerAccessNotEnabled,
    PrimaryContextActive,
    ContextIsDestroyed,
    Assert,
    TooManyPeers,
    HostMemoryAlreadyRegistered,
    HostMemoryNotRegistered,
    HardwareStackError,
    IllegalInstruction,
    MisalignedAddress,
    InvalidAddressSpace,
    InvalidPc,
    LaunchFailed,
    CooperativeLaunchTooLarge,
    NotPermitted,
    NotSupported,
    SystemNotReady,
    SystemDriverMismatch,
    CompatNotSupportedOnDevice,
    MpsConnectionFailed,
    MpsRpcFailure,
    MpsServerNotReady,
    MpsMaxClientsReached,
    MpsMaxConnectionsReached,
    MpsClientShouldRetry,
    StreamCaptureUnsupported,
    StreamCaptureInvalidated,
    StreamCaptureMerge,
    StreamCaptureUnmatched,
    StreamCaptureSequentialLack,
    StreamCaptureWrongThread,
    Timeout,
    GraphExecUpdateFailure,
    ExternalDevice,
    InvalidClusterSize,
    /// Unknown or unmapped CUDA error code.
    Unknown(i32),
}

impl DriverError {
    pub fn as_i32(self) -> i32 {
        match self {
            DriverError::InvalidValue => 1,
            DriverError::OutOfMemory => 2,
            DriverError::NotInitialized => 3,
            DriverError::Deinitialized => 4,
            DriverError::ProfilerDisabled => 5,
            DriverError::ProfilerNotInitialized => 6,
            DriverError::ProfilerAlreadyStarted => 7,
            DriverError::ProfilerAlreadyStopped => 8,
            DriverError::StubLibrary => 34,
            DriverError::DeviceUnavailable => 46,
            DriverError::NoDevice => 100,
            DriverError::InvalidDevice => 101,
            DriverError::DeviceNotLicensed => 102,
            DriverError::InvalidImage => 200,
            DriverError::InvalidContext => 201,
            DriverError::ContextAlreadyCurrent => 202,
            DriverError::MapFailed => 205,
            DriverError::UnmapFailed => 206,
            DriverError::ArrayIsMapped => 207,
            DriverError::AlreadyMapped => 208,
            DriverError::NoBinaryForGpu => 209,
            DriverError::AlreadyAcquired => 210,
            DriverError::NotMapped => 211,
            DriverError::NotMappedAsArray => 212,
            DriverError::NotMappedAsPointer => 213,
            DriverError::EccUncorrectable => 214,
            DriverError::UnsupportedLimit => 215,
            DriverError::ContextAlreadyInUse => 216,
            DriverError::PeerAccessUnsupported => 217,
            DriverError::InvalidPtx => 218,
            DriverError::InvalidGraphicsContext => 219,
            DriverError::NvlinkUncorrectable => 220,
            DriverError::JitCompilerNotFound => 221,
            DriverError::UnsupportedPtxVersion => 222,
            DriverError::JitCompilationDisabled => 223,
            DriverError::UnsupportedExecAffinity => 224,
            DriverError::UnsupportedDevsideSync => 225,
            DriverError::InvalidSource => 300,
            DriverError::FileNotFound => 301,
            DriverError::SharedObjectSymbolNotFound => 302,
            DriverError::SharedObjectInitFailed => 303,
            DriverError::OperatingSystem => 304,
            DriverError::InvalidHandle => 400,
            DriverError::IllegalState => 401,
            DriverError::NotFound => 500,
            DriverError::NotReady => 600,
            DriverError::IllegalAddress => 700,
            DriverError::LaunchOutOfResources => 701,
            DriverError::LaunchTimeout => 702,
            DriverError::LaunchIncompatibleTexturing => 703,
            DriverError::PeerAccessAlreadyEnabled => 704,
            DriverError::PeerAccessNotEnabled => 705,
            DriverError::PrimaryContextActive => 708,
            DriverError::ContextIsDestroyed => 709,
            DriverError::Assert => 710,
            DriverError::TooManyPeers => 711,
            DriverError::HostMemoryAlreadyRegistered => 712,
            DriverError::HostMemoryNotRegistered => 713,
            DriverError::HardwareStackError => 714,
            DriverError::IllegalInstruction => 715,
            DriverError::MisalignedAddress => 716,
            DriverError::InvalidAddressSpace => 717,
            DriverError::InvalidPc => 718,
            DriverError::LaunchFailed => 719,
            DriverError::CooperativeLaunchTooLarge => 720,
            DriverError::NotPermitted => 800,
            DriverError::NotSupported => 801,
            DriverError::SystemNotReady => 802,
            DriverError::SystemDriverMismatch => 803,
            DriverError::CompatNotSupportedOnDevice => 804,
            DriverError::MpsConnectionFailed => 805,
            DriverError::MpsRpcFailure => 806,
            DriverError::MpsServerNotReady => 807,
            DriverError::MpsMaxClientsReached => 808,
            DriverError::MpsMaxConnectionsReached => 809,
            DriverError::MpsClientShouldRetry => 810,
            DriverError::StreamCaptureUnsupported => 900,
            DriverError::StreamCaptureInvalidated => 901,
            DriverError::StreamCaptureMerge => 902,
            DriverError::StreamCaptureUnmatched => 903,
            DriverError::StreamCaptureSequentialLack => 904,
            DriverError::StreamCaptureWrongThread => 905,
            DriverError::Timeout => 906,
            DriverError::GraphExecUpdateFailure => 907,
            DriverError::ExternalDevice => 908,
            DriverError::InvalidClusterSize => 909,
            DriverError::Unknown(c) => c,
        }
    }
}

impl From<crate::ffi::CuResult> for DriverError {
    fn from(code: crate::ffi::CuResult) -> Self {
        match code {
            1 => DriverError::InvalidValue,
            2 => DriverError::OutOfMemory,
            3 => DriverError::NotInitialized,
            4 => DriverError::Deinitialized,
            5 => DriverError::ProfilerDisabled,
            6 => DriverError::ProfilerNotInitialized,
            7 => DriverError::ProfilerAlreadyStarted,
            8 => DriverError::ProfilerAlreadyStopped,
            34 => DriverError::StubLibrary,
            46 => DriverError::DeviceUnavailable,
            100 => DriverError::NoDevice,
            101 => DriverError::InvalidDevice,
            102 => DriverError::DeviceNotLicensed,
            200 => DriverError::InvalidImage,
            201 => DriverError::InvalidContext,
            202 => DriverError::ContextAlreadyCurrent,
            205 => DriverError::MapFailed,
            206 => DriverError::UnmapFailed,
            207 => DriverError::ArrayIsMapped,
            208 => DriverError::AlreadyMapped,
            209 => DriverError::NoBinaryForGpu,
            210 => DriverError::AlreadyAcquired,
            211 => DriverError::NotMapped,
            212 => DriverError::NotMappedAsArray,
            213 => DriverError::NotMappedAsPointer,
            214 => DriverError::EccUncorrectable,
            215 => DriverError::UnsupportedLimit,
            216 => DriverError::ContextAlreadyInUse,
            217 => DriverError::PeerAccessUnsupported,
            218 => DriverError::InvalidPtx,
            219 => DriverError::InvalidGraphicsContext,
            220 => DriverError::NvlinkUncorrectable,
            221 => DriverError::JitCompilerNotFound,
            222 => DriverError::UnsupportedPtxVersion,
            223 => DriverError::JitCompilationDisabled,
            224 => DriverError::UnsupportedExecAffinity,
            225 => DriverError::UnsupportedDevsideSync,
            300 => DriverError::InvalidSource,
            301 => DriverError::FileNotFound,
            302 => DriverError::SharedObjectSymbolNotFound,
            303 => DriverError::SharedObjectInitFailed,
            304 => DriverError::OperatingSystem,
            400 => DriverError::InvalidHandle,
            401 => DriverError::IllegalState,
            500 => DriverError::NotFound,
            600 => DriverError::NotReady,
            700 => DriverError::IllegalAddress,
            701 => DriverError::LaunchOutOfResources,
            702 => DriverError::LaunchTimeout,
            703 => DriverError::LaunchIncompatibleTexturing,
            704 => DriverError::PeerAccessAlreadyEnabled,
            705 => DriverError::PeerAccessNotEnabled,
            708 => DriverError::PrimaryContextActive,
            709 => DriverError::ContextIsDestroyed,
            710 => DriverError::Assert,
            711 => DriverError::TooManyPeers,
            712 => DriverError::HostMemoryAlreadyRegistered,
            713 => DriverError::HostMemoryNotRegistered,
            714 => DriverError::HardwareStackError,
            715 => DriverError::IllegalInstruction,
            716 => DriverError::MisalignedAddress,
            717 => DriverError::InvalidAddressSpace,
            718 => DriverError::InvalidPc,
            719 => DriverError::LaunchFailed,
            720 => DriverError::CooperativeLaunchTooLarge,
            800 => DriverError::NotPermitted,
            801 => DriverError::NotSupported,
            802 => DriverError::SystemNotReady,
            803 => DriverError::SystemDriverMismatch,
            804 => DriverError::CompatNotSupportedOnDevice,
            805 => DriverError::MpsConnectionFailed,
            806 => DriverError::MpsRpcFailure,
            807 => DriverError::MpsServerNotReady,
            808 => DriverError::MpsMaxClientsReached,
            809 => DriverError::MpsMaxConnectionsReached,
            810 => DriverError::MpsClientShouldRetry,
            900 => DriverError::StreamCaptureUnsupported,
            901 => DriverError::StreamCaptureInvalidated,
            902 => DriverError::StreamCaptureMerge,
            903 => DriverError::StreamCaptureUnmatched,
            904 => DriverError::StreamCaptureSequentialLack,
            905 => DriverError::StreamCaptureWrongThread,
            906 => DriverError::Timeout,
            907 => DriverError::GraphExecUpdateFailure,
            908 => DriverError::ExternalDevice,
            909 => DriverError::InvalidClusterSize,
            999 => DriverError::Unknown(999), // also explicit mapping to be safe
            other => DriverError::Unknown(other),
        }
    }
}

/// CUDA layer error representation. No `panic`/`unwrap` in production paths (coding.md rules).
#[derive(Debug)]
pub enum CudaError {
    /// Dynamic library loading failed to find a candidate library.
    Load(String),
    /// Symbol resolution failed for a required symbol.
    Symbol(String),
    /// A CUDA Driver API call returned an error code.
    Driver {
        op: &'static str,
        code: DriverError,
        msg: String,
    },
    /// VRAM memory region access out of bounds (offset + len > size).
    OutOfRange { off: usize, len: usize, size: usize },
}

impl fmt::Display for CudaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CudaError::Load(s) => write!(f, "failed to load CUDA library: {s}"),
            CudaError::Symbol(s) => write!(f, "required CUDA symbol missing: {s}"),
            CudaError::Driver { op, code, msg } => {
                write!(f, "{op} failed (CUresult={}): {msg}", code.as_i32())
            }
            CudaError::OutOfRange { off, len, size } => {
                write!(f, "out of bounds access: off={off} len={len} > size={size}")
            }
        }
    }
}

impl core::error::Error for CudaError {}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn test_driver_error_mapping() {
        assert_eq!(DriverError::from(2), DriverError::OutOfMemory);
        assert_eq!(DriverError::from(201), DriverError::InvalidContext);
        assert_eq!(DriverError::from(999), DriverError::Unknown(999));
        assert_eq!(DriverError::from(-1), DriverError::Unknown(-1));
    }

    #[test]
    fn test_driver_error_as_i32() {
        assert_eq!(DriverError::OutOfMemory.as_i32(), 2);
        assert_eq!(DriverError::Unknown(-1).as_i32(), -1);
    }
}
