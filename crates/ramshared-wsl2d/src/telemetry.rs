//! Telemetry & reconciliation of the broker (SPEC `docs/specs/no-milestone/broker-telemetry-reconciliation/`).
//!
//! Types shared between the data-plane (writes counters) and the control-plane (reads + reconciles) +
//! the **pure** reconciliation logic. The invariant is **occupancy** (DT-4): compares the borrowed
//! capacity (`Σ slice.len` Active|Draining) with the swap actually occupied in our slices;
//! throughput (`bytes_served`/`io_count`) is separate telemetry, outside of the invariant. Eviction is
//! detected by the **canary** (`demotes_delta`), not by VRAM subtraction (DT-6).

use std::sync::atomic::AtomicU64;

/// IO counters per slice: the worker (data-plane) writes, the `BrokerCore` reads (DT-1). `Relaxed`
/// is sufficient — each counter is independent; the `(bytes, io)` pair is not read atomically together
/// (a one-tick skew is accepted, telemetry, not accounting).
#[derive(Default)]
pub struct SliceIoCounters {
    pub bytes_served: AtomicU64,
    pub io_count: AtomicU64,
}

/// VRAM gauge published by the worker's residency closure (DT-5). `total == 0` is the sentinel
/// for "no VRAM data" (e.g., `--backend ram`, without GPU) → `vram_*` fields output `None`.
#[derive(Default)]
pub struct VramGauge {
    pub free: AtomicU64,
    pub total: AtomicU64,
}

/// Reconciliation verdict (RF-4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconcileFlag {
    /// Convergent (occupied ≤ borrowed + tolerance).
    None,
    /// Some source missing (degrade) — partial sample.
    Partial,
    /// Canary triggered DEMOTE since the last sample (WDDM squeezed VRAM).
    Eviction,
    /// Slice stuck in `Draining` (zero not confirmed) beyond threshold.
    StuckSlice,
    /// Occupied > borrowed + tolerance (tenant swapping outside our slices / drift).
    Unaccounted,
}

/// Pure input for reconciliation — already collected from core/gauge (DT-4/DT-6).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReconcileInput {
    /// `Σ slice.len` of `Active|Draining` slices (borrowed capacity).
    pub alloc_active_bytes: u64,
    /// `Σ used` (bytes) of our NBD devices (DT-10), already filtered/converted by the `Psi` handler.
    pub occupied_swap_bytes: u64,
    /// Some slice in `pending_zero` ≥ `ZERO_RETRY_ERROR` ticks.
    pub stuck_draining: bool,
    /// Canary DEMOTEs since the last sample (DT-6).
    pub demotes_delta: u64,
    /// Some source missing (no VRAM gauge, no `mem`) → `Partial`.
    pub any_source_missing: bool,
}

/// Sample emitted by the **core** (without `t`/`branch`/`commit` — DT-8). `PartialEq` to enter
/// `Outbound`; `f64` prevents `Eq` (and `Outbound` does not require `Eq`).
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct TelemetryCore {
    pub tenant: Option<String>,
    pub slice: Option<u16>,
    pub swap_used: u64,
    pub alloc_active: u64,
    pub page_io_s: Option<u64>,
    pub vram_alloc_daemon: u64,
    pub vram_total_used: Option<u64>,
    pub vram_outros: Option<u64>,
    pub canario_demotes: u64,
    pub demote_reason: Option<String>,
    pub reconcile_delta: f64,
    pub flag: ReconcileFlag,
}

/// Final line (the IO layer wraps [`TelemetryCore`], adding `t`/`branch`/`commit` —
/// DT-8). 1 JSON object per line (`docs/benchmarks/results.jsonl`, RF-5).
#[derive(Clone, Debug, serde::Serialize)]
pub struct TelemetrySample {
    /// Epoch in seconds (timestamped by the IO layer; the core does not read the clock).
    pub t: u64,
    pub branch: Option<String>,
    pub commit: Option<String>,
    #[serde(flatten)]
    pub core: TelemetryCore,
}

/// VRAM of "others" (graphics/Windows) by subtraction, clamped at 0 (DT-4/DT-5). Call only when
/// VRAM data exists (`total > 0`).
pub fn vram_outros(total_used: u64, alloc_daemon: u64) -> u64 {
    total_used.saturating_sub(alloc_daemon)
}

/// Pure reconciliation (RF-4). Returns `(delta, flag)` where `delta = (occupied − borrowed)/borrowed`
/// (positive = occupied more than borrowed). The `streak` (hysteresis, DT-12) is applied **externally**,
/// in `on_tick`. F-v2-1: `delta` is computed **before** any return.
pub fn reconcile(inp: &ReconcileInput, tol_frac: f64) -> (f64, ReconcileFlag) {
    // alloc=0 (nothing borrowed): fraction is undefined → 0.0 if nothing is occupied, else 1.0 (total
    // drift). Avoids reporting raw `occupied` (giant number) in the `reconcile_delta` of the JSONL (M1).
    let delta = if inp.alloc_active_bytes == 0 {
        if inp.occupied_swap_bytes == 0 {
            0.0
        } else {
            1.0
        }
    } else {
        (inp.occupied_swap_bytes as f64 - inp.alloc_active_bytes as f64)
            / inp.alloc_active_bytes as f64
    };
    if inp.any_source_missing {
        return (delta, ReconcileFlag::Partial);
    }
    if inp.demotes_delta > 0 {
        // Canary is the authority on eviction (DT-6); VRAM subtraction does not detect WDDM eviction.
        return (delta, ReconcileFlag::Eviction);
    }
    if inp.stuck_draining {
        return (delta, ReconcileFlag::StuckSlice);
    }
    if delta > tol_frac {
        return (delta, ReconcileFlag::Unaccounted);
    }
    (delta, ReconcileFlag::None)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn base() -> ReconcileInput {
        ReconcileInput {
            alloc_active_bytes: 1 << 30,    // 1 GiB borrowed
            occupied_swap_bytes: 512 << 20, // 512 MiB occupied (half)
            stuck_draining: false,
            demotes_delta: 0,
            any_source_missing: false,
        }
    }

    #[test]
    fn reconcile_idle_none() {
        let (delta, flag) = reconcile(&base(), 0.10);
        assert_eq!(flag, ReconcileFlag::None);
        assert!(delta < 0.0, "ocupado < emprestado => delta negativo");
    }

    #[test]
    fn reconcile_unaccounted_when_occupied_gt_alloc() {
        let mut inp = base();
        inp.occupied_swap_bytes = inp.alloc_active_bytes + (200 << 20); // occupies more than borrowed
        let (delta, flag) = reconcile(&inp, 0.10);
        assert_eq!(flag, ReconcileFlag::Unaccounted);
        assert!(delta > 0.10);
    }

    #[test]
    fn reconcile_eviction_when_demotes() {
        let mut inp = base();
        inp.demotes_delta = 1; // canary triggered -> eviction has priority
        inp.occupied_swap_bytes = inp.alloc_active_bytes + (200 << 20); // even with "over", eviction wins
        assert_eq!(reconcile(&inp, 0.10).1, ReconcileFlag::Eviction);
    }

    #[test]
    fn reconcile_stuckslice() {
        let mut inp = base();
        inp.stuck_draining = true;
        assert_eq!(reconcile(&inp, 0.10).1, ReconcileFlag::StuckSlice);
    }

    #[test]
    fn reconcile_partial_when_missing() {
        let mut inp = base();
        inp.any_source_missing = true;
        inp.demotes_delta = 1; // partial has priority over everything (cannot be trusted)
        assert_eq!(reconcile(&inp, 0.10).1, ReconcileFlag::Partial);
    }

    #[test]
    fn reconcile_delta_computed_before_partial_branch() {
        // F-v2-1: even in the Partial path, the delta is computed (not 0/garbage).
        let mut inp = base();
        inp.any_source_missing = true;
        let (delta, _) = reconcile(&inp, 0.10);
        assert!((delta - (-0.5)).abs() < 0.01, "512MiB/1GiB - 1 = -0.5");
    }

    #[test]
    fn reconcile_alloc_zero_no_giant_delta() {
        // M1: nothing borrowed → delta defined (not raw `occupied`). Empty=None, with swap=Unaccounted.
        let mut inp = base();
        inp.alloc_active_bytes = 0;
        inp.occupied_swap_bytes = 0;
        assert_eq!(reconcile(&inp, 0.10), (0.0, ReconcileFlag::None));
        inp.occupied_swap_bytes = 999 << 20;
        let (delta, flag) = reconcile(&inp, 0.10);
        assert_eq!(flag, ReconcileFlag::Unaccounted);
        assert!(
            (delta - 1.0).abs() < 1e-9,
            "delta = 1.0 (drift total), não occupied cru"
        );
    }

    #[test]
    fn vram_outros_clamps_at_zero() {
        assert_eq!(vram_outros(2000, 500), 1500);
        assert_eq!(vram_outros(500, 2000), 0); // clamp (sampling skew)
    }

    #[test]
    fn telemetry_sample_serializes_flat_jsonl() {
        // RF-5/DT-8: `core` is flattened at the root level (one JSON line) + flag in snake_case.
        let core = TelemetryCore {
            tenant: Some("civm".into()),
            slice: None,
            swap_used: 1024,
            alloc_active: 2048,
            page_io_s: Some(512),
            vram_alloc_daemon: 4096,
            vram_total_used: Some(8192),
            vram_outros: Some(4096),
            canario_demotes: 0,
            demote_reason: None,
            reconcile_delta: -0.5,
            flag: ReconcileFlag::None,
        };
        let sample = TelemetrySample {
            t: 1718,
            branch: Some("b".into()),
            commit: Some("c".into()),
            core,
        };
        let line = serde_json::to_string(&sample).expect("serializa JSON");
        assert!(line.contains("\"t\":1718"));
        assert!(
            line.contains("\"swap_used\":1024"),
            "flatten: campo do core na raiz"
        );
        assert!(line.contains("\"flag\":\"none\""), "snake_case");
        assert!(!line.contains("\"core\":"), "flatten não aninha");
    }
}
