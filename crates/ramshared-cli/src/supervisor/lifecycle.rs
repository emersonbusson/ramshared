use crate::bounded_process;
use crate::workload;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// The timeout duration for systemd identity queries.
pub const SYSTEMCTL_IDENTITY_TIMEOUT: Duration = Duration::from_secs(1);

/// Runs a bounded systemctl command with a standard timeout.
pub fn run_systemctl_bounded(args: &[&str]) -> Result<(), String> {
    run_systemctl_bounded_for(Path::new("systemctl"), args, Duration::from_secs(1))
}

/// Runs a bounded systemctl command with a custom path and timeout.
pub fn run_systemctl_bounded_for(
    command: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<(), String> {
    let mut command = Command::new(command);
    command.args(args);
    let output = bounded_process::run_capture_command(
        &mut command,
        "systemctl action",
        timeout,
        bounded_process::DEFAULT_OUTPUT_LIMIT,
        |_| {},
    )
    .map_err(|error| format!("bounded systemctl action failed: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            Err(format!("systemctl exited with {}", output.status))
        } else {
            Err(format!("systemctl exited with {}: {stderr}", output.status))
        }
    }
}

/// A trait for executing bounded actions against systemd units.
pub trait UnitActionRunner {
    fn current_invocation_id(&self, unit: &str) -> Result<String, String>;
    fn run(&self, args: &[&str]) -> Result<(), String>;
}

/// A runner that executes actions via the host's `systemctl` binary.
pub struct SystemUnitActionRunner;

impl UnitActionRunner for SystemUnitActionRunner {
    fn current_invocation_id(&self, unit: &str) -> Result<String, String> {
        query_unit_invocation_id(unit)
    }

    fn run(&self, args: &[&str]) -> Result<(), String> {
        run_systemctl_bounded(args)
    }
}

/// Parses the systemd identity query output for a specific unit.
pub fn parse_unit_invocation_id(unit: &str, output: &str) -> Result<String, String> {
    let mut id = None;
    let mut invocation_id = None;
    for line in output.lines() {
        let (name, value) = line
            .split_once('=')
            .ok_or("malformed systemd identity response")?;
        let target = match name {
            "Id" => &mut id,
            "InvocationID" => &mut invocation_id,
            _ => return Err("unexpected systemd identity field".into()),
        };
        if target.replace(value.to_string()).is_some() {
            return Err("duplicate systemd identity field".into());
        }
    }
    if id.as_deref() != Some(unit) {
        return Err("systemd unit identity changed".into());
    }
    invocation_id
        .filter(|value| workload::valid_systemd_invocation_id(value))
        .ok_or_else(|| "systemd InvocationID is missing or invalid".into())
}

/// Queries the current invocation ID of a managed systemd unit.
pub fn query_unit_invocation_id(unit: &str) -> Result<String, String> {
    if !super::valid_scope_unit(unit) {
        return Err("invalid managed scope unit".into());
    }
    let mut command = Command::new("systemctl");
    command.args(["show", "--property=Id", "--property=InvocationID", unit]);
    let output = bounded_process::run_capture_command(
        &mut command,
        "systemd identity query",
        SYSTEMCTL_IDENTITY_TIMEOUT,
        bounded_process::DEFAULT_OUTPUT_LIMIT,
        |_| {},
    )
    .map_err(|error| format!("bounded systemd identity query failed: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "systemd identity query exited with {}",
            output.status
        ));
    }
    let output = String::from_utf8(output.stdout)
        .map_err(|error| format!("systemd identity query returned non-UTF-8 output: {error}"))?;
    parse_unit_invocation_id(unit, &output)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::expect_used)]
    use super::*;
    use std::fs;

    struct TestDir {
        pub path: std::path::PathBuf,
    }

    impl TestDir {
        pub fn new() -> Self {
            let path = std::env::temp_dir().join(format!("supervisor-test-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        pub fn program(&self, name: &str, source: &str) -> std::path::PathBuf {
            let path = self.path.join(name);
            fs::write(&path, source).unwrap();
            let mut perms = fs::metadata(&path).unwrap().permissions();
            use std::os::unix::fs::PermissionsExt;
            perms.set_mode(0o755);
            fs::set_permissions(&path, perms).unwrap();
            path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    // TestName: bounded_systemctl_adapter_is_fixture_scoped_under_parallel_execution
    fn bounded_systemctl_adapter_is_fixture_scoped_under_parallel_execution() {
        let fixture = TestDir::new();
        let systemctl = fixture.program(
            "systemctl-fixture",
            "#!/bin/sh\ncase \"$1\" in\n  --version) exit 0 ;;\n  ramshared-invalid-command) exit 1 ;;\n  *) exit 2 ;;\nesac\n",
        );
        assert!(
            run_systemctl_bounded_for(&systemctl, &["--version"], Duration::from_millis(100),)
                .is_ok()
        );
        assert!(
            run_systemctl_bounded_for(
                &systemctl,
                &["ramshared-invalid-command"],
                Duration::from_millis(100),
            )
            .is_err()
        );
    }
}
