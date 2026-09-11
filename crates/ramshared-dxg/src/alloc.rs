//! Minimal DXG allocation and synchronization module.

use crate::{ioctl_mut, DxgError};
use std::fs::File;

pub mod ffi {
    pub const CREATE_ALLOCATION_IOCTL: u64 = 0xc048_4706;
    pub const DESTROY_ALLOCATION_IOCTL: u64 = 0xc018_4713;
    pub const WAIT_FOR_SYNCHRONIZATION_OBJECT_IOCTL: u64 = 0xc0c8_4712;
    pub const MAX_OBJECT_WAITED_ON: usize = 32;

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    pub struct D3dKmtHandle {
        pub handle: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub union CreateAllocationFlags {
        pub value: u32,
    }

    impl Default for CreateAllocationFlags {
        fn default() -> Self {
            Self { value: 0 }
        }
    }

    impl std::fmt::Debug for CreateAllocationFlags {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            // SAFETY: safe to read the union as `value` since it's just a u32 wrapper for bitfields.
            unsafe { write!(f, "CreateAllocationFlags {{ value: {:#x} }}", self.value) }
        }
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct AllocationInfo2 {
        pub allocation: D3dKmtHandle,
        pub sysmem: u64,
        pub priv_drv_data: u64,
        pub priv_drv_data_size: u32,
        pub vidpn_source_id: u32,
        pub flags_value: u32,
        pub h_section_or_unused: u64,
        pub reserved: [u64; 5],
    }

    impl Default for AllocationInfo2 {
        fn default() -> Self {
            // SAFETY: AllocationInfo2 is plain old data (POD) with standard u32/u64s and handles.
            // Zero-initializing it is safe and explicitly defined as the default.
            unsafe { std::mem::zeroed() }
        }
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct CreateAllocation {
        pub device: D3dKmtHandle,
        pub resource: D3dKmtHandle,
        pub global_share: D3dKmtHandle,
        pub reserved: u32,
        pub private_runtime_data: u64,
        pub private_runtime_data_size: u32,
        pub reserved1: u32,
        pub standard_allocation_or_priv: u64,
        pub priv_drv_data_size: u32,
        pub alloc_count: u32,
        pub allocation_info: u64,
        pub flags: CreateAllocationFlags,
        pub reserved2: u32,
        pub private_runtime_resource_handle: u64,
    }

    impl Default for CreateAllocation {
        fn default() -> Self {
            // SAFETY: CreateAllocation is plain old data (POD) with standard u32/u64s and handles.
            // Zero-initializing it is safe and explicitly defined as the default.
            unsafe { std::mem::zeroed() }
        }
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct DestroyAllocation2 {
        pub device: D3dKmtHandle,
        pub resource: D3dKmtHandle,
        pub allocation_info: u64,
        pub alloc_count: u32,
        pub flags_value: u32,
    }

    impl Default for DestroyAllocation2 {
        fn default() -> Self {
            // SAFETY: DestroyAllocation2 is plain old data (POD) with standard u32/u64s and handles.
            // Zero-initializing it is safe and explicitly defined as the default.
            unsafe { std::mem::zeroed() }
        }
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct WaitForSynchronizationObject2 {
        pub context: D3dKmtHandle,
        pub object_count: u32,
        pub object_array: [D3dKmtHandle; MAX_OBJECT_WAITED_ON],
        pub fence_or_reserved: [u64; 8],
    }

    impl Default for WaitForSynchronizationObject2 {
        fn default() -> Self {
            // SAFETY: WaitForSynchronizationObject2 is plain old data (POD).
            // Zero-initializing it is safe and explicitly defined as the default.
            unsafe { std::mem::zeroed() }
        }
    }
}

pub struct DxgAllocation<'a> {
    file: &'a File,
    device: ffi::D3dKmtHandle,
    resource: ffi::D3dKmtHandle,
    allocations: Vec<ffi::D3dKmtHandle>,
}

impl<'a> DxgAllocation<'a> {
    pub fn new(file: &'a File, mut request: ffi::CreateAllocation) -> Result<Self, DxgError> {
        if request.alloc_count == 0 {
            return Err(DxgError::Malformed("alloc_count"));
        }
        if request.allocation_info == 0 {
            return Err(DxgError::BadAddress);
        }

        ioctl_mut(file, ffi::CREATE_ALLOCATION_IOCTL, &mut request)?;

        // SAFETY: We explicitly validated alloc_count above. allocation_info points to a valid slice
        // created and passed from safe Rust code (e.g. AllocationInfo2 wrapper).
        // Since we cannot generically track the pointer lifetime in this low-level wrapper easily,
        // we copy the handles out into our RAII wrapper directly.
        let alloc_slice = unsafe {
            std::slice::from_raw_parts(
                request.allocation_info as *const ffi::AllocationInfo2,
                request.alloc_count as usize,
            )
        };

        let allocations = alloc_slice.iter().map(|info| info.allocation).collect();

        Ok(Self {
            file,
            device: request.device,
            resource: request.resource,
            allocations,
        })
    }
}

impl<'a> Drop for DxgAllocation<'a> {
    fn drop(&mut self) {
        if self.resource.handle != 0 {
            let mut destroy = ffi::DestroyAllocation2 {
                device: self.device,
                resource: self.resource,
                ..Default::default()
            };
            let _ = ioctl_mut(self.file, ffi::DESTROY_ALLOCATION_IOCTL, &mut destroy);
        } else if !self.allocations.is_empty() {
            let mut destroy = ffi::DestroyAllocation2 {
                device: self.device,
                allocation_info: self.allocations.as_ptr() as u64,
                alloc_count: self.allocations.len() as u32,
                ..Default::default()
            };
            let _ = ioctl_mut(self.file, ffi::DESTROY_ALLOCATION_IOCTL, &mut destroy);
        }
    }
}

/// Waits for a synchronization object (e.g. fence) to be signaled.
pub fn wait_for_synchronization_object(
    file: &File,
    request: &mut ffi::WaitForSynchronizationObject2,
) -> Result<(), DxgError> {
    if request.object_count == 0 || request.object_count as usize > ffi::MAX_OBJECT_WAITED_ON {
        return Err(DxgError::Malformed("object_count"));
    }
    ioctl_mut(file, ffi::WAIT_FOR_SYNCHRONIZATION_OBJECT_IOCTL, request)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_alloc_uapi_layouts_and_ioctl_numbers_match_wsl_618() {
        assert_eq!(ffi::CREATE_ALLOCATION_IOCTL, 0xc048_4706);
        assert_eq!(ffi::DESTROY_ALLOCATION_IOCTL, 0xc018_4713);
        assert_eq!(ffi::WAIT_FOR_SYNCHRONIZATION_OBJECT_IOCTL, 0xc0c8_4712);
        assert_eq!(std::mem::size_of::<ffi::CreateAllocation>(), 72);
        assert_eq!(std::mem::size_of::<ffi::DestroyAllocation2>(), 24);
        assert_eq!(std::mem::size_of::<ffi::AllocationInfo2>(), 88);
        assert_eq!(std::mem::size_of::<ffi::WaitForSynchronizationObject2>(), 200);
    }

    #[test]
    fn validate_alloc_rejects_malformed_requests() {
        let file_res = File::open("/dev/null");
        if file_res.is_err() {
            return;
        }
        let file = file_res.unwrap();

        let mut alloc = ffi::CreateAllocation::default();
        alloc.alloc_count = 0;
        assert_eq!(
            DxgAllocation::new(&file, alloc).err(),
            Some(DxgError::Malformed("alloc_count"))
        );

        alloc.alloc_count = 1;
        alloc.allocation_info = 0; // NULL pointer
        assert_eq!(
            DxgAllocation::new(&file, alloc).err(),
            Some(DxgError::BadAddress)
        );
    }

    #[test]
    fn validate_wait_rejects_malformed_requests() {
        let file_res = File::open("/dev/null");
        if file_res.is_err() {
            return;
        }
        let file = file_res.unwrap();

        let mut wait = ffi::WaitForSynchronizationObject2::default();
        wait.object_count = 0;
        assert_eq!(
            wait_for_synchronization_object(&file, &mut wait),
            Err(DxgError::Malformed("object_count"))
        );

        wait.object_count = ffi::MAX_OBJECT_WAITED_ON as u32 + 1;
        assert_eq!(
            wait_for_synchronization_object(&file, &mut wait),
            Err(DxgError::Malformed("object_count"))
        );
    }
}
