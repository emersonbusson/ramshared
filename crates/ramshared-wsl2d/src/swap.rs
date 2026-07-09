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
