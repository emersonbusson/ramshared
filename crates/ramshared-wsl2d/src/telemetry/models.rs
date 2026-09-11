use std::sync::atomic::AtomicU64;

use super::reconcile::ReconcileFlag;

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

/// W3C Trace Context (traceparent) standard fields.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct TraceContext {
    pub trace_id: String,
    pub span_id: String,
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
    pub trace_context: Option<TraceContext>,
    #[serde(flatten)]
    pub core: TelemetryCore,
}

/// VRAM of "others" (graphics/Windows) by subtraction, clamped at 0 (DT-4/DT-5). Call only when
/// VRAM data exists (`total > 0`).
pub fn vram_outros(total_used: u64, alloc_daemon: u64) -> u64 {
    total_used.saturating_sub(alloc_daemon)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn vram_outros_normal_subtraction() {
        assert_eq!(vram_outros(2000, 500), 1500);
    }

    #[test]
    fn vram_outros_clamps_at_zero() {
        assert_eq!(vram_outros(500, 2000), 0); // clamp (sampling skew)
    }

    #[test]
    fn vram_outros_equal_values() {
        assert_eq!(vram_outros(1024, 1024), 0);
    }

    #[test]
    fn vram_outros_zero_values() {
        assert_eq!(vram_outros(0, 0), 0);
    }

    #[test]
    fn vram_outros_max_values() {
        assert_eq!(vram_outros(u64::MAX, u64::MAX), 0);
        assert_eq!(vram_outros(u64::MAX, 1), u64::MAX - 1);
        assert_eq!(vram_outros(0, u64::MAX), 0);
    }

    #[test]
    fn telemetry_sample_serializes_flat_jsonl() {
        // RF-5/DT-8: `core` is flattened at the root level (one JSON line) + flag in snake_case.
        let core = TelemetryCore {
            tenant: Some("guest-tenant".into()),
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
            trace_context: Some(TraceContext {
                trace_id: "4bf92f3577b34da6a3ce929d0e0e4736".into(),
                span_id: "00f067aa0ba902b7".into(),
            }),
            core,
        };
        let line = serde_json::to_string(&sample).expect("serializes JSON");
        assert!(line.contains("\"t\":1718"));
        assert!(
            line.contains("\"swap_used\":1024"),
            "flatten: core field at the root"
        );
        assert!(line.contains("\"flag\":\"none\""), "snake_case");
        assert!(!line.contains("\"core\":"), "flatten does not nest");
        assert!(line.contains("\"trace_id\":\"4bf92f3577b34da6a3ce929d0e0e4736\""));
        assert!(line.contains("\"span_id\":\"00f067aa0ba902b7\""));
    }
}
