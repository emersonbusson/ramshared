//! Local diagnostics for broker/daemon JSONL evidence.
//!
//! This is intentionally deterministic. It summarizes recorded facts and does
//! not attribute pressure to a process unless the event stream contains that
//! attribution explicitly.
#![forbid(unsafe_code)]

use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
struct Event {
    #[serde(default)]
    t: Option<u64>,
    #[serde(default)]
    tenant: Option<String>,
    #[serde(default)]
    slice: Option<u16>,
    #[serde(default)]
    swap_used: Option<u64>,
    #[serde(default)]
    alloc_active: Option<u64>,
    #[serde(default)]
    page_io_s: Option<u64>,
    #[serde(default)]
    vram_alloc_daemon: Option<u64>,
    #[serde(default)]
    vram_total_used: Option<u64>,
    #[serde(default)]
    vram_outros: Option<u64>,
    #[serde(default)]
    canario_demotes: Option<u64>,
    #[serde(default)]
    demote_reason: Option<String>,
    #[serde(default)]
    reconcile_delta: Option<f64>,
    #[serde(default)]
    flag: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Diagnosis {
    samples: usize,
    first_t: Option<u64>,
    last_t: Option<u64>,
    demotes: u64,
    max_vram_other: Option<u64>,
    max_swap_used: Option<u64>,
    max_page_io_s: Option<u64>,
    flags: Vec<String>,
    timeline: Vec<String>,
    recommendations: Vec<String>,
}

pub fn run(args: &[String]) -> Result<(), String> {
    let (path, json) = parse_args(args)?;
    let text = fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let diagnosis = diagnose_jsonl(&text)?;
    if json {
        println!("{}", render_json(&diagnosis));
    } else {
        print_text(&diagnosis);
    }
    Ok(())
}

fn parse_args(args: &[String]) -> Result<(PathBuf, bool), String> {
    let mut path = None;
    let mut json = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => json = true,
            "--events" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "--events requires a path".to_string())?;
                path = Some(PathBuf::from(value));
            }
            other => return Err(format!("unsupported diagnose argument: {other}")),
        }
        i += 1;
    }
    Ok((
        path.ok_or_else(|| "usage: ramshared diagnose --events PATH [--json]".to_string())?,
        json,
    ))
}

fn diagnose_jsonl(text: &str) -> Result<Diagnosis, String> {
    let mut events = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let event: Event =
            serde_json::from_str(line).map_err(|e| format!("line {}: {e}", idx + 1))?;
        events.push(event);
    }
    Ok(diagnose_events(&events))
}

fn diagnose_events(events: &[Event]) -> Diagnosis {
    let mut diagnosis = Diagnosis {
        samples: events.len(),
        first_t: events.iter().filter_map(|e| e.t).min(),
        last_t: events.iter().filter_map(|e| e.t).max(),
        ..Diagnosis::default()
    };

    let mut previous_demotes = 0;
    let mut flags = Vec::<String>::new();
    for event in events {
        let demotes = event.canario_demotes.unwrap_or(0);
        if demotes > previous_demotes {
            let delta = demotes - previous_demotes;
            diagnosis.demotes = diagnosis.demotes.saturating_add(delta);
            let reason = event
                .demote_reason
                .as_deref()
                .unwrap_or("reason not recorded");
            diagnosis.timeline.push(format!(
                "{} DEMOTE observed: {reason}; process not attributed",
                ts(event)
            ));
        }
        previous_demotes = demotes;

        if let Some(flag) = event.flag.as_deref()
            && flag != "none"
            && !flags.iter().any(|existing| existing == flag)
        {
            flags.push(flag.to_string());
            diagnosis
                .timeline
                .push(format!("{} reconciliation flag={flag}", ts(event)));
        }

        diagnosis.max_vram_other = max_opt(diagnosis.max_vram_other, event.vram_outros);
        diagnosis.max_swap_used = max_opt(diagnosis.max_swap_used, event.swap_used);
        diagnosis.max_page_io_s = max_opt(diagnosis.max_page_io_s, event.page_io_s);

        if let Some(tenant) = event.tenant.as_deref() {
            diagnosis.timeline.push(format!(
                "{} tenant={tenant} slice={:?}",
                ts(event),
                event.slice
            ));
        }
    }
    diagnosis.flags = flags;
    diagnosis.recommendations = recommendations(&diagnosis);
    diagnosis
}

fn recommendations(d: &Diagnosis) -> Vec<String> {
    let mut recs = Vec::new();
    if d.samples == 0 {
        recs.push("No events found. Start ramsharedd with --telemetry-jsonl PATH.".to_string());
        return recs;
    }
    if d.demotes > 0 {
        recs.push(
            "DEMOTE occurred. Keep the VRAM tier conservative or lower the lease size before the next workload window.".to_string(),
        );
    }
    if d.flags.iter().any(|f| f == "partial") {
        recs.push("Telemetry was partial. Verify VRAM gauge and agent PSI inputs.".to_string());
    }
    if d.flags.iter().any(|f| f == "stuck_slice") {
        recs.push("A slice was stuck draining. Keep swapoff-first teardown and inspect the agent responsible for zero confirmation.".to_string());
    }
    if d.flags.iter().any(|f| f == "unaccounted") {
        recs.push("Swap usage exceeded borrowed capacity. Check for unmanaged swap devices or stale tenant state.".to_string());
    }
    if d.max_page_io_s.unwrap_or(0) > 10_000 {
        recs.push("Page I/O is high. Compare zram, VRAM, and disk tier priorities before increasing VRAM capacity.".to_string());
    }
    if recs.is_empty() {
        recs.push("No anomaly detected in the provided event window.".to_string());
    }
    recs
}

fn max_opt(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn ts(event: &Event) -> String {
    event
        .t
        .map(|t| format!("t={t}"))
        .unwrap_or_else(|| "t=unknown".to_string())
}

fn print_text(d: &Diagnosis) {
    println!("RamShared diagnosis");
    println!("samples: {}", d.samples);
    println!("window: {:?}..{:?}", d.first_t, d.last_t);
    println!("demotes: {}", d.demotes);
    println!(
        "max: vram_other={} swap_used={} page_io_s={}",
        opt_num(d.max_vram_other),
        opt_num(d.max_swap_used),
        opt_num(d.max_page_io_s)
    );
    println!(
        "flags: {}",
        if d.flags.is_empty() {
            "none".into()
        } else {
            d.flags.join(",")
        }
    );
    println!("timeline:");
    for item in &d.timeline {
        println!("  - {item}");
    }
    println!("recommendations:");
    for item in &d.recommendations {
        println!("  - {item}");
    }
}

fn opt_num(value: Option<u64>) -> String {
    value
        .map(|n| n.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn render_json(d: &Diagnosis) -> String {
    serde_json::json!({
        "samples": d.samples,
        "first_t": d.first_t,
        "last_t": d.last_t,
        "demotes": d.demotes,
        "max_vram_other": d.max_vram_other,
        "max_swap_used": d.max_swap_used,
        "max_page_io_s": d.max_page_io_s,
        "flags": d.flags,
        "timeline": d.timeline,
        "recommendations": d.recommendations,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnoses_demote_without_process_attribution() {
        let input = r#"{"t":1,"canario_demotes":0,"flag":"none"}
{"t":2,"canario_demotes":1,"demote_reason":"free_floor","flag":"eviction","vram_outros":1024}"#;
        let d = match diagnose_jsonl(input) {
            Ok(d) => d,
            Err(e) => panic!("diagnose failed: {e}"),
        };
        assert_eq!(d.samples, 2);
        assert_eq!(d.demotes, 1);
        assert!(
            d.timeline
                .iter()
                .any(|line| line.contains("process not attributed"))
        );
        assert!(
            d.recommendations
                .iter()
                .any(|line| line.contains("DEMOTE occurred"))
        );
    }

    #[test]
    fn empty_events_recommend_telemetry() {
        let d = match diagnose_jsonl("") {
            Ok(d) => d,
            Err(e) => panic!("diagnose failed: {e}"),
        };
        assert_eq!(d.samples, 0);
        assert!(d.recommendations[0].contains("--telemetry-jsonl"));
    }
}
