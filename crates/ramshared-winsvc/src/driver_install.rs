use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(windows)]
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
#[cfg(windows)]
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteKeyW, RegOpenKeyExW, RegSetValueExW, HKEY,
    HKEY_LOCAL_MACHINE, KEY_ALL_ACCESS, REG_DWORD, REG_SZ, REG_EXPAND_SZ, REG_VALUE_TYPE,
};

/// A transactional wrapper for driver installation that rolls back copied files
/// and created registry keys upon failure (unless explicitly committed).
pub struct DriverInstallTransaction {
    files_to_delete: Vec<PathBuf>,
    #[cfg_attr(not(windows), allow(dead_code))]
    registry_keys_to_delete: Vec<Vec<u16>>,
    committed: bool,
}

impl Default for DriverInstallTransaction {
    fn default() -> Self {
        Self::new()
    }
}

impl DriverInstallTransaction {
    /// Creates a new driver installation transaction.
    pub fn new() -> Self {
        Self {
            files_to_delete: Vec::new(),
            registry_keys_to_delete: Vec::new(),
            committed: false,
        }
    }

    /// Copies a driver file to the destination path and tracks it for rollback.
    ///
    /// # Errors
    /// Returns an `io::Error` if the file could not be copied.
    pub fn copy_file<P: AsRef<Path>, Q: AsRef<Path>>(
        &mut self,
        from: P,
        to: Q,
    ) -> io::Result<u64> {
        let size = fs::copy(from, to.as_ref())?;
        self.files_to_delete.push(to.as_ref().to_path_buf());
        Ok(size)
    }

    /// Helper to convert a string to a null-terminated UTF-16 representation
    #[cfg(windows)]
    fn str_to_utf16(s: &str) -> Vec<u16> {
        let mut wide: Vec<u16> = s.encode_utf16().collect();
        wide.push(0); // null terminator
        wide
    }

    /// Creates a registry key under `HKEY_LOCAL_MACHINE` and tracks it for rollback.
    ///
    /// # Errors
    /// Returns an `io::Error` if the key could not be created.
    #[cfg(windows)]
    pub fn create_registry_key(&mut self, subkey: &str) -> io::Result<()> {
        let subkey_wide = Self::str_to_utf16(subkey);
        let mut hkey: HKEY = 0;

        // SAFETY: Calling RegCreateKeyExW is safe because we pass valid utf16 null-terminated pointers and a valid output HKEY reference.
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_LOCAL_MACHINE,
                subkey_wide.as_ptr(),
                0,
                std::ptr::null(),
                0,
                KEY_ALL_ACCESS,
                std::ptr::null(),
                &mut hkey,
                std::ptr::null_mut(),
            )
        };

        if status == ERROR_SUCCESS {
            // SAFETY: RegCloseKey is safe to call with a valid, successfully opened HKEY.
            unsafe { RegCloseKey(hkey) };
            self.registry_keys_to_delete.push(subkey_wide);
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(status as i32))
        }
    }

    #[cfg(not(windows))]
    pub fn create_registry_key(&mut self, _subkey: &str) -> io::Result<()> {
        Ok(())
    }

    /// Helper to convert a string to a null-terminated UTF-16 byte slice for Windows Registry
    #[cfg(windows)]
    fn str_to_utf16_bytes(s: &str) -> Vec<u8> {
        let wide = Self::str_to_utf16(s);
        let byte_len = wide.len() * 2;
        let mut bytes = Vec::with_capacity(byte_len);
        for w in wide {
            bytes.extend_from_slice(&w.to_ne_bytes());
        }
        bytes
    }

    /// Sets a string value on an existing registry key under `HKEY_LOCAL_MACHINE`.
    /// This requires the key to already exist.
    ///
    /// # Errors
    /// Returns an `io::Error` if the value could not be set.
    #[cfg(windows)]
    pub fn set_registry_value_sz(&mut self, subkey: &str, value_name: &str, data: &str) -> io::Result<()> {
        let bytes = Self::str_to_utf16_bytes(data);
        self.set_registry_value(subkey, value_name, REG_SZ, &bytes)
    }

    #[cfg(not(windows))]
    pub fn set_registry_value_sz(&mut self, _subkey: &str, _value_name: &str, _data: &str) -> io::Result<()> {
        Ok(())
    }

    /// Sets a string value (with expandable environment variables) on an existing registry key.
    ///
    /// # Errors
    /// Returns an `io::Error` if the value could not be set.
    #[cfg(windows)]
    pub fn set_registry_value_expand_sz(&mut self, subkey: &str, value_name: &str, data: &str) -> io::Result<()> {
        let bytes = Self::str_to_utf16_bytes(data);
        self.set_registry_value(subkey, value_name, REG_EXPAND_SZ, &bytes)
    }

    #[cfg(not(windows))]
    pub fn set_registry_value_expand_sz(&mut self, _subkey: &str, _value_name: &str, _data: &str) -> io::Result<()> {
        Ok(())
    }

    /// Sets a DWORD value on an existing registry key.
    ///
    /// # Errors
    /// Returns an `io::Error` if the value could not be set.
    #[cfg(windows)]
    pub fn set_registry_value_dword(&mut self, subkey: &str, value_name: &str, data: u32) -> io::Result<()> {
        self.set_registry_value(subkey, value_name, REG_DWORD, &data.to_ne_bytes())
    }

    #[cfg(not(windows))]
    pub fn set_registry_value_dword(&mut self, _subkey: &str, _value_name: &str, _data: u32) -> io::Result<()> {
        Ok(())
    }

    #[cfg(windows)]
    fn set_registry_value(
        &mut self,
        subkey: &str,
        value_name: &str,
        value_type: REG_VALUE_TYPE,
        data: &[u8],
    ) -> io::Result<()> {
        let subkey_wide = Self::str_to_utf16(subkey);
        let value_name_wide = Self::str_to_utf16(value_name);
        let mut hkey: HKEY = 0;

        // SAFETY: Calling RegOpenKeyExW is safe because we pass a valid utf16 null-terminated pointer and a valid output HKEY reference.
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                subkey_wide.as_ptr(),
                0,
                KEY_ALL_ACCESS,
                &mut hkey,
            )
        };

        if status == ERROR_SUCCESS {
            // SAFETY: RegSetValueExW is safe because hkey is valid, value_name_wide is valid, and the data buffer is correctly sized.
            let set_status = unsafe {
                RegSetValueExW(
                    hkey,
                    value_name_wide.as_ptr(),
                    0,
                    value_type,
                    data.as_ptr(),
                    data.len() as u32,
                )
            };
            // SAFETY: RegCloseKey is safe to call with a valid, successfully opened HKEY.
            unsafe { RegCloseKey(hkey) };
            if set_status == ERROR_SUCCESS {
                Ok(())
            } else {
                Err(io::Error::from_raw_os_error(set_status as i32))
            }
        } else {
            Err(io::Error::from_raw_os_error(status as i32))
        }
    }

    /// Commits the transaction, preventing the rollback of files and registry keys.
    pub fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for DriverInstallTransaction {
    fn drop(&mut self) {
        if !self.committed {
            // Delete files in reverse order
            for file in self.files_to_delete.iter().rev() {
                let _ = fs::remove_file(file);
            }

            // Delete registry keys in reverse order (to delete subkeys before parents)
            #[cfg(windows)]
            for subkey in self.registry_keys_to_delete.iter().rev() {
                // SAFETY: RegDeleteKeyW is safe because we are passing a valid null-terminated utf16 pointer for a previously created key.
                unsafe {
                    RegDeleteKeyW(
                        HKEY_LOCAL_MACHINE,
                        subkey.as_ptr(),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_rollback_files() {
        use std::io::Write;
        let temp_dir = std::env::temp_dir();
        let file1 = temp_dir.join("test_rollback_1.txt");
        let file2 = temp_dir.join("test_rollback_2.txt");

        {
            let mut file = fs::File::create(&file1).unwrap_or_else(|_| panic!("Failed to create file1"));
            writeln!(file, "hello").unwrap_or_else(|_| panic!("Failed to write to file1"));
        }

        {
            let mut tx = DriverInstallTransaction::new();
            tx.copy_file(&file1, &file2).unwrap_or_else(|_| panic!("Failed to copy file"));
            assert!(file2.exists());
            // Drops without commit, so file2 should be deleted
        }

        assert!(!file2.exists());
        let _ = fs::remove_file(&file1);
    }

    #[test]
    fn test_transaction_commit_files() {
        use std::io::Write;
        let temp_dir = std::env::temp_dir();
        let file1 = temp_dir.join("test_commit_1.txt");
        let file2 = temp_dir.join("test_commit_2.txt");

        {
            let mut file = fs::File::create(&file1).unwrap_or_else(|_| panic!("Failed to create file1"));
            writeln!(file, "hello").unwrap_or_else(|_| panic!("Failed to write to file1"));
        }

        {
            let mut tx = DriverInstallTransaction::new();
            tx.copy_file(&file1, &file2).unwrap_or_else(|_| panic!("Failed to copy file"));
            tx.commit();
        }

        assert!(file2.exists());
        let _ = fs::remove_file(&file1);
        let _ = fs::remove_file(&file2);
    }
}
