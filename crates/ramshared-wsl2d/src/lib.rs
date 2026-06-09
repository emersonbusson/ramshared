//! ramshared-wsl2d (lib) — máquina de estados (§7) + `VramBackend` que liga
//! `ramshared-cuda` ao `ramshared-block`. A fiação do `/dev/nbdX` (ioctl) e o
//! canário de residência (§9) entram nos próximos incrementos.
#![forbid(unsafe_code)]

pub mod backend;
pub mod canary_probe;
pub mod conn;
pub mod residency;
pub mod state;
pub mod ublk;
pub mod ublk_control;
pub mod ublk_queue;
pub mod uring_smoke;

pub use backend::VramBackend;
pub use canary_probe::{CANARY_BYTES, CANARY_EVERY, Cadence, CanaryProbe};
pub use conn::{CHAN_CAP, Job, LiveCount, Reply, WMsg, spawn_acceptor, spawn_reader, spawn_writer};
pub use residency::{Canary, DemoteReason, ResidencyConfig, ResidencySampler, Verdict};
pub use state::State;
