use std::path::Path;
use crate::Config;
use crate::ConfigError;

/// Load configuration from a file, validating permissions before parsing.
pub fn load_file<P: AsRef<Path>>(path: P) -> Result<Config, ConfigError> {
    let path = path.as_ref();
    validate_permissions(path)?;
    let text = read_secure(path)?;
    Config::parse(&text)
}

pub fn read_secure(path: &Path) -> Result<String, ConfigError> {
    if std::env::var_os("RAMSHARED_DISABLE_PERM_CHECK").is_none() {
        validate_permissions(path)?;
    }
    std::fs::read_to_string(path).map_err(|e| ConfigError::InvalidInput(e.to_string()))
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn validate_permissions(path: &Path) -> Result<(), ConfigError> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path)
        .map_err(|e| ConfigError::InvalidInput(e.to_string()))?;





    let mode = meta.mode() & 0o777;
    if mode != 0o640 {
        return Err(ConfigError::InvalidInput(format!("config file mode must be 0640, got {:04o}", mode)));
    }
    {
        if meta.uid() != 0 {
            return Err(ConfigError::InvalidInput("config file must be owned by root".into()));
        }

        let ramshared_gid = get_ramshared_gid()
            .ok_or_else(|| ConfigError::InvalidInput("ramshared group not found".into()))?;
        if meta.gid() != ramshared_gid {
            return Err(ConfigError::InvalidInput("config file must be owned by group ramshared".into()));
        }
    }
Ok(())
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn get_ramshared_gid() -> Option<u32> {
    let name = std::ffi::CString::new("ramshared").ok()?;
    let mut grp = unsafe { std::mem::zeroed::<libc::group>() };
    let mut buf = vec![0; 4096];
    let mut result = std::ptr::null_mut();

    let ret = unsafe {
        libc::getgrnam_r(
            name.as_ptr(),
            &mut grp,
            buf.as_mut_ptr(),
            buf.len(),
            &mut result,
        )
    };

    if ret == 0 && !result.is_null() {
        Some(grp.gr_gid)
    } else {
        None
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
#[allow(clippy::uninit_assumed_init)]
fn validate_permissions(path: &Path) -> Result<(), ConfigError> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows::Win32::Security::{OWNER_SECURITY_INFORMATION, DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR};
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use std::ffi::c_void;

    let mut wide_path: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide_path.push(0);

    let mut psid_owner: *mut c_void = std::ptr::null_mut();
    let mut sd = PSECURITY_DESCRIPTOR::default();

    let res = unsafe {
        GetNamedSecurityInfoW(
            windows::core::PCWSTR(wide_path.as_ptr()),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            Some(&mut psid_owner as *mut _ as _),
            None,
            None,
            None,
            &mut sd as *mut _ as _,
        )
    };

    if res != ERROR_SUCCESS {
        return Err(ConfigError::InvalidInput(format!("failed to get security info: {:?}", res)));
    }

    let mut is_valid = true;
    let mut reason = String::new();

    if psid_owner.is_null() {
        is_valid = false;
        reason = "missing owner".to_string();
    }

    if !sd.0.is_null() {
        let mut dacl_present = windows::Win32::Foundation::BOOL(0);
        let mut pacl = std::ptr::null_mut();
        let mut dacl_defaulted = windows::Win32::Foundation::BOOL(0);

        let dacl_res = unsafe {
            windows::Win32::Security::GetSecurityDescriptorDacl(
                sd,
                &mut dacl_present,
                &mut pacl,
                &mut dacl_defaulted,
            )
        };

        if dacl_res.is_ok() && dacl_present.as_bool() && !pacl.is_null() {
            let mut ace_count = 0;
            let mut info: windows::Win32::Security::ACL_SIZE_INFORMATION = unsafe { std::mem::zeroed() };
            if unsafe { windows::Win32::Security::GetAclInformation(pacl as _, &mut info as *mut _ as *mut c_void, std::mem::size_of::<windows::Win32::Security::ACL_SIZE_INFORMATION>() as u32, windows::Win32::Security::AclSizeInformation) }.is_ok() {
                ace_count = info.AceCount;
            }
            // Check DACL emptiness as a minimal security metric to avoid world readability.
            if ace_count == 0 {
                is_valid = false;
                reason = "config file DACL is empty (world readable)".into();
            }
        } else {
            is_valid = false;
            reason = "config file DACL is missing (world readable)".into();
        }
        unsafe { windows::Win32::Foundation::LocalFree(windows::Win32::Foundation::HLOCAL(sd.0)) };
    } else {
        is_valid = false;
        reason = "security descriptor is null".to_string();
    }

    if !is_valid {
        return Err(ConfigError::InvalidInput(reason));
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    #[cfg(unix)]
    fn test_load_file_rejects_world_readable() {
        use std::os::unix::fs::PermissionsExt;
        let temp_dir = std::env::temp_dir();
        let config_path = temp_dir.join("world_readable_config.toml");

        fs::write(&config_path, "[broker]\nslices=1\n").unwrap();

        let mut perms = fs::metadata(&config_path).unwrap().permissions();
        perms.set_mode(0o644); // World readable
        fs::set_permissions(&config_path, perms).unwrap();

        let result = load_file(&config_path);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("0640"), "Error was: {}", err_msg);
    }
}
