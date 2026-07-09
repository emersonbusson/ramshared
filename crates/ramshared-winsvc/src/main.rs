//! ramshared-winsvc — Windows service that backs the StorPort virtual disk with VRAM.
//!
//! SPEC: `docs/specs/no-milestone/windows-swap-driver/SPEC.md` (ITEM-3/6/7, DT-16).
//!
//! Linux / non-Windows builds a **stub** so `cargo test --workspace` stays green.
//! Library logic: `ramshared_winsvc` crate.

#[cfg(windows)]
fn main() {
    // SCM entry via windows-service lands with full Windows host wiring.
    // Until then, print and exit so accidental start is visible.
    eprintln!("ramshared-winsvc: Windows service entry (ITEM-3+) — use lib APIs for provision");
    // Keep process exit non-zero until SCM registration is complete.
    std::process::exit(2);
}

#[cfg(not(windows))]
fn main() {
    eprintln!("ramshared-winsvc: Windows-only binary (stub on this host)");
    std::process::exit(2);
}
