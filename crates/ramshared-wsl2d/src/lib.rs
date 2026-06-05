//! ramshared-wsl2d (lib) — máquina de estados (§7) + `VramBackend` que liga
//! `ramshared-cuda` ao `ramshared-block`. A fiação do `/dev/nbdX` (ioctl) e o
//! canário de residência (§9) entram nos próximos incrementos.
#![forbid(unsafe_code)]

pub mod backend;
pub mod residency;
pub mod state;

pub use backend::VramBackend;
pub use residency::{Canary, DemoteReason, ResidencyConfig, Verdict};
pub use state::State;
