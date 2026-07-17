//! Swap execution over NBD: connects `nbd-client`, formats with `mkswap` (DT-16), and activates
//! with `swapon` (priority DT-7); the reverse path deactivates and disconnects. The argv construction
//! is pure/testable; functions that spawn processes are thin wrappers (`Command`) and validated
//! live in qemu / civm drills (not on WSL2 — session rule).
//!
//! DT-14: `nbd-client` ALWAYS with `-timeout 30` and NEVER `-persist` (no auto-reconnect; the
//! broker re-subscribes). DT-16: `mkswap` is mandatory at each attach (VRAM returns zeroed/dirty).

use std::io::{Error, Result};
use std::process::Command;

use ramshared_broker::protocol::NbdEndpoint;

/// Assembles `nbd-client` argv to attach `export` to `dev` (DT-14: `-timeout 30`, no
/// `-persist`). Unix uses `-unix <path>`; TCP uses positional `<host> <port>`.
pub fn nbd_args(endpoint: &NbdEndpoint, export: &str, dev: &str) -> Vec<String> {
    let mut a: Vec<String> = vec!["-N".into(), export.into()];
    match endpoint {
        NbdEndpoint::Unix { path } => {
            a.push("-unix".into());
            a.push(path.clone());
            a.push(dev.into());
        }
        NbdEndpoint::Tcp { host, port } => {
            a.push(host.clone());
            a.push(port.to_string());
            a.push(dev.into());
        }
    }
    a.push("-timeout".into());
    a.push("30".into());
    a
}

/// Assembles `swapon` argv (DT-7: only emits `-p <prio>` when priority is defined).
pub fn swapon_args(dev: &str, prio: Option<i32>) -> Vec<String> {
    let mut a = Vec::new();
    if let Some(p) = prio {
        a.push("-p".to_string());
        a.push(p.to_string());
    }
    a.push(dev.to_string());
    a
}

/// Runs a command and converts non-zero exit into `Err` with details (never swallows the error).
fn run(cmd: &str, args: &[String]) -> Result<()> {
    let status = Command::new(cmd).args(args).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::other(format!(
            "{cmd} {} -> {status}",
            args.join(" ")
        )))
    }
}

pub fn attach_swap_with<F>(
    endpoint: &NbdEndpoint,
    export: &str,
    dev: &str,
    prio: Option<i32>,
    mut run_cmd: F,
) -> std::result::Result<(), String>
where
    F: FnMut(&str, &[String]) -> Result<()>,
{
    run_cmd("nbd-client", &nbd_args(endpoint, export, dev))
        .map_err(|e| format!("nbd-client: {e}"))?;
    // DT-16: exported VRAM returns dirty/zeroed; the swap header needs to be rewritten.
    run_cmd("mkswap", &[dev.to_string()]).map_err(|e| format!("mkswap: {e}"))?;
    run_cmd("swapon", &swapon_args(dev, prio)).map_err(|e| format!("swapon: {e}"))?;
    Ok(())
}

/// Full attach sequence (DT-16): `nbd-client` → `mkswap` → `swapon`. On failure,
/// returns the message for the agent to report via `SwapOnDone{ok:false,detail}`.
pub fn attach_swap(
    endpoint: &NbdEndpoint,
    export: &str,
    dev: &str,
    prio: Option<i32>,
) -> std::result::Result<(), String> {
    attach_swap_with(endpoint, export, dev, prio, run)
}

pub fn detach_swap_with<F>(dev: &str, mut run_cmd: F) -> std::result::Result<(), String>
where
    F: FnMut(&str, &[String]) -> Result<()>,
{
    run_cmd("swapoff", &[dev.to_string()]).map_err(|e| format!("swapoff: {e}"))?;
    let _ = run_cmd("nbd-client", &["-d".to_string(), dev.to_string()]);
    Ok(())
}

/// Detach sequence: `swapoff` → `nbd-client -d`. Best-effort on disconnect (the device might
/// have already fallen); what matters for integrity is that `swapoff` exited successfully.
pub fn detach_swap(dev: &str) -> std::result::Result<(), String> {
    detach_swap_with(dev, run)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn nbd_args_tcp_has_timeout_no_persist() {
        let ep = NbdEndpoint::Tcp {
            host: "10.0.0.1".into(),
            port: 10809,
        };
        let a = nbd_args(&ep, "s0", "/dev/nbd0");
        assert_eq!(
            a,
            vec![
                "-N",
                "s0",
                "10.0.0.1",
                "10809",
                "/dev/nbd0",
                "-timeout",
                "30"
            ]
        );
        assert!(!a.iter().any(|x| x == "-persist"), "DT-14: never -persist");
    }

    #[test]
    fn nbd_args_unix_uses_unix_flag() {
        let ep = NbdEndpoint::Unix {
            path: "/run/r.sock".into(),
        };
        let a = nbd_args(&ep, "s2", "/dev/nbd2");
        assert_eq!(
            a,
            vec![
                "-N",
                "s2",
                "-unix",
                "/run/r.sock",
                "/dev/nbd2",
                "-timeout",
                "30"
            ]
        );
        assert!(!a.iter().any(|x| x == "-persist"));
    }

    #[test]
    fn swapon_args_emits_prio_only_when_set() {
        assert_eq!(
            swapon_args("/dev/nbd0", Some(-5)),
            vec!["-p", "-5", "/dev/nbd0"]
        );
        assert_eq!(swapon_args("/dev/nbd0", None), vec!["/dev/nbd0"]);
    }

    #[test]
    fn attach_swap_with_nbd_client_fails() {
        let ep = NbdEndpoint::Unix {
            path: "/sock".into(),
        };
        let res = attach_swap_with(&ep, "export", "/dev/nbd0", None, |cmd, _| {
            if cmd == "nbd-client" {
                Err(Error::other("mock error"))
            } else {
                Ok(())
            }
        });
        assert_eq!(res, Err("nbd-client: mock error".into()));
    }

    #[test]
    fn attach_swap_with_mkswap_fails() {
        let ep = NbdEndpoint::Unix {
            path: "/sock".into(),
        };
        let res = attach_swap_with(&ep, "export", "/dev/nbd0", None, |cmd, _| {
            if cmd == "mkswap" {
                Err(Error::other("mock error"))
            } else {
                Ok(())
            }
        });
        assert_eq!(res, Err("mkswap: mock error".into()));
    }

    #[test]
    fn attach_swap_with_swapon_fails() {
        let ep = NbdEndpoint::Unix {
            path: "/sock".into(),
        };
        let res = attach_swap_with(&ep, "export", "/dev/nbd0", None, |cmd, _| {
            if cmd == "swapon" {
                Err(Error::other("mock error"))
            } else {
                Ok(())
            }
        });
        assert_eq!(res, Err("swapon: mock error".into()));
    }

    #[test]
    fn attach_swap_with_success() {
        let ep = NbdEndpoint::Unix {
            path: "/sock".into(),
        };
        let res = attach_swap_with(&ep, "export", "/dev/nbd0", None, |_, _| Ok(()));
        assert_eq!(res, Ok(()));
    }

    #[test]
    fn detach_swap_error_path() {
        let res = detach_swap("/dev/invalid_device_for_test");
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.starts_with("swapoff: "));
    }
}
