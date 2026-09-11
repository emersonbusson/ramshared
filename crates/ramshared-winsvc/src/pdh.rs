//! Windows Performance Counters (PDH) host memory pressure evaluation (SPEC DT-1).
#![cfg(windows)]

use std::ptr;
use windows_sys::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterValue,
    PdhOpenQueryW, PDH_FMT_COUNTERVALUE, PDH_FMT_LARGE, PDH_HCOUNTER, PDH_HQUERY,
};

/// Safe RAII wrapper for PDH queries.
struct PdhQueryGuard {
    hquery: PDH_HQUERY,
}

impl PdhQueryGuard {
    fn new() -> Result<Self, String> {
        let mut hquery: PDH_HQUERY = ptr::null_mut();
        // SAFETY: PdhOpenQueryW initializes hquery. null args mean standard realtime source.
        let status = unsafe { PdhOpenQueryW(ptr::null(), 0, &mut hquery) };
        if status == 0 {
            Ok(Self { hquery })
        } else {
            Err(format!("PdhOpenQueryW failed with status: {:#010X}", status))
        }
    }
}

impl Drop for PdhQueryGuard {
    fn drop(&mut self) {
        // SAFETY: The handle was initialized in new() and is closed once on drop.
        unsafe {
            PdhCloseQuery(self.hquery);
        }
    }
}

/// Snapshot of host memory pressure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostMemoryPressure {
    pub available_mbytes: u64,
    pub pages_per_sec: u64,
}

impl HostMemoryPressure {
    /// Queries the host memory pressure using Windows Performance Counters.
    pub fn query() -> Result<Self, String> {
        let guard = PdhQueryGuard::new()?;

        let mut hcounter_avail: PDH_HCOUNTER = ptr::null_mut();
        // Path matches English PDH counter "\Memory\Available MBytes"
        let counter_path_avail: Vec<u16> = "\\Memory\\Available MBytes"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: We provide a null-terminated UTF-16 string and store the counter handle into hcounter_avail.
        let status = unsafe {
            PdhAddEnglishCounterW(
                guard.hquery,
                counter_path_avail.as_ptr(),
                0,
                &mut hcounter_avail,
            )
        };
        if status != 0 {
            return Err(format!("PdhAddEnglishCounterW(Available MBytes) failed: {:#010X}", status));
        }

        let mut hcounter_pages: PDH_HCOUNTER = ptr::null_mut();
        let counter_path_pages: Vec<u16> = "\\Memory\\Pages/sec"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: We provide a null-terminated UTF-16 string and store the counter handle into hcounter_pages.
        let status = unsafe {
            PdhAddEnglishCounterW(
                guard.hquery,
                counter_path_pages.as_ptr(),
                0,
                &mut hcounter_pages,
            )
        };
        if status != 0 {
            return Err(format!("PdhAddEnglishCounterW(Pages/sec) failed: {:#010X}", status));
        }

        // First sample for rate counters.
        // SAFETY: guard.hquery is a valid initialized handle.
        let status = unsafe { PdhCollectQueryData(guard.hquery) };
        if status != 0 {
            return Err(format!("PdhCollectQueryData (1st) failed: {:#010X}", status));
        }

        // Wait to gather delta for rate counter.
        std::thread::sleep(std::time::Duration::from_secs(1));

        // Second sample for rate counters.
        // SAFETY: guard.hquery is a valid initialized handle.
        let status = unsafe { PdhCollectQueryData(guard.hquery) };
        if status != 0 {
            return Err(format!("PdhCollectQueryData (2nd) failed: {:#010X}", status));
        }

        // SAFETY: mem::zeroed is safe for PDH_FMT_COUNTERVALUE.
        let mut fmt_value_avail: PDH_FMT_COUNTERVALUE = unsafe { std::mem::zeroed() };
        // SAFETY: hcounter_avail is a valid handle, output is written to properly initialized fmt_value_avail.
        let status = unsafe {
            PdhGetFormattedCounterValue(
                hcounter_avail,
                PDH_FMT_LARGE,
                ptr::null_mut(),
                &mut fmt_value_avail,
            )
        };
        if status != 0 {
            return Err(format!("PdhGetFormattedCounterValue(Available MBytes) failed: {:#010X}", status));
        }
        // SAFETY: Accessing union field largeValue which is valid for PDH_FMT_LARGE.
        let available_mbytes = unsafe { fmt_value_avail.Anonymous.largeValue } as u64;

        // SAFETY: mem::zeroed is safe for PDH_FMT_COUNTERVALUE.
        let mut fmt_value_pages: PDH_FMT_COUNTERVALUE = unsafe { std::mem::zeroed() };
        // SAFETY: hcounter_pages is a valid handle, output is written to properly initialized fmt_value_pages.
        let status = unsafe {
            PdhGetFormattedCounterValue(
                hcounter_pages,
                PDH_FMT_LARGE,
                ptr::null_mut(),
                &mut fmt_value_pages,
            )
        };
        if status != 0 {
            return Err(format!("PdhGetFormattedCounterValue(Pages/sec) failed: {:#010X}", status));
        }
        // SAFETY: Accessing union field largeValue which is valid for PDH_FMT_LARGE.
        let pages_per_sec = unsafe { fmt_value_pages.Anonymous.largeValue } as u64;

        Ok(Self {
            available_mbytes,
            pages_per_sec,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_memory_pressure_struct_properties() {
        let hmp = HostMemoryPressure {
            available_mbytes: 1024,
            pages_per_sec: 10,
        };
        assert_eq!(hmp.available_mbytes, 1024);
        assert_eq!(hmp.pages_per_sec, 10);
    }
}
