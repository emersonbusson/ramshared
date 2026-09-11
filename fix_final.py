import re
import os
import shutil

# Step 1: Copy and setup files
os.makedirs("crates/ramshared-cli/src/supervisor", exist_ok=True)
shutil.copy("crates/ramshared-cli/src/supervisor.rs", "crates/ramshared-cli/src/supervisor/mod.rs")
os.remove("crates/ramshared-cli/src/supervisor.rs")

with open("crates/ramshared-cli/src/supervisor/mod.rs", "r") as f:
    text = f.read()

# Step 2: Extract parts
part1_pattern = r"(fn run_systemctl_bounded\(.*?\nfn query_unit_invocation_id\(.*?^\})"
part1_match = re.search(part1_pattern, text, re.MULTILINE | re.DOTALL)
part1_str = part1_match.group(1)

tests_pattern = r"(    #\[test\]\n    // TestName: bounded_systemctl_adapter_is_fixture_scoped_under_parallel_execution.*?^\s*\})"
tests_match = re.search(tests_pattern, text, re.MULTILINE | re.DOTALL)
tests_str = tests_match.group(1)

# Step 3: Remove extracted parts from mod.rs
text = text.replace(part1_str, "")
text = text.replace(tests_str, "")

# Step 4: Inject lifecycle module loading to mod.rs
text = text.replace("use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};", "use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};\npub mod lifecycle;")
text = text.replace("UnitActionRunner", "lifecycle::UnitActionRunner")
text = text.replace("Systemlifecycle::UnitActionRunner", "lifecycle::SystemUnitActionRunner")
text = text.replace("SystemUnitActionRunner", "lifecycle::SystemUnitActionRunner")
text = text.replace("use std::process::Command;\n", "")
text = text.replace("use crate::bounded_process;\n", "")
text = text.replace("const SYSTEMCTL_IDENTITY_TIMEOUT: Duration = Duration::from_secs(1);\n", "")

text = text.replace("run_systemctl_bounded_for(", "lifecycle::run_systemctl_bounded_for(")

with open("crates/ramshared-cli/src/supervisor/mod.rs", "w") as f:
    f.write(text)

# Step 5: Write lifecycle.rs
lifecycle_code = """use crate::bounded_process;
use crate::workload;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// The timeout duration for systemd identity queries.
pub const SYSTEMCTL_IDENTITY_TIMEOUT: Duration = Duration::from_secs(1);

""" + part1_str + """

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::expect_used)]
    use super::*;
    use std::fs;
    use std::time::Instant;

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

""" + tests_str + """
}
"""

lifecycle_code = lifecycle_code.replace("fn run_systemctl_bounded(", "/// Runs a bounded systemctl command with a standard timeout.\npub fn run_systemctl_bounded(")
lifecycle_code = lifecycle_code.replace("fn run_systemctl_bounded_for(", "/// Runs a bounded systemctl command with a custom path and timeout.\npub fn run_systemctl_bounded_for(")
lifecycle_code = lifecycle_code.replace("trait UnitActionRunner", "/// A trait for executing bounded actions against systemd units.\npub trait UnitActionRunner")
lifecycle_code = lifecycle_code.replace("struct SystemUnitActionRunner", "/// A runner that executes actions via the host's `systemctl` binary.\npub struct SystemUnitActionRunner")
lifecycle_code = lifecycle_code.replace("fn parse_unit_invocation_id(", "/// Parses the systemd identity query output for a specific unit.\npub fn parse_unit_invocation_id(")
lifecycle_code = lifecycle_code.replace("fn query_unit_invocation_id(", "/// Queries the current invocation ID of a managed systemd unit.\npub fn query_unit_invocation_id(")
lifecycle_code = lifecycle_code.replace("!valid_scope_unit(", "!super::valid_scope_unit(")

with open("crates/ramshared-cli/src/supervisor/lifecycle.rs", "w") as f:
    f.write(lifecycle_code)
