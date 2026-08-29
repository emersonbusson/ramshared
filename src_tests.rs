#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;

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
}
