//! Minimal `/dev/dxg` WDDM video-memory budget provider.
//!
//! The layouts mirror Microsoft's WSL 6.18 `d3dkmthk.h`. This crate is the
//! only `unsafe` boundary for dxg ioctls; policy remains in safe Rust.

pub mod alloc;
pub mod error;

pub use error::DxgError;

use std::fmt;
use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::path::Path;
use std::time::Instant;

unsafe extern "C" {
    fn ioctl(fd: i32, request: u64, ...) -> i32;
}

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

/// A minimal `/dev/dxg` client for video memory metrics.
pub struct DxgBudgetProvider {
    file: File,
    adapter_handle: u32,
    adapter_luid: AdapterLuid,
}

impl Drop for DxgBudgetProvider {
    fn drop(&mut self) {
        close_adapter(&self.file, self.adapter_handle);
    }
}

fn select_adapter(
    infos: &[uapi::AdapterInfo],
    requested: Option<AdapterLuid>,
) -> Result<AdapterLuid, DxgError> {
    match infos {
        [single] if requested.is_none() => Ok(AdapterLuid {
            low: single.luid_low,
            high: single.luid_high,
        }),
        _ if requested.is_some() => {
            let luid = requested.unwrap_or_else(|| unreachable!());
            infos
                .iter()
                .find(|info| info.luid_low == luid.low && info.luid_high == luid.high)
                .map(|_| luid)
                .ok_or(DxgError::AdapterNotFound(luid))
        }
        [] => Err(DxgError::NoAdapters),
        many => Err(DxgError::AmbiguousAdapters(many.len())),
    }
}

impl DxgBudgetProvider {
    /// Opens `/dev/dxg` and claims an adapter, matching `requested` if provided
    /// or defaulting to the only adapter if unambiguous.
    pub fn open(requested: Option<AdapterLuid>) -> Result<Self, DxgError> {
        Self::open_path("/dev/dxg", requested)
    }

    fn open_path(
        path: impl AsRef<Path>,
        requested: Option<AdapterLuid>,
    ) -> Result<Self, DxgError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|error| DxgError::Unavailable(error.to_string()))?;
        let infos = enumerate(&file)?;
        Self::from_infos(file, infos, requested)
    }

    fn from_infos(
        file: File,
        infos: Vec<uapi::AdapterInfo>,
        requested: Option<AdapterLuid>,
    ) -> Result<Self, DxgError> {
        let selected = select_adapter(
            &infos,
            requested,
        )?;
        let mut handle = 0;
        for info in infos {
            if info.luid_low == selected.low && info.luid_high == selected.high {
                handle = info.adapter_handle;
            } else {
                close_adapter(&file, info.adapter_handle);
            }
        }
        if handle == 0 {
            return Err(DxgError::AdapterNotFound(selected));
        }
        if handle == u32::MAX {
            return Err(DxgError::Malformed("adapter_handle"));
        }
        Ok(Self {
            file,
            adapter_handle: handle,
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
            // 0 is DXGK_SEGMENT_GROUP_LOCAL (VRAM). 1 would be Non-Local (GART).
            memory_segment_group: 0,
            ..Default::default()
        };
        ioctl_mut(&self.file, uapi::QUERY_VIDEO_MEMORY_INFO_IOCTL, &mut query)?;
        validate_query(&query)?;
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

fn enumerate(file: &File) -> Result<Vec<uapi::AdapterInfo>, DxgError> {
    let mut infos = vec![uapi::AdapterInfo::default(); uapi::MAX_ADAPTERS];
    let mut request = uapi::EnumAdapters2 {
        num_adapters: infos.len() as u32,
        ..Default::default()
    };
    request.adapters = infos.as_mut_ptr() as u64;
    ioctl_mut(file, uapi::ENUM_ADAPTERS2_IOCTL, &mut request)?;
    validate_enum(&request, Some(infos.len()))?;
    infos.truncate(request.num_adapters as usize);
    Ok(infos)
}

fn validate_enum(request: &uapi::EnumAdapters2, capacity: Option<usize>) -> Result<(), DxgError> {
    if request.reserved != 0 {
        return Err(DxgError::Malformed("enum.reserved"));
    }
    if request.num_adapters == 0 {
        return Err(DxgError::NoAdapters);
    }
    let limit = capacity.unwrap_or(uapi::MAX_ADAPTERS);
    if request.num_adapters as usize > limit {
        return Err(DxgError::TooManyAdapters(request.num_adapters));
    }
    Ok(())
}

fn validate_query(query: &uapi::QueryVideoMemoryInfo) -> Result<(), DxgError> {
    if query.process != 0 {
        return Err(DxgError::Malformed("process"));
    }
    if query.adapter == 0 {
        return Err(DxgError::Malformed("adapter"));
    }
    Ok(())
}

fn close_adapter(file: &File, handle: u32) {
    let mut handle = handle;
    let _ = ioctl_mut(file, uapi::CLOSE_ADAPTER_IOCTL, &mut handle);
}

pub(crate) fn ioctl_mut<T>(file: &File, request: u64, value: &mut T) -> Result<(), DxgError> {
    // SAFETY: `value` points to the exact repr(C) layout for `request` and stays
    // alive for the synchronous ioctl. The kernel validates nested pointers.
    let result = unsafe { ioctl(file.as_raw_fd(), request, value as *mut T) };
    if result < 0 {
        Err(DxgError::from_sys_error(std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AdapterLuid, BudgetSnapshot, DxgBudgetProvider, GpuBudgetProvider, select_adapter,
    };
    use crate::DxgError;

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
        assert_eq!(select_adapter(&[super::uapi::AdapterInfo { luid_low: a.low, luid_high: a.high, ..Default::default() }], None), Ok(a));
        assert!(select_adapter(&[], None).is_err());
        assert!(select_adapter(&[super::uapi::AdapterInfo { luid_low: a.low, luid_high: a.high, ..Default::default() }, super::uapi::AdapterInfo { luid_low: b.low, luid_high: b.high, ..Default::default() }], None).is_err());
        assert_eq!(select_adapter(&[super::uapi::AdapterInfo { luid_low: a.low, luid_high: a.high, ..Default::default() }, super::uapi::AdapterInfo { luid_low: b.low, luid_high: b.high, ..Default::default() }], Some(b)), Ok(b));
        assert!(select_adapter(&[super::uapi::AdapterInfo { luid_low: a.low, luid_high: a.high, ..Default::default() }], Some(b)).is_err());
    }

    #[test]
    fn provider_trait_carries_host_budget_fields() {
        struct Fake;
        impl GpuBudgetProvider for Fake {
            fn snapshot(&self) -> Result<BudgetSnapshot, DxgError> {
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
        assert!(DxgError::Unavailable("missing".into()).permits_startup_fallback());
        assert!(!DxgError::Io("ioctl".into()).permits_startup_fallback());
        assert!(!DxgError::Malformed("process").permits_startup_fallback());
        assert!(!DxgError::NoAdapters.permits_startup_fallback());
        assert!(!DxgError::TooManyAdapters(65).permits_startup_fallback());
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

    #[test]
    fn ioctl_maps_kernel_errors_to_typed_variants() {
        assert_eq!(
            DxgError::from_sys_error(std::io::Error::from_raw_os_error(libc::ENODEV)),
            DxgError::DeviceNotFound
        );
        assert_eq!(
            DxgError::from_sys_error(std::io::Error::from_raw_os_error(libc::ENOTTY)),
            DxgError::UnsupportedHardware
        );
        assert_eq!(
            DxgError::from_sys_error(std::io::Error::from_raw_os_error(libc::EOVERFLOW)),
            DxgError::BufferOverflow
        );
        assert_eq!(
            DxgError::from_sys_error(std::io::Error::from_raw_os_error(libc::EACCES)),
            DxgError::PermissionDenied
        );
        assert_eq!(
            DxgError::from_sys_error(std::io::Error::from_raw_os_error(libc::EPERM)),
            DxgError::PermissionDenied
        );
        assert_eq!(
            DxgError::from_sys_error(std::io::Error::from_raw_os_error(libc::EFAULT)),
            DxgError::BadAddress
        );
        let unknown = DxgError::from_sys_error(std::io::Error::from_raw_os_error(9999));
        assert!(matches!(unknown, DxgError::Io(_)));
    }

    #[test]
    fn all_error_messages_and_luid_are_stable() {
        let luid = AdapterLuid {
            low: 0x12,
            high: 0x34,
        };
        assert_eq!(luid.to_string(), "00000034:00000012");
        let cases = [
            DxgError::Unavailable("gone".into()),
            DxgError::Io("bad".into()),
            DxgError::NoAdapters,
            DxgError::AmbiguousAdapters(2),
            DxgError::AdapterNotFound(luid),
            DxgError::TooManyAdapters(65),
            DxgError::Malformed("field"),
            DxgError::DeviceNotFound,
            DxgError::UnsupportedHardware,
            DxgError::BufferOverflow,
            DxgError::PermissionDenied,
            DxgError::BadAddress,
        ];
        for error in cases {
            assert!(!error.to_string().is_empty());
        }
    }

    #[test]
    fn live_provider_rejects_unknown_requested_luid() {
        if !std::path::Path::new("/dev/dxg").exists() {
            return;
        }
        let missing = AdapterLuid {
            low: u32::MAX,
            high: u32::MAX,
        };
        assert!(matches!(
            DxgBudgetProvider::open(Some(missing)),
            Err(DxgError::AdapterNotFound(value)) if value == missing
        ));
    }

    #[test]
    fn injected_multi_adapter_closes_unselected_and_builds_selected() {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/null")
            .unwrap_or_else(|error| panic!("open /dev/null: {error}"));
        let infos = vec![
            super::uapi::AdapterInfo {
                adapter_handle: 1,
                luid_low: 10,
                luid_high: 11,
                ..Default::default()
            },
            super::uapi::AdapterInfo {
                adapter_handle: 2,
                luid_low: 20,
                luid_high: 21,
                ..Default::default()
            },
        ];
        let selected = AdapterLuid { low: 20, high: 21 };
        let provider = DxgBudgetProvider::from_infos(file, infos, Some(selected))
            .unwrap_or_else(|error| panic!("from_infos: {error}"));
        assert_eq!(provider.adapter_luid(), selected);
    }

    #[test]
    fn malformed_enum_and_query_variants_are_rejected() {
        let mut request = super::uapi::EnumAdapters2 {
            num_adapters: 1,
            reserved: 1,
            adapters: 0,
        };
        assert_eq!(
            super::validate_enum(&request, None),
            Err(DxgError::Malformed("enum.reserved"))
        );
        request.reserved = 0;
        request.num_adapters = 0;
        assert_eq!(
            super::validate_enum(&request, None),
            Err(DxgError::NoAdapters)
        );
        request.num_adapters = 65;
        assert_eq!(
            super::validate_enum(&request, None),
            Err(DxgError::TooManyAdapters(65))
        );
        request.num_adapters = 2;
        assert_eq!(
            super::validate_enum(&request, Some(1)),
            Err(DxgError::TooManyAdapters(2))
        );
        request.num_adapters = 1;
        assert_eq!(super::validate_enum(&request, Some(1)), Ok(()));

        let mut query = super::uapi::QueryVideoMemoryInfo {
            process: 1,
            ..Default::default()
        };
        assert_eq!(
            super::validate_query(&query),
            Err(DxgError::Malformed("process"))
        );
        query.process = 0;
        assert_eq!(
            super::validate_query(&query),
            Err(DxgError::Malformed("adapter"))
        );
        query.adapter = 1;
        assert_eq!(super::validate_query(&query), Ok(()));
    }
}
