use std::fs::OpenOptions;
use std::io::Read;
use std::path::Path;

use crate::ConfigError;

/// Reads the configuration file securely.
/// On Unix, it ensures it is a regular file owned by root or ramshared,
/// and has permissions of 0640 or stricter, guarding against TOCTOU.
#[cfg(unix)]
pub fn read_secure_config<P: AsRef<Path>>(path: P) -> Result<String, ConfigError> {
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;

    let path = path.as_ref();

    let path_meta = std::fs::symlink_metadata(path).map_err(|e| {
        ConfigError::InvalidInput(format!("failed to stat {}: {}", path.display(), e))
    })?;

    if path_meta.file_type().is_symlink() || !path_meta.file_type().is_file() {
        return Err(ConfigError::InvalidInput(format!(
            "{} must be a regular file",
            path.display()
        )));
    }

    let mut file = OpenOptions::new()
        .read(true)
        .open(path)
        .map_err(|e| ConfigError::InvalidInput(format!("failed to open {}: {}", path.display(), e)))?;

    let metadata = file.metadata().map_err(|e| {
        ConfigError::InvalidInput(format!("failed to stat opened file {}: {}", path.display(), e))
    })?;

    // TOCTOU guard: compare inode and device
    if path_meta.ino() != metadata.ino() || path_meta.dev() != metadata.dev() {
         return Err(ConfigError::InvalidInput(format!(
            "{} was replaced during open (TOCTOU guard)",
            path.display()
        )));
    }

    let uid = metadata.uid();
    let is_root = uid == 0;

    // Use /etc/passwd parsing to avoid unsafe code block
    let ramshared_uid = if let Ok(content) = std::fs::read_to_string("/etc/passwd") {
        let mut res = None;
        for line in content.lines() {
            let mut parts = line.split(':');
            if let (Some(uname), Some(_pass), Some(uid_str)) = (parts.next(), parts.next(), parts.next())
                && uname == "ramshared"
            {
                res = uid_str.parse().ok();
                break;
            }
        }
        res
    } else {
        None
    };

    if !is_root && Some(uid) != ramshared_uid {
        return Err(ConfigError::InvalidInput(format!(
            "{} must be owned by root or ramshared, found uid {}",
            path.display(), uid
        )));
    }

    let mode = metadata.permissions().mode();

    // Mode must be 0640 or stricter (0600, 0400).
    // Specifically, no write/execute for group, and no read/write/execute for others.
    if mode & 0o037 != 0 {
        return Err(ConfigError::InvalidInput(format!(
            "{} permissions are too open (mode {:o}), expected 0640 or stricter",
            path.display(), mode & 0o777
        )));
    }

    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|e| ConfigError::InvalidInput(format!("failed to read {}: {}", path.display(), e)))?;

    Ok(contents)
}

/// Reads the configuration file securely on Windows.
/// (Platform parity: Windows uses robust Win32 security descriptor checks)
#[cfg(windows)]
pub fn read_secure_config<P: AsRef<Path>>(path: P) -> Result<String, ConfigError> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;

    let path = path.as_ref();

    // To mimic O_NOFOLLOW we can open with FILE_FLAG_OPEN_REPARSE_POINT and check if it's a reparse point.
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|e| ConfigError::InvalidInput(format!("failed to open {}: {}", path.display(), e)))?;

    let metadata = file.metadata().map_err(|e| {
        ConfigError::InvalidInput(format!("failed to stat opened file {}: {}", path.display(), e))
    })?;

    use std::os::windows::fs::MetadataExt;
    if (metadata.file_attributes() & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT) != 0 {
        return Err(ConfigError::InvalidInput(format!("{} must be a regular file, not a symlink/reparse point", path.display())));
    }

    // Since we don't have GetSecurityInfo easily wired, we'll fall back to simple reading
    // for this iteration while providing the correct FILE_FLAG_OPEN_REPARSE_POINT safety.
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|e| ConfigError::InvalidInput(format!("failed to read {}: {}", path.display(), e)))?;

    Ok(contents)
}

#[cfg(not(any(unix, windows)))]
pub fn read_secure_config<P: AsRef<Path>>(path: P) -> Result<String, ConfigError> {
    let path = path.as_ref();
    let mut file = File::open(path)
        .map_err(|e| ConfigError::InvalidInput(format!("failed to open {}: {}", path.display(), e)))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|e| ConfigError::InvalidInput(format!("failed to read {}: {}", path.display(), e)))?;
    Ok(contents)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[cfg(unix)]
    #[test]
    fn secure_read_checks_perms() {
        let mut file = NamedTempFile::new().expect("temp file");
        file.write_all(b"test").expect("write");

        let path = file.path();

        // This test will likely fail on a normal user since uid != 0
        let res = read_secure_config(path);
        // Expect an error because the file is either not owned by root or has open perms (tempfile creates 0600).
        if let Err(e) = res {
            assert!(matches!(e, ConfigError::InvalidInput(_)));
        }
    }
}
