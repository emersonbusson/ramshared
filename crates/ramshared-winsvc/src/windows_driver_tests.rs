#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use std::time::Duration;

    #[test]
    fn test_create_disk_reserved_non_zero() {
        let mut link = WindowsDriverLink {
            handle: INVALID_HANDLE_VALUE,
            event: std::ptr::null_mut(),
            pending: false,
        };
        let params = DiskParams {
            size_bytes: 4096,
            block_size: 512,
            reserved: 1, // non-zero
            serial: [0; 16],
        };
        let res = link.create_disk(&params);
        match res {
            Err(IoctlError::Invalid(s)) => assert_eq!(s, "disk reserved non-zero"),
            _ => panic!("Expected IoctlError::Invalid"),
        }
    }

    #[test]
    fn test_register_queue_reserved_non_zero() {
        let mut link = WindowsDriverLink {
            handle: INVALID_HANDLE_VALUE,
            event: std::ptr::null_mut(),
            pending: false,
        };
        let reg = Register {
            abi_version: 1,
            disk_id: 0,
            queue_depth: 64,
            block_size: 512,
            max_io_bytes: 4096,
            reserved: 1, // non-zero
            sq_ring_va: 0,
            cq_ring_va: 0,
            data_area_va: 0,
            data_area_len: 0,
            sq_event_handle: 0,
            cq_event_handle: 0,
        };
        let res = link.register_queue(&reg);
        match res {
            Err(IoctlError::Invalid(s)) => assert_eq!(s, "register reserved non-zero"),
            _ => panic!("Expected IoctlError::Invalid"),
        }
    }

    #[test]
    fn test_register_queue_disk_id_non_zero() {
        let mut link = WindowsDriverLink {
            handle: INVALID_HANDLE_VALUE,
            event: std::ptr::null_mut(),
            pending: false,
        };
        let reg = Register {
            abi_version: 1,
            disk_id: 1, // non-zero
            queue_depth: 64,
            block_size: 512,
            max_io_bytes: 4096,
            reserved: 0,
            sq_ring_va: 0,
            cq_ring_va: 0,
            data_area_va: 0,
            data_area_len: 0,
            sq_event_handle: 0,
            cq_event_handle: 0,
        };
        let res = link.register_queue(&reg);
        match res {
            Err(IoctlError::Invalid(s)) => assert_eq!(s, "disk_id must be 0"),
            _ => panic!("Expected IoctlError::Invalid"),
        }
    }

    #[test]
    fn test_commit_and_fetch_already_pending() {
        let mut link = WindowsDriverLink {
            handle: INVALID_HANDLE_VALUE,
            event: std::ptr::null_mut(),
            pending: true, // already pending
        };
        let res = link.commit_and_fetch(Duration::from_secs(1));
        match res {
            Err(IoctlError::Invalid(s)) => assert_eq!(s, "commit already pending"),
            _ => panic!("Expected IoctlError::Invalid"),
        }

        // ensure pending flag stays true or gets changed? The code checks and returns without modifying pending flag
        assert_eq!(link.pending, true);
        link.pending = false; // clear for Drop
    }

    #[test]
    fn test_cancel_fetch_not_pending() {
        let mut link = WindowsDriverLink {
            handle: INVALID_HANDLE_VALUE,
            event: std::ptr::null_mut(),
            pending: false, // not pending
        };
        let res = link.cancel_fetch();
        assert!(res.is_ok());
    }
}
