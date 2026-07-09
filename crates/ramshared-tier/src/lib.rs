//! ramshared-tier — Orchestration of the RamShared swap cascade in WSL2.
//!
//! SPEC: `docs/specs/no-milestone/wsl2-cascade-swap/SPEC.md` §1 (architecture), §6.2 (`up`),
//! §9 (DEMOTE/residency).
//!
//! The cascade is priority-ordered using `swapon` settings, validated empirically in Phase 0
//! (`docs/reliability/wsl2-fase0-final.md`):
//!
//! ```text
//! memory pressure → zram (HOT, prio 200) → VRAM (COLD, prio 100) → VHDX (prio < 100)
//! ```
//!
//! This crate contains **pure, testable logic** (no root access, no filesystem I/O, no FFI):
//! priority constants, cascade order validation, and the DEMOTE safety net invariant
//! (finding A1 from the v3 audit).
#![forbid(unsafe_code)]

pub mod cascade;
pub mod priority;

pub use cascade::{SafetyNet, Tier, vram_safety_net};
pub use priority::{OrderError, TierPriorities, VRAM_PRIO, ZRAM_PRIO, validate_order};
