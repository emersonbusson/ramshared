import os

with open("crates/ramshared-winsvc/src/config.rs", "r") as f:
    content = f.read()

method = """    /// Reload configuration in place, retaining existing state if new config is invalid.
    pub fn reload(&mut self, text: &str) -> Result<(), ConfigError> {
        let candidate = Self::from_toml(text)?;
        *self = candidate;
        Ok(())
    }

    /// Validate invariants before provision (DT-2)."""

content = content.replace("    /// Validate invariants before provision (DT-2).", method)

with open("crates/ramshared-winsvc/src/config.rs", "w") as f:
    f.write(content)

with open("crates/ramshared-winsvc/src/config.rs", "a") as f:
    f.write("""
#[cfg(test)]
mod reload_tests {
    use super::*;

    const GOOD: &str = r#"
[win_drive]
size_bytes = 536870912
block_size = 4096
cuda_device = 0
reserve_bytes = 536870912
queue_depth = 4
max_io_bytes = 1048576
evidence_path = "C:\\\\ProgramData\\\\RamShared\\\\evidence"
volume_letter = "D"
broker_pipe = "named_pipe_v1"
broker_ready_timeout_secs = 30
tenant = "windrive-host"
"#;

    #[test]
    fn reload_retains_state_on_failure() {
        let mut c = WinDriveConfig::from_toml(GOOD).unwrap();
        let initial_size = c.size_bytes;

        let bad = GOOD.replace("536870912", "0");
        let e = c.reload(&bad).unwrap_err();

        assert!(matches!(e, ConfigError::Invalid { field: "size_bytes", .. }));
        assert_eq!(c.size_bytes, initial_size);
    }
}
""")
