#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_disk_reserved_non_zero() {
        let mut link = WindowsDriverLink {
            handle: INVALID_HANDLE_VALUE, // won't use handle because it errors out early
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

        // ensure handle wasn't used/closed if we bypass it? Wait, drop trait calls CloseHandle on event and handle!
        // INVALID_HANDLE_VALUE doesn't get closed. std::ptr::null_mut() doesn't get closed.
        // so we can construct a dummy one!
    }
}
