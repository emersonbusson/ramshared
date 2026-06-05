//! ramshared-tier — orquestração da cascata de swap do RamShared no WSL2.
//!
//! SPEC: `docs/vram-as-ram/SPECv3-WSL2.md` §1 (arquitetura), §6.2 (`up`),
//! §9 (DEMOTE/residência).
//!
//! A cascata é por prioridade de `swapon`, validada empiricamente na Fase 0
//! (`docs/vram-as-ram/FASE0-FINAL.md`):
//!
//! ```text
//! pressão de memória → zram (HOT, prio 200) → VRAM (COLD, prio 100) → VHDX (prio < 100)
//! ```
//!
//! Este crate é **lógica pura e testável** (sem root, sem I/O, sem FFI):
//! constantes de prioridade, validação da ordem da cascata e o invariante de
//! rede de segurança do DEMOTE (finding A1 da auditoria do v3).
#![forbid(unsafe_code)]

pub mod cascade;
pub mod priority;

pub use cascade::{SafetyNet, Tier, vram_safety_net};
pub use priority::{OrderError, TierPriorities, VRAM_PRIO, ZRAM_PRIO, validate_order};
