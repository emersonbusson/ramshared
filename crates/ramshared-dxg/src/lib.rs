//! Minimal `/dev/dxg` WDDM video-memory budget provider.
//!
//! The layouts mirror Microsoft's WSL 6.18 `d3dkmthk.h`. This crate is the
//! only `unsafe` boundary for dxg ioctls; policy remains in safe Rust.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::path::Path;
use std::time::Instant;

pub mod uapi {
    pub const ENUM_ADAPTERS2_IOCTL: u64 = 0xc010_4714;
    pub const QUERY_VIDEO_MEMORY_INFO_IOCTL: u64 = 0xc038_470a;
    pub const CLOSE_ADAPTER_IOCTL: u64 = 0xc004_4715;
    pub const MAX_ADAPTERS: usize = 64;

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default)]
    pub struct AdapterInfo {
        pub adapter_handle: u32,
        pub luid_low: u32,
        pub luid_high: u32,
        pub num_sources: u32,
        pub present_move_regions_preferred: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default)]
    pub struct EnumAdapters2 {
        pub num_adapters: u32,
        pub reserved: u32,
        pub adapters: u64,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default)]
    pub struct QueryVideoMemoryInfo {
        pub process: u64,
        pub adapter: u32,
        pub memory_segment_group: i32,
        pub budget: u64,
        pub current_usage: u64,
        pub current_reservation: u64,
        pub available_for_reservation: u64,
        pub physical_adapter_index: u32,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdapterLuid {
    pub low: u32,
    pub high: u32,
}

impl fmt::Display for AdapterLuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:08x}:{:08x}", self.high, self.low)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BudgetSnapshot {
    pub adapter: AdapterLuid,
    pub budget: u64,
    pub current_usage: u64,
    pub current_reservation: u64,
    pub available_for_reservation: u64,
    pub sampled_at: Instant,
}

pub trait GpuBudgetProvider {
    fn snapshot(&self) -> Result<BudgetSnapshot, DxgError>;
}

#[derive(Debug, Eq, PartialEq)]
pub enum DxgError {
    Unavailable(String),
    Io(String),
    NoAdapters,
    AmbiguousAdapters(usize),
    AdapterNotFound(AdapterLuid),
    TooManyAdapters(u32),
    Malformed(&'static str),
}

impl fmt::Display for DxgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(message) => write!(f, "dxg unavailable: {message}"),
            Self::Io(message) => write!(f, "dxg ioctl failed: {message}"),
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
}

pub fn select_adapter(
    adapters: &[AdapterLuid],
    requested: Option<AdapterLuid>,
) -> Result<AdapterLuid, DxgError> {
    if let Some(luid) = requested {
        return adapters
            .iter()
            .copied()
            .find(|candidate| *candidate == luid)
            .ok_or(DxgError::AdapterNotFound(luid));
    }
    match adapters {
        [] => Err(DxgError::NoAdapters),
        [only] => Ok(*only),
        many => Err(DxgError::AmbiguousAdapters(many.len())),
    }
}

pub struct DxgBudgetProvider {
    file: File,
    adapter_handle: u32,
    adapter_luid: AdapterLuid,
}

impl DxgBudgetProvider {
    pub fn open(requested: Option<AdapterLuid>) -> Result<Self, DxgError> {
        Self::open_path("/dev/dxg", requested)
    }

    pub fn open_path(
        path: impl AsRef<Path>,
        requested: Option<AdapterLuid>,
    ) -> Result<Self, DxgError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|error| DxgError::Unavailable(error.to_string()))?;
        let infos = enumerate(&file)?;
        let luids: Vec<_> = infos
            .iter()
            .map(|info| AdapterLuid {
                low: info.luid_low,
                high: info.luid_high,
            })
            .collect();
        let selected = select_adapter(&luids, requested)?;
        let selected_info = infos
            .iter()
            .find(|info| info.luid_low == selected.low && info.luid_high == selected.high)
            .copied()
            .ok_or(DxgError::AdapterNotFound(selected))?;
        for info in &infos {
            if info.adapter_handle != selected_info.adapter_handle {
                close_adapter(&file, info.adapter_handle);
            }
        }
        Ok(Self {
            file,
            adapter_handle: selected_info.adapter_handle,
            adapter_luid: selected,
        })
    }

    pub fn adapter_luid(&self) -> AdapterLuid {
        self.adapter_luid
    }
}

impl GpuBudgetProvider for DxgBudgetProvider {
    fn snapshot(&self) -> Result<BudgetSnapshot, DxgError> {
        let mut query = uapi::QueryVideoMemoryInfo {
            adapter: self.adapter_handle,
            memory_segment_group: 0,
            ..Default::default()
        };
        ioctl_mut(&self.file, uapi::QUERY_VIDEO_MEMORY_INFO_IOCTL, &mut query)?;
        if query.process != 0 {
            return Err(DxgError::Malformed("process"));
        }
        Ok(BudgetSnapshot {
            adapter: self.adapter_luid,
            budget: query.budget,
            current_usage: query.current_usage,
            current_reservation: query.current_reservation,
            available_for_reservation: query.available_for_reservation,
            sampled_at: Instant::now(),
        })
    }
}

impl Drop for DxgBudgetProvider {
    fn drop(&mut self) {
        close_adapter(&self.file, self.adapter_handle);
    }
}

fn enumerate(file: &File) -> Result<Vec<uapi::AdapterInfo>, DxgError> {
    let mut request = uapi::EnumAdapters2::default();
    ioctl_mut(file, uapi::ENUM_ADAPTERS2_IOCTL, &mut request)?;
    if request.reserved != 0 {
        return Err(DxgError::Malformed("enum.reserved"));
    }
    if request.num_adapters == 0 {
        return Err(DxgError::NoAdapters);
    }
    if request.num_adapters as usize > uapi::MAX_ADAPTERS {
        return Err(DxgError::TooManyAdapters(request.num_adapters));
    }
    let mut infos = vec![uapi::AdapterInfo::default(); request.num_adapters as usize];
    request.adapters = infos.as_mut_ptr() as u64;
    ioctl_mut(file, uapi::ENUM_ADAPTERS2_IOCTL, &mut request)?;
    if request.num_adapters as usize > infos.len() {
        return Err(DxgError::TooManyAdapters(request.num_adapters));
    }
    infos.truncate(request.num_adapters as usize);
    Ok(infos)
}

fn close_adapter(file: &File, handle: u32) {
    let mut handle = handle;
    let _ = ioctl_mut(file, uapi::CLOSE_ADAPTER_IOCTL, &mut handle);
}

fn ioctl_mut<T>(file: &File, request: u64, value: &mut T) -> Result<(), DxgError> {
    unsafe extern "C" {
        fn ioctl(fd: i32, request: u64, ...) -> i32;
    }
    // SAFETY: `value` points to the exact repr(C) layout for `request` and stays
    // alive for the synchronous ioctl. The kernel validates nested pointers.
    let result = unsafe { ioctl(file.as_raw_fd(), request, value as *mut T) };
    if result < 0 {
        Err(DxgError::Io(std::io::Error::last_os_error().to_string()))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AdapterLuid, BudgetSnapshot, DxgBudgetProvider, GpuBudgetProvider, select_adapter,
    };

    #[test]
    fn official_uapi_layouts_and_ioctl_numbers_match_wsl_618() {
        assert_eq!(super::uapi::ENUM_ADAPTERS2_IOCTL, 0xc010_4714);
        assert_eq!(super::uapi::QUERY_VIDEO_MEMORY_INFO_IOCTL, 0xc038_470a);
        assert_eq!(super::uapi::CLOSE_ADAPTER_IOCTL, 0xc004_4715);
        assert_eq!(size_of::<super::uapi::EnumAdapters2>(), 16);
        assert_eq!(size_of::<super::uapi::AdapterInfo>(), 20);
        assert_eq!(size_of::<super::uapi::QueryVideoMemoryInfo>(), 56);
    }

    #[test]
    fn adapter_selection_rejects_ambiguity() {
        let a = AdapterLuid { low: 1, high: 2 };
        let b = AdapterLuid { low: 3, high: 4 };
        assert_eq!(select_adapter(&[a], None), Ok(a));
        assert!(select_adapter(&[], None).is_err());
        assert!(select_adapter(&[a, b], None).is_err());
        assert_eq!(select_adapter(&[a, b], Some(b)), Ok(b));
        assert!(select_adapter(&[a], Some(b)).is_err());
    }

    #[test]
    fn provider_trait_carries_host_budget_fields() {
        struct Fake;
        impl GpuBudgetProvider for Fake {
            fn snapshot(&self) -> Result<BudgetSnapshot, super::DxgError> {
                Ok(BudgetSnapshot {
                    adapter: AdapterLuid { low: 7, high: 8 },
                    budget: 100,
                    current_usage: 40,
                    current_reservation: 10,
                    available_for_reservation: 60,
                    sampled_at: std::time::Instant::now(),
                })
            }
        }
        let snap = Fake
            .snapshot()
            .unwrap_or_else(|error| panic!("unexpected error: {error}"));
        assert_eq!(snap.budget, 100);
        let _type_check: Option<DxgBudgetProvider> = None;
    }

    #[test]
    fn only_unavailable_device_permits_cuda_fallback() {
        assert!(super::DxgError::Unavailable("missing".into()).permits_startup_fallback());
        assert!(!super::DxgError::Io("ioctl".into()).permits_startup_fallback());
        assert!(!super::DxgError::Malformed("process").permits_startup_fallback());
        assert!(!super::DxgError::NoAdapters.permits_startup_fallback());
        assert!(!super::DxgError::TooManyAdapters(65).permits_startup_fallback());
    }

    #[test]
    fn live_provider_queries_budget_when_dxg_exists() {
        if !std::path::Path::new("/dev/dxg").exists() {
            return;
        }
        let provider = DxgBudgetProvider::open(None)
            .unwrap_or_else(|error| panic!("live dxg open failed: {error}"));
        let snapshot = provider
            .snapshot()
            .unwrap_or_else(|error| panic!("live dxg query failed: {error}"));
        assert_eq!(snapshot.adapter, provider.adapter_luid());
        assert!(snapshot.budget > 0);
    }

    #[test]
    fn missing_path_is_unavailable_but_non_dxg_ioctl_is_not() {
        let missing = DxgBudgetProvider::open_path("/definitely/missing/dxg", None)
            .err()
            .unwrap_or_else(|| panic!("missing path unexpectedly opened"));
        assert!(missing.permits_startup_fallback());
        let invalid = DxgBudgetProvider::open_path("/dev/null", None)
            .err()
            .unwrap_or_else(|| panic!("/dev/null unexpectedly behaved like dxg"));
        assert!(!invalid.permits_startup_fallback());
    }
}
