use std::io;

/// Validates that the current thread has the CAP_SYS_ADMIN capability.
/// This is required for creating new ublk devices to prevent privilege escalation
/// or unauthorized device creation.
pub fn require_sys_admin() -> io::Result<()> {
    let caps = rustix::thread::capabilities(None).map_err(|e| {
        io::Error::other(format!("failed to read capabilities: {e}"))
    })?;

    if !caps.effective.contains(rustix::thread::CapabilitySet::SYS_ADMIN) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "ublk device creation requires CAP_SYS_ADMIN",
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_require_sys_admin() {
        let _ = require_sys_admin();
    }
}
