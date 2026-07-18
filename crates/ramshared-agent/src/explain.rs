//! Deterministic, evidence-backed explanations for status/DEMOTE events.
//!
//! This module intentionally does not infer a process name from aggregate GPU
//! pressure. A future AI UI may summarize these records, but it must receive
//! the same facts and preserve unknown attribution.
#![forbid(unsafe_code)]

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DemoteEvidence {
    pub reason: String,
    pub vram_free_bytes: Option<u64>,
    pub free_floor_bytes: u64,
    pub swapoff_ms: Option<u64>,
    pub pages_moved: Option<u64>,
    pub process_attribution: Option<String>,
}

pub fn explain_demote(e: &DemoteEvidence) -> String {
    let trigger = match e.vram_free_bytes {
        Some(free) if free < e.free_floor_bytes => format!(
            "VRAM free bytes fell below the floor ({} < {} bytes)",
            free, e.free_floor_bytes
        ),
        Some(free) => format!("DEMOTE requested by {} with {} free bytes", e.reason, free),
        None => format!(
            "DEMOTE requested by {} (VRAM free bytes not observed)",
            e.reason
        ),
    };
    let attribution = e
        .process_attribution
        .as_deref()
        .unwrap_or("process not attributed");
    let duration = e.swapoff_ms.map_or_else(
        || "duration not observed".into(),
        |ms| format!("swapoff took {ms} ms"),
    );
    format!("{trigger}; {duration}; process: {attribution}.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explanation_reports_observed_floor_and_unknown_process() {
        let text = explain_demote(&DemoteEvidence {
            reason: "WddmBudget".into(),
            vram_free_bytes: Some(128),
            free_floor_bytes: 512,
            swapoff_ms: Some(20),
            pages_moved: Some(4),
            process_attribution: None,
        });
        assert!(text.contains("128 < 512"));
        assert!(text.contains("process not attributed"));
        assert!(!text.contains("GpuApp"));
    }

    #[test]
    fn explanation_preserves_explicit_attribution_only() {
        let text = explain_demote(&DemoteEvidence {
            reason: "external_pressure".into(),
            vram_free_bytes: None,
            free_floor_bytes: 1,
            swapoff_ms: None,
            pages_moved: None,
            process_attribution: Some("GpuApp.exe".into()),
        });
        assert!(text.contains("GpuApp.exe"));
        assert!(text.contains("not observed"));
    }
}
