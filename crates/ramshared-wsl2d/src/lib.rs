//! ramshared-wsl2d (lib) — state machine (§7) + `VramBackend` connecting
//! `ramshared-cuda` to `ramshared-block`. The wiring of `/dev/nbdX` (ioctl) and the
//! residency canary (§9) are integrated in subsequent increments.
#![forbid(unsafe_code)]

pub mod backend;
pub mod broker_srv;
pub mod canary_probe;
pub mod conn;
pub mod residency;
pub mod state;
pub mod swap;
pub mod telemetry;
pub mod ublk;
pub mod ublk_control;
pub mod ublk_queue;
pub mod ublk_server;
pub mod uring_smoke;

pub use backend::{RamBackend, SliceView, VramBackend};
pub use canary_probe::{CANARY_BYTES, CANARY_EVERY, Cadence, CanaryProbe};
pub use conn::{CHAN_CAP, Job, LiveCount, Reply, WMsg, spawn_acceptor, spawn_reader, spawn_writer};
pub use residency::{Canary, DemoteReason, ResidencyConfig, ResidencySampler, Verdict};
pub use state::State;
pub use telemetry::{
    ReconcileFlag, ReconcileInput, SliceIoCounters, TelemetryCore, TelemetrySample, VramGauge,
    reconcile, vram_outros,
};
