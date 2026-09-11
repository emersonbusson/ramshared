use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::os::unix::fs::PermissionsExt;
use std::fs;
use std::process::Command;
use crate::bounded_process;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MemoryObservation {
    pub total_kib: u64,
    pub available_kib: u64,
    pub swap_total_kib: u64,
    pub swap_free_kib: u64,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
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
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
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
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MemoryEvents {
    pub high: u64,
    pub max: u64,
    pub oom: u64,
    pub oom_kill: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
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
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GpuObservation {
    pub name: String,
    pub total_mib: u64,
    pub used_mib: u64,
    pub free_mib: u64,
}

#[derive(Clone, Debug, Serialize)]
#[derive(Clone, Debug, Deserialize, Serialize)]
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

pub fn parse_memory_pressure(text: &str) -> ControlPlaneObservation {
    fn average(line: Option<&str>, name: &str) -> f64 {
        line.and_then(|line| {
            line.split_whitespace().find_map(|field| {
                field
                    .strip_prefix(name)
                    .and_then(|value| value.parse::<f64>().ok())
            })
        })
        .unwrap_or(0.0)
    }
    let some = text.lines().find(|line| line.starts_with("some "));
    let full = text.lines().find(|line| line.starts_with("full "));
    ControlPlaneObservation {
        memory_psi_some_avg10: average(some, "avg10="),
        memory_psi_some_avg60: average(some, "avg60="),
        memory_psi_some_avg300: average(some, "avg300="),
        memory_psi_full_avg10: average(full, "avg10="),
        memory_psi_full_avg60: average(full, "avg60="),
        memory_psi_full_avg300: average(full, "avg300="),
        ..ControlPlaneObservation::default()
    }
}

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

pub fn parse_swap_diskstats(text: &str) -> (u64, u64) {
    let mut read_bytes = 0u64;
    let mut write_bytes = 0u64;
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 10 {
            let dev = fields[2];
            if (dev.starts_with("nbd")
                || dev.starts_with("zram")
                || dev == "sdc"
                || dev.starts_with("ramshared"))
                && let (Ok(read_sectors), Ok(write_sectors)) =
                    (fields[5].parse::<u64>(), fields[9].parse::<u64>())
            {
                read_bytes = read_bytes.saturating_add(read_sectors.saturating_mul(512));
                write_bytes = write_bytes.saturating_add(write_sectors.saturating_mul(512));
            }
        }
    }
    (read_bytes, write_bytes)
}

pub fn parse_per_tier_diskstats(text: &str) -> (TierIoStats, TierIoStats, TierIoStats) {
    let mut zram = TierIoStats::default();
    let mut vram = TierIoStats::default();
    let mut disk = TierIoStats::default();

    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 10
            && let (Ok(read_sectors), Ok(write_sectors)) =
                (fields[5].parse::<u64>(), fields[9].parse::<u64>())
        {
            let dev = fields[2];
            let rb = read_sectors.saturating_mul(512);
            let wb = write_sectors.saturating_mul(512);
            if dev.starts_with("zram") {
                zram.read_bytes = zram.read_bytes.saturating_add(rb);
                zram.write_bytes = zram.write_bytes.saturating_add(wb);
            } else if dev.starts_with("nbd") || dev.starts_with("ramshared") {
                vram.read_bytes = vram.read_bytes.saturating_add(rb);
                vram.write_bytes = vram.write_bytes.saturating_add(wb);
            } else if dev == "sdc" {
                disk.read_bytes = disk.read_bytes.saturating_add(rb);
                disk.write_bytes = disk.write_bytes.saturating_add(wb);
            }
        }
    }
    (zram, vram, disk)
}

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

pub fn parse_uptime_seconds(uptime_str: &str) -> Option<u64> {
    uptime_str
        .split_whitespace()
        .next()
        .and_then(|s| s.parse::<f64>().ok())
        .map(|f| f as u64)
}

pub fn query_unit_startup_ms() -> Option<u64> {
    let output = Command::new("systemctl")
        .args([
            "show",
            "ramshared-vram-tier.service",
            "ramshared-vram.service",
            "--property=ActiveEnterTimestampMonotonic,InactiveExitTimestampMonotonic",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    parse_unit_startup_ms(&text)
}

pub fn gpu_query_candidates() -> [&'static str; 2] {
    ["nvidia-smi", "/usr/lib/wsl/lib/nvidia-smi"]
}

pub fn query_gpu_bounded(timeout: Duration) -> Result<Option<GpuObservation>, String> {
    if let Ok(cuda) = ramshared_cuda::Cuda::load() {
        let dev_opt = cuda.device(0).ok();
        if let Some(dev) = dev_opt {
            let res = cuda.create_context(&dev).and_then(|ctx| ctx.mem_info());
            if let Ok((free_b, total_b)) = res {
                let total_mib = (total_b / 1_048_576) as u64;
                let free_mib = (free_b / 1_048_576) as u64;
                let used_mib = total_mib.saturating_sub(free_mib);
                let name = dev.name().to_string();
                return Ok(Some(GpuObservation {
                    name,
                    total_mib,
                    used_mib,
                    free_mib,
                }));
            }
        }
    }

    let mut last_error = "gpu_query_unavailable".to_string();
    for candidate in gpu_query_candidates() {
        match query_gpu_command(candidate, timeout) {
            Ok(sample) => return Ok(Some(sample)),
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

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

pub fn parse_gpu_number(value: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|_| "gpu_query_invalid_number".to_string())
}

pub fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn collect_top_processes(proc_root: &Path, limit: usize) -> Vec<ProcessObservation> {
    let mut processes = fs::read_dir(proc_root)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .bytes()
                .all(|byte| byte.is_ascii_digit())
        })
        .filter_map(|entry| process_observation(&entry.path()))
        .collect::<Vec<_>>();
    processes
        .sort_by_key(|process| std::cmp::Reverse(process.rss_kib.saturating_add(process.swap_kib)));
    processes.truncate(limit);
    processes
}

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

pub fn process_observation(path: &Path) -> Option<ProcessObservation> {
    let comm = fs::read_to_string(path.join("comm")).ok()?;
    let comm = sanitize_label(comm.trim(), 64);
    let cgroup_text = fs::read_to_string(path.join("cgroup")).unwrap_or_default();
    let cgroup = cgroup_text
        .lines()
        .find_map(|line| line.split_once("::").map(|(_, value)| value))
        .map(|value| sanitize_cgroup(value, 256))
        .unwrap_or_else(|| "/".into());
    let unit = cgroup
        .split('/')
        .rev()
        .find(|component| {
            component.ends_with(".scope")
                || component.ends_with(".service")
                || component.ends_with(".slice")
        })
        .map(|value| sanitize_label(value, 128))
        .unwrap_or_else(|| "unknown".into());
    let status = fs::read_to_string(path.join("status")).unwrap_or_default();
    let status_kib = |name: &str| {
        status
            .lines()
            .find_map(|line| {
                let (key, rest) = line.split_once(':')?;
                (key == name)
                    .then(|| rest.split_whitespace().next()?.parse::<u64>().ok())
                    .flatten()
            })
            .unwrap_or(0)
    };
    let stat = fs::read_to_string(path.join("stat")).unwrap_or_default();
    let cpu_ticks = stat
        .rsplit_once(") ")
        .map(|(_, fields)| fields.split_whitespace().collect::<Vec<_>>())
        .and_then(|fields| {
            Some(fields.get(11)?.parse::<u64>().ok()? + fields.get(12)?.parse::<u64>().ok()?)
        })
        .unwrap_or(0);
    let io = fs::read_to_string(path.join("io")).unwrap_or_default();
    let io_value = |name: &str| {
        io.lines()
            .find_map(|line| {
                let (key, value) = line.split_once(':')?;
                (key == name)
                    .then(|| value.trim().parse::<u64>().ok())
                    .flatten()
            })
            .unwrap_or(0)
    };
    Some(ProcessObservation {
        comm,
        unit,
        managed: cgroup.contains("ramshared-workloads"),
        cgroup,
        rss_kib: status_kib("VmRSS"),
        swap_kib: status_kib("VmSwap"),
        cpu_ticks,
        read_bytes: io_value("read_bytes"),
        write_bytes: io_value("write_bytes"),
    })
}

pub fn sanitize_label(value: &str, limit: usize) -> String {
    value
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | '@')
        })
        .take(limit)
        .collect()
}

pub fn sanitize_cgroup(value: &str, limit: usize) -> String {
    value
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '/' | '.' | '_' | '-' | '@' | ':')
        })
        .take(limit)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitor::{apply_measurement_failure, count_scope_dirs, read_reservation_totals, format_tier_latency};
    #[test]
    #[test]
    fn gpu_measurement_failure_is_explicit_and_not_green() {
        let mut status = Map::from_iter([
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

    #[test]
    #[test]
    fn gpu_query_contains_descendant_inherited_pipe_and_keeps_success_valid() {
        let root = std::env::temp_dir().join(format!(
            "ramshared-monitor-gpu-child-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap_or_else(|_| panic!("unwrap failed"));
        let write_program = |name: &str, source: &str| {
            let path = root.join(name);
            fs::write(&path, source).unwrap_or_else(|_| panic!("unwrap failed"));
            let mut permissions = fs::metadata(&path).unwrap_or_else(|_| panic!("unwrap failed")).permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(&path, permissions).unwrap_or_else(|_| panic!("unwrap failed"));
            path
        };
        let success = write_program(
            "gpu-success",
            "#!/bin/sh\nprintf 'Fixture GPU, 6144, 2048, 4096\\n'\n",
        );
        let sample = query_gpu_command(success.to_str().unwrap_or_else(|| panic!("unwrap failed")), Duration::from_millis(250))
            .unwrap_or_else(|_| panic!("legitimate GPU fixture must remain accepted"));
        assert_eq!(sample.name, "Fixture GPU");
        assert_eq!(
            (sample.total_mib, sample.used_mib, sample.free_mib),
            (6144, 2048, 4096)
        );

        let inherited = write_program(
            "gpu-inherited-output",
            "#!/bin/sh\n(sleep 1) &\nprintf 'Fixture GPU, 6144, 2048, 4096\\n'\nexit 0\n",
        );
        let started = Instant::now();
        let error = query_gpu_command(inherited.to_str().unwrap_or_else(|| panic!("unwrap failed")), Duration::from_millis(100))
            .err().unwrap_or_else(|| unreachable!("an inherited output pipe must not be accepted as GPU success"));
        fs::remove_dir_all(root).unwrap_or_else(|_| panic!("unwrap failed"));

        assert!(started.elapsed() < Duration::from_millis(750));
        assert!(error.contains("output"), "{error}");
    }

    #[test]
    #[test]
    #[test]
    fn parses_diskstats_and_startup_ms() {
        let stats = " 252       0 zram0 10 0 200 0 20 0 400 0 0 0 0\n  43       0 nbd0 5 0 100 0 15 0 300 0 0 0 0\n   8      32 sdc 2 0 40 0 4 0 80 0 0 0 0\n";
        let (tot_r, tot_w) = parse_swap_diskstats(stats);
        assert_eq!(tot_r, (200 + 100 + 40) * 512);
        assert_eq!(tot_w, (400 + 300 + 80) * 512);

        let (z, v, d) = parse_per_tier_diskstats(stats);
        assert_eq!(z.read_bytes, 200 * 512);
        assert_eq!(z.write_bytes, 400 * 512);
        assert_eq!(v.read_bytes, 100 * 512);
        assert_eq!(v.write_bytes, 300 * 512);
        assert_eq!(d.read_bytes, 40 * 512);
        assert_eq!(d.write_bytes, 80 * 512);

        let show_out =
            "InactiveExitTimestampMonotonic=1000000\nActiveEnterTimestampMonotonic=3890000\n";
        assert_eq!(parse_unit_startup_ms(show_out), Some(2890));
        assert_eq!(parse_unit_startup_ms(""), None);

        assert_eq!(parse_uptime_seconds("1540.25 3080.50"), Some(1540));
        assert_eq!(parse_uptime_seconds("invalid"), None);

        assert_eq!(sanitize_label("my-app_1.0@daemon!", 10), "my-app_1.0");
        assert_eq!(
            sanitize_cgroup("/system.slice/test.service", 20),
            "/system.slice/test.s"
        );
        let temp_empty = std::env::temp_dir().join(format!("test-scopes-{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_empty);
        assert_eq!(count_scope_dirs(&temp_empty), 0);
        assert_eq!(read_reservation_totals(&temp_empty), (0, 0));
        let _ = fs::remove_dir_all(&temp_empty);

        let io_sample = TierIoStats {
            min_lat_us: 0.04,
            avg_lat_us: 0.08,
            max_lat_us: 0.15,
            ..TierIoStats::default()
        };
        let lat_str = format_tier_latency(&io_sample, 0.04, 0.08, 0.15, "In-RAM LZ4");
        assert_eq!(lat_str, "0.04..0.08..0.15µs (In-RAM LZ4)");

        let io_disk = TierIoStats {
            min_lat_us: 85.0,
            avg_lat_us: 180.0,
            max_lat_us: 1200.0,
            ..TierIoStats::default()
        };
        let disk_lat_str = format_tier_latency(&io_disk, 85.0, 180.0, 1200.0, "Host VHDX");
        assert_eq!(disk_lat_str, "85..180..1.2ms (Host VHDX)");

        let mem_txt = "MemTotal:       20480 kB\nMemAvailable:   16384 kB\nSwapTotal:       4096 kB\nSwapFree:        2048 kB\n";
        let mem = parse_meminfo(mem_txt);
        assert_eq!(mem.total_kib, 20480);
        assert_eq!(mem.available_kib, 16384);
        assert_eq!(mem.swap_total_kib, 4096);
        assert_eq!(mem.swap_free_kib, 2048);
    }
}
