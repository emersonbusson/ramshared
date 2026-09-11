//! Kernel module version detection and compatibility checks.
//!
//! Provides fail-closed checks against the loaded `ramshared` kernel module.

#![forbid(unsafe_code)]

use std::fs;
use std::path::Path;

/// Check if the loaded kernel module version matches the CLI version.
///
/// Reads `/sys/module/ramshared/version` and compares it to the userspace
/// `CARGO_PKG_VERSION`. If they don't match, or if the file cannot be read
/// (e.g. permissions, module not loaded), it returns an error with a warning.
pub fn check_module_compatibility() -> Result<(), String> {
    let sys_path = Path::new("/sys/module/ramshared/version");

    let loaded_version = fs::read_to_string(sys_path)
        .map_err(|e| format!("Failed to read kernel module version at {}: {}", sys_path.display(), e))?
        .trim()
        .to_string();

    let cli_version = env!("CARGO_PKG_VERSION");

    if loaded_version != cli_version {
        return Err(format!(
            "Version mismatch: kernel module is v{}, but CLI is v{}. \
             Please reload the kernel module.",
            loaded_version, cli_version
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_missing_module_version_file_returns_error() {
        // As tests run in a sandbox, the actual module version file is unlikely to exist.
        let result = check_module_compatibility();
        assert!(result.is_err());
        let err = result.err().unwrap_or_else(|| unreachable!());
        assert!(err.contains("Failed to read kernel module version"));
    }
}
