use std::ffi::OsString;
use std::process::Command;

/// Sandbox configuration for exact, bounded custody.
///
/// Ensures untrusted helpers are contained within isolated filesystem
/// namespaces using mount propagation rules.
pub(crate) fn isolate_filesystem_namespace(command: &mut Command) -> &mut Command {
    // `unshare` is a Linux-only command. In order to support Windows platform parity,
    // we only apply the isolation conditionally for Linux. Otherwise, we fallback to returning
    // the un-wrapped command.
    if !cfg!(target_os = "linux") {
        return command;
    }

    let original_program = command.get_program().to_os_string();
    let original_args: Vec<OsString> = command.get_args().map(|a| a.to_os_string()).collect();

    let mut new_command = Command::new("unshare");
    new_command.arg("-U");
    new_command.arg("-m");
    new_command.arg("--propagation");
    new_command.arg("private");
    new_command.arg("--");
    new_command.arg(original_program);
    new_command.args(original_args);

    if let Some(dir) = command.get_current_dir() {
        new_command.current_dir(dir);
    }

    for (k, v) in command.get_envs() {
        match v {
            Some(val) => new_command.env(k, val),
            None => new_command.env_remove(k),
        };
    }

    std::mem::swap(command, &mut new_command);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolate_filesystem_namespace_replaces_command_with_unshare() {
        let mut command = Command::new("echo");
        command.arg("hello");
        command.env("TEST_ENV", "1");
        command.current_dir("/");

        isolate_filesystem_namespace(&mut command);

        assert_eq!(command.get_program(), "unshare");
        let args: Vec<_> = command
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert_eq!(
            args,
            vec![
                "-U",
                "-m",
                "--propagation",
                "private",
                "--",
                "echo",
                "hello"
            ]
        );
        assert_eq!(
            command
                .get_current_dir()
                .unwrap_or_else(|| panic!("expected current_dir"))
                .to_str()
                .unwrap_or_else(|| panic!("expected valid utf-8")),
            "/"
        );
        let envs: Vec<_> = command.get_envs().collect();
        assert!(envs.iter().any(|(k, v)| {
            k.to_str().unwrap_or_default() == "TEST_ENV"
                && v.unwrap_or_default().to_str().unwrap_or_default() == "1"
        }));
    }
}
