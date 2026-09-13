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
    pub chosen_tier: Option<String>,
    pub chosen_capacity_bytes: Option<u64>,
    pub reasoning: Option<String>,
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
    let mut explanation = format!("{trigger}; {duration}; process: {attribution}.");

    if e.chosen_tier.is_some() || e.chosen_capacity_bytes.is_some() || e.reasoning.is_some() {
        let tier = e.chosen_tier.as_deref().unwrap_or("unknown tier");
        let cap = e.chosen_capacity_bytes.map(|c| format!("{} bytes", c)).unwrap_or_else(|| "unknown capacity".into());
        let r = e.reasoning.as_deref().unwrap_or("no reasoning provided");
        explanation.push_str(&format!(" Reasoning chain: selected {tier} with {cap} because {r}."));
    }
    explanation
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
            chosen_tier: None,
            chosen_capacity_bytes: None,
            reasoning: None,
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
            chosen_tier: None,
            chosen_capacity_bytes: None,
            reasoning: None,
        });
        assert!(text.contains("GpuApp.exe"));
        assert!(text.contains("not observed"));
    }

    #[test]
    fn explanation_reports_free_bytes_without_floor_breach() {
        let text = explain_demote(&DemoteEvidence {
            reason: "AppRequest".into(),
            vram_free_bytes: Some(1024),
            free_floor_bytes: 512,
            swapoff_ms: Some(15),
            pages_moved: Some(2),
            process_attribution: None,
            chosen_tier: None,
            chosen_capacity_bytes: None,
            reasoning: None,
        });
        assert!(text.contains("DEMOTE requested by AppRequest with 1024 free bytes"));
        assert!(!text.contains("fell below the floor"));
        assert!(text.contains("process not attributed"));
    }

    #[test]
    fn explanation_reports_reasoning_chain() {
        let text = explain_demote(&DemoteEvidence {
            reason: "PolicyRequest".into(),
            vram_free_bytes: Some(2048),
            free_floor_bytes: 1024,
            swapoff_ms: None,
            pages_moved: None,
            process_attribution: None,
            chosen_tier: Some("NVMe".into()),
            chosen_capacity_bytes: Some(8192),
            reasoning: Some("latency requirements allow NVMe tier".into()),
        });
        assert!(text.contains("Reasoning chain: selected NVMe with 8192 bytes because latency requirements allow NVMe tier."));
    }
}
