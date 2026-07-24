//! Tier `swapoff` (DEMOTE path). Extracted from the daemon for reuse by both
//! transports: NBD (`main.rs`) and the ublk worker DT-3 (`ublk_server.rs`). The
//! swapoff runs **outside** of the path serving the swap (separate thread) — Discipline 3
//! (anti-deadlock): blocking the server during swapoff would hang the swap itself.

use std::sync::mpsc::Receiver;

/// Absolute path of `swapoff` (#2c: a root daemon MUST NOT depend on `$PATH`;
/// avoids malicious shims in the environment). Fallback to `$PATH` only as a last resort.
pub fn swapoff_bin() -> &'static str {
    const CANDIDATES: &[&str] = &["/usr/sbin/swapoff", "/sbin/swapoff"];
    for c in CANDIDATES {
        if std::path::Path::new(c).exists() {
            return c;
        }
    }
    "swapoff"
}

pub fn swapon_bin() -> &'static str {
    const CANDIDATES: &[&str] = &["/usr/sbin/swapon", "/sbin/swapon"];
    for candidate in CANDIDATES {
        if std::path::Path::new(candidate).exists() {
            return candidate;
        }
    }
    "swapon"
}

pub fn activate_swap(dev: &str, priority: i16) -> bool {
    std::process::Command::new(swapon_bin())
        .args(["-p", &priority.to_string(), dev])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Spawns `swapoff <dev>` in a separate thread (does not block the server) and returns the
/// channel confirming the outcome (`true` = success). Unified DEMOTE path (DT-8):
/// used by per-request latency and cadence probe.
pub fn spawn_swapoff(dev: &str) -> Receiver<bool> {
    let (tx, rx) = std::sync::mpsc::channel();
    let dev = dev.to_string();
    std::thread::spawn(move || {
        let ok = std::process::Command::new(swapoff_bin())
            .arg(&dev)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        let _ = tx.send(ok);
    });
    rx
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn test_swapoff_bin() {
        let bin = swapoff_bin();
        assert!(bin.ends_with("swapoff"));
    }

    #[test]
    fn test_swapon_bin() {
        let bin = swapon_bin();
        assert!(bin.ends_with("swapon"));
    }

    #[test]
    fn test_activate_swap_invalid_dev() {
        assert!(!activate_swap(
            "/dev/invalid_device_that_does_not_exist",
            10
        ));
    }

    #[test]
    fn test_spawn_swapoff_invalid_dev() {
        let rx = spawn_swapoff("/dev/invalid_device_that_does_not_exist");
        assert!(!rx.recv().unwrap());
    }
}
