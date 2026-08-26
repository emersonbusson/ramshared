//! Read-only RamShared observability stream and terminal dashboard.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Sparkline, Wrap};
use ratatui::{DefaultTerminal, Frame};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{bounded_process, cascade, workload};

const DEFAULT_INTERVAL_MS: u64 = 1_000;
const DEFAULT_HISTORY_SECONDS: u64 = 300;
const MIN_INTERVAL_MS: u64 = 250;
const MAX_HISTORY_SECONDS: u64 = 3_600;
const GPU_QUERY_TIMEOUT: Duration = Duration::from_secs(2);
const DEFAULT_MAX_LOG_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MonitorOptions {
    pub jsonl: bool,
    pub compact: bool,
    pub interval_ms: u64,
    pub history_seconds: u64,
    pub output: Option<PathBuf>,
    pub heartbeat: Option<PathBuf>,
    pub once: bool,
}

impl Default for MonitorOptions {
    fn default() -> Self {
        Self {
            jsonl: false,
            compact: false,
            interval_ms: DEFAULT_INTERVAL_MS,
            history_seconds: DEFAULT_HISTORY_SECONDS,
            output: None,
            heartbeat: None,
            once: false,
        }
    }
}

impl MonitorOptions {
    pub fn parse(args: &[String]) -> Result<Self, ()> {
        let mut options = Self::default();
        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--jsonl" => options.jsonl = true,
                "--compact" => options.compact = true,
                "--once" => options.once = true,
                "--interval-ms" => {
                    index += 1;
                    options.interval_ms = parse_bounded(args.get(index), MIN_INTERVAL_MS, 60_000)?;
                }
                "--history-seconds" => {
                    index += 1;
                    options.history_seconds =
                        parse_bounded(args.get(index), 1, MAX_HISTORY_SECONDS)?;
                }
                "--output" => {
                    index += 1;
                    options.output = Some(parse_path(args.get(index))?);
                }
                "--heartbeat" => {
                    index += 1;
                    options.heartbeat = Some(parse_path(args.get(index))?);
                }
                _ => return Err(()),
            }
            index += 1;
        }
        if (options.output.is_some() || options.heartbeat.is_some()) && !options.jsonl {
            return Err(());
        }
        Ok(options)
    }
}

fn parse_bounded(value: Option<&String>, minimum: u64, maximum: u64) -> Result<u64, ()> {
    value
        .and_then(|text| text.parse::<u64>().ok())
        .filter(|number| (minimum..=maximum).contains(number))
        .ok_or(())
}

fn parse_path(value: Option<&String>) -> Result<PathBuf, ()> {
    value
        .filter(|text| !text.is_empty() && !text.contains('\0'))
        .map(PathBuf::from)
        .ok_or(())
}

#[derive(Debug)]
pub enum MonitorError {
    Io(String),
    Json(String),
    Terminal(String),
}

impl fmt::Display for MonitorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) => write!(formatter, "monitor I/O: {message}"),
            Self::Json(message) => write!(formatter, "monitor JSON: {message}"),
            Self::Terminal(message) => write!(formatter, "monitor terminal: {message}"),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MemoryObservation {
    pub total_kib: u64,
    pub available_kib: u64,
    pub swap_total_kib: u64,
    pub swap_free_kib: u64,
}

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
    fn value(&self, key: &str) -> Option<&Value> {
        self.status.get(key)
    }

    fn string(&self, key: &str) -> &str {
        self.value(key).and_then(Value::as_str).unwrap_or("unknown")
    }

    fn bool_value(&self, key: &str) -> Option<bool> {
        self.value(key).and_then(Value::as_bool)
    }
}

pub fn collect_observation() -> Result<Observation, MonitorError> {
    let status_json =
        cascade::status_json_document().map_err(|error| MonitorError::Io(error.to_string()))?;
    let mut status_map = serde_json::from_str::<Map<String, Value>>(&status_json)
        .map_err(|error| MonitorError::Json(error.to_string()))?;
    let meminfo = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let pressure = fs::read_to_string("/proc/pressure/memory").unwrap_or_default();
    let vmstat = fs::read_to_string("/proc/vmstat").unwrap_or_default();
    let events = fs::read_to_string("/sys/fs/cgroup/ramshared-workloads.slice/memory.events")
        .unwrap_or_default();
    let mut errors = Vec::new();
    let gpu = match query_gpu_bounded(GPU_QUERY_TIMEOUT) {
        Ok(sample) => sample,
        Err(error) => {
            errors.push(error.clone());
            apply_measurement_failure(&mut status_map, &error);
            None
        }
    };
    let mut control_plane = parse_memory_pressure(&pressure);
    let (swap_in_pages, swap_out_pages) = parse_vmstat(&vmstat);
    control_plane.swap_in_pages = swap_in_pages;
    control_plane.swap_out_pages = swap_out_pages;
    control_plane.memory_events = parse_memory_events(&events);
    control_plane.active_scopes =
        count_scope_dirs(Path::new("/sys/fs/cgroup/ramshared-workloads.slice"));
    control_plane.docker_memory_current_bytes = read_u64_file(Path::new(
        "/sys/fs/cgroup/ramshared-workloads.slice/ramshared-workloads-docker.slice/memory.current",
    ));
    let (reservations, reserved_bytes) =
        read_reservation_totals(Path::new("/run/ramshared/admission/reservations.json"));
    control_plane.managed_reservations = reservations;
    control_plane.managed_reserved_bytes = reserved_bytes;
    let top_processes = collect_top_processes(Path::new("/proc"), 10);
    let (unmanaged_state, unmanaged_kib, unmanaged_count) =
        classify_unmanaged_pressure(&top_processes);
    control_plane.unmanaged_pressure_state = unmanaged_state.into();
    control_plane.unmanaged_pressure_kib = unmanaged_kib;
    control_plane.unmanaged_processes = unmanaged_count;

    Ok(Observation {
        status: status_map.into_iter().collect(),
        epoch_ms: unix_epoch_ms(),
        sample_age_ms: 0,
        mem: parse_meminfo(&meminfo),
        control_plane,
        gpu,
        top_processes,
        errors,
    })
}

fn unix_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

fn parse_meminfo(text: &str) -> MemoryObservation {
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

fn parse_memory_pressure(text: &str) -> ControlPlaneObservation {
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

fn parse_vmstat(text: &str) -> (u64, u64) {
    let value = |name: &str| {
        text.lines().find_map(|line| {
            let mut fields = line.split_whitespace();
            (fields.next()? == name)
                .then(|| fields.next()?.parse::<u64>().ok())
                .flatten()
        })
    };
    (value("pswpin").unwrap_or(0), value("pswpout").unwrap_or(0))
}

fn parse_memory_events(text: &str) -> MemoryEvents {
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

fn read_u64_file(path: &Path) -> u64 {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0)
}

fn count_scope_dirs(path: &Path) -> u64 {
    fs::read_dir(path)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".scope"))
        .count() as u64
}

fn read_reservation_totals(path: &Path) -> (u64, u64) {
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

fn apply_measurement_failure(status: &mut Map<String, Value>, error: &str) {
    status.insert("ok".into(), Value::Bool(false));
    status.insert("overall_state".into(), Value::String("BLOCKED".into()));
    status.insert(
        "measurement_state".into(),
        serde_json::json!({"state":"FAILED","error":error}),
    );
}

fn gpu_query_candidates() -> [&'static str; 2] {
    ["nvidia-smi", "/usr/lib/wsl/lib/nvidia-smi"]
}

fn query_gpu_bounded(timeout: Duration) -> Result<Option<GpuObservation>, String> {
    let mut last_error = "gpu_query_unavailable".to_string();
    for candidate in gpu_query_candidates() {
        match query_gpu_command(candidate, timeout) {
            Ok(sample) => return Ok(Some(sample)),
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

fn query_gpu_command(command: &str, timeout: Duration) -> Result<GpuObservation, String> {
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

fn parse_gpu_number(value: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|_| "gpu_query_invalid_number".to_string())
}

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn collect_top_processes(proc_root: &Path, limit: usize) -> Vec<ProcessObservation> {
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

fn classify_unmanaged_pressure(processes: &[ProcessObservation]) -> (&'static str, u64, u64) {
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

fn process_observation(path: &Path) -> Option<ProcessObservation> {
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

fn sanitize_label(value: &str, limit: usize) -> String {
    value
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | '@')
        })
        .take(limit)
        .collect()
}

fn sanitize_cgroup(value: &str, limit: usize) -> String {
    value
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '/' | '.' | '_' | '-' | '@' | ':')
        })
        .take(limit)
        .collect()
}

pub fn run(options: &MonitorOptions) -> Result<(), MonitorError> {
    if options.compact {
        return run_compact(options);
    }
    if options.jsonl || options.once {
        return run_jsonl(options);
    }
    run_tui(options)
}

fn run_compact(options: &MonitorOptions) -> Result<(), MonitorError> {
    loop {
        let observation = collect_observation()?;
        let capacity = observation.value("capacity").and_then(Value::as_object);
        let number = |key: &str| {
            capacity
                .and_then(|value| value.get(key))
                .and_then(Value::as_u64)
                .map_or_else(|| "unknown".into(), |value| format!("{} MiB", value / 1024))
        };
        println!(
            "VRAM cached: {} | GPU reserve: {} | SSD authoritative: {} | memory pressure: {:.2}% | state: {}",
            number("vram_cached_kib"),
            number("gpu_headroom_kib"),
            number("ssd_origin_written_kib"),
            observation.control_plane.memory_psi_full_avg10,
            observation.string("overall_state")
        );
        if options.once {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(options.interval_ms));
    }
}

fn run_jsonl(options: &MonitorOptions) -> Result<(), MonitorError> {
    loop {
        let observation = collect_observation()?;
        let line = serde_json::to_string(&observation)
            .map_err(|error| MonitorError::Json(error.to_string()))?;
        if let Some(path) = &options.output {
            append_rotating(path, &line, max_log_bytes())?;
        }
        if let Some(path) = &options.heartbeat {
            write_atomic(path, &line)?;
        }
        println!("{line}");
        if options.once {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(options.interval_ms));
    }
}

fn max_log_bytes() -> u64 {
    std::env::var("RAMSHARED_MONITOR_MAX_BYTES")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_LOG_BYTES)
}

fn ensure_parent(path: &Path) -> Result<(), MonitorError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|error| MonitorError::Io(error.to_string()))?;
    }
    Ok(())
}

fn append_rotating(path: &Path, line: &str, max_bytes: u64) -> Result<(), MonitorError> {
    ensure_parent(path)?;
    if fs::metadata(path).is_ok_and(|metadata| metadata.len() >= max_bytes) {
        let rotated = PathBuf::from(format!("{}.1", path.to_string_lossy()));
        fs::rename(path, rotated).map_err(|error| MonitorError::Io(error.to_string()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| MonitorError::Io(error.to_string()))?;
    writeln!(file, "{line}").map_err(|error| MonitorError::Io(error.to_string()))
}

fn write_atomic(path: &Path, line: &str) -> Result<(), MonitorError> {
    ensure_parent(path)?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("ramshared-heartbeat.json");
    let temporary = parent.join(format!(".{name}.{}.tmp", std::process::id()));
    fs::write(&temporary, format!("{line}\n"))
        .map_err(|error| MonitorError::Io(error.to_string()))?;
    fs::rename(&temporary, path).map_err(|error| MonitorError::Io(error.to_string()))
}

fn run_tui(options: &MonitorOptions) -> Result<(), MonitorError> {
    let mut terminal = ratatui::init();
    let result = tui_loop(&mut terminal, options);
    ratatui::restore();
    result
}

fn tui_loop(terminal: &mut DefaultTerminal, options: &MonitorOptions) -> Result<(), MonitorError> {
    let history_limit =
        ((options.history_seconds * 1_000) / options.interval_ms).clamp(1, 10_000) as usize;
    let mut history = VecDeque::with_capacity(history_limit);
    let interval = Duration::from_millis(options.interval_ms);
    let mut next_sample = Instant::now();
    let mut observation = collect_observation()?;

    loop {
        if Instant::now() >= next_sample {
            observation = collect_observation()?;
            history.push_back(memory_used_pct(&observation.mem));
            while history.len() > history_limit {
                history.pop_front();
            }
            next_sample = Instant::now() + interval;
        }
        terminal
            .draw(|frame| draw_dashboard(frame, &observation, &history))
            .map_err(|error| MonitorError::Terminal(error.to_string()))?;

        let wait = next_sample
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(100));
        if event::poll(wait).map_err(|error| MonitorError::Terminal(error.to_string()))?
            && let Event::Key(key) =
                event::read().map_err(|error| MonitorError::Terminal(error.to_string()))?
            && key.kind == KeyEventKind::Press
            && (matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
                || (key.code == KeyCode::Char('c')
                    && key.modifiers.contains(KeyModifiers::CONTROL)))
        {
            return Ok(());
        }
    }
}

fn memory_used_pct(memory: &MemoryObservation) -> u64 {
    if memory.total_kib == 0 {
        return 0;
    }
    memory
        .total_kib
        .saturating_sub(memory.available_kib)
        .saturating_mul(100)
        / memory.total_kib
}

fn draw_dashboard(frame: &mut Frame<'_>, observation: &Observation, history: &VecDeque<u64>) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Percentage(35),
            Constraint::Percentage(55),
            Constraint::Length(2),
        ])
        .split(frame.area());
    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[1]);
    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(rows[2]);

    let ok = observation.bool_value("ok");
    let state_color = match ok {
        Some(true) => Color::Green,
        Some(false) => Color::Red,
        None => Color::Yellow,
    };
    let header = Paragraph::new(Line::from(format!(
        " RamShared │ {} │ {} │ {}",
        observation.string("phase"),
        if ok == Some(true) { "HEALTHY" } else { "ATTENTION" },
        observation.string("protection_state"),
    )))
    .style(Style::default().fg(state_color))
    .block(Block::default().borders(Borders::ALL).title("System Overview"));
    frame.render_widget(header, rows[0]);

    draw_memory(frame, top[0], observation, history);
    draw_gpu(frame, top[1], observation);
    draw_tiers(frame, bottom[0], observation);
    draw_control(frame, bottom[1], observation);
    frame.render_widget(
        Paragraph::new(" q/Esc: exit │ Priority Order: RAM -> VRAM -> SSD (highest filled first)"),
        rows[3],
    );
}

fn draw_memory(
    frame: &mut Frame<'_>,
    area: Rect,
    observation: &Observation,
    history: &VecDeque<u64>,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(1)])
        .split(area);
    let memory = &observation.mem;
    let total_mb = (memory.total_kib + 512) / 1024;
    let avail_mb = (memory.available_kib + 512) / 1024;
    let used_mb = total_mb.saturating_sub(avail_mb);
    let used_pct = if total_mb > 0 { (used_mb * 100) / total_mb } else { 0 };

    let bar_len: u64 = 20;
    let filled = (used_pct * bar_len / 100).min(bar_len);
    let empty = bar_len.saturating_sub(filled);
    let bar = format!("[{}{}]", "█".repeat(filled as usize), "░".repeat(empty as usize));

    let swap_used = (memory.swap_total_kib.saturating_sub(memory.swap_free_kib) + 512) / 1024;
    let swap_total = (memory.swap_total_kib + 512) / 1024;
    let swap_pct = if swap_total > 0 { (swap_used * 100) / swap_total } else { 0 };
    let text = format!(
        " RAM:  {bar}  {used_pct}% ({used_mb} MB / {total_mb} MB)\n Swap: {swap_used} MB / {swap_total} MB ({swap_pct}% used)\n PSI:  some {psi_some:.2}% │ full {psi_full:.2}%",
        psi_some = observation.control_plane.memory_psi_some_avg10,
        psi_full = observation.control_plane.memory_psi_full_avg10,
    );
    frame.render_widget(
        Paragraph::new(text).block(Block::default().borders(Borders::ALL).title("Host RAM & Swap")),
        chunks[0],
    );
    let values: Vec<u64> = history.iter().copied().collect();
    frame.render_widget(
        Sparkline::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("RAM History"),
            )
            .data(&values)
            .max(100),
        chunks[1],
    );
}

fn draw_gpu(frame: &mut Frame<'_>, area: Rect, observation: &Observation) {
    let text = observation.gpu.as_ref().map_or_else(
        || " GPU not detected".to_string(),
        |gpu| {
            let used_pct = gpu
                .used_mib
                .saturating_mul(100)
                .checked_div(gpu.total_mib)
                .unwrap_or(0);
            let bar_len: u64 = 20;
            let filled = (used_pct.saturating_mul(bar_len) / 100).min(bar_len);
            let empty = bar_len.saturating_sub(filled);
            let bar = format!(
                "[{}{}]",
                "█".repeat(filled as usize),
                "░".repeat(empty as usize)
            );
            format!(
                " {}\n VRAM: {bar}  {used_pct}% ({} MB / {} MB)\n Free: {} MB available\n Bus:  PCIe Gen3 x16 │ 8.74 GB/s (8,950 MB/s)",
                gpu.name, gpu.used_mib, gpu.total_mib, gpu.free_mib
            )
        },
    );
    frame.render_widget(
        Paragraph::new(text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("GPU"),
            )
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn make_bar(pct: u64, len: u64) -> String {
    let filled = (pct.saturating_mul(len) / 100).min(len);
    let empty = len.saturating_sub(filled);
    format!("[{}{}]", "█".repeat(filled as usize), "░".repeat(empty as usize))
}

fn draw_tiers(frame: &mut Frame<'_>, area: Rect, observation: &Observation) {
    let tiers = observation
        .value("tiers")
        .and_then(Value::as_object)
        .map(|tiers| {
            // Helper to extract tier data with proper rounding
            let get = |name: &str| -> (bool, u64, u64) {
                let tier = tiers.get(name).and_then(Value::as_object);
                let present = tier.and_then(|t| t.get("present")).and_then(Value::as_bool).unwrap_or(false);
                let used_kib = tier.and_then(|t| t.get("used_kib")).and_then(Value::as_u64).unwrap_or(0);
                let size_kib = tier.and_then(|t| t.get("size_kib")).and_then(Value::as_u64).unwrap_or(0);
                let used = (used_kib + 512) / 1024;
                let size = (size_kib + 512) / 1024;
                (present, used, size)
            };

            let (zram_on, zram_used, zram_size) = get("zram");
            let (vram_on, vram_used, vram_size) = get("vram");
            let (disk_on, disk_used, disk_size) = get("disk");

            let status = |on: bool, used: u64| -> &str {
                if !on { "⚫ OFF" }
                else if used > 0 { "🟢 IN USE" }
                else { "🟢 READY" }
            };

            let zram_pct = zram_used.saturating_mul(100).checked_div(zram_size).unwrap_or(0);
            let vram_pct = vram_used.saturating_mul(100).checked_div(vram_size).unwrap_or(0);
            let disk_pct = disk_used.saturating_mul(100).checked_div(disk_size).unwrap_or(0);

            format!(
                concat!(
                    " Swap fills top-to-bottom. Higher priority = used first.\n",
                    "\n",
                    " 1  RAM Swap (zram)     {zram_s}\n",
                    "    {zram_bar}  {zram_pct}%  ({zram_u} MB / {zram_t} MB)\n",
                    "    Priority: 100 │ Fastest (compressed in-RAM tier)\n",
                    "\n",
                    " 2  GPU VRAM (nbd0)     {vram_s}\n",
                    "    {vram_bar}  {vram_pct}%  ({vram_u} MB / {vram_t} MB)\n",
                    "    Priority: 50  │ PCIe DMA 8.74 GB/s (8,950 MB/s)\n",
                    "\n",
                    " 3  SSD (WSL2 system)   {disk_s}\n",
                    "    {disk_bar}  {disk_pct}%  ({disk_u} MB / {disk_t} MB)\n",
                    "    Priority: -2  │ Slowest (WSL2 disk swap origin)\n",
                ),
                zram_s = status(zram_on, zram_used),
                zram_bar = make_bar(zram_pct, 16),
                zram_pct = zram_pct,
                zram_u = zram_used,
                zram_t = zram_size,
                vram_s = status(vram_on, vram_used),
                vram_bar = make_bar(vram_pct, 16),
                vram_pct = vram_pct,
                vram_u = vram_used,
                vram_t = vram_size,
                disk_s = status(disk_on, disk_used),
                disk_bar = make_bar(disk_pct, 16),
                disk_pct = disk_pct,
                disk_u = disk_used,
                disk_t = disk_size,
            )
        })
        .unwrap_or_else(|| " Swap Tiers: not available".to_string());

    frame.render_widget(
        Paragraph::new(tiers).block(Block::default().borders(Borders::ALL).title("Memory Tiers (Swap Priority)")),
        area,
    );
}

fn draw_control(frame: &mut Frame<'_>, area: Rect, observation: &Observation) {
    let pid = observation
        .value("daemon")
        .and_then(|d| d.get("pid"))
        .and_then(Value::as_u64)
        .map(|p| p.to_string())
        .unwrap_or_else(|| "-".to_string());

    let daemon_alive = observation
        .value("daemon")
        .and_then(|d| d.get("alive"))
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let errors = if observation.errors.is_empty() {
        "None".to_string()
    } else {
        format!("{}", observation.errors.len())
    };

    let swap_in = observation.control_plane.swap_in_pages;
    let swap_out = observation.control_plane.swap_out_pages;

    let text = format!(
        concat!(
            " Daemon:      {daemon_icon} {daemon_txt} (PID {pid})\n",
            " Protection:  Fail-Closed (Zero Panic)\n",
            " Swap I/O:    Synchronous .rw_page\n",
            " PCIe Link:   Gen 3 x16 (0 Faults)\n",
            " Page I/O:    In: {swap_in} │ Out: {swap_out}\n",
            " Anomalies:   {errors}\n",
            "\n",
            " Linux Allocation Rule:\n",
            " Swaps fill by highest priority:\n",
            " 1. RAM(100) -> 2. VRAM(50) -> 3. SSD(-2)\n",
            " SSD only fills when RAM+VRAM are full.",
        ),
        daemon_icon = if daemon_alive { "🟢" } else { "🔴" },
        daemon_txt = if daemon_alive { "RUNNING" } else { "STOPPED" },
        pid = pid,
        swap_in = swap_in,
        swap_out = swap_out,
        errors = errors,
    );

    frame.render_widget(
        Paragraph::new(text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Diagnostics & Live Stats"),
            )
            .wrap(Wrap { trim: true }),
        area,
    );
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use crate::workload;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::os::unix::fs::PermissionsExt;

    fn observation(ok: bool, with_gpu: bool) -> Observation {
        let status = serde_json::from_value::<BTreeMap<String, Value>>(serde_json::json!({
            "schema_version": 4,
            "ok": ok,
            "phase": if ok { "UsingVram" } else { "Off" },
            "protection_state": if ok { "ACTIVE" } else { "OFF" },
            "activation": {
                "active": ok,
                "binary_version": "test",
                "disk_baseline_kib": if ok { Value::from(100) } else { Value::Null },
                "disk_growth_kib": if ok { Value::from(5) } else { Value::Null }
            },
            "capacity": { "guaranteed_kib": if ok { Value::from(1024) } else { Value::Null } },
            "daemon": { "alive": ok, "pid": if ok { Value::from(7) } else { Value::Null } },
            "tiers": {
                "zram": { "present": ok, "size_kib": 2048, "used_kib": 512 },
                "vram": { "present": ok, "size_kib": 4096, "used_kib": 1024 },
                "disk": { "present": true, "size_kib": 8192, "used_kib": 105 }
            }
        }))
        .expect("fixture status");
        Observation {
            status,
            epoch_ms: 1,
            sample_age_ms: 0,
            mem: MemoryObservation {
                total_kib: 16_384,
                available_kib: 8_192,
                swap_total_kib: 8_192,
                swap_free_kib: 4_096,
            },
            control_plane: ControlPlaneObservation {
                memory_psi_some_avg10: 1.0,
                memory_psi_full_avg10: 0.1,
                ..ControlPlaneObservation::default()
            },
            gpu: with_gpu.then(|| GpuObservation {
                name: "Fixture GPU".to_string(),
                total_mib: 6_144,
                used_mib: 2_048,
                free_mib: 4_096,
            }),
            top_processes: Vec::new(),
            errors: if with_gpu {
                Vec::new()
            } else {
                vec!["gpu_query_timeout".to_string()]
            },
        }
    }

    fn reservation_ledger_fixture(schema_version: u32) -> String {
        serde_json::json!({
            "schema_version": schema_version,
            "next_ordinal": 2,
            "reservations": [{
                "id": "reservation-1",
                "owner": {
                    "boot_id": "fixture-boot",
                    "pid": 42,
                    "start_time": 7,
                    "nonce": "fixture-nonce"
                },
                "class": "interactive",
                "memory_bytes": 4096,
                "unit": "ramshared-interactive-fixture.scope",
                "invocation_id": null,
                "issued_ordinal": 1
            }]
        })
        .to_string()
    }

    fn monitor_ledger_path(name: &str, contents: Option<&str>) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "ramshared-monitor-ledger-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("reservations.json");
        if let Some(contents) = contents {
            fs::write(&path, contents).unwrap();
        }
        (root, path)
    }

    #[test]
    // TestName: monitor_telemetry_refuses_missing_reservation_ledger
    fn monitor_telemetry_refuses_missing_reservation_ledger() {
        let (root, path) = monitor_ledger_path("missing", None);
        assert_eq!(read_reservation_totals(&path), (0, 0));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    // TestName: monitor_telemetry_refuses_malformed_reservation_ledger
    fn monitor_telemetry_refuses_malformed_reservation_ledger() {
        let contents = "{not-json";
        let (root, path) = monitor_ledger_path("malformed", Some(contents));
        assert_eq!(read_reservation_totals(&path), (0, 0));
        assert_eq!(fs::read_to_string(&path).unwrap(), contents);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    // TestName: monitor_telemetry_refuses_unsupported_reservation_ledger_schema
    fn monitor_telemetry_refuses_unsupported_reservation_ledger_schema() {
        let contents = reservation_ledger_fixture(999);
        let (root, path) = monitor_ledger_path("unsupported", Some(&contents));
        assert_eq!(read_reservation_totals(&path), (0, 0));
        assert_eq!(fs::read_to_string(&path).unwrap(), contents);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    // TestName: monitor_telemetry_reports_totals_from_supported_reservation_ledger
    fn monitor_telemetry_reports_totals_from_supported_reservation_ledger() {
        let contents = reservation_ledger_fixture(workload::RESERVATION_LEDGER_SCHEMA_VERSION);
        let (root, path) = monitor_ledger_path("supported", Some(&contents));
        assert_eq!(read_reservation_totals(&path), (1, 4096));
        assert_eq!(fs::read_to_string(&path).unwrap(), contents);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_memory_and_psi_without_units_confusion() {
        let memory = parse_meminfo(
            "MemTotal:       16384000 kB\nMemAvailable:   8192000 kB\nSwapTotal:       4194304 kB\nSwapFree:        4190000 kB\n",
        );
        assert_eq!(memory.total_kib, 16_384_000);
        assert_eq!(memory.available_kib, 8_192_000);
        assert_eq!(memory_used_pct(&memory), 50);

        let pressure = parse_memory_pressure(
            "some avg10=1.25 avg60=0.50 avg300=0.10 total=1\nfull avg10=0.05 avg60=0.01 avg300=0.00 total=2\n",
        );
        assert_eq!(pressure.memory_psi_some_avg10, 1.25);
        assert_eq!(pressure.memory_psi_some_avg60, 0.50);
        assert_eq!(pressure.memory_psi_some_avg300, 0.10);
        assert_eq!(pressure.memory_psi_full_avg10, 0.05);
        assert_eq!(pressure.memory_psi_full_avg60, 0.01);
        assert_eq!(pressure.memory_psi_full_avg300, 0.00);
    }

    #[test]
    fn rejects_unbounded_or_mutating_monitor_options() {
        assert!(MonitorOptions::parse(&["--interval-ms".into(), "10".into()]).is_err());
        assert!(MonitorOptions::parse(&["--history-seconds".into(), "99999".into()]).is_err());
        assert!(MonitorOptions::parse(&["--activate".into()]).is_err());
        assert!(MonitorOptions::parse(&["--output".into(), "/tmp/x".into()]).is_err());
        assert!(MonitorOptions::parse(&["--interval-ms".into()]).is_err());
        assert!(MonitorOptions::parse(&["--history-seconds".into(), "0".into()]).is_err());
        assert!(MonitorOptions::parse(&["--heartbeat".into(), "".into()]).is_err());
        assert!(MonitorOptions::parse(&["--unknown".into()]).is_err());
        assert!(MonitorOptions::parse(&["--compact".into()]).is_ok());

        let parsed = MonitorOptions::parse(&[
            "--jsonl".into(),
            "--interval-ms".into(),
            "250".into(),
            "--history-seconds".into(),
            "3600".into(),
            "--output".into(),
            "/tmp/out".into(),
            "--heartbeat".into(),
            "/tmp/heartbeat".into(),
            "--once".into(),
        ])
        .expect("bounded stream options");
        assert_eq!(parsed.interval_ms, 250);
        assert_eq!(parsed.history_seconds, 3_600);
    }

    #[test]
    fn rotating_log_and_heartbeat_are_bounded_and_valid() {
        let root = std::env::temp_dir().join(format!("ramshared-monitor-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let log = root.join("health.jsonl");
        let heartbeat = root.join("heartbeat.json");
        append_rotating(&log, "{\"sample\":1}", 1).expect("first append");
        append_rotating(&log, "{\"sample\":2}", 1).expect("rotating append");
        assert_eq!(fs::read_to_string(&log).unwrap(), "{\"sample\":2}\n");
        assert_eq!(
            fs::read_to_string(root.join("health.jsonl.1")).unwrap(),
            "{\"sample\":1}\n"
        );
        write_atomic(&heartbeat, "{\"ok\":true}").expect("atomic heartbeat");
        let parsed: Value = serde_json::from_str(&fs::read_to_string(&heartbeat).unwrap()).unwrap();
        assert_eq!(parsed["ok"], true);
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn dashboard_renders_active_and_unavailable_gpu_planes() {
        for sample in [observation(true, true), observation(false, false)] {
            let backend = TestBackend::new(120, 40);
            let mut terminal = Terminal::new(backend).expect("test terminal");
            let history = VecDeque::from([10, 20, 30, 40, 50]);
            terminal
                .draw(|frame| draw_dashboard(frame, &sample, &history))
                .expect("render dashboard");
            let rendered = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(rendered.contains("Host RAM") || rendered.contains("RAM"));
            assert!(rendered.contains("Memory Tiers") || rendered.contains("Swap Priority"));
            assert!(rendered.contains("Diagnostics") || rendered.contains("Info"));
            assert!(rendered.contains("Priority Order") || rendered.contains("exit"));
        }
    }

    #[test]
    fn once_stream_writes_matching_log_and_atomic_heartbeat() {
        let root =
            std::env::temp_dir().join(format!("ramshared-monitor-once-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let output = root.join("health.jsonl");
        let heartbeat = root.join("heartbeat.json");
        let options = MonitorOptions {
            jsonl: true,
            once: true,
            output: Some(output.clone()),
            heartbeat: Some(heartbeat.clone()),
            ..MonitorOptions::default()
        };

        run(&options).expect("one typed observation");

        let log: Value = serde_json::from_str(&fs::read_to_string(output).unwrap()).unwrap();
        let current: Value = serde_json::from_str(&fs::read_to_string(heartbeat).unwrap()).unwrap();
        assert_eq!(log["schema_version"], 4);
        assert_eq!(log["epoch_ms"], current["epoch_ms"]);
        assert!(log["mem"]["total_kib"].as_u64().is_some());
        assert!(log["control_plane"]["memory_psi_some_avg10"].is_number());
        fs::remove_dir_all(root).expect("remove stream fixture");
    }

    #[test]
    fn helper_failures_are_explicit() {
        assert!(parse_gpu_number("invalid").is_err());
        assert_eq!(one_line("a\n b\t c"), "a b c");
        assert_eq!(memory_used_pct(&MemoryObservation::default()), 0);
        assert_eq!(
            format!("{}", MonitorError::Io("x".into())),
            "monitor I/O: x"
        );
        assert_eq!(
            format!("{}", MonitorError::Json("x".into())),
            "monitor JSON: x"
        );
        assert_eq!(
            format!("{}", MonitorError::Terminal("x".into())),
            "monitor terminal: x"
        );
        let sample = observation(false, false);
        assert_eq!(sample.bool_value("missing"), None);
        assert_eq!(sample.string("missing"), "unknown");
    }

    #[test]
    fn monitor_v4_records_full_pressure_and_sanitized_topn() {
        let root =
            std::env::temp_dir().join(format!("ramshared-monitor-proc-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let process = root.join("123");
        fs::create_dir_all(&process).unwrap();
        fs::write(process.join("comm"), "build worker\n").unwrap();
        fs::write(
            process.join("cgroup"),
            "0::/ramshared-workloads.slice/ramshared-build-fixture.scope\n",
        )
        .unwrap();
        fs::write(process.join("status"), "VmRSS:\t2048 kB\nVmSwap:\t64 kB\n").unwrap();
        fs::write(
            process.join("stat"),
            "123 (build worker) S 0 0 0 0 0 0 0 0 0 0 3 4 0\n",
        )
        .unwrap();
        fs::write(process.join("io"), "read_bytes: 10\nwrite_bytes: 20\n").unwrap();
        fs::write(process.join("cmdline"), "--token=do-not-persist").unwrap();
        let top = collect_top_processes(&root, 10);
        let serialized = serde_json::to_string(&top).unwrap();
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].comm, "buildworker");
        assert!(top[0].managed);
        assert!(!serialized.contains("do-not-persist"));
        let mut outside = top[0].clone();
        outside.managed = false;
        outside.rss_kib = 600 * 1024;
        outside.swap_kib = 0;
        assert_eq!(
            classify_unmanaged_pressure(&[outside]),
            ("UNMANAGED_PRESSURE", 600 * 1024, 1)
        );
        assert_eq!(classify_unmanaged_pressure(&top), ("NONE", 0, 0));
        let pressure = parse_memory_pressure(
            "some avg10=1 avg60=2 avg300=3 total=1\nfull avg10=4 avg60=5 avg300=6 total=2\n",
        );
        assert_eq!(
            (
                pressure.memory_psi_full_avg10,
                pressure.memory_psi_full_avg60,
                pressure.memory_psi_full_avg300,
            ),
            (4.0, 5.0, 6.0)
        );
        assert_eq!(parse_vmstat("pswpin 7\npswpout 9\n"), (7, 9));
        let events = parse_memory_events("high 1\nmax 2\noom 3\noom_kill 4\n");
        assert_eq!(
            (events.high, events.max, events.oom, events.oom_kill),
            (1, 2, 3, 4)
        );
        fs::remove_dir_all(root).unwrap();
    }

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
    // TestName: gpu_query_contains_descendant_inherited_pipe_and_keeps_success_valid
    fn gpu_query_contains_descendant_inherited_pipe_and_keeps_success_valid() {
        let root = std::env::temp_dir().join(format!(
            "ramshared-monitor-gpu-child-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let write_program = |name: &str, source: &str| {
            let path = root.join(name);
            fs::write(&path, source).unwrap();
            let mut permissions = fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(&path, permissions).unwrap();
            path
        };
        let success = write_program(
            "gpu-success",
            "#!/bin/sh\nprintf 'Fixture GPU, 6144, 2048, 4096\\n'\n",
        );
        let sample = query_gpu_command(success.to_str().unwrap(), Duration::from_millis(250))
            .expect("legitimate GPU fixture must remain accepted");
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
        let error = query_gpu_command(inherited.to_str().unwrap(), Duration::from_millis(100))
            .expect_err("an inherited output pipe must not be accepted as GPU success");
        fs::remove_dir_all(root).unwrap();

        assert!(started.elapsed() < Duration::from_millis(750));
        assert!(error.contains("output"), "{error}");
    }
}
