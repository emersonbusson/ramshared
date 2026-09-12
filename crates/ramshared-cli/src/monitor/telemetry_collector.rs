use crate::bounded_process;
use crate::workload;
use serde::{Deserialize, Serialize};
use serde_json::Map;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// Formats a value as a single line.
pub fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
/// Observation of system memory pressure and distribution.
pub struct MemoryObservation {
    pub total_kib: u64,
    pub available_kib: u64,
    pub swap_total_kib: u64,
    pub swap_free_kib: u64,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
/// Observation of tier IO statistics.
pub struct TierIoStats {
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub read_mbs: f64,
    pub write_mbs: f64,
    pub min_mbs: f64,
    pub avg_mbs: f64,
    pub max_mbs: f64,
    pub peak_mbs: f64,
    pub min_lat_us: f64,
    pub avg_lat_us: f64,
    pub max_lat_us: f64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
/// Observation of the ramshared control plane.
pub struct ControlPlaneObservation {
    pub memory_psi_some_avg10: f64,
    pub memory_psi_some_avg60: f64,
    pub memory_psi_some_avg300: f64,
    pub memory_psi_full_avg10: f64,
    pub memory_psi_full_avg60: f64,
    pub memory_psi_full_avg300: f64,
    pub swap_in_pages: u64,
    pub swap_out_pages: u64,
    pub swap_read_bytes: u64,
    pub swap_write_bytes: u64,
    pub swap_read_mbs: f64,
    pub swap_write_mbs: f64,
    pub swap_peak_mbs: f64,
    pub zram_peak_used_mb: u64,
    pub vram_peak_used_mb: u64,
    pub disk_peak_used_mb: u64,
    pub zram_io: TierIoStats,
    pub vram_io: TierIoStats,
    pub disk_io: TierIoStats,
    pub boot_tier_latency_ms: Option<u64>,
    pub uptime_seconds: u64,
    pub pgfault_total: u64,
    pub pgmajfault_total: u64,
    pub pgfault_per_sec: u64,
    pub pgmajfault_per_sec: u64,
    pub memory_events: MemoryEvents,
    pub active_scopes: u64,
    pub docker_memory_current_bytes: u64,
    pub managed_reservations: u64,
    pub managed_reserved_bytes: u64,
    pub unmanaged_pressure_state: String,
    pub unmanaged_pressure_kib: u64,
    pub unmanaged_processes: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MemoryEvents {
    pub high: u64,
    pub max: u64,
    pub oom: u64,
    pub oom_kill: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
/// Observation of a system process.
pub struct ProcessObservation {
    pub comm: String,
    pub unit: String,
    pub cgroup: String,
    pub rss_kib: u64,
    pub swap_kib: u64,
    pub cpu_ticks: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub managed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
/// Observation of a managed GPU.
pub struct GpuObservation {
    pub name: String,
    pub total_mib: u64,
    pub used_mib: u64,
    pub free_mib: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Observation {
    #[serde(flatten)]
    pub status: BTreeMap<String, Value>,
    pub epoch_ms: u64,
    pub sample_age_ms: u64,
    pub mem: MemoryObservation,
    pub control_plane: ControlPlaneObservation,
    pub gpu: Option<GpuObservation>,
    pub top_processes: Vec<ProcessObservation>,
    pub errors: Vec<String>,
}

impl Observation {
    pub fn value(&self, key: &str) -> Option<&Value> {
        self.status.get(key)
    }

    pub fn string(&self, key: &str) -> &str {
        self.value(key).and_then(Value::as_str).unwrap_or("unknown")
    }

    pub fn bool_value(&self, key: &str) -> Option<bool> {
        self.value(key).and_then(Value::as_bool)
    }
}

/// Parses `/proc/meminfo` contents.
pub fn parse_meminfo(text: &str) -> MemoryObservation {
    let value = |name: &str| {
        text.lines()
            .find_map(|line| {
                let (key, rest) = line.split_once(':')?;
                (key == name)
                    .then(|| rest.split_whitespace().next()?.parse::<u64>().ok())
                    .flatten()
            })
            .unwrap_or(0)
    };
    MemoryObservation {
        total_kib: value("MemTotal"),
        available_kib: value("MemAvailable"),
        swap_total_kib: value("SwapTotal"),
        swap_free_kib: value("SwapFree"),
    }
}

pub fn parse_memory_events(text: &str) -> MemoryEvents {
    let value = |name: &str| {
        text.lines()
            .find_map(|line| {
                let mut fields = line.split_whitespace();
                (fields.next()? == name)
                    .then(|| fields.next()?.parse::<u64>().ok())
                    .flatten()
            })
            .unwrap_or(0)
    };
    MemoryEvents {
        high: value("high"),
        max: value("max"),
        oom: value("oom"),
        oom_kill: value("oom_kill"),
    }
}

/// Parses vmstat.
pub fn parse_vmstat(text: &str) -> (u64, u64, u64, u64) {
    let value = |name: &str| {
        text.lines().find_map(|line| {
            let mut fields = line.split_whitespace();
            (fields.next()? == name)
                .then(|| fields.next()?.parse::<u64>().ok())
                .flatten()
        })
    };
    (
        value("pswpin").unwrap_or(0),
        value("pswpout").unwrap_or(0),
        value("pgfault").unwrap_or(0),
        value("pgmajfault").unwrap_or(0),
    )
}

/// Reads reservation totals from the ledger.
pub fn read_reservation_totals(path: &Path) -> (u64, u64) {
    workload::read_reservation_ledger(path).map_or_else(
        |_| (0, 0),
        |reservations| {
            let reserved_bytes = reservations
                .iter()
                .map(|reservation| reservation.memory_bytes)
                .sum();
            (reservations.len() as u64, reserved_bytes)
        },
    )
}

/// Parses unit startup milliseconds.
pub fn parse_unit_startup_ms(show_output: &str) -> Option<u64> {
    let mut inactive_exit = None;
    let mut active_enter = None;

    for line in show_output.lines() {
        if line.is_empty() {
            if let (Some(start), Some(end)) = (inactive_exit, active_enter)
                && end > start
                && start > 0
            {
                return Some((end - start) / 1000);
            }
            inactive_exit = None;
            active_enter = None;
            continue;
        }
        if let Some(val) = line.strip_prefix("InactiveExitTimestampMonotonic=") {
            inactive_exit = val.trim().parse::<u64>().ok();
        } else if let Some(val) = line.strip_prefix("ActiveEnterTimestampMonotonic=") {
            active_enter = val.trim().parse::<u64>().ok();
        }
    }

    if let (Some(start), Some(end)) = (inactive_exit, active_enter)
        && end > start
        && start > 0
    {
        return Some((end - start) / 1000);
    }
    None
}

/// Classifies unmanaged system pressure.
pub fn classify_unmanaged_pressure(processes: &[ProcessObservation]) -> (&'static str, u64, u64) {
    let mut count = 0u64;
    let mut kib = 0u64;
    for process in processes {
        let footprint = process.rss_kib.saturating_add(process.swap_kib);
        if !process.managed && footprint >= 512 * 1024 {
            count = count.saturating_add(1);
            kib = kib.saturating_add(footprint);
        }
    }
    if count == 0 {
        ("NONE", 0, 0)
    } else {
        ("UNMANAGED_PRESSURE", kib, count)
    }
}

/// Returns commands used to query GPUs.
pub fn gpu_query_candidates() -> [&'static str; 2] {
    ["nvidia-smi", "/usr/lib/wsl/lib/nvidia-smi"]
}

/// Executes a GPU query command.
pub fn query_gpu_command(command: &str, timeout: Duration) -> Result<GpuObservation, String> {
    let mut query = Command::new(command);
    query.args([
        "--query-gpu=name,memory.total,memory.used,memory.free",
        "--format=csv,noheader,nounits",
    ]);
    let output = match bounded_process::run_capture_command(
        &mut query,
        &format!("GPU query {command}"),
        timeout,
        bounded_process::DEFAULT_OUTPUT_LIMIT,
        |_| {},
    ) {
        Ok(output) => output,
        Err(error) if error.is_not_found() => {
            return Err(format!("gpu_query_not_found:{command}"));
        }
        Err(error) => return Err(format!("gpu_query_output:{error}")),
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("gpu_query_failed:{}", one_line(&stderr)));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let Some(line) = stdout.lines().next().filter(|line| !line.trim().is_empty()) else {
        return Err("gpu_query_empty".into());
    };
    let fields: Vec<&str> = line.split(',').map(str::trim).collect();
    if fields.len() != 4 {
        return Err("gpu_query_invalid_field_count".to_string());
    }
    Ok(GpuObservation {
        name: fields[0].to_string(),
        total_mib: parse_gpu_number(fields[1])?,
        used_mib: parse_gpu_number(fields[2])?,
        free_mib: parse_gpu_number(fields[3])?,
    })
}

/// Applies a measurement failure to a status map.
pub fn apply_measurement_failure(status: &mut Map<String, Value>, error: &str) {
    status.insert("ok".into(), Value::Bool(false));
    status.insert("overall_state".into(), Value::String("BLOCKED".into()));
    status.insert(
        "measurement_state".into(),
        serde_json::json!({"state":"FAILED","error":error}),
    );
}

/// Parses a numeric value from GPU output.
pub fn parse_gpu_number(value: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|_| "gpu_query_invalid_number".to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn gpu_measurement_failure_is_explicit_and_not_green() {
        let mut status = serde_json::Map::from_iter([
            ("ok".into(), Value::Bool(true)),
            ("overall_state".into(), Value::String("HEALTHY".into())),
        ]);
        apply_measurement_failure(&mut status, "gpu_query_timeout");
        assert_eq!(status["ok"], false);
        assert_eq!(status["overall_state"], "BLOCKED");
        assert_eq!(status["measurement_state"]["error"], "gpu_query_timeout");
        assert_eq!(
            gpu_query_candidates(),
            ["nvidia-smi", "/usr/lib/wsl/lib/nvidia-smi"]
        );
    }

    use super::*;
    use crate::monitor::{apply_measurement_failure, gpu_query_candidates};
}
