//! Read-only RamShared observability stream and terminal dashboard.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Sparkline, Wrap};
use ratatui::{DefaultTerminal, Frame};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::cascade;

const DEFAULT_INTERVAL_MS: u64 = 2_000;
const DEFAULT_HISTORY_SECONDS: u64 = 300;
const MIN_INTERVAL_MS: u64 = 250;
const MAX_HISTORY_SECONDS: u64 = 3_600;
const GPU_QUERY_TIMEOUT: Duration = Duration::from_secs(2);
const DEFAULT_MAX_LOG_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MonitorOptions {
    pub jsonl: bool,
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
    pub memory_psi_full_avg10: f64,
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
    let status_json = cascade::status_json_document();
    let status_map = serde_json::from_str::<Map<String, Value>>(&status_json)
        .map_err(|error| MonitorError::Json(error.to_string()))?;
    let meminfo = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let pressure = fs::read_to_string("/proc/pressure/memory").unwrap_or_default();
    let mut errors = Vec::new();
    let gpu = match query_gpu_bounded(GPU_QUERY_TIMEOUT) {
        Ok(sample) => sample,
        Err(error) => {
            errors.push(error);
            None
        }
    };

    Ok(Observation {
        status: status_map.into_iter().collect(),
        epoch_ms: unix_epoch_ms(),
        sample_age_ms: 0,
        mem: parse_meminfo(&meminfo),
        control_plane: parse_memory_pressure(&pressure),
        gpu,
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
    fn avg10(line: Option<&str>) -> f64 {
        line.and_then(|line| {
            line.split_whitespace().find_map(|field| {
                field
                    .strip_prefix("avg10=")
                    .and_then(|value| value.parse::<f64>().ok())
            })
        })
        .unwrap_or(0.0)
    }
    ControlPlaneObservation {
        memory_psi_some_avg10: avg10(text.lines().find(|line| line.starts_with("some "))),
        memory_psi_full_avg10: avg10(text.lines().find(|line| line.starts_with("full "))),
    }
}

fn query_gpu_bounded(timeout: Duration) -> Result<Option<GpuObservation>, String> {
    let mut child = match Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.total,memory.used,memory.free",
            "--format=csv,noheader,nounits",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("gpu_query_spawn:{error}")),
    };
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("gpu_query_timeout".to_string());
            }
            Err(error) => return Err(format!("gpu_query_wait:{error}")),
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("gpu_query_output:{error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("gpu_query_failed:{}", one_line(&stderr)));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let Some(line) = stdout.lines().next().filter(|line| !line.trim().is_empty()) else {
        return Ok(None);
    };
    let fields: Vec<&str> = line.split(',').map(str::trim).collect();
    if fields.len() != 4 {
        return Err("gpu_query_invalid_field_count".to_string());
    }
    Ok(Some(GpuObservation {
        name: fields[0].to_string(),
        total_mib: parse_gpu_number(fields[1])?,
        used_mib: parse_gpu_number(fields[2])?,
        free_mib: parse_gpu_number(fields[3])?,
    }))
}

fn parse_gpu_number(value: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|_| "gpu_query_invalid_number".to_string())
}

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn run(options: &MonitorOptions) -> Result<(), MonitorError> {
    if options.jsonl || options.once {
        return run_jsonl(options);
    }
    run_tui(options)
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
            Constraint::Percentage(45),
            Constraint::Percentage(45),
            Constraint::Length(2),
        ])
        .split(frame.area());
    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[1]);
    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[2]);

    let ok = observation.bool_value("ok");
    let state_color = match ok {
        Some(true) => Color::Green,
        Some(false) => Color::Red,
        None => Color::Yellow,
    };
    let header = Paragraph::new(Line::from(format!(
        "RamShared {} | phase {} | {} | sample age {} ms",
        observation.string("protection_state"),
        observation.string("phase"),
        if ok == Some(true) {
            "healthy"
        } else {
            "attention"
        },
        observation.sample_age_ms
    )))
    .style(Style::default().fg(state_color))
    .block(Block::default().borders(Borders::ALL).title("Live status"));
    frame.render_widget(header, rows[0]);

    draw_memory(frame, top[0], observation, history);
    draw_gpu(frame, top[1], observation);
    draw_tiers(frame, bottom[0], observation);
    draw_control(frame, bottom[1], observation);
    frame.render_widget(
        Paragraph::new("q / Esc / Ctrl-C: exit | read-only; no pressure or lifecycle controls"),
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
        .constraints([Constraint::Length(4), Constraint::Min(1)])
        .split(area);
    let memory = &observation.mem;
    let text = format!(
        "available: {} / {} MiB\nswap free: {} / {} MiB\nPSI full avg10: {:.2}",
        memory.available_kib / 1024,
        memory.total_kib / 1024,
        memory.swap_free_kib / 1024,
        memory.swap_total_kib / 1024,
        observation.control_plane.memory_psi_full_avg10
    );
    frame.render_widget(
        Paragraph::new(text).block(Block::default().borders(Borders::ALL).title("Memory / PSI")),
        chunks[0],
    );
    let values: Vec<u64> = history.iter().copied().collect();
    frame.render_widget(
        Sparkline::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("RAM used % history"),
            )
            .data(&values)
            .max(100),
        chunks[1],
    );
}

fn draw_gpu(frame: &mut Frame<'_>, area: Rect, observation: &Observation) {
    let text = observation.gpu.as_ref().map_or_else(
        || "GPU telemetry unavailable".to_string(),
        |gpu| {
            format!(
                "{}\ntotal: {} MiB\nused: {} MiB\nfree: {} MiB",
                gpu.name, gpu.total_mib, gpu.used_mib, gpu.free_mib
            )
        },
    );
    frame.render_widget(
        Paragraph::new(text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Physical GPU / VRAM"),
            )
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_tiers(frame: &mut Frame<'_>, area: Rect, observation: &Observation) {
    let tiers = observation
        .value("tiers")
        .and_then(Value::as_object)
        .map(|tiers| {
            ["zram", "vram", "disk"]
                .iter()
                .map(|name| {
                    let tier = tiers.get(*name).and_then(Value::as_object);
                    let present = tier
                        .and_then(|value| value.get("present"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let used = tier
                        .and_then(|value| value.get("used_kib"))
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    let size = tier
                        .and_then(|value| value.get("size_kib"))
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    format!(
                        "{name}: present={present} used={} / {} MiB",
                        used / 1024,
                        size / 1024
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_else(|| "tier telemetry unavailable".to_string());
    frame.render_widget(
        Paragraph::new(tiers).block(Block::default().borders(Borders::ALL).title("Swap tiers")),
        area,
    );
}

fn draw_control(frame: &mut Frame<'_>, area: Rect, observation: &Observation) {
    let activation = observation
        .value("activation")
        .cloned()
        .unwrap_or(Value::Null);
    let capacity = observation
        .value("capacity")
        .cloned()
        .unwrap_or(Value::Null);
    let daemon = observation.value("daemon").cloned().unwrap_or(Value::Null);
    let errors = if observation.errors.is_empty() {
        "none".to_string()
    } else {
        observation.errors.join(", ")
    };
    let text = format!(
        "activation: {activation}\ncapacity: {capacity}\ndaemon: {daemon}\nmeasurement errors: {errors}"
    );
    frame.render_widget(
        Paragraph::new(text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Control plane"),
            )
            .wrap(Wrap { trim: true }),
        area,
    );
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn observation(ok: bool, with_gpu: bool) -> Observation {
        let status = serde_json::from_value::<BTreeMap<String, Value>>(serde_json::json!({
            "schema_version": 3,
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
            },
            gpu: with_gpu.then(|| GpuObservation {
                name: "Fixture GPU".to_string(),
                total_mib: 6_144,
                used_mib: 2_048,
                free_mib: 4_096,
            }),
            errors: if with_gpu {
                Vec::new()
            } else {
                vec!["gpu_query_timeout".to_string()]
            },
        }
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
        assert_eq!(pressure.memory_psi_full_avg10, 0.05);
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
            assert!(rendered.contains("Memory / PSI"));
            assert!(rendered.contains("Swap tiers"));
            assert!(rendered.contains("Control plane"));
            assert!(rendered.contains("read-only"));
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
        assert_eq!(log["schema_version"], 3);
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
}
