//! Native Rust Memory & Swap Stress Governor & Qualification Battery for RamShared.
//!
//! Provides deterministic, GC-free, microsecond-accurate 1%-by-1%
//! memory escalation with closed-loop safety floor and latency probing.
//!
//! Features:
//! 1. Telemetry Gauge & Pressure Level Logger.
//! 2. Multi-Phase Qualification Battery (--battery):
//!    - Phase 1: Gradual 1%-by-1% Micro-Step Ramp.
//!    - Phase 2: Tier Waterfall Overflow (RAM -> ZRAM -> GPU VRAM).
//!    - Phase 3: Active Page Swapper & Cycler (animates live TUI speedometers).
//!    - Phase 4: Flash-Reclaim & Atomic Deallocation Benchmark.
//! 3. Autonomous Watchdog Daemon Thread for fail-closed Hyper-V / WSL2 anti-hang protection.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const WSL2_MIN_PHYSICAL_HEADROOM_MB: u64 = 600;
pub const MIN_ORDER_7_BUDDY_CHUNKS: u64 = 8;
pub const PROACTIVE_COMPACTION_TRIGGER_CHUNKS: u64 = 16;
const BARE_METAL_MULTI_TIER_FLOOR_MB: u64 = 256;
const MULTI_TIER_MIN_USABLE_AVAIL_MB: u64 = 100;
const SINGLE_TIER_MIN_USABLE_AVAIL_MB: u64 = 200;
const TIER1_AND_TIER2_QUALIFICATION_PCT: u64 = 95;
const TIER3_HEADROOM_RESERVE_MB: u64 = 16;
const TIER3_FULL_STEP_HEADROOM_MB: u64 = 48;
const TIER3_MAX_STEP_MB: u64 = 32;
const PRE_TIER3_FULL_STEP_HEADROOM_MB: u64 = 200;
const PRE_TIER3_HEADROOM_RESERVE_MB: u64 = 50;
const PRE_TIER3_MAX_STEP_MB: u64 = 128;
const WSL2_TIER3_STEP_INTERVAL_MS: u64 = 500;
const TIER3_HEAVY_LOAD_PCT: u64 = 80;
const TIER3_HEAVY_HOLD_INTERVAL_MS: u64 = 1_000;
const NORMAL_HOLD_INTERVAL_MS: u64 = 500;
const TIER3_HEAVY_TOUCH_BYTES: usize = 2 * 1024 * 1024;
const NORMAL_TOUCH_BYTES: usize = 16 * 1024 * 1024;
const TIER3_HEAVY_TOUCH_STRIDE_BYTES: usize = 32 * 1024;
const NORMAL_TOUCH_STRIDE_BYTES: usize = 16 * 1024;
const CASCADE_RAMP_LIMIT_PCT: u64 = 1_000;
const TIER3_QUALIFICATION_RAMP_LIMIT_PCT: u64 = 6_000;

#[derive(Clone, Debug, PartialEq)]
pub struct StressOptions {
    pub start_pct: u64,
    pub target_pct: u64,
    pub step_pct: u64,
    pub interval_ms: u64,
    pub hold_sec: u64,
    pub min_ram_mb: u64,
    pub max_psi_full: f64,
    pub max_latency_ms: f64,
    pub tier3_target_pct: Option<u64>,
    pub battery: bool,
    pub cascade: bool,
    pub telemetry_log: String,
    pub json: bool,
    pub threads: u64,
    pub min_order_7_chunks: u64,
}

impl Default for StressOptions {
    fn default() -> Self {
        Self {
            start_pct: 1,
            target_pct: 90,
            step_pct: 1,
            interval_ms: 1500,
            hold_sec: 10,
            min_ram_mb: 600,
            max_psi_full: 20.0,
            max_latency_ms: 8.0,
            tier3_target_pct: None,
            battery: false,
            cascade: false,
            telemetry_log: "/tmp/ramshared-stress-telemetry.log".to_string(),
            json: false,
            threads: 1,
            min_order_7_chunks: MIN_ORDER_7_BUDDY_CHUNKS,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize)]
pub struct TierCapacityStats {
    pub total_mb: u64,
    pub used_mb: u64,
    pub pct: u64,
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct TelemetryReading {
    pub timestamp_ms: u64,
    pub pressure_index: f64,
    pub gauge: String,
    pub latency_ms: f64,
    pub psi_full: f64,
    pub ram_used_mb: u64,
    pub swap_used_mb: u64,
    pub tier1_zram_pct: u64,
    pub tier2_vram_pct: u64,
    pub tier3_ssd_pct: u64,
    pub classification: String,
}

impl TelemetryReading {
    pub fn with_tier_pcts(mut self, t1: u64, t2: u64, t3: u64) -> Self {
        self.tier1_zram_pct = t1;
        self.tier2_vram_pct = t2;
        self.tier3_ssd_pct = t3;
        self
    }
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct StressReport {
    #[serde(default)]
    pub metric_version: u32,
    pub battery_mode: bool,
    pub cascade_mode: bool,
    pub max_safe_pct: u64,
    pub total_allocated_mb: u64,
    pub peak_swap_mb: u64,
    pub tier1_zram_mb: u64,
    pub tier1_zram_pct: u64,
    pub tier2_vram_mb: u64,
    pub tier2_vram_pct: u64,
    #[serde(default)]
    pub tier2_logical_swap_mb: u64,
    #[serde(default)]
    pub tier2_nbd_throughput_mbs: f64,
    #[serde(default)]
    pub tier2_physical_cache_target_mb: u64,
    #[serde(default)]
    pub simultaneous_full_tiers: bool,
    #[serde(default)]
    pub physical_cache_samples: usize,
    pub tier3_ssd_mb: u64,
    pub tier3_ssd_pct: u64,
    #[serde(default)]
    pub tier1_throughput_mbs: f64,
    #[serde(default)]
    pub tier2_throughput_mbs: Option<f64>,
    #[serde(default)]
    pub tier3_throughput_mbs: f64,
    #[serde(default)]
    pub tier2_speedup_vs_ssd: Option<f64>,
    pub peak_pressure_index: f64,
    pub telemetry_readings_count: usize,
    pub active_io_cycles_completed: usize,
    pub reclaim_duration_ms: Option<f64>,
    pub reclaim_speed_gbs: Option<f64>,
    #[serde(default)]
    pub buffer_drop_duration_ms: f64,
    pub post_reclaim_free_ram_mb: u64,
    pub status: String,
    #[serde(default)]
    pub avg_cycle_latency_ms: f64,
    #[serde(default)]
    pub p50_cycle_latency_ms: f64,
    #[serde(default)]
    pub p90_cycle_latency_ms: f64,
    #[serde(default)]
    pub p99_cycle_latency_ms: f64,
    #[serde(default)]
    pub max_cycle_latency_ms: f64,
    #[serde(default)]
    pub estimated_page_fault_lat_us: Option<f64>,
    #[serde(default)]
    pub host_vram_min_free_mb: u64,
    #[serde(default)]
    pub vram_evicted_chunks_count: Option<usize>,
    #[serde(default)]
    pub dma_watchdog_trips_count: Option<u64>,
    #[serde(default)]
    pub tier3_spillover_mb: u64,
    #[serde(default)]
    pub vram_eviction_p99_latency_ms: Option<f64>,
    #[serde(default)]
    pub kernel_d_state_hung_tasks: u64,
}

pub fn parse_stress_args(args: &[String]) -> Result<StressOptions, String> {
    let mut opts = StressOptions::default();
    let mut target_explicit = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--start" => {
                i += 1;
                opts.start_pct = args
                    .get(i)
                    .ok_or_else(|| "--start requires a value (1-99)".to_string())?
                    .parse()
                    .map_err(|_| "invalid --start value")?;
            }
            "--target" => {
                i += 1;
                opts.target_pct = args
                    .get(i)
                    .ok_or_else(|| "--target requires a value (1-99)".to_string())?
                    .parse()
                    .map_err(|_| "invalid --target value")?;
                target_explicit = true;
            }
            "--step" => {
                i += 1;
                opts.step_pct = args
                    .get(i)
                    .ok_or_else(|| "--step requires a value".to_string())?
                    .parse()
                    .map_err(|_| "invalid --step value")?;
            }
            "--interval-ms" => {
                i += 1;
                opts.interval_ms = args
                    .get(i)
                    .ok_or_else(|| "--interval-ms requires a value".to_string())?
                    .parse()
                    .map_err(|_| "invalid --interval-ms value")?;
            }
            "--hold-sec" => {
                i += 1;
                opts.hold_sec = args
                    .get(i)
                    .ok_or_else(|| "--hold-sec requires a value".to_string())?
                    .parse()
                    .map_err(|_| "invalid --hold-sec value")?;
            }
            "--min-ram-mb" => {
                i += 1;
                opts.min_ram_mb = args
                    .get(i)
                    .ok_or_else(|| "--min-ram-mb requires a value".to_string())?
                    .parse()
                    .map_err(|_| "invalid --min-ram-mb value")?;
            }
            "--battery" => {
                opts.battery = true;
            }
            "--cascade" => {
                opts.cascade = true;
                opts.battery = true;
                if !target_explicit {
                    opts.target_pct = 100;
                }
                if opts.tier3_target_pct.is_none() {
                    opts.tier3_target_pct = Some(15);
                }
                opts.max_psi_full = opts.max_psi_full.max(50.0);
                if opts.step_pct == 1 {
                    opts.step_pct = 5;
                }
                if opts.interval_ms == 1500 {
                    opts.interval_ms = 500;
                }
            }
            "--tier3-target-pct" => {
                i += 1;
                let val: u64 = args
                    .get(i)
                    .ok_or_else(|| "--tier3-target-pct requires a value (1-100)".to_string())?
                    .parse()
                    .map_err(|_| "invalid --tier3-target-pct value")?;
                opts.tier3_target_pct = Some(val.clamp(1, 100));
                opts.cascade = true;
                opts.battery = true;
                if !target_explicit {
                    opts.target_pct = 100;
                }
                opts.max_psi_full = opts.max_psi_full.max(50.0);
                if opts.step_pct == 1 {
                    opts.step_pct = 5;
                }
                if opts.interval_ms == 1500 {
                    opts.interval_ms = 500;
                }
            }
            "--max-psi-full" => {
                i += 1;
                opts.max_psi_full = args
                    .get(i)
                    .ok_or_else(|| "--max-psi-full requires a value".to_string())?
                    .parse()
                    .map_err(|_| "invalid --max-psi-full value")?;
            }
            "--max-latency-ms" => {
                i += 1;
                opts.max_latency_ms = args
                    .get(i)
                    .ok_or_else(|| "--max-latency-ms requires a value".to_string())?
                    .parse()
                    .map_err(|_| "invalid --max-latency-ms value")?;
            }
            "--min-order-7-chunks" => {
                i += 1;
                opts.min_order_7_chunks = args
                    .get(i)
                    .ok_or_else(|| "--min-order-7-chunks requires a value".to_string())?
                    .parse()
                    .map_err(|_| "invalid --min-order-7-chunks value")?;
            }
            "--log" => {
                i += 1;
                opts.telemetry_log = args
                    .get(i)
                    .ok_or_else(|| "--log requires a file path".to_string())?
                    .clone();
            }
            "--json" => {
                opts.json = true;
            }
            "--threads" => {
                i += 1;
                opts.threads = args
                    .get(i)
                    .ok_or_else(|| "--threads requires a value".to_string())?
                    .parse()
                    .map_err(|_| "invalid --threads value")?;
            }
            other => return Err(format!("unknown stress argument: {other}")),
        }
        i += 1;
    }

    let max_threads = std::thread::available_parallelism()
        .map(|n| n.get() as u64)
        .unwrap_or(1);

    if opts.threads > max_threads {
        return Err(format!(
            "thread count {} exceeds physical hardware limit ({})",
            opts.threads, max_threads
        ));
    }
    opts.threads = opts.threads.max(1);
    opts.start_pct = opts.start_pct.clamp(1, 200);
    opts.target_pct = opts.target_pct.clamp(opts.start_pct, 200);
    opts.step_pct = opts.step_pct.clamp(1, 25);
    Ok(opts)
}

/// True when running under Microsoft WSL2 (shared kernel VM).
pub fn is_wsl2() -> bool {
    fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|s| s.contains("microsoft") || s.contains("WSL"))
        .unwrap_or(false)
        || std::path::Path::new("/proc/sys/fs/binfmt_misc/WSLInterop").exists()
        || std::env::var_os("WSL_INTEROP").is_some()
}

pub fn read_mem_info() -> (u64, u64) {
    let text = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let mut total_kib = 0u64;
    let mut avail_kib = 0u64;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            total_kib = rest
                .split_whitespace()
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
            avail_kib = rest
                .split_whitespace()
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
        }
    }
    ((total_kib + 512) / 1024, (avail_kib + 512) / 1024)
}

pub fn read_sysctl_min_free_mb() -> u64 {
    fs::read_to_string("/proc/sys/vm/min_free_kbytes")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|kib| (kib + 512) / 1024)
        .unwrap_or(512)
}

pub fn query_gpu_free_vram_mb() -> Option<u64> {
    let output = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=memory.free", "--format=csv,noheader,nounits"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.trim().lines().next()?.trim().parse::<u64>().ok()
}

pub fn count_kernel_hung_tasks() -> u64 {
    let output = std::process::Command::new("dmesg").output().ok();
    if let Some(out) = output {
        let text = String::from_utf8_lossy(&out.stdout);
        text.lines()
            .filter(|line| line.contains("blocked for more than") || line.contains("hung_task"))
            .count() as u64
    } else {
        0
    }
}

const GPU_SAMPLE_INTERVAL_MS: u64 = 1_000;

/// Samples the minimum physical GPU free VRAM seen during stress runs.
/// Rate-limited to at most once every `GPU_SAMPLE_INTERVAL_MS` to prevent command fork churn.
pub fn sample_min_gpu_headroom(last_sample_ms: &mut u64, min_gpu_free_mb: &mut Option<u64>) {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    if now_ms.saturating_sub(*last_sample_ms) >= GPU_SAMPLE_INTERVAL_MS {
        *last_sample_ms = now_ms;
        if let Some(free_gpu) = query_gpu_free_vram_mb() {
            *min_gpu_free_mb = Some(min_gpu_free_mb.map_or(free_gpu, |m| m.min(free_gpu)));
        }
    }
}

#[allow(dead_code)]
pub fn parse_buddyinfo_order_7_chunks(content: &str) -> Option<u64> {
    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 15 && parts.get(3).copied() == Some("Normal") {
            return parts.get(11).and_then(|s| s.parse::<u64>().ok());
        }
    }
    None
}

/// Computes the effective order-7 chunk availability.
/// In the Linux buddy allocator, high-order chunks (order 8, 9, 10) are split on demand
/// into order-7 chunks (512 KiB). Therefore, genuine high-order exhaustion only occurs
/// if raw order-7 AND all split-eligible higher orders are depleted below threshold.
pub fn parse_buddyinfo_effective_order_7_chunks(content: &str) -> Option<u64> {
    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 15 && parts.get(3).copied() == Some("Normal") {
            let o7 = parts.get(11).and_then(|s| s.parse::<u64>().ok())?;
            let o8 = parts
                .get(12)
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let o9 = parts
                .get(13)
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let o10 = parts
                .get(14)
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let higher_equivalent = (o8 * 2) + (o9 * 4) + (o10 * 8);
            return Some(o7 + higher_equivalent);
        }
    }
    None
}

pub fn read_buddyinfo_order_7() -> Option<u64> {
    fs::read_to_string("/proc/buddyinfo")
        .ok()
        .and_then(|content| parse_buddyinfo_effective_order_7_chunks(&content))
}

pub fn is_order_7_depleted(order_7_chunks: Option<u64>, threshold: u64) -> bool {
    match order_7_chunks {
        Some(count) => count < threshold,
        None => false,
    }
}

pub fn trigger_proactive_compaction() {
    let _ = fs::write("/proc/sys/vm/compact_memory", "1\n");
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuddyInterlockAction {
    Continue,
    CompactionTriggered,
    Halt { detected_chunks: u64 },
}

#[allow(dead_code)]
pub fn decide_buddyinfo_action(effective_order_7: Option<u64>) -> BuddyInterlockAction {
    decide_buddyinfo_action_with_threshold(effective_order_7, MIN_ORDER_7_BUDDY_CHUNKS)
}

pub fn decide_buddyinfo_action_with_threshold(
    effective_order_7: Option<u64>,
    threshold: u64,
) -> BuddyInterlockAction {
    if threshold == 0 {
        return BuddyInterlockAction::Continue;
    }
    match effective_order_7 {
        Some(o7) if is_order_7_depleted(Some(o7), threshold) => BuddyInterlockAction::Halt {
            detected_chunks: o7,
        },
        Some(o7)
            if (threshold..PROACTIVE_COMPACTION_TRIGGER_CHUNKS.max(threshold + 8))
                .contains(&o7) =>
        {
            trigger_proactive_compaction();
            BuddyInterlockAction::CompactionTriggered
        }
        _ => BuddyInterlockAction::Continue,
    }
}

pub fn read_psi_full() -> f64 {
    let text = fs::read_to_string("/proc/pressure/memory").unwrap_or_default();
    for line in text.lines() {
        if line.starts_with("full") {
            for part in line.split_whitespace() {
                if let Some(val) = part.strip_prefix("avg10=") {
                    return val.parse().unwrap_or(0.0);
                }
            }
        }
    }
    0.0
}

pub fn read_swap_tiers() -> (u64, u64, u64, u64) {
    let text = fs::read_to_string("/proc/swaps").unwrap_or_default();
    let mut zram_kib = 0u64;
    let mut vram_kib = 0u64;
    let mut ssd_kib = 0u64;
    for line in text.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 {
            let name = parts[0];
            let used_kib = parts[3].parse::<u64>().unwrap_or(0);
            if name.contains("zram") {
                zram_kib += used_kib;
            } else if name.contains("nbd") || name.contains("ramshared") {
                vram_kib += used_kib;
            } else {
                ssd_kib += used_kib;
            }
        }
    }
    let z_mb = (zram_kib + 512) / 1024;
    let v_mb = (vram_kib + 512) / 1024;
    let s_mb = (ssd_kib + 512) / 1024;
    (z_mb + v_mb + s_mb, z_mb, v_mb, s_mb)
}

pub fn read_swap_tier_capacities() -> (TierCapacityStats, TierCapacityStats, TierCapacityStats) {
    let text = fs::read_to_string("/proc/swaps").unwrap_or_default();
    let mut z_tot = 0u64;
    let mut z_use = 0u64;
    let mut v_tot = 0u64;
    let mut v_use = 0u64;
    let mut s_tot = 0u64;
    let mut s_use = 0u64;

    for line in text.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 {
            let name = parts[0];
            let size_kib = parts[2].parse::<u64>().unwrap_or(0);
            let used_kib = parts[3].parse::<u64>().unwrap_or(0);
            if name.contains("zram") {
                z_tot += size_kib;
                z_use += used_kib;
            } else if name.contains("nbd") || name.contains("ramshared") {
                v_tot += size_kib;
                v_use += used_kib;
            } else {
                s_tot += size_kib;
                s_use += used_kib;
            }
        }
    }

    let z_tot_mb = (z_tot + 512) / 1024;
    let z_use_mb = (z_use + 512) / 1024;
    let z_pct = (z_use_mb * 100).checked_div(z_tot_mb).unwrap_or(0);

    let v_tot_mb = (v_tot + 512) / 1024;
    let v_use_mb = (v_use + 512) / 1024;
    let v_pct = (v_use_mb * 100).checked_div(v_tot_mb).unwrap_or(0);

    let s_tot_mb = (s_tot + 512) / 1024;
    let s_use_mb = (s_use + 512) / 1024;
    let s_pct = (s_use_mb * 100).checked_div(s_tot_mb).unwrap_or(0);

    (
        TierCapacityStats {
            total_mb: z_tot_mb,
            used_mb: z_use_mb,
            pct: z_pct,
        },
        TierCapacityStats {
            total_mb: v_tot_mb,
            used_mb: v_use_mb,
            pct: v_pct,
        },
        TierCapacityStats {
            total_mb: s_tot_mb,
            used_mb: s_use_mb,
            pct: s_pct,
        },
    )
}

pub fn read_tier_disk_total_bytes() -> (u64, u64, u64) {
    let text = fs::read_to_string("/proc/diskstats").unwrap_or_default();
    let swaps = fs::read_to_string("/proc/swaps").unwrap_or_default();
    tier_disk_bytes_from(&text, &swaps)
}

fn tier_disk_bytes_from(diskstats: &str, swaps: &str) -> (u64, u64, u64) {
    let disk_devices: std::collections::HashSet<&str> = swaps
        .lines()
        .skip(1)
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| {
            !name.contains("zram") && !name.contains("nbd") && !name.contains("ramshared")
        })
        .filter_map(|name| name.rsplit('/').next())
        .collect();
    let mut zram_bytes = 0u64;
    let mut vram_bytes = 0u64;
    let mut disk_bytes = 0u64;

    for line in diskstats.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 10
            && let (Ok(read_sectors), Ok(write_sectors)) =
                (fields[5].parse::<u64>(), fields[9].parse::<u64>())
        {
            let dev = fields[2];
            let total_bytes = read_sectors
                .saturating_add(write_sectors)
                .saturating_mul(512);
            if dev.starts_with("zram") {
                zram_bytes = zram_bytes.saturating_add(total_bytes);
            } else if dev.starts_with("nbd") || dev.starts_with("ramshared") {
                vram_bytes = vram_bytes.saturating_add(total_bytes);
            } else if disk_devices.contains(dev) {
                disk_bytes = disk_bytes.saturating_add(total_bytes);
            }
        }
    }
    (zram_bytes, vram_bytes, disk_bytes)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct CacheSample {
    cached_mib: u64,
    target_mib: u64,
    at_target: bool,
}

fn parse_cache_status_sample(
    text: &str,
    now_ms: u64,
    daemon_instance_id: &str,
) -> Option<CacheSample> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let written = value.get("written_at_unix_ms")?.as_u64()?;
    if written > now_ms.saturating_add(1000) || now_ms.saturating_sub(written) > 3000 {
        return None;
    }
    if value.get("daemon_instance_id")?.as_str()? != daemon_instance_id
        || !value.get("ok")?.as_bool()?
        || value.get("origin_state")?.as_str()? != "READY"
        || value.get("cache_state")?.as_str()? != "ACTIVE"
    {
        return None;
    }
    let cached_kib = value.get("vram_cached_kib")?.as_u64()?;
    let target_kib = value.get("cache_target_kib")?.as_u64()?;
    if target_kib == 0 || cached_kib > value.get("logical_capacity_kib")?.as_u64()? {
        return None;
    }
    Some(CacheSample {
        cached_mib: cached_kib / 1024,
        target_mib: target_kib / 1024,
        at_target: cached_kib >= target_kib,
    })
}

fn current_daemon_instance_id() -> Option<String> {
    let pid = fs::read_to_string("/run/ramshared/ramsharedd.pid").ok()?;
    let pid: u32 = pid.trim().parse().ok()?;
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = stat.rsplit_once(") ")?.1;
    let start_ticks = rest.split_whitespace().nth(19)?;
    Some(format!("{pid}-{start_ticks}"))
}

fn read_cache_status_sample() -> Option<CacheSample> {
    let text = fs::read_to_string("/run/ramshared/cache-status.json").ok()?;
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis() as u64;
    parse_cache_status_sample(&text, now_ms, &current_daemon_instance_id()?)
}

fn require_physical_cache_before_cascade(
    cascade: bool,
    tier3_target_pct: Option<u64>,
    sample: Option<CacheSample>,
) -> Result<(), String> {
    if (cascade || tier3_target_pct.is_some()) && sample.is_none() {
        return Err(
            "cascade stress requires fresh ACTIVE physical GPU cache telemetry with a nonzero target"
                .into(),
        );
    }
    Ok(())
}

fn full_tier_snapshot(
    zram_pct: u64,
    logical_nbd_pct: u64,
    ssd_pct: u64,
    cache: Option<CacheSample>,
) -> bool {
    zram_pct >= 100
        && logical_nbd_pct >= 100
        && ssd_pct >= 100
        && cache.is_some_and(|sample| sample.target_mib > 0 && sample.at_target)
}

pub fn probe_allocation_latency_ms() -> f64 {
    let t0 = Instant::now();
    let mut page = vec![0u8; 4096];
    page[0] = 1;
    page[4095] = 2;
    std::hint::black_box(&page);
    t0.elapsed().as_secs_f64() * 1000.0
}

fn safe_allocation_mb(
    is_multi_tier: bool,
    tier3_active: bool,
    avail_mb: u64,
    hard_floor: u64,
    one_pct_mb: u64,
) -> u64 {
    if !is_multi_tier {
        return one_pct_mb.min(avail_mb.saturating_sub(hard_floor)).min(128);
    }

    if tier3_active {
        if avail_mb > hard_floor + TIER3_FULL_STEP_HEADROOM_MB {
            one_pct_mb
                .min(avail_mb.saturating_sub(hard_floor + TIER3_HEADROOM_RESERVE_MB))
                .min(TIER3_MAX_STEP_MB)
        } else if avail_mb > hard_floor + TIER3_HEADROOM_RESERVE_MB {
            TIER3_HEADROOM_RESERVE_MB
        } else {
            0
        }
    } else if avail_mb > hard_floor + PRE_TIER3_FULL_STEP_HEADROOM_MB {
        one_pct_mb
            .min(avail_mb.saturating_sub(hard_floor + PRE_TIER3_HEADROOM_RESERVE_MB))
            .min(PRE_TIER3_MAX_STEP_MB)
    } else if avail_mb > hard_floor + 20 {
        TIER3_MAX_STEP_MB
    } else if avail_mb > hard_floor {
        TIER3_HEADROOM_RESERVE_MB
    } else {
        0
    }
}

fn step_interval_ms(is_wsl2_host: bool, tier3_active: bool, requested_ms: u64) -> u64 {
    if is_wsl2_host && tier3_active {
        requested_ms.max(WSL2_TIER3_STEP_INTERVAL_MS)
    } else {
        requested_ms
    }
}

fn tier3_target_reached(tier3_pct: u64, target_pct: u64) -> bool {
    tier3_pct >= target_pct
}

pub fn compute_latency_percentiles(latencies: &[f64]) -> (f64, f64, f64, f64, f64) {
    if latencies.is_empty() {
        return (0.0, 0.0, 0.0, 0.0, 0.0);
    }
    let mut sorted = latencies.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let len = sorted.len();
    let sum: f64 = sorted.iter().sum();
    let avg = sum / len as f64;
    let p50 = sorted[(len as f64 * 0.50) as usize % len];
    let p90 = sorted[(len as f64 * 0.90) as usize % len];
    let p99 = sorted[(len as f64 * 0.99) as usize % len];
    let max = *sorted.last().unwrap_or(&0.0);
    (
        (avg * 10000.0).round() / 10000.0,
        (p50 * 10000.0).round() / 10000.0,
        (p90 * 10000.0).round() / 10000.0,
        (p99 * 10000.0).round() / 10000.0,
        (max * 10000.0).round() / 10000.0,
    )
}

pub fn compute_telemetry_reading(
    latency_ms: f64,
    psi_full: f64,
    ram_alloc_mb: u64,
    ram_total_mb: u64,
    swap_used_mb: u64,
) -> TelemetryReading {
    let ram_ratio = if ram_total_mb > 0 {
        ram_alloc_mb as f64 / ram_total_mb as f64
    } else {
        0.0
    };

    let idx_raw = 1.0
        + (latency_ms * 0.8)
        + (psi_full * 0.25)
        + (ram_ratio * 3.5)
        + ((swap_used_mb as f64 / 1024.0) * 0.6);
    let pressure_index = idx_raw.clamp(1.0, 10.0);

    let (gauge, classification) = if pressure_index < 2.5 {
        ("[██░░░░░░░░]", "🟢 NOMINAL (RAM Active)")
    } else if pressure_index < 4.5 {
        ("[████░░░░░░]", "🟢 TIER-1 ACTIVE (In-RAM LZ4)")
    } else if pressure_index < 6.5 {
        ("[██████░░░░]", "🟡 TIER-2 ACTIVE (PCIe DMA VRAM)")
    } else if pressure_index < 8.5 {
        ("[████████░░]", "🟠 HIGH PRESSURE (Floor Protected)")
    } else {
        ("[██████████]", "🛡️  CEILING INTERLOCK (Damped)")
    };

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    TelemetryReading {
        timestamp_ms: ts,
        pressure_index,
        gauge: gauge.to_string(),
        latency_ms,
        psi_full,
        ram_used_mb: ram_alloc_mb,
        swap_used_mb,
        tier1_zram_pct: 0,
        tier2_vram_pct: 0,
        tier3_ssd_pct: 0,
        classification: classification.to_string(),
    }
}

pub fn append_telemetry_log(path: &str, reading: &TelemetryReading) {
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(
            f,
            "[{}] IDX {:>4.1} {} Lat: {:>5.2}ms │ PSI: {:>4.1}% │ RAM: {:>5}MB │ Swap: {:>5}MB [T1:{:>3}%|T2:{:>3}%|T3:{:>3}%] │ {}",
            reading.timestamp_ms,
            reading.pressure_index,
            reading.gauge,
            reading.latency_ms,
            reading.psi_full,
            reading.ram_used_mb,
            reading.swap_used_mb,
            reading.tier1_zram_pct,
            reading.tier2_vram_pct,
            reading.tier3_ssd_pct,
            reading.classification
        );
    }
}

pub fn run(opts: &StressOptions) -> Result<(), String> {
    require_physical_cache_before_cascade(
        opts.cascade,
        opts.tier3_target_pct,
        read_cache_status_sample(),
    )?;
    let term_signal = Arc::new(AtomicBool::new(false));
    let last_heartbeat = Arc::new(AtomicU64::new(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    ));

    let chunks = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));

    // Autonomous Watchdog Thread: If main thread stalls > 3s, clears memory automatically
    let chunks_watchdog = chunks.clone();
    let heartbeat_watchdog = last_heartbeat.clone();
    let term_watchdog = term_signal.clone();
    let watchdog_handle = thread::spawn(move || {
        while !term_watchdog.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(500));
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let last = heartbeat_watchdog.load(Ordering::Relaxed);
            if now.saturating_sub(last) > 4 {
                if let Ok(mut guard) = chunks_watchdog.lock()
                    && !guard.is_empty()
                {
                    guard.clear();
                }
                break;
            }
        }
    });

    let (ram_total_mb, ram_avail_init) = read_mem_info();
    let (swap_init_total, _, _, _) = read_swap_tiers();

    if !opts.json {
        println!("{}", "═".repeat(105));
        println!(" 🚀 RamShared Native Stress Governor & Multi-Tier Qualification Battery");
        println!(
            " Mode: {} │ Range: {}% ➔ {}% (+{}%) │ Safety Floor: >= {} MB │ Log: {}",
            if opts.battery {
                "FULL BATTERY (4 Phases)"
            } else {
                "Progressive 1%-by-1% Governor"
            },
            opts.start_pct,
            opts.target_pct,
            opts.step_pct,
            opts.min_ram_mb,
            opts.telemetry_log
        );
        println!(
            "[i] Physical Host RAM: {} MB (Available: {} MB) │ Active Swap: {} MB",
            ram_total_mb, ram_avail_init, swap_init_total
        );
        println!("{}", "═".repeat(105));
        println!(
            "┌───────┬────────────┬──────────────┬──────────────┬──────────────┬──────────────┬────────┬──────────┬─────────────┬───────────────────────────┐"
        );
        println!(
            "│ Level │ Alloc RAM  │ ZRAM (Tier1) │ VRAM (Tier2) │ SSD (Tier3)  │ Total Swap   │ PSI-F  │ Latency  │ Stress Bar  │ Tier Operating Status     │"
        );
        println!(
            "├───────┼────────────┼──────────────┼──────────────┼──────────────┼──────────────┼────────┼──────────┼─────────────┼───────────────────────────┤"
        );
    }

    let mut total_allocated_mb = 0u64;
    let mut max_safe_pct = 0u64;
    let mut peak_zram = 0u64;
    let mut peak_vram = 0u64;
    let mut peak_physical_vram = 0u64;
    let mut peak_physical_target = 0u64;
    let mut physical_cache_samples = 0usize;
    let mut simultaneous_full_tiers = false;
    let mut peak_ssd = 0u64;
    let mut peak_total_swap = 0u64;
    let mut peak_pressure = 1.0f64;
    let mut readings_count = 0usize;
    let mut active_cycles_done = 0usize;
    let (mut prev_z_bytes, mut prev_v_bytes, mut prev_s_bytes) = read_tier_disk_total_bytes();
    let mut prev_sample_time = Instant::now();
    let mut peak_zram_mbs: f64 = 0.0;
    let mut peak_vram_mbs: f64 = 0.0;
    let mut peak_ssd_mbs: f64 = 0.0;
    let mut latencies_ms: Vec<f64> = Vec::new();

    // Phase 1: 1%-by-1% Micro-Step Ramp
    let effective_target =
        if (opts.tier3_target_pct.is_some() || opts.cascade) && opts.target_pct == 100 {
            if opts.tier3_target_pct.is_some() {
                TIER3_QUALIFICATION_RAMP_LIMIT_PCT
            } else {
                CASCADE_RAMP_LIMIT_PCT
            }
        } else {
            opts.target_pct
        };
    let mut current_target = opts.start_pct;
    let mut min_gpu_free_mb: Option<u64> = None;
    let mut last_gpu_sample_ms = 0u64;
    while current_target <= effective_target {
        if term_signal.load(Ordering::Relaxed) {
            break;
        }

        last_heartbeat.store(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            Ordering::Relaxed,
        );

        let (_, avail_mb) = read_mem_info();
        let psi_full = read_psi_full();
        let lat_ms = probe_allocation_latency_ms();
        latencies_ms.push(lat_ms);
        let (tot_swap, z_mb, v_mb, s_mb) = read_swap_tiers();
        let (cap1, cap2, cap3) = read_swap_tier_capacities();
        let cache_sample = read_cache_status_sample();
        if let Some(sample) = cache_sample {
            physical_cache_samples += 1;
            peak_physical_vram = peak_physical_vram.max(sample.cached_mib);
            peak_physical_target = peak_physical_target.max(sample.target_mib);
        }
        simultaneous_full_tiers |= full_tier_snapshot(cap1.pct, cap2.pct, cap3.pct, cache_sample);

        sample_min_gpu_headroom(&mut last_gpu_sample_ms, &mut min_gpu_free_mb);

        let reading =
            compute_telemetry_reading(lat_ms, psi_full, total_allocated_mb, ram_total_mb, tot_swap)
                .with_tier_pcts(cap1.pct, cap2.pct, cap3.pct);
        peak_pressure = peak_pressure.max(reading.pressure_index);
        readings_count += 1;
        append_telemetry_log(&opts.telemetry_log, &reading);

        let sysctl_min_free_mb = read_sysctl_min_free_mb();
        let is_multi_tier = opts.cascade || opts.tier3_target_pct.is_some();
        const SWAP_DRAIN_POLL_INTERVAL: Duration = Duration::from_millis(150);
        const MAX_SWAP_DRAIN_IDLE_CYCLES: usize = 80; // 80 * 150ms = 12.0s of zero swap growth before declaring limit

        // Under WSL2, Hyper-V synthetic devices (hv_balloon, vmicvmswitch) and host-guest
        // heartbeat require at least 600 MB of total physical memory headroom.
        // In Linux, /proc/meminfo MemAvailable ALREADY discounts sysctl_min_free_mb (reserved pages).
        // Therefore: physical_headroom = MemAvailable + sysctl_min_free_mb.
        // We calibrate hard_floor against MemAvailable such that:
        //   hard_floor + sysctl_min_free_mb >= target_physical_floor (600 MB on WSL2).
        let target_physical_floor = if is_wsl2() {
            opts.min_ram_mb.max(WSL2_MIN_PHYSICAL_HEADROOM_MB)
        } else {
            opts.min_ram_mb.max(BARE_METAL_MULTI_TIER_FLOOR_MB)
        };

        let min_usable_avail = if is_multi_tier {
            MULTI_TIER_MIN_USABLE_AVAIL_MB
        } else {
            SINGLE_TIER_MIN_USABLE_AVAIL_MB
        };
        let hard_floor = target_physical_floor
            .saturating_sub(sysctl_min_free_mb)
            .max(min_usable_avail);

        let mut avail_mb = avail_mb;
        let mut last_swap_val = tot_swap;
        let mut idle_cycles = 0;
        let max_idle_cycles = MAX_SWAP_DRAIN_IDLE_CYCLES;

        while avail_mb <= hard_floor && is_multi_tier {
            if term_signal.load(Ordering::Relaxed) {
                break;
            }
            last_heartbeat.store(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                Ordering::Relaxed,
            );
            thread::sleep(SWAP_DRAIN_POLL_INTERVAL);
            let (_, new_avail) = read_mem_info();
            avail_mb = new_avail;
            let (cur_swap, _, _, _) = read_swap_tiers();
            if cur_swap > last_swap_val {
                // kswapd is actively draining dirty pages into VRAM/SSD
                last_swap_val = cur_swap;
                idle_cycles = 0;
            } else {
                idle_cycles += 1;
            }
            if idle_cycles >= max_idle_cycles || avail_mb > hard_floor {
                break;
            }
        }

        let floor_breached = if is_multi_tier {
            avail_mb <= hard_floor && idle_cycles >= max_idle_cycles
        } else {
            avail_mb <= hard_floor
        };

        if floor_breached {
            if !opts.json {
                println!(
                    "│ {:>4}%  │ {:>8} MB │ {:>8} MB │ {:>8} MB │ {:>8} MB │ {:>8} MB │ {:>5.1}% │ {:>6.2}ms │ {:>11} │ 🛑 RAM FLOOR REACHED      │",
                    current_target,
                    total_allocated_mb,
                    peak_zram,
                    peak_vram,
                    peak_ssd,
                    peak_total_swap,
                    psi_full,
                    lat_ms,
                    reading.gauge
                );
                println!(
                    "\n[🛑 SAFETY FLOOR REACHED] Available RAM reached floor ({} MB <= {} MB). Halted at {}% (Zero Hang Protection).",
                    avail_mb, hard_floor, max_safe_pct
                );
            }
            break;
        }

        if is_wsl2() && opts.min_order_7_chunks > 0 {
            let mut action = decide_buddyinfo_action_with_threshold(
                read_buddyinfo_order_7(),
                opts.min_order_7_chunks,
            );
            if let BuddyInterlockAction::CompactionTriggered = action {
                thread::sleep(Duration::from_millis(150));
            } else if let BuddyInterlockAction::Halt { .. } = action {
                // Before halting, trigger proactive compaction and allow kcompactd to coalesce pages
                for _ in 0..3 {
                    trigger_proactive_compaction();
                    thread::sleep(Duration::from_millis(200));
                    action = decide_buddyinfo_action_with_threshold(
                        read_buddyinfo_order_7(),
                        opts.min_order_7_chunks,
                    );
                    if !matches!(action, BuddyInterlockAction::Halt { .. }) {
                        break;
                    }
                }
            }
            if let BuddyInterlockAction::Halt { detected_chunks } = action {
                if !opts.json {
                    println!(
                        "│ {:>4}%  │ {:>8} MB │ {:>8} MB │ {:>8} MB │ {:>8} MB │ {:>8} MB │ {:>5.1}% │ {:>6.2}ms │ {:>11} │ 🛡️  BUDDY INTERLOCK   │",
                        current_target,
                        total_allocated_mb,
                        peak_zram,
                        peak_vram,
                        peak_ssd,
                        peak_total_swap,
                        psi_full,
                        lat_ms,
                        reading.gauge
                    );
                    println!(
                        "\n[🛡️ VMBUS BUDDY INTERLOCK] Order-7 physical memory depleted (<{} chunks, detected {}). Halted at {}% (Zero Hang Protection).",
                        opts.min_order_7_chunks, detected_chunks, max_safe_pct
                    );
                }
                break;
            }
        }

        if psi_full >= opts.max_psi_full {
            if is_multi_tier {
                // Transient PSI spike during heavy multi-tier swap; damp and wait up to 5s
                let mut calmed = false;
                for _ in 0..10 {
                    if term_signal.load(Ordering::Relaxed) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(500));
                    let fresh_psi = read_psi_full();
                    if fresh_psi < opts.max_psi_full {
                        calmed = true;
                        break;
                    }
                }
                if !calmed {
                    if !opts.json {
                        println!(
                            "│ {:>4}%  │ {:>8} MB │ {:>8} MB │ {:>8} MB │ {:>8} MB │ {:>8} MB │ {:>5.1}% │ {:>6.2}ms │ {:>11} │ ⚠️  PSI LIMIT DAMPING     │",
                            current_target,
                            total_allocated_mb,
                            peak_zram,
                            peak_vram,
                            peak_ssd,
                            peak_total_swap,
                            psi_full,
                            lat_ms,
                            reading.gauge
                        );
                        println!(
                            "\n[⚠️  PRESSURE DAMPING] PSI Full pressure sustained ({:.1}%) >= {:.1}%. Halted at {}%.",
                            psi_full, opts.max_psi_full, max_safe_pct
                        );
                    }
                    break;
                }
            } else {
                if !opts.json {
                    println!(
                        "│ {:>4}%  │ {:>8} MB │ {:>8} MB │ {:>8} MB │ {:>8} MB │ {:>8} MB │ {:>5.1}% │ {:>6.2}ms │ {:>11} │ ⚠️  PSI LIMIT DAMPING     │",
                        current_target,
                        total_allocated_mb,
                        peak_zram,
                        peak_vram,
                        peak_ssd,
                        peak_total_swap,
                        psi_full,
                        lat_ms,
                        reading.gauge
                    );
                    println!(
                        "\n[⚠️  PRESSURE DAMPING] PSI Full pressure ({:.1}%) >= {:.1}%. Halted at {}%.",
                        psi_full, opts.max_psi_full, max_safe_pct
                    );
                }
                break;
            }
        }

        if lat_ms >= opts.max_latency_ms {
            if !opts.json {
                println!(
                    "│ {:>4}%  │ {:>8} MB │ {:>8} MB │ {:>8} MB │ {:>8} MB │ {:>8} MB │ {:>5.1}% │ {:>6.2}ms │ {:>11} │ ⏱️  LATENCY SPIKE DAMP    │",
                    current_target,
                    total_allocated_mb,
                    peak_zram,
                    peak_vram,
                    peak_ssd,
                    peak_total_swap,
                    psi_full,
                    lat_ms,
                    reading.gauge
                );
                println!(
                    "\n[⏱️  LATENCY SPIKE DAMPING] Memory latency spiked to {:.2} ms >= {:.2} ms. Halted at {}%.",
                    lat_ms, opts.max_latency_ms, max_safe_pct
                );
            }
            break;
        }

        let (cap1, cap2, cap3) = read_swap_tier_capacities();
        if let Some(t3_target) = opts.tier3_target_pct
            && cap1.pct >= TIER1_AND_TIER2_QUALIFICATION_PCT
            && cap2.pct >= TIER1_AND_TIER2_QUALIFICATION_PCT
            && tier3_target_reached(cap3.pct, t3_target)
        {
            if !opts.json {
                println!(
                    "\n[🎯 LOGICAL SWAP TARGET REACHED] ZRAM: {}%, NBD: {}%, SSD: {}% (Target: {}%). Physical VRAM remains a separate check.",
                    cap1.pct, cap2.pct, cap3.pct, t3_target
                );
            }
            break;
        }

        let one_pct_mb = ((ram_total_mb * opts.step_pct) / 100).max(50);
        let safe_alloc_mb = safe_allocation_mb(
            is_multi_tier,
            cap3.used_mb > 0,
            avail_mb,
            hard_floor,
            one_pct_mb,
        );

        if safe_alloc_mb == 0 {
            if is_multi_tier && !floor_breached && psi_full < opts.max_psi_full {
                // Headroom is temporarily below allocation threshold.
                // Wait for kswapd to complete ongoing writebacks and recover headroom above hard_floor.
                let mut recovered = false;
                for _ in 0..60 {
                    // 60 * 100ms = 6.0s max wait for disk writeback
                    if term_signal.load(Ordering::Relaxed) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                    last_heartbeat.store(
                        SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                        Ordering::Relaxed,
                    );
                    let (_, fresh_avail) = read_mem_info();
                    let threshold = hard_floor + TIER3_HEADROOM_RESERVE_MB;
                    if fresh_avail > threshold {
                        avail_mb = fresh_avail;
                        recovered = true;
                        break;
                    }
                }
                if !recovered {
                    if !opts.json {
                        println!(
                            "\n[🛑 RAM FLOOR BOUND] Memory headroom cannot recover above safe floor ({} MB <= {} MB). Halting ramp safely at {}%.",
                            avail_mb, hard_floor, max_safe_pct
                        );
                    }
                    break;
                }
                // Re-read swap capacity and headroom before allocating. The values that
                // triggered the wait are stale after writeback recovery.
                continue;
            } else {
                break;
            }
        }

        // Allocate and dirty pages with realistic workload entropy
        let num_bytes = (safe_alloc_mb as usize) * 1024 * 1024;
        let mut slice = vec![0u8; num_bytes];
        for i in (0..num_bytes).step_by(4096) {
            let base = (current_target as u8).wrapping_add((i & 0xFF) as u8);
            for offset in (0..4096).step_by(128) {
                slice[i + offset] = base.wrapping_add((offset as u8) ^ 0xA5);
            }
            if (i & 0x1FFFFF) == 0 {
                // Every 2 MiB, yield CPU so Hyper-V VMBus IC heartbeat interrupt handler is never starved
                thread::yield_now();
            }
        }

        if let Ok(mut guard) = chunks.lock() {
            guard.push(slice);
        }
        total_allocated_mb += safe_alloc_mb;

        peak_zram = peak_zram.max(z_mb);
        peak_vram = peak_vram.max(v_mb);
        peak_ssd = peak_ssd.max(s_mb);
        peak_total_swap = peak_total_swap.max(tot_swap);

        max_safe_pct = current_target;

        if !opts.json {
            println!(
                "│ {:>4}%  │ {:>8} MB │ {:>8} MB │ {:>8} MB │ {:>8} MB │ {:>8} MB │ {:>5.1}% │ {:>6.2}ms │ {:>11} │ {:<25} │",
                current_target,
                total_allocated_mb,
                z_mb,
                v_mb,
                s_mb,
                tot_swap,
                psi_full,
                lat_ms,
                reading.gauge,
                reading.classification
            );
        }

        // Adaptive StorVSC I/O Pacing:
        // Tier 3 is backed by Hyper-V synthetic SCSI (storvsc) writing to swap.vhdx on NTFS.
        // If Tier 3 is active, pace steps by at least 500ms to allow StorVSC ring buffer completions.
        let step_interval = step_interval_ms(is_wsl2(), cap3.used_mb > 0, opts.interval_ms);
        thread::sleep(Duration::from_millis(step_interval));
        let now = Instant::now();
        let dt = now.duration_since(prev_sample_time).as_secs_f64();
        if dt > 0.05 {
            let (cur_z_bytes, cur_v_bytes, cur_s_bytes) = read_tier_disk_total_bytes();
            let dz = cur_z_bytes.saturating_sub(prev_z_bytes) as f64 / (1024.0 * 1024.0);
            let dv = cur_v_bytes.saturating_sub(prev_v_bytes) as f64 / (1024.0 * 1024.0);
            let ds = cur_s_bytes.saturating_sub(prev_s_bytes) as f64 / (1024.0 * 1024.0);

            peak_zram_mbs = peak_zram_mbs.max(dz / dt);
            peak_vram_mbs = peak_vram_mbs.max(dv / dt);
            peak_ssd_mbs = peak_ssd_mbs.max(ds / dt);

            prev_z_bytes = cur_z_bytes;
            prev_v_bytes = cur_v_bytes;
            prev_s_bytes = cur_s_bytes;
            prev_sample_time = now;
        }
        current_target += opts.step_pct;
    }

    // Phase 2 & 3: Active Page Swapper & Cycler (Only in Battery Mode or when hold_sec > 0)
    if opts.battery || opts.hold_sec > 0 {
        if !opts.json {
            println!("{}", "═".repeat(105));
            println!(
                " 🌊 PHASE 2 & 3: ACTIVE PAGE CYCLER & TIER TRAFFIC (Holding Peak {}% for {}s)",
                max_safe_pct, opts.hold_sec
            );
            println!(
                " (Cycling dirty pages between RAM, ZRAM, and GPU VRAM to animate live speedometer graphs)"
            );
            println!("{}", "═".repeat(105));
        }

        let hold_end = Instant::now() + Duration::from_secs(opts.hold_sec);
        let mut cycle: usize = 0;
        let (_, _, init_cap3) = read_swap_tier_capacities();
        let mut hold_cap3_pct = init_cap3.pct;
        while Instant::now() < hold_end && !term_signal.load(Ordering::Relaxed) {
            cycle += 1;
            last_heartbeat.store(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                Ordering::Relaxed,
            );

            // Modify chunk pages smoothly to stimulate active tier traffic without saturating kernel queues.
            // If Tier 3 is heavily loaded, touch ONLY recent resident pages to avoid
            // triggering a catastrophic swap-in/swap-out thrash cycle against a near-full swap device.
            if let Ok(mut guard) = chunks.lock()
                && !guard.is_empty()
            {
                let len = guard.len();
                let target_chunk = if hold_cap3_pct >= TIER3_HEAVY_LOAD_PCT {
                    if let Some(last) = guard.last_mut() {
                        last
                    } else {
                        continue;
                    }
                } else {
                    let idx = (cycle.wrapping_mul(7)) % len;
                    &mut guard[idx]
                };
                let chunk_len = target_chunk.len();
                let (limit, stride) = if hold_cap3_pct >= TIER3_HEAVY_LOAD_PCT {
                    (
                        chunk_len.min(TIER3_HEAVY_TOUCH_BYTES),
                        TIER3_HEAVY_TOUCH_STRIDE_BYTES,
                    )
                } else {
                    (chunk_len.min(NORMAL_TOUCH_BYTES), NORMAL_TOUCH_STRIDE_BYTES)
                };
                for offset in (0..limit).step_by(stride) {
                    target_chunk[offset] = (cycle as u8).wrapping_add((offset & 0xFF) as u8);
                }
            }

            let (_, free_mb) = read_mem_info();
            let (tot_swap, z_mb, v_mb, s_mb) = read_swap_tiers();
            peak_zram = peak_zram.max(z_mb);
            peak_vram = peak_vram.max(v_mb);
            peak_ssd = peak_ssd.max(s_mb);
            peak_total_swap = peak_total_swap.max(tot_swap);
            let psi_full = read_psi_full();
            let lat_ms = probe_allocation_latency_ms();
            latencies_ms.push(lat_ms);

            let (cap1, cap2, cap3) = read_swap_tier_capacities();
            let cache_sample = read_cache_status_sample();
            if let Some(sample) = cache_sample {
                physical_cache_samples += 1;
                peak_physical_vram = peak_physical_vram.max(sample.cached_mib);
                peak_physical_target = peak_physical_target.max(sample.target_mib);
            }
            simultaneous_full_tiers |=
                full_tier_snapshot(cap1.pct, cap2.pct, cap3.pct, cache_sample);
            hold_cap3_pct = cap3.pct;
            let reading = compute_telemetry_reading(
                lat_ms,
                psi_full,
                total_allocated_mb,
                ram_total_mb,
                tot_swap,
            )
            .with_tier_pcts(cap1.pct, cap2.pct, cap3.pct);
            peak_pressure = peak_pressure.max(reading.pressure_index);
            readings_count += 1;
            active_cycles_done += 1;
            append_telemetry_log(&opts.telemetry_log, &reading);

            if !opts.json {
                print!(
                    " [🌊 Cycle #{:>2}] Idx: {:>4.1} {} Swap: {:>5} MB │ Free RAM: {:>5} MB │ PSI: {:>4.1}% │ Lat: {:.2}ms\r",
                    cycle,
                    reading.pressure_index,
                    reading.gauge,
                    tot_swap,
                    free_mb,
                    psi_full,
                    lat_ms
                );
                let _ = io::stdout().flush();
            }
            let hold_sleep_ms = if hold_cap3_pct >= TIER3_HEAVY_LOAD_PCT {
                TIER3_HEAVY_HOLD_INTERVAL_MS
            } else {
                NORMAL_HOLD_INTERVAL_MS
            };
            thread::sleep(Duration::from_millis(hold_sleep_ms));
            let now = Instant::now();
            let dt = now.duration_since(prev_sample_time).as_secs_f64();
            if dt > 0.05 {
                let (cur_z_bytes, cur_v_bytes, cur_s_bytes) = read_tier_disk_total_bytes();
                let dz = cur_z_bytes.saturating_sub(prev_z_bytes) as f64 / (1024.0 * 1024.0);
                let dv = cur_v_bytes.saturating_sub(prev_v_bytes) as f64 / (1024.0 * 1024.0);
                let ds = cur_s_bytes.saturating_sub(prev_s_bytes) as f64 / (1024.0 * 1024.0);

                peak_zram_mbs = peak_zram_mbs.max(dz / dt);
                peak_vram_mbs = peak_vram_mbs.max(dv / dt);
                peak_ssd_mbs = peak_ssd_mbs.max(ds / dt);

                prev_z_bytes = cur_z_bytes;
                prev_v_bytes = cur_v_bytes;
                prev_s_bytes = cur_s_bytes;
                prev_sample_time = now;
            }
        }
        if !opts.json {
            println!();
        }
    }

    // Phase 4: drop the test buffers; this is not a physical reclaim benchmark.
    let t_reclaim_start = Instant::now();
    if let Ok(mut guard) = chunks.lock() {
        guard.clear();
    }
    let reclaim_duration = t_reclaim_start.elapsed();

    term_signal.store(true, Ordering::Relaxed);
    let _ = watchdog_handle.join();

    thread::sleep(Duration::from_millis(500));
    let (_, post_free_ram) = read_mem_info();
    let (post_swap, _, _, _) = read_swap_tiers();
    let (cap1, _cap2, cap3) = read_swap_tier_capacities();

    let (
        avg_cycle_latency_ms,
        p50_cycle_latency_ms,
        p90_cycle_latency_ms,
        p99_cycle_latency_ms,
        max_cycle_latency_ms,
    ) = compute_latency_percentiles(&latencies_ms);
    let report = StressReport {
        metric_version: 2,
        battery_mode: opts.battery,
        cascade_mode: opts.cascade,
        max_safe_pct,
        total_allocated_mb,
        peak_swap_mb: peak_total_swap,
        tier1_zram_mb: peak_zram,
        tier1_zram_pct: (peak_zram * 100)
            .checked_div(cap1.total_mb)
            .unwrap_or(cap1.pct),
        tier2_vram_mb: peak_physical_vram,
        tier2_vram_pct: peak_physical_vram
            .saturating_mul(100)
            .checked_div(peak_physical_target)
            .unwrap_or(0),
        tier2_logical_swap_mb: peak_vram,
        tier2_nbd_throughput_mbs: (peak_vram_mbs * 10.0).round() / 10.0,
        tier2_physical_cache_target_mb: peak_physical_target,
        simultaneous_full_tiers,
        physical_cache_samples,
        tier3_ssd_mb: peak_ssd,
        tier3_ssd_pct: (peak_ssd * 100)
            .checked_div(cap3.total_mb)
            .unwrap_or(cap3.pct),
        tier1_throughput_mbs: (peak_zram_mbs * 10.0).round() / 10.0,
        // NBD block traffic does not measure GPU DMA throughput.
        tier2_throughput_mbs: None,
        tier3_throughput_mbs: (peak_ssd_mbs * 10.0).round() / 10.0,
        tier2_speedup_vs_ssd: None,
        peak_pressure_index: peak_pressure,
        telemetry_readings_count: readings_count,
        active_io_cycles_completed: active_cycles_done,
        reclaim_duration_ms: None,
        reclaim_speed_gbs: None,
        buffer_drop_duration_ms: reclaim_duration.as_secs_f64() * 1000.0,
        post_reclaim_free_ram_mb: post_free_ram,
        // This report lacks a bit-exact pressure check and an independent kernel-log window.
        status: "INCONCLUSIVE".to_string(),
        avg_cycle_latency_ms,
        p50_cycle_latency_ms,
        p90_cycle_latency_ms,
        p99_cycle_latency_ms,
        max_cycle_latency_ms,
        estimated_page_fault_lat_us: None,
        host_vram_min_free_mb: min_gpu_free_mb
            .unwrap_or_else(|| query_gpu_free_vram_mb().unwrap_or(0)),
        vram_evicted_chunks_count: None,
        dma_watchdog_trips_count: None,
        tier3_spillover_mb: peak_ssd,
        vram_eviction_p99_latency_ms: None,
        kernel_d_state_hung_tasks: count_kernel_hung_tasks(),
    };

    if opts.json {
        let json_out = serde_json::to_string_pretty(&report)
            .map_err(|e| format!("failed to serialize stress report: {e}"))?;
        println!("{json_out}");
    } else {
        println!("{}", "═".repeat(105));
        println!(" 🧹 PHASE 4: ATOMIC MEMORY RECLAIM & FLASH DEALLOCATION BENCHMARK");
        println!("{}", "═".repeat(105));
        println!(
            "[i] Test buffer drop duration: {:.2} ms (not physical reclaim)",
            report.buffer_drop_duration_ms
        );
        println!("[✓] Post-Reclaim Swap:      {} MB", post_swap);
        println!("[✓] Post-Reclaim Free RAM:  {} MB available", post_free_ram);
        println!("{}", "-".repeat(105));
        println!(" 📊 STRESS BATTERY QUALIFICATION REPORT:");
        println!(
            "  • Execution Mode:          {}",
            if report.cascade_mode {
                "MULTI-TIER CASCADE OBSERVATION"
            } else if report.battery_mode {
                "FULL 4-PHASE BATTERY"
            } else {
                "PROGRESSIVE GOVERNOR"
            }
        );
        println!(
            "  • Max Observed Allocation: {}% of RAM",
            report.max_safe_pct
        );
        println!(
            "  • Peak Memory Pressure:    {:.1} / 10.0",
            report.peak_pressure_index
        );
        println!("  • Flight Telemetry Log:    {}", opts.telemetry_log);
        println!(
            "  • Peak Allocated Memory:   {} MB",
            report.total_allocated_mb
        );
        println!("  • Peak Total Swap Used:    {} MB", report.peak_swap_mb);
        println!(
            "  • Tier 1 (ZRAM Swap):      {} MB Peak ({}% capacity, {:.1} MB/s Peak)",
            report.tier1_zram_mb, report.tier1_zram_pct, report.tier1_throughput_mbs
        );
        println!(
            "  • Physical VRAM cache:    {} MB Peak ({}% of {} MB target); NBD logical swap {} MB, {:.1} MB/s block traffic",
            report.tier2_vram_mb,
            report.tier2_vram_pct,
            report.tier2_physical_cache_target_mb,
            report.tier2_logical_swap_mb,
            report.tier2_nbd_throughput_mbs,
        );
        println!(
            "  • Tier 3 (SSD Storage):    {} MB Peak ({}% capacity, {:.1} MB/s Peak)",
            report.tier3_ssd_mb, report.tier3_ssd_pct, report.tier3_throughput_mbs
        );
        println!(
            "  • Active I/O Cycles:       {} cycles completed",
            report.active_io_cycles_completed
        );
        println!(
            "  • Simultaneous full tiers: {}",
            report.simultaneous_full_tiers
        );
        println!(
            "  • Physical cache samples:  {}",
            report.physical_cache_samples
        );
        println!(
            "  • Allocation Latency (P50): {:.4} ms (Median) │ P99: {:.4} ms (Tail Jitter) │ Max: {:.4} ms",
            report.p50_cycle_latency_ms, report.p99_cycle_latency_ms, report.max_cycle_latency_ms
        );
        println!(
            "  • Qualification verdict:  {} (requires independent integrity and kernel-log evidence)",
            report.status
        );
        println!("{}", "═".repeat(105));
    }

    archive_and_compare_benchmark(&report, opts.json);

    Ok(())
}

fn format_system_time(st: SystemTime) -> String {
    let dur = st.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let days = dur / 86400;
    let rem_sec = dur % 86400;
    let hours = rem_sec / 3600;
    let minutes = (rem_sec % 3600) / 60;
    let seconds = rem_sec % 60;
    let mut y = 1970;
    let mut d = days;
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0);
        let days_in_year = if leap { 366 } else { 365 };
        if d < days_in_year {
            break;
        }
        d -= days_in_year;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0);
    let days_in_months = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 1;
    for &dim in &days_in_months {
        if d < dim {
            break;
        }
        d -= dim;
        m += 1;
    }
    let day = d + 1;
    format!("{y:04}-{m:02}-{day:02}_{hours:02}-{minutes:02}-{seconds:02}")
}

fn archive_and_compare_benchmark(report: &StressReport, suppress_stdout: bool) {
    if cfg!(test) {
        return;
    }
    let directory = std::env::temp_dir().join("ramshared-benchmarks");
    if fs::create_dir_all(&directory).is_err() {
        return;
    }
    let filename = format!("observation-{}.json", format_system_time(SystemTime::now()));
    let path = directory.join(filename);
    if let Ok(encoded) = serde_json::to_vec_pretty(report)
        && fs::write(&path, encoded).is_ok()
        && !suppress_stdout
    {
        println!(
            "Observation saved at {} (unqualified until evidence review)",
            path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_throughput_uses_the_active_swap_device() {
        let swaps = "Filename Type Size Used Priority\n/dev/sdb partition 4194304 1000 -2\n";
        let diskstats =
            "8 16 sdb 2 0 8 0 3 0 16 0 0 0 0 0\n8 32 sdc 200 0 800 0 300 0 1600 0 0 0 0 0\n";
        assert_eq!(tier_disk_bytes_from(diskstats, swaps).2, 24 * 512);
    }

    #[test]
    fn cache_residency_requires_fresh_matching_daemon_identity() {
        let status = r#"{"ok":true,"origin_state":"READY","cache_state":"ACTIVE","daemon_instance_id":"73692-345270","written_at_unix_ms":10000,"vram_cached_kib":1048576,"cache_target_kib":4194304,"logical_capacity_kib":4194304}"#;
        let Some(sample) = parse_cache_status_sample(status, 11000, "73692-345270") else {
            panic!("fresh physical cache sample");
        };
        assert_eq!(sample.cached_mib, 1024);
        assert_eq!(sample.target_mib, 4096);
        assert!(parse_cache_status_sample(status, 15000, "73692-345270").is_none());
        assert!(parse_cache_status_sample(status, 11000, "73692-foreign").is_none());
    }

    #[test]
    fn full_tier_claim_requires_physical_cache_in_one_snapshot() {
        let cache = CacheSample {
            cached_mib: 1024,
            target_mib: 4096,
            at_target: false,
        };
        assert!(!full_tier_snapshot(100, 100, 100, Some(cache)));
        assert!(!full_tier_snapshot(100, 100, 100, None));
        let full = CacheSample {
            cached_mib: 4096,
            target_mib: 4096,
            at_target: true,
        };
        assert!(full_tier_snapshot(100, 100, 100, Some(full)));
        assert!(!full_tier_snapshot(100, 100, 99, Some(full)));
    }

    #[test]
    fn cascade_stress_refuses_missing_physical_cache_before_allocation() {
        assert!(require_physical_cache_before_cascade(true, None, None).is_err());
        assert!(require_physical_cache_before_cascade(false, Some(100), None).is_err());
        assert!(require_physical_cache_before_cascade(false, None, None).is_ok());
        assert!(
            require_physical_cache_before_cascade(
                true,
                Some(100),
                Some(CacheSample {
                    cached_mib: 0,
                    target_mib: 2048,
                    at_target: false,
                }),
            )
            .is_ok()
        );
    }

    #[test]
    fn parses_stress_cli_arguments_with_battery() {
        let args = vec![
            "--start".to_string(),
            "5".to_string(),
            "--target".to_string(),
            "80".to_string(),
            "--step".to_string(),
            "2".to_string(),
            "--interval-ms".to_string(),
            "500".to_string(),
            "--hold-sec".to_string(),
            "3".to_string(),
            "--min-ram-mb".to_string(),
            "800".to_string(),
            "--battery".to_string(),
            "--log".to_string(),
            "/tmp/test-telemetry.log".to_string(),
            "--json".to_string(),
        ];
        let opts = parse_stress_args(&args).unwrap_or_default();
        assert_eq!(opts.start_pct, 5);
        assert_eq!(opts.target_pct, 80);
        assert_eq!(opts.step_pct, 2);
        assert_eq!(opts.interval_ms, 500);
        assert_eq!(opts.hold_sec, 3);
        assert_eq!(opts.min_ram_mb, 800);
        assert!(opts.battery);
        assert_eq!(opts.telemetry_log, "/tmp/test-telemetry.log");
        assert!(opts.json);
    }

    #[test]
    fn rejects_thread_count_exceeding_physical_limits() {
        let args = vec!["--threads".to_string(), "999999".to_string()];
        let res = parse_stress_args(&args);
        match res {
            Err(err) => assert!(err.contains("exceeds physical hardware limit")),
            Ok(_) => panic!("expected thread limit validation error"),
        }
    }

    #[test]
    fn parses_stress_cli_argument_errors() {
        assert!(parse_stress_args(&["--start".to_string()]).is_err());
        assert!(parse_stress_args(&["--start".to_string(), "invalid".to_string()]).is_err());
        assert!(parse_stress_args(&["--target".to_string()]).is_err());
        assert!(parse_stress_args(&["--target".to_string(), "invalid".to_string()]).is_err());
        assert!(parse_stress_args(&["--step".to_string()]).is_err());
        assert!(parse_stress_args(&["--step".to_string(), "invalid".to_string()]).is_err());
        assert!(parse_stress_args(&["--interval-ms".to_string()]).is_err());
        assert!(parse_stress_args(&["--interval-ms".to_string(), "invalid".to_string()]).is_err());
        assert!(parse_stress_args(&["--hold-sec".to_string()]).is_err());
        assert!(parse_stress_args(&["--hold-sec".to_string(), "invalid".to_string()]).is_err());
        assert!(parse_stress_args(&["--min-ram-mb".to_string()]).is_err());
        assert!(parse_stress_args(&["--min-ram-mb".to_string(), "invalid".to_string()]).is_err());
        assert!(parse_stress_args(&["--log".to_string()]).is_err());
        assert!(parse_stress_args(&["--unknown-flag".to_string()]).is_err());
    }

    #[test]
    fn computes_telemetry_reading_accurately() {
        let s0 = compute_telemetry_reading(0.01, 0.0, 1000, 0, 0);
        assert!(s0.pressure_index >= 1.0);

        let s1 = compute_telemetry_reading(0.01, 0.0, 1000, 20000, 0);
        assert!(s1.pressure_index >= 1.0 && s1.pressure_index < 3.0);
        assert!(s1.classification.contains("NOMINAL"));

        let s2 = compute_telemetry_reading(0.5, 1.0, 10000, 20000, 300);
        assert!(s2.classification.contains("TIER-1"));

        let s3 = compute_telemetry_reading(1.5, 4.0, 14000, 20000, 1000);
        assert!(s3.classification.contains("TIER-2"));

        let s4 = compute_telemetry_reading(2.0, 5.0, 16000, 20000, 1500);
        assert!(s4.classification.contains("HIGH PRESSURE"));

        let s5 = compute_telemetry_reading(10.0, 25.0, 19000, 20000, 6000);
        assert!(s5.classification.contains("CEILING INTERLOCK"));
        assert!(s5.gauge.contains('█'));
    }

    #[test]
    fn appends_telemetry_log_file() {
        let path = "/tmp/test-ramshared-telemetry-unit.log";
        let reading = compute_telemetry_reading(0.05, 1.2, 2000, 20000, 100);
        append_telemetry_log(path, &reading);
        let content = fs::read_to_string(path).unwrap_or_default();
        assert!(content.contains("IDX"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn executes_micro_stress_runs_safely() {
        let opts_text = StressOptions {
            start_pct: 1,
            target_pct: 1,
            step_pct: 1,
            interval_ms: 10,
            hold_sec: 1,
            min_ram_mb: 200,
            battery: true,
            json: false,
            ..StressOptions::default()
        };
        assert!(run(&opts_text).is_ok());

        let opts_cascade = StressOptions {
            start_pct: 1,
            target_pct: 1,
            step_pct: 1,
            interval_ms: 10,
            hold_sec: 1,
            min_ram_mb: 200,
            battery: true,
            cascade: true,
            json: false,
            ..StressOptions::default()
        };
        assert!(run(&opts_cascade).is_err());

        let parsed_res = parse_stress_args(&["--cascade".to_string()]);
        assert!(parsed_res.is_ok());
        let parsed_cascade = parsed_res.unwrap_or_default();
        assert!(parsed_cascade.cascade);
        assert!(parsed_cascade.battery);
    }

    #[test]
    fn helper_probes_execute_safely() {
        let (total, avail) = read_mem_info();
        assert!(total > 0 || avail == 0);
        let _ = read_psi_full();
        let _ = read_swap_tiers();
        let (c1, c2, c3) = read_swap_tier_capacities();
        let _ = format!("{c1:?} {c2:?} {c3:?}");
        let lat = probe_allocation_latency_ms();
        assert!(lat >= 0.0);
        let _ = read_buddyinfo_order_7();
        trigger_proactive_compaction();
        let _ = read_tier_disk_total_bytes();
        let _ = is_wsl2();
        let _ = read_sysctl_min_free_mb();
    }

    #[test]
    fn executes_micro_stress_runs_json_and_telemetry() {
        let opts_json = StressOptions {
            start_pct: 1,
            target_pct: 1,
            step_pct: 1,
            interval_ms: 10,
            hold_sec: 1,
            min_ram_mb: 200,
            battery: true,
            json: true,
            ..StressOptions::default()
        };
        assert!(run(&opts_json).is_ok());

        let reading = compute_telemetry_reading(0.1, 0.5, 100, 1000, 10).with_tier_pcts(10, 20, 30);
        assert_eq!(reading.tier1_zram_pct, 10);
        assert_eq!(reading.tier2_vram_pct, 20);
        assert_eq!(reading.tier3_ssd_pct, 30);
    }

    #[test]
    fn parse_all_stress_flags_and_help() {
        let all_args = vec![
            "--start".to_string(),
            "5".to_string(),
            "--target".to_string(),
            "90".to_string(),
            "--step".to_string(),
            "2".to_string(),
            "--interval-ms".to_string(),
            "50".to_string(),
            "--hold-sec".to_string(),
            "3".to_string(),
            "--min-ram-mb".to_string(),
            "300".to_string(),
            "--max-psi-full".to_string(),
            "65.5".to_string(),
            "--max-latency-ms".to_string(),
            "15.2".to_string(),
            "--tier3-target-pct".to_string(),
            "75".to_string(),
            "--log".to_string(),
            "/tmp/test-tel.log".to_string(),
            "--json".to_string(),
            "--battery".to_string(),
            "--cascade".to_string(),
        ];
        let parsed = parse_stress_args(&all_args).unwrap_or_default();
        assert_eq!(parsed.start_pct, 5);
        assert_eq!(parsed.target_pct, 90);
        assert_eq!(parsed.step_pct, 2);
        assert_eq!(parsed.interval_ms, 50);
        assert_eq!(parsed.hold_sec, 3);
        assert_eq!(parsed.min_ram_mb, 300);
        assert!((parsed.max_psi_full - 65.5).abs() < 0.01);
        assert!((parsed.max_latency_ms - 15.2).abs() < 0.01);
        assert_eq!(parsed.tier3_target_pct, Some(75));
        assert_eq!(parsed.telemetry_log, "/tmp/test-tel.log");
        assert!(parsed.json);
        assert!(parsed.battery);
        assert!(parsed.cascade);

        let unknown_res = parse_stress_args(&["--unknown-flag".to_string()]);
        assert!(unknown_res.is_err());
    }

    #[test]
    fn benchmark_archiving_and_formatting_tests() {
        let now = SystemTime::now();
        let formatted = format_system_time(now);
        assert!(!formatted.is_empty());
        assert!(formatted.contains('_'));

        let report = StressReport {
            metric_version: 2,
            battery_mode: true,
            cascade_mode: true,
            max_safe_pct: 90,
            total_allocated_mb: 1000,
            peak_swap_mb: 500,
            tier1_zram_mb: 200,
            tier1_zram_pct: 50,
            tier2_vram_mb: 200,
            tier2_vram_pct: 50,
            tier2_logical_swap_mb: 200,
            tier2_nbd_throughput_mbs: 500.0,
            tier2_physical_cache_target_mb: 400,
            simultaneous_full_tiers: false,
            physical_cache_samples: 1,
            tier3_ssd_mb: 100,
            tier3_ssd_pct: 25,
            tier1_throughput_mbs: 1000.0,
            tier2_throughput_mbs: Some(500.0),
            tier3_throughput_mbs: 20.0,
            tier2_speedup_vs_ssd: Some(25.0),
            peak_pressure_index: 10.0,
            telemetry_readings_count: 5,
            active_io_cycles_completed: 2,
            reclaim_duration_ms: Some(1.0),
            reclaim_speed_gbs: Some(1000.0),
            buffer_drop_duration_ms: 1.0,
            post_reclaim_free_ram_mb: 8000,
            status: "PASS_ZERO_PANIC".to_string(),
            avg_cycle_latency_ms: 0.05,
            p50_cycle_latency_ms: 0.02,
            p90_cycle_latency_ms: 0.08,
            p99_cycle_latency_ms: 0.15,
            max_cycle_latency_ms: 0.50,
            estimated_page_fault_lat_us: Some(0.85),
            host_vram_min_free_mb: 2048,
            vram_evicted_chunks_count: Some(0),
            dma_watchdog_trips_count: Some(0),
            tier3_spillover_mb: 100,
            vram_eviction_p99_latency_ms: Some(0.0),
            kernel_d_state_hung_tasks: 0,
        };
        archive_and_compare_benchmark(&report, false);
        archive_and_compare_benchmark(&report, true);
    }

    #[test]
    fn computes_latency_percentiles_accurately() {
        let (avg, p50, p90, p99, max) = compute_latency_percentiles(&[]);
        assert_eq!(avg, 0.0);
        assert_eq!(p50, 0.0);
        assert_eq!(p90, 0.0);
        assert_eq!(p99, 0.0);
        assert_eq!(max, 0.0);

        let latencies = vec![0.01, 0.02, 0.03, 0.04, 0.05, 0.10, 0.20, 0.50, 1.00, 2.00];
        let (avg, p50, p90, p99, max) = compute_latency_percentiles(&latencies);
        assert!(avg > 0.0);
        assert!((0.05..=0.20).contains(&p50));
        assert!(p90 >= p50);
        assert!(p99 >= 1.00);
        assert_eq!(max, 2.00);
    }

    #[test]
    fn safe_allocation_never_crosses_the_physical_floor() {
        assert_eq!(safe_allocation_mb(true, true, 600, 600, 128), 0);
        assert_eq!(safe_allocation_mb(true, true, 616, 600, 128), 0);
        assert_eq!(safe_allocation_mb(true, true, 617, 600, 128), 16);
        assert_eq!(safe_allocation_mb(true, true, 649, 600, 128), 32);
        assert_eq!(safe_allocation_mb(true, false, 600, 600, 128), 0);
        assert_eq!(safe_allocation_mb(false, false, 600, 600, 128), 0);
    }

    #[test]
    fn tier3_target_requires_the_requested_tier3_percentage() {
        assert!(!tier3_target_reached(98, 99));
        assert!(tier3_target_reached(99, 99));
    }

    #[test]
    fn tier3_pacing_has_a_wsl2_floor() {
        assert_eq!(step_interval_ms(true, true, 200), 500);
        assert_eq!(step_interval_ms(true, true, 800), 800);
        assert_eq!(step_interval_ms(true, false, 200), 200);
        assert_eq!(step_interval_ms(false, true, 200), 200);
    }

    #[test]
    fn wsl2_hard_floor_enforces_safety_ceiling() {
        if is_wsl2() {
            let sysctl_min = read_sysctl_min_free_mb();
            let opts = StressOptions::default();
            let target_physical_floor = opts.min_ram_mb.max(WSL2_MIN_PHYSICAL_HEADROOM_MB);
            let hard_floor = target_physical_floor.saturating_sub(sysctl_min).max(100);
            assert!(
                hard_floor + sysctl_min >= 600,
                "Total physical headroom (hard_floor + sysctl_min) on WSL2 must never be lower than 600 MB"
            );
        }
    }

    #[test]
    fn test_buddyinfo_order_7_parsing() {
        let sample = "Node 0, zone      DMA      1      0      1      0      2      1      1      0      1      1      3 \n\
                      Node 0, zone    DMA32      2      1      2      0      1      1      1      2      0      2    974 \n\
                      Node 0, zone   Normal   2702   4568   2435   1170    635    356    215    147     91    112   1292 \n";
        assert_eq!(parse_buddyinfo_order_7_chunks(sample), Some(147));

        let zero_sample =
            "Node 0, zone   Normal   815   1332   2330   1837   3290   753   3   0   0   0   0 \n";
        assert_eq!(parse_buddyinfo_order_7_chunks(zero_sample), Some(0));

        assert_eq!(
            parse_buddyinfo_order_7_chunks("Node 0, zone DMA 1 2 3"),
            None
        );
    }

    #[test]
    fn test_buddyinfo_effective_order_7_parsing() {
        let sample = "Node 0, zone      DMA      1      0      1      0      2      1      1      0      1      1      3 \n\
                      Node 0, zone    DMA32      2      1      2      0      1      1      1      2      0      2    974 \n\
                      Node 0, zone   Normal   2702   4568   2435   1170    635    356    215    147     91    112   1292 \n";
        // 147 + (91 * 2) + (112 * 4) + (1292 * 8) = 11113
        assert_eq!(
            parse_buddyinfo_effective_order_7_chunks(sample),
            Some(11113)
        );

        // When order-7 is 3, but order-10 has 1030 chunks, effective order-7 is 3 + (1030 * 8) = 8243
        let abundant_higher = "Node 0, zone   Normal   710   882   1943   488   488   331   143     3     0      0   1030 \n";
        assert_eq!(
            parse_buddyinfo_effective_order_7_chunks(abundant_higher),
            Some(8243)
        );

        let zero_sample =
            "Node 0, zone   Normal   815   1332   2330   1837   3290   753   3   0   0   0   0 \n";
        assert_eq!(
            parse_buddyinfo_effective_order_7_chunks(zero_sample),
            Some(0)
        );
    }

    #[test]
    fn test_buddyinfo_order_7_interlock_threshold() {
        assert!(is_order_7_depleted(Some(0), MIN_ORDER_7_BUDDY_CHUNKS));
        assert!(is_order_7_depleted(Some(7), MIN_ORDER_7_BUDDY_CHUNKS));
        assert!(!is_order_7_depleted(Some(8), MIN_ORDER_7_BUDDY_CHUNKS));
        assert!(!is_order_7_depleted(Some(147), MIN_ORDER_7_BUDDY_CHUNKS));
        assert!(!is_order_7_depleted(None, MIN_ORDER_7_BUDDY_CHUNKS));
    }

    #[test]
    fn test_wsl2_headroom_floor_enforces_600_mb() {
        const {
            assert!(
                WSL2_MIN_PHYSICAL_HEADROOM_MB >= 600,
                "WSL2_MIN_PHYSICAL_HEADROOM_MB must be at least 600 MB to provide physical headroom"
            );
        }
    }

    #[test]
    fn test_decide_buddyinfo_action() {
        // Depleted below MIN_ORDER_7_BUDDY_CHUNKS (8) -> Halt
        assert_eq!(
            decide_buddyinfo_action(Some(0)),
            BuddyInterlockAction::Halt { detected_chunks: 0 }
        );
        assert_eq!(
            decide_buddyinfo_action(Some(7)),
            BuddyInterlockAction::Halt { detected_chunks: 7 }
        );

        // Healthy abundant (> PROACTIVE_COMPACTION_TRIGGER_CHUNKS = 16) -> Continue
        assert_eq!(
            decide_buddyinfo_action(Some(50)),
            BuddyInterlockAction::Continue
        );

        // None (not readable or non-WSL2) -> Continue
        assert_eq!(
            decide_buddyinfo_action(None),
            BuddyInterlockAction::Continue
        );
    }

    #[test]
    fn test_sample_min_gpu_headroom_rate_limiting() {
        let mut last_sample_ms = 0u64;
        let mut min_gpu_free_mb = None;

        // First call samples or skips depending on environment, but updates timestamp
        sample_min_gpu_headroom(&mut last_sample_ms, &mut min_gpu_free_mb);
        assert!(last_sample_ms > 0);

        let saved_ms = last_sample_ms;
        // Immediate second call (< 1000ms) should be rate-limited and preserve timestamp
        sample_min_gpu_headroom(&mut last_sample_ms, &mut min_gpu_free_mb);
        assert_eq!(last_sample_ms, saved_ms);
    }
}
