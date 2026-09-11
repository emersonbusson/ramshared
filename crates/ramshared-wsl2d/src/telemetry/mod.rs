//! Telemetry & reconciliation of the broker (SPEC `docs/specs/no-milestone/broker-telemetry-reconciliation/`).
//!
//! Types shared between the data-plane (writes counters) and the control-plane (reads + reconciles) +
//! the **pure** reconciliation logic. The invariant is **occupancy** (DT-4): compares the borrowed
//! capacity (`Σ slice.len` Active|Draining) with the swap actually occupied in our slices;
//! throughput (`bytes_served`/`io_count`) is separate telemetry, outside of the invariant. Eviction is
//! detected by the **canary** (`demotes_delta`), not by VRAM subtraction (DT-6).

pub mod models;
pub mod reconcile;

pub use models::{
    SliceIoCounters, TelemetryCore, TelemetrySample, TraceContext, VramGauge, vram_outros,
};
pub use reconcile::{ReconcileFlag, ReconcileInput, reconcile};
