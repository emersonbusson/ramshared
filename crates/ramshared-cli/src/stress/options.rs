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
    pub io_depth: u64,
    pub queue_depth: u64,
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
            io_depth: 1,
            queue_depth: 1,
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
    pub battery_mode: bool,
    pub cascade_mode: bool,
    pub max_safe_pct: u64,
    pub total_allocated_mb: u64,
    pub peak_swap_mb: u64,
    pub tier1_zram_mb: u64,
    pub tier1_zram_pct: u64,
    pub tier2_vram_mb: u64,
    pub tier2_vram_pct: u64,
    pub tier3_ssd_mb: u64,
    pub tier3_ssd_pct: u64,
    #[serde(default)]
    pub tier1_throughput_mbs: f64,
    #[serde(default)]
    pub tier2_throughput_mbs: f64,
    #[serde(default)]
    pub tier3_throughput_mbs: f64,
    #[serde(default)]
    pub tier2_speedup_vs_ssd: f64,
    pub peak_pressure_index: f64,
    pub telemetry_readings_count: usize,
    pub active_io_cycles_completed: usize,
    pub reclaim_duration_ms: f64,
    pub reclaim_speed_gbs: f64,
    pub post_reclaim_free_ram_mb: u64,
    pub status: String,
}

pub fn parse_stress_args(args: &[String]) -> Result<StressOptions, String> {
    let mut opts = StressOptions::default();
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
                if opts.tier3_target_pct.is_none() {
                    opts.tier3_target_pct = Some(30);
                }
            }
            "--tier3-target-pct" => {
                i += 1;
                let val: u64 = args
                    .get(i)
                    .ok_or_else(|| "--tier3-target-pct requires a value (1-99)".to_string())?
                    .parse()
                    .map_err(|_| "invalid --tier3-target-pct value")?;
                opts.tier3_target_pct = Some(val.clamp(1, 99));
                opts.cascade = true;
                opts.battery = true;
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
            "--io-depth" => {
                i += 1;
                opts.io_depth = args
                    .get(i)
                    .ok_or_else(|| "--io-depth requires a value".to_string())?
                    .parse()
                    .map_err(|_| "invalid --io-depth value")?;
            }
            "--queue-depth" => {
                i += 1;
                opts.queue_depth = args
                    .get(i)
                    .ok_or_else(|| "--queue-depth requires a value".to_string())?
                    .parse()
                    .map_err(|_| "invalid --queue-depth value")?;
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
#[cfg(test)]
mod tests {
    use super::*;
    use crate::stress::*;

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
        assert!(run(&opts_cascade).is_ok());

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
            "--io-depth".to_string(),
            "4".to_string(),
            "--queue-depth".to_string(),
            "32".to_string(),
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
        assert_eq!(parsed.io_depth, 4);
        assert_eq!(parsed.queue_depth, 32);

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
            battery_mode: true,
            cascade_mode: true,
            max_safe_pct: 90,
            total_allocated_mb: 1000,
            peak_swap_mb: 500,
            tier1_zram_mb: 200,
            tier1_zram_pct: 50,
            tier2_vram_mb: 200,
            tier2_vram_pct: 50,
            tier3_ssd_mb: 100,
            tier3_ssd_pct: 25,
            tier1_throughput_mbs: 1000.0,
            tier2_throughput_mbs: 500.0,
            tier3_throughput_mbs: 20.0,
            tier2_speedup_vs_ssd: 25.0,
            peak_pressure_index: 10.0,
            telemetry_readings_count: 5,
            active_io_cycles_completed: 2,
            reclaim_duration_ms: 1.0,
            reclaim_speed_gbs: 1000.0,
            post_reclaim_free_ram_mb: 8000,
            status: "PASS_ZERO_PANIC".to_string(),
        };
        archive_and_compare_benchmark(&report, false);
        archive_and_compare_benchmark(&report, true);
    }
}
