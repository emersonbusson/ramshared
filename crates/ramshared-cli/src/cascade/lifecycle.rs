//! Cascade lifecycle phase derivation (SPEC cascade-lifecycle-observability).
//!
//! Pure: no swapoff, no probe, no CUDA. Inputs are snapshots; outputs are phase + reasons.

use std::env;

/// Default active-use threshold (KiB). Residual nbd under this still counts as Armed.
pub const DEFAULT_ACTIVE_KIB: u64 = 1024;

/// Stable phase tags (JSON + logs). SPEC DT-1.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CascadePhase {
    Off,
    Armed,
    UsingZram,
    UsingVram,
    UsingDisk,
    Demoting,
    Degraded,
}

impl CascadePhase {
    pub fn as_str(self) -> &'static str {
        match self {
            CascadePhase::Off => "Off",
            CascadePhase::Armed => "Armed",
            CascadePhase::UsingZram => "UsingZram",
            CascadePhase::UsingVram => "UsingVram",
            CascadePhase::UsingDisk => "UsingDisk",
            CascadePhase::Demoting => "Demoting",
            CascadePhase::Degraded => "Degraded",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TierSample {
    pub present: bool,
    pub prio: Option<i32>,
    pub size_kib: u64,
    pub used_kib: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct DemoteSnapshot {
    pub total: Option<u64>,
    pub last_reason: Option<String>,
    pub in_progress: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CascadeSnapshot {
    pub zram: TierSample,
    pub vram: TierSample,
    pub disk: TierSample,
    pub ghost: bool,
    pub order_ok: bool,
    pub daemon_alive: bool,
    pub daemon_pid: Option<u32>,
    pub demote: DemoteSnapshot,
    pub active_kib: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecycleView {
    pub phase: CascadePhase,
    pub phase_reason: &'static str,
    pub ok: bool,
    pub reasons: Vec<String>,
}

/// Parse `RAMSHARED_STATUS_ACTIVE_KIB`; invalid or missing → default.
pub fn active_threshold_kib_from_env() -> u64 {
    match env::var("RAMSHARED_STATUS_ACTIVE_KIB") {
        Ok(s) => s.trim().parse::<u64>().unwrap_or(DEFAULT_ACTIVE_KIB),
        Err(_) => DEFAULT_ACTIVE_KIB,
    }
}

/// SPEC DT-2: first match wins.
pub fn derive_lifecycle(s: &CascadeSnapshot) -> LifecycleView {
    let thr = if s.active_kib == 0 {
        DEFAULT_ACTIVE_KIB
    } else {
        s.active_kib
    };
    let mut reasons: Vec<String> = Vec::new();

    if s.ghost {
        reasons.push("ghost".into());
    }
    if !s.order_ok {
        reasons.push("priority_order_bad".into());
    }
    let hot_vram_no_daemon =
        s.vram.present && !s.daemon_alive && s.vram.used_kib >= thr;
    if hot_vram_no_daemon {
        reasons.push("daemon_dead_hot_vram".into());
    }
    let vram_present_no_daemon = s.vram.present && !s.daemon_alive && s.vram.used_kib < thr;
    // Half-state: vram swapon without daemon even if used low (degraded safety).
    if vram_present_no_daemon {
        reasons.push("vram_tier_without_daemon".into());
    }

    let degraded = s.ghost || !s.order_ok || hot_vram_no_daemon || vram_present_no_daemon;
    if degraded {
        return LifecycleView {
            phase: CascadePhase::Degraded,
            phase_reason: if s.ghost {
                "ghost"
            } else if !s.order_ok {
                "priority_order_bad"
            } else if hot_vram_no_daemon {
                "daemon_dead_hot_vram"
            } else {
                "vram_tier_without_daemon"
            },
            ok: false,
            reasons,
        };
    }

    if s.demote.in_progress {
        return LifecycleView {
            phase: CascadePhase::Demoting,
            phase_reason: "demote_in_progress",
            ok: true,
            reasons,
        };
    }

    if s.disk.present && s.disk.used_kib >= thr {
        return LifecycleView {
            phase: CascadePhase::UsingDisk,
            phase_reason: "disk_used_ge_threshold",
            ok: true,
            reasons,
        };
    }
    if s.vram.present && s.vram.used_kib >= thr {
        return LifecycleView {
            phase: CascadePhase::UsingVram,
            phase_reason: "vram_used_ge_threshold",
            ok: true,
            reasons,
        };
    }
    if s.zram.present && s.zram.used_kib >= thr {
        return LifecycleView {
            phase: CascadePhase::UsingZram,
            phase_reason: "zram_used_ge_threshold",
            ok: true,
            reasons,
        };
    }
    if s.daemon_alive && s.vram.present && s.order_ok && !s.ghost {
        return LifecycleView {
            phase: CascadePhase::Armed,
            phase_reason: "armed_low_vram_used",
            ok: true,
            reasons,
        };
    }

    LifecycleView {
        phase: CascadePhase::Off,
        phase_reason: "no_product_cascade",
        ok: true,
        reasons,
    }
}

/// Build tier samples + order_ok from swap lines (filename lowercase match).
pub fn tiers_from_swap_names(
    entries: &[(String, u64, u64, i32)],
) -> (TierSample, TierSample, TierSample, bool) {
    let mut zram = TierSample::default();
    let mut vram = TierSample::default();
    let mut disk = TierSample::default();

    for (name, size, used, prio) in entries {
        let low = name.to_ascii_lowercase();
        if low.contains("zram") {
            merge_tier(&mut zram, *size, *used, *prio);
        } else if low.contains("nbd") || low.contains("ublk") {
            merge_tier(&mut vram, *size, *used, *prio);
        } else {
            merge_tier(&mut disk, *size, *used, *prio);
        }
    }

    let order_ok = compute_order_ok(&zram, &vram, &disk);
    (zram, vram, disk, order_ok)
}

fn merge_tier(t: &mut TierSample, size: u64, used: u64, prio: i32) {
    t.present = true;
    t.size_kib = t.size_kib.saturating_add(size);
    t.used_kib = t.used_kib.saturating_add(used);
    t.prio = Some(match t.prio {
        Some(p) => p.max(prio),
        None => prio,
    });
}

fn compute_order_ok(zram: &TierSample, vram: &TierSample, disk: &TierSample) -> bool {
    if zram.present && vram.present {
        match (zram.prio, vram.prio) {
            (Some(pz), Some(pv)) if pz <= pv => return false,
            _ => {}
        }
    }
    if vram.present && disk.present {
        match (vram.prio, disk.prio) {
            (Some(pv), Some(pd)) if pv <= pd => return false,
            _ => {}
        }
    }
    true
}

/// Escape a string for JSON.
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn tier_json(t: &TierSample) -> String {
    let prio = match t.prio {
        Some(p) => p.to_string(),
        None => "null".into(),
    };
    format!(
        "{{\"present\":{},\"prio\":{},\"size_kib\":{},\"used_kib\":{}}}",
        if t.present { "true" } else { "false" },
        prio,
        t.size_kib,
        t.used_kib
    )
}

/// Serialize lifecycle + snapshot to one JSON object (SPEC schema).
pub fn render_status_json(
    view: &LifecycleView,
    snap: &CascadeSnapshot,
    ts: &str,
) -> String {
    let reasons = if view.reasons.is_empty() {
        "[]".into()
    } else {
        let parts: Vec<String> = view.reasons.iter().map(|r| json_escape(r)).collect();
        format!("[{}]", parts.join(","))
    };
    let demote_total = match snap.demote.total {
        Some(n) => n.to_string(),
        None => "null".into(),
    };
    let demote_reason = match &snap.demote.last_reason {
        Some(r) => json_escape(r),
        None => "null".into(),
    };
    let pid = match snap.daemon_pid {
        Some(p) => p.to_string(),
        None => "null".into(),
    };
    format!(
        "{{\"phase\":{phase},\"phase_reason\":{reason},\"ok\":{ok},\"reasons\":{reasons},\
\"tiers\":{{\"zram\":{z},\"vram\":{v},\"disk\":{d}}},\
\"order_ok\":{order},\"ghost\":{ghost},\
\"daemon\":{{\"alive\":{alive},\"pid\":{pid}}},\
\"demote\":{{\"total\":{dt},\"last_reason\":{dr},\"in_progress\":{di}}},\
\"thresholds_kib\":{{\"active\":{thr}}},\
\"ts\":{ts}}}",
        phase = json_escape(view.phase.as_str()),
        reason = json_escape(view.phase_reason),
        ok = if view.ok { "true" } else { "false" },
        reasons = reasons,
        z = tier_json(&snap.zram),
        v = tier_json(&snap.vram),
        d = tier_json(&snap.disk),
        order = if snap.order_ok { "true" } else { "false" },
        ghost = if snap.ghost { "true" } else { "false" },
        alive = if snap.daemon_alive { "true" } else { "false" },
        pid = pid,
        dt = demote_total,
        dr = demote_reason,
        di = if snap.demote.in_progress {
            "true"
        } else {
            "false"
        },
        thr = snap.active_kib,
        ts = json_escape(ts),
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn base() -> CascadeSnapshot {
        CascadeSnapshot {
            zram: TierSample {
                present: true,
                prio: Some(200),
                size_kib: 2_097_148,
                used_kib: 0,
            },
            vram: TierSample {
                present: true,
                prio: Some(100),
                size_kib: 2_097_148,
                used_kib: 0,
            },
            disk: TierSample {
                present: true,
                prio: Some(-2),
                size_kib: 8_388_608,
                used_kib: 0,
            },
            ghost: false,
            order_ok: true,
            daemon_alive: true,
            daemon_pid: Some(1),
            demote: DemoteSnapshot::default(),
            active_kib: DEFAULT_ACTIVE_KIB,
        }
    }

    #[test]
    fn phase_off_when_no_tiers_no_daemon() {
        let s = CascadeSnapshot {
            zram: TierSample::default(),
            vram: TierSample::default(),
            disk: TierSample::default(),
            ghost: false,
            order_ok: true,
            daemon_alive: false,
            daemon_pid: None,
            demote: DemoteSnapshot::default(),
            active_kib: DEFAULT_ACTIVE_KIB,
        };
        let v = derive_lifecycle(&s);
        assert_eq!(v.phase, CascadePhase::Off);
        assert_eq!(v.phase_reason, "no_product_cascade");
    }

    #[test]
    fn phase_armed_low_vram_used() {
        let mut s = base();
        s.vram.used_kib = 176;
        s.zram.used_kib = 100;
        let v = derive_lifecycle(&s);
        assert_eq!(v.phase, CascadePhase::Armed);
        assert_eq!(v.phase_reason, "armed_low_vram_used");
        assert!(v.ok);
    }

    #[test]
    fn phase_using_zram() {
        let mut s = base();
        s.zram.used_kib = 50_000;
        s.vram.used_kib = 100;
        let v = derive_lifecycle(&s);
        assert_eq!(v.phase, CascadePhase::UsingZram);
    }

    #[test]
    fn phase_using_vram() {
        let mut s = base();
        s.vram.used_kib = 10_000;
        s.zram.used_kib = 50_000;
        let v = derive_lifecycle(&s);
        assert_eq!(v.phase, CascadePhase::UsingVram);
    }

    #[test]
    fn phase_using_disk() {
        let mut s = base();
        s.disk.used_kib = 5_000;
        s.vram.used_kib = 10_000;
        let v = derive_lifecycle(&s);
        assert_eq!(v.phase, CascadePhase::UsingDisk);
    }

    #[test]
    fn phase_degraded_ghost() {
        let mut s = base();
        s.ghost = true;
        s.vram.used_kib = 99_000;
        let v = derive_lifecycle(&s);
        assert_eq!(v.phase, CascadePhase::Degraded);
        assert_eq!(v.phase_reason, "ghost");
        assert!(!v.ok);
        assert!(v.reasons.iter().any(|r| r == "ghost"));
    }

    #[test]
    fn phase_degraded_order() {
        let mut s = base();
        s.order_ok = false;
        let v = derive_lifecycle(&s);
        assert_eq!(v.phase, CascadePhase::Degraded);
        assert_eq!(v.phase_reason, "priority_order_bad");
    }

    #[test]
    fn phase_degraded_hot_vram_no_daemon() {
        let mut s = base();
        s.daemon_alive = false;
        s.daemon_pid = None;
        s.vram.used_kib = 50_000;
        let v = derive_lifecycle(&s);
        assert_eq!(v.phase, CascadePhase::Degraded);
        assert_eq!(v.phase_reason, "daemon_dead_hot_vram");
    }

    #[test]
    fn phase_demoting_only_when_flag() {
        let mut s = base();
        s.demote.in_progress = true;
        s.vram.used_kib = 50_000;
        let v = derive_lifecycle(&s);
        assert_eq!(v.phase, CascadePhase::Demoting);
        // without flag → UsingVram
        s.demote.in_progress = false;
        let v2 = derive_lifecycle(&s);
        assert_eq!(v2.phase, CascadePhase::UsingVram);
    }

    #[test]
    fn priority_degraded_beats_using_vram() {
        let mut s = base();
        s.ghost = true;
        s.vram.used_kib = 99_000;
        s.demote.in_progress = true;
        let v = derive_lifecycle(&s);
        assert_eq!(v.phase, CascadePhase::Degraded);
    }

    #[test]
    fn active_threshold_from_env_invalid_defaults() {
        // Without relying on process env mutation (clippy deny set_var): pure default path.
        assert_eq!(DEFAULT_ACTIVE_KIB, 1024);
        let mut s = base();
        s.active_kib = 0; // treated as default inside derive for thr
        s.vram.used_kib = 500;
        // thr becomes DEFAULT when active_kib==0
        let v = derive_lifecycle(&s);
        assert_eq!(v.phase, CascadePhase::Armed);
    }

    #[test]
    fn json_shape_golden_or_roundtrip() {
        let s = base();
        let v = derive_lifecycle(&s);
        let j = render_status_json(&v, &s, "2026-07-14T00:00:00-03:00");
        assert!(j.contains("\"phase\":\"Armed\""));
        assert!(j.contains("\"phase_reason\":\"armed_low_vram_used\""));
        assert!(j.contains("\"ok\":true"));
        assert!(j.contains("\"order_ok\":true"));
        assert!(j.contains("\"ghost\":false"));
        assert!(j.contains("\"thresholds_kib\":{\"active\":1024}"));
        assert!(j.contains("\"in_progress\":false"));
        assert!(j.starts_with('{') && j.ends_with('}'));
    }

    #[test]
    fn tiers_from_swap_names_order() {
        let entries = vec![
            ("/dev/zram0".into(), 100u64, 10u64, 200i32),
            ("/dev/nbd0".into(), 200, 5, 100),
            ("/dev/sdc".into(), 300, 0, -2),
        ];
        let (z, v, d, ok) = tiers_from_swap_names(&entries);
        assert!(z.present && v.present && d.present);
        assert!(ok);
        assert_eq!(z.prio, Some(200));
        let bad = vec![
            ("/dev/zram0".into(), 100u64, 0u64, 50i32),
            ("/dev/nbd0".into(), 200, 0, 100),
        ];
        let (_, _, _, ok2) = tiers_from_swap_names(&bad);
        assert!(!ok2);
    }

    #[test]
    fn json_escape_quotes() {
        assert_eq!(json_escape(r#"a"b"#), r#""a\"b""#);
    }

    #[test]
    fn vram_without_daemon_low_used_is_degraded() {
        let mut s = base();
        s.daemon_alive = false;
        s.vram.used_kib = 10;
        let v = derive_lifecycle(&s);
        assert_eq!(v.phase, CascadePhase::Degraded);
        assert_eq!(v.phase_reason, "vram_tier_without_daemon");
    }
}
