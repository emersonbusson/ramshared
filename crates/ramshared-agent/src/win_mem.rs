//! Windows memory-pressure sampling boundary.
//!
//! The runtime sampler uses native GlobalMemoryStatusEx; a fallback is provided for non-Windows.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemorySample {
    pub available_bytes: u64,
    pub commit_limit_bytes: u64,
    pub load_percentage: u32,
}

#[cfg(windows)]
pub fn sample() -> Option<MemorySample> {
    use std::mem;
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::Foundation::GetLastError;

    let mut mem_status: MEMORYSTATUSEX = unsafe { mem::zeroed() };
    mem_status.dwLength = mem::size_of::<MEMORYSTATUSEX>() as u32;

    // SAFETY: We initialize `dwLength` correctly to the size of `MEMORYSTATUSEX`
    // before calling `GlobalMemoryStatusEx`. The passed pointer is valid as it
    // points to a stack-allocated, zero-initialized struct.
    let success = unsafe { GlobalMemoryStatusEx(&mut mem_status) };
    if success != 0 {
        Some(MemorySample {
            available_bytes: mem_status.ullAvailPhys,
            commit_limit_bytes: mem_status.ullTotalPageFile,
            load_percentage: mem_status.dwMemoryLoad,
        })
    } else {
        let _err = unsafe { GetLastError() };
        None
    }
}

#[cfg(not(windows))]
pub fn sample() -> Option<MemorySample> {
    None
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[cfg(not(windows))]
    #[test]
    fn sample_fallback_returns_none() {
        assert_eq!(sample(), None);
    }

    #[cfg(windows)]
    #[test]
    fn sample_native_returns_some() {
        assert!(sample().is_some());
    }
}
