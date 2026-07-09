//! ramshared-winsvc — Windows service that backs the StorPort virtual disk with VRAM.
//!
//! SPEC: `docs/specs/no-milestone/windows-swap-driver/SPEC.md` (ITEM-3/6/7).
//! Preflight scaffold: Linux builds a **stub** so the workspace stays green (DT-16).

mod proto;

#[cfg(windows)]
fn main() {
    // ITEM-3+: SCM entry via windows-service. Not implemented yet.
    eprintln!("ramshared-winsvc: Windows service not yet implemented (ITEM-3)");
    std::process::exit(2);
}

#[cfg(not(windows))]
fn main() {
    eprintln!("ramshared-winsvc: Windows-only binary (stub on this host)");
    std::process::exit(2);
}
