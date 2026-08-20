//! ramshared CLI — preflight (check/doctor) and cascade orchestration (up/down).
//! No `unsafe`: the CUDA probe uses the audited `ramshared-cuda` crate (Day-0).
#![forbid(unsafe_code)]

use std::env;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use ramshared_cuda::Cuda;

mod cascade;
mod diagnose;
mod monitor;
mod workload;

use monitor::MonitorOptions;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Status {
    Ok,
    RequiresPrivilege,
    Fail,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::RequiresPrivilege => "requires-root",
            Status::Fail => "fail",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Decision {
    Ready,
    Blocked,
}

impl Decision {
    fn as_str(self) -> &'static str {
        match self {
            Decision::Ready => "ready",
            Decision::Blocked => "blocked",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KernelConfig {
    BuiltIn,
    Module,
    Disabled,
}

impl KernelConfig {
    fn enabled(self) -> bool {
        matches!(self, KernelConfig::BuiltIn | KernelConfig::Module)
    }

    fn as_str(self) -> &'static str {
        match self {
            KernelConfig::BuiltIn => "y",
            KernelConfig::Module => "m",
            KernelConfig::Disabled => "n",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IoUringRuntime {
    Enabled,
    Restricted,
    Disabled,
}

impl IoUringRuntime {
    fn as_sysctl_value(self) -> u8 {
        match self {
            IoUringRuntime::Enabled => 0,
            IoUringRuntime::Restricted => 1,
            IoUringRuntime::Disabled => 2,
        }
    }

    fn enabled_for_ublk(self) -> bool {
        self == IoUringRuntime::Enabled
    }
}

#[derive(Debug)]
struct WslProbe {
    status: Status,
    release: String,
    version: String,
}

#[derive(Debug)]
struct SwapEntry {
    filename: String,
    kind: String,
    size_kib: u64,
    used_kib: u64,
    priority: i32,
}

#[derive(Debug)]
struct KernelFeatures {
    config_source: Option<String>,
    swap: Option<KernelConfig>,
    io_uring: Option<KernelConfig>,
    io_uring_runtime: Option<IoUringRuntime>,
    nbd: Option<KernelConfig>,
    ublk: Option<KernelConfig>,
    zram: Option<KernelConfig>,
}

#[derive(Debug)]
struct BackendProbe {
    nbd_status: Status,
    nbd_detail: String,
    ublk_status: Status,
    ublk_detail: String,
}

#[derive(Clone, Copy, Debug)]
struct BackendEnv {
    nbd_device_present: bool,
    nbd_module_loaded: bool,
    ublk_control_present: bool,
    ublk_control_openable: bool,
}

#[derive(Debug)]
struct CudaProbe {
    status: Status,
    libcuda_path: Option<String>,
    dxg_present: bool,
    nvidia_smi_path: Option<String>,
    nvidia_smi_status: Option<i32>,
    nvidia_smi_output: Option<String>,
    gpu: Option<GpuInfo>,
    detail: String,
}

#[derive(Debug)]
struct GpuInfo {
    name: String,
    total_bytes: u64,
    free_bytes: u64,
}

#[derive(Debug)]
struct CheckReport {
    wsl: WslProbe,
    swaps: Vec<SwapEntry>,
    kernel: KernelFeatures,
    cuda: CudaProbe,
    backends: BackendProbe,
    blockers: Vec<String>,
    warnings: Vec<String>,
}

impl CheckReport {
    fn decision(&self) -> Decision {
        if self.blockers.is_empty() {
            Decision::Ready
        } else {
            Decision::Blocked
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CliCommand {
    Version,
    Run { args: Vec<String> },
    Check { json: bool },
    Doctor { json: bool },
    Up { args: Vec<String> },
    Down,
    Status { json: bool },
    Monitor { options: MonitorOptions },
    Diagnose { args: Vec<String> },
    Help,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CliParseError {
    UnsupportedCommand(String),
    InvalidOption {
        command: &'static str,
        options: Vec<String>,
    },
}

impl fmt::Display for CliParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliParseError::UnsupportedCommand(command) => {
                write!(f, "unsupported command: {command}")
            }
            CliParseError::InvalidOption { command, options } => {
                write!(f, "invalid {command} option: {}", options.join(" "))
            }
        }
    }
}

fn parse_json_option(command: &'static str, options: &[String]) -> Result<bool, CliParseError> {
    match options {
        [] => Ok(false),
        [option] if option == "--json" => Ok(true),
        _ => Err(CliParseError::InvalidOption {
            command,
            options: options.to_vec(),
        }),
    }
}

fn parse_cli_command(args: &[String]) -> Result<CliCommand, CliParseError> {
    let Some((command, options)) = args.split_first() else {
        return Ok(CliCommand::Help);
    };

    match command.as_str() {
        "version" | "-V" | "--version" => {
            if options.is_empty() {
                Ok(CliCommand::Version)
            } else {
                Err(CliParseError::InvalidOption {
                    command: "version",
                    options: options.to_vec(),
                })
            }
        }
        "check" => Ok(CliCommand::Check {
            json: parse_json_option("check", options)?,
        }),
        "run" => Ok(CliCommand::Run {
            args: options.to_vec(),
        }),
        "doctor" => Ok(CliCommand::Doctor {
            json: parse_json_option("doctor", options)?,
        }),
        "up" => Ok(CliCommand::Up {
            args: options.to_vec(),
        }),
        "down" => {
            if options.is_empty() {
                Ok(CliCommand::Down)
            } else {
                Err(CliParseError::InvalidOption {
                    command: "down",
                    options: options.to_vec(),
                })
            }
        }
        "status" => Ok(CliCommand::Status {
            json: parse_json_option("status", options)?,
        }),
        "monitor" => Ok(CliCommand::Monitor {
            options: MonitorOptions::parse(options).map_err(|_| CliParseError::InvalidOption {
                command: "monitor",
                options: options.to_vec(),
            })?,
        }),
        "diagnose" => Ok(CliCommand::Diagnose {
            args: options.to_vec(),
        }),
        "-h" | "--help" => Ok(CliCommand::Help),
        other => Err(CliParseError::UnsupportedCommand(other.to_string())),
    }
}

trait CliActionRunner {
    fn check(&mut self, json: bool, stdout: &mut dyn Write, stderr: &mut dyn Write) -> ExitCode;
    fn doctor(&mut self, json: bool, stdout: &mut dyn Write, stderr: &mut dyn Write) -> ExitCode;
    fn up(&mut self, args: &[String], stdout: &mut dyn Write, stderr: &mut dyn Write) -> ExitCode;
    fn down(&mut self, stdout: &mut dyn Write, stderr: &mut dyn Write) -> ExitCode;
    fn status(&mut self, json: bool, stdout: &mut dyn Write, stderr: &mut dyn Write) -> ExitCode;
    fn monitor(
        &mut self,
        options: &MonitorOptions,
        stdout: &mut dyn Write,
        stderr: &mut dyn Write,
    ) -> ExitCode;
    fn diagnose(
        &mut self,
        args: &[String],
        stdout: &mut dyn Write,
        stderr: &mut dyn Write,
    ) -> ExitCode;
    fn run_workload(
        &mut self,
        args: &[String],
        stdout: &mut dyn Write,
        stderr: &mut dyn Write,
    ) -> ExitCode;
}

struct SystemCliActions;

impl CliActionRunner for SystemCliActions {
    fn check(&mut self, json: bool, stdout: &mut dyn Write, stderr: &mut dyn Write) -> ExitCode {
        let report = run_check();
        let output = if json {
            writeln!(stdout, "{}", render_json(&report))
        } else {
            print_text_report(&report, stdout)
        };
        if let Err(error) = output {
            let _ = writeln!(stderr, "failed to write check report: {error}");
            return ExitCode::from(1);
        }
        match report.decision() {
            Decision::Ready => ExitCode::SUCCESS,
            Decision::Blocked => ExitCode::from(1),
        }
    }

    fn doctor(&mut self, json: bool, stdout: &mut dyn Write, stderr: &mut dyn Write) -> ExitCode {
        let report = run_check();
        let recommendations = recommendations_for(&report);
        let output = if json {
            writeln!(stdout, "{}", render_doctor_json(&report, &recommendations))
        } else {
            print_text_report(&report, stdout)
                .and_then(|()| print_recommendations(&recommendations, stdout))
        };
        if let Err(error) = output {
            let _ = writeln!(stderr, "failed to write doctor report: {error}");
            return ExitCode::from(1);
        }
        match report.decision() {
            Decision::Ready => ExitCode::SUCCESS,
            Decision::Blocked => ExitCode::from(1),
        }
    }

    fn up(&mut self, args: &[String], _stdout: &mut dyn Write, stderr: &mut dyn Write) -> ExitCode {
        to_exit(cascade::up_with_args(args), stderr)
    }

    fn down(&mut self, _stdout: &mut dyn Write, stderr: &mut dyn Write) -> ExitCode {
        to_exit(cascade::down(), stderr)
    }

    fn status(&mut self, json: bool, _stdout: &mut dyn Write, stderr: &mut dyn Write) -> ExitCode {
        to_exit(cascade::status(json), stderr)
    }

    fn monitor(
        &mut self,
        options: &MonitorOptions,
        _stdout: &mut dyn Write,
        stderr: &mut dyn Write,
    ) -> ExitCode {
        to_exit(monitor::run(options), stderr)
    }

    fn diagnose(
        &mut self,
        args: &[String],
        _stdout: &mut dyn Write,
        stderr: &mut dyn Write,
    ) -> ExitCode {
        to_exit(diagnose::run(args), stderr)
    }

    fn run_workload(
        &mut self,
        args: &[String],
        _stdout: &mut dyn Write,
        stderr: &mut dyn Write,
    ) -> ExitCode {
        to_exit(workload::run(args), stderr)
    }
}

fn main() -> ExitCode {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let mut actions = SystemCliActions;
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let mut stdout = stdout.lock();
    let mut stderr = stderr.lock();
    run_from_args(&args, &mut actions, &mut stdout, &mut stderr)
}

fn run_from_args<R: CliActionRunner>(
    args: &[String],
    actions: &mut R,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> ExitCode {
    match parse_cli_command(args) {
        Ok(CliCommand::Version) => {
            let _ = writeln!(stdout, "ramshared {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(CliCommand::Run { args }) => actions.run_workload(&args, stdout, stderr),
        Ok(CliCommand::Check { json }) => actions.check(json, stdout, stderr),
        Ok(CliCommand::Doctor { json }) => actions.doctor(json, stdout, stderr),
        Ok(CliCommand::Up { args }) => actions.up(&args, stdout, stderr),
        Ok(CliCommand::Down) => actions.down(stdout, stderr),
        Ok(CliCommand::Status { json }) => actions.status(json, stdout, stderr),
        Ok(CliCommand::Monitor { options }) => actions.monitor(&options, stdout, stderr),
        Ok(CliCommand::Diagnose { args }) => actions.diagnose(&args, stdout, stderr),
        Ok(CliCommand::Help) => {
            print_usage(stderr);
            ExitCode::SUCCESS
        }
        Err(error) => {
            let _ = writeln!(stderr, "{error}");
            print_usage(stderr);
            ExitCode::from(2)
        }
    }
}

fn to_exit<E: fmt::Display>(r: Result<(), E>, stderr: &mut dyn Write) -> ExitCode {
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            let _ = writeln!(stderr, "{e}");
            ExitCode::from(1)
        }
    }
}

fn print_usage(stderr: &mut dyn Write) {
    let _ = writeln!(stderr, "usage:");
    let _ = writeln!(stderr, "  ramshared --version");
    let _ = writeln!(
        stderr,
        "  ramshared run --profile safe -- <command> [args...]"
    );
    let _ = writeln!(stderr, "  ramshared check [--json]");
    let _ = writeln!(stderr, "  ramshared doctor [--json]");
    let _ = writeln!(stderr, "  ramshared diagnose --events PATH [--json]");
    let _ = writeln!(
        stderr,
        "  ramshared up [--vram MiB] [--zram MiB] [--daemon PATH]"
    );
    let _ = writeln!(
        stderr,
        "      defaults: 1024 MiB each, or RAMSHARED_VRAM_MIB / RAMSHARED_ZRAM_MIB"
    );
    let _ = writeln!(stderr, "      --zram 0  skip zram (VRAM/NBD only)");
    let _ = writeln!(
        stderr,
        "  ramshared status [--json]   # phase Armed/UsingVram/… + tiers"
    );
    let _ = writeln!(
        stderr,
        "  ramshared monitor [--jsonl] [--once] [--interval-ms N] [--history-seconds N]"
    );
    let _ = writeln!(
        stderr,
        "  ramshared down   # always swapoff before stopping the daemon (anti hang)"
    );
    let _ = writeln!(stderr);
    let _ = writeln!(stderr, "boot on WSL2 (opt-in, fail-closed):");
    let _ = writeln!(
        stderr,
        "  sudo bash scripts/safety/install-cascade-boot.sh --enable"
    );
    let _ = writeln!(
        stderr,
        "  sudo bash scripts/safety/uninstall-cascade-boot.sh"
    );
}

fn run_check() -> CheckReport {
    let wsl = probe_wsl();
    let swaps = parse_swaps(&read_to_string("/proc/swaps").unwrap_or_default());
    let kernel = probe_kernel_features(&wsl.release);
    let cuda = probe_cuda();
    let backends = probe_backends(&kernel);

    let mut blockers = Vec::new();
    let mut warnings = Vec::new();

    if wsl.status == Status::Fail {
        blockers.push("kernel does not appear to be WSL2".to_string());
    }
    if cuda.status == Status::Fail {
        blockers.push(format!("CUDA unavailable: {}", cuda.detail));
    }
    if backends.nbd_status != Status::Ok && backends.ublk_status != Status::Ok {
        blockers.push("no block backend is available without a custom kernel".to_string());
    }
    if kernel.swap != Some(KernelConfig::BuiltIn) {
        match kernel.swap {
            Some(config) if config.enabled() => {}
            Some(_) => blockers.push("CONFIG_SWAP is disabled".to_string()),
            None => warnings.push("could not confirm CONFIG_SWAP".to_string()),
        }
    }
    if kernel.io_uring != Some(KernelConfig::BuiltIn) {
        match kernel.io_uring {
            Some(config) if config.enabled() => {}
            Some(_) => warnings.push("CONFIG_IO_URING is disabled".to_string()),
            None => warnings.push("could not confirm CONFIG_IO_URING".to_string()),
        }
    }
    if backends.nbd_detail.contains("module-not-loaded") {
        warnings.push(
            "CONFIG_BLK_DEV_NBD exists, but /dev/nbd* is not present; start may require modprobe nbd"
                .to_string(),
        );
    }

    CheckReport {
        wsl,
        swaps,
        kernel,
        cuda,
        backends,
        blockers,
        warnings,
    }
}

fn probe_wsl() -> WslProbe {
    let release = read_to_string("/proc/sys/kernel/osrelease")
        .or_else(|| command_stdout("uname", &["-r"]))
        .unwrap_or_default()
        .trim()
        .to_string();
    let version = read_to_string("/proc/version").unwrap_or_default();
    let combined = format!("{} {}", release, version).to_lowercase();
    let is_wsl2 = combined.contains("microsoft-standard-wsl2")
        || combined.contains("wsl2")
        || (combined.contains("microsoft") && combined.contains("wsl"));

    WslProbe {
        status: if is_wsl2 { Status::Ok } else { Status::Fail },
        release,
        version: version.trim().to_string(),
    }
}

fn probe_kernel_features(release: &str) -> KernelFeatures {
    let (config_source, config_text) = read_kernel_config(release);

    KernelFeatures {
        config_source,
        swap: config_text
            .as_deref()
            .and_then(|text| parse_kernel_config(text, "CONFIG_SWAP")),
        io_uring: config_text
            .as_deref()
            .and_then(|text| parse_kernel_config(text, "CONFIG_IO_URING")),
        io_uring_runtime: read_to_string("/proc/sys/kernel/io_uring_disabled")
            .as_deref()
            .and_then(parse_io_uring_runtime),
        nbd: config_text
            .as_deref()
            .and_then(|text| parse_kernel_config(text, "CONFIG_BLK_DEV_NBD")),
        ublk: config_text
            .as_deref()
            .and_then(|text| parse_kernel_config(text, "CONFIG_BLK_DEV_UBLK")),
        zram: config_text
            .as_deref()
            .and_then(|text| parse_kernel_config(text, "CONFIG_ZRAM")),
    }
}

fn read_kernel_config(release: &str) -> (Option<String>, Option<String>) {
    let boot_config = format!("/boot/config-{release}");
    if let Some(text) = read_to_string(&boot_config) {
        return (Some(boot_config), Some(text));
    }

    let proc_config = Path::new("/proc/config.gz");
    if proc_config.exists() {
        match Command::new("zcat").arg("--").arg(proc_config).output() {
            Ok(output) if output.status.success() => {
                let text = String::from_utf8_lossy(&output.stdout).into_owned();
                return (Some("/proc/config.gz".to_string()), Some(text));
            }
            _ => return (Some("/proc/config.gz".to_string()), None),
        }
    }

    (None, None)
}

fn parse_kernel_config(text: &str, name: &str) -> Option<KernelConfig> {
    let built_in = format!("{name}=y");
    let module = format!("{name}=m");
    let disabled = format!("# {name} is not set");

    for line in text.lines() {
        let line = line.trim();
        if line == built_in {
            return Some(KernelConfig::BuiltIn);
        }
        if line == module {
            return Some(KernelConfig::Module);
        }
        if line == disabled {
            return Some(KernelConfig::Disabled);
        }
    }

    None
}

fn parse_io_uring_runtime(text: &str) -> Option<IoUringRuntime> {
    match text.trim() {
        "0" => Some(IoUringRuntime::Enabled),
        "1" => Some(IoUringRuntime::Restricted),
        "2" => Some(IoUringRuntime::Disabled),
        _ => None,
    }
}

fn parse_swaps(text: &str) -> Vec<SwapEntry> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let filename = fields.next()?.to_string();
            let kind = fields.next()?.to_string();
            let size_kib = fields.next()?.parse().ok()?;
            let used_kib = fields.next()?.parse().ok()?;
            let priority = fields.next()?.parse().ok()?;

            Some(SwapEntry {
                filename,
                kind,
                size_kib,
                used_kib,
                priority,
            })
        })
        .collect()
}

fn probe_backends(kernel: &KernelFeatures) -> BackendProbe {
    let (ublk_control_present, ublk_control_openable) =
        probe_ublk_control(Path::new("/dev/ublk-control"));

    probe_backends_with_env(
        kernel,
        BackendEnv {
            nbd_device_present: has_dev_prefix("nbd"),
            nbd_module_loaded: Path::new("/sys/module/nbd").exists(),
            ublk_control_present,
            ublk_control_openable,
        },
    )
}

fn probe_backends_with_env(kernel: &KernelFeatures, env: BackendEnv) -> BackendProbe {
    let nbd_enabled = kernel.nbd.is_some_and(KernelConfig::enabled);
    let nbd_status = if nbd_enabled {
        Status::Ok
    } else {
        Status::Fail
    };
    let nbd_detail = if env.nbd_device_present {
        "device-present".to_string()
    } else if env.nbd_module_loaded {
        "module-loaded-no-device".to_string()
    } else if nbd_enabled {
        "module-not-loaded".to_string()
    } else {
        "CONFIG_BLK_DEV_NBD disabled or unknown".to_string()
    };

    let ublk_enabled = kernel.ublk.is_some_and(KernelConfig::enabled);
    let io_uring_enabled = kernel.io_uring.is_some_and(KernelConfig::enabled);
    let io_uring_runtime_enabled = kernel
        .io_uring_runtime
        .is_some_and(IoUringRuntime::enabled_for_ublk);
    let ublk_status = if ublk_enabled
        && env.ublk_control_present
        && env.ublk_control_openable
        && io_uring_enabled
        && io_uring_runtime_enabled
    {
        Status::Ok
    } else if ublk_enabled
        && env.ublk_control_present
        && !env.ublk_control_openable
        && io_uring_enabled
        && io_uring_runtime_enabled
    {
        Status::RequiresPrivilege
    } else {
        Status::Fail
    };
    let ublk_detail = if !ublk_enabled {
        "CONFIG_BLK_DEV_UBLK disabled or unknown".to_string()
    } else if !env.ublk_control_present {
        "/dev/ublk-control missing".to_string()
    } else if !env.ublk_control_openable {
        "/dev/ublk-control present; elevated capability probe required".to_string()
    } else if !io_uring_enabled {
        "CONFIG_IO_URING disabled or unknown".to_string()
    } else if !io_uring_runtime_enabled {
        io_uring_runtime_detail(kernel.io_uring_runtime)
    } else {
        "ready".to_string()
    };

    BackendProbe {
        nbd_status,
        nbd_detail,
        ublk_status,
        ublk_detail,
    }
}

fn io_uring_runtime_detail(value: Option<IoUringRuntime>) -> String {
    value.map_or_else(
        || "kernel.io_uring_disabled unknown".to_string(),
        |value| format!("kernel.io_uring_disabled={}", value.as_sysctl_value()),
    )
}

fn probe_ublk_control(path: &Path) -> (bool, bool) {
    if !path.exists() {
        return (false, false);
    }

    let openable = OpenOptions::new().read(true).write(true).open(path).is_ok();
    (true, openable)
}

fn has_dev_prefix(prefix: &str) -> bool {
    fs::read_dir("/dev").is_ok_and(|entries| {
        entries.filter_map(Result::ok).any(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(prefix))
        })
    })
}

fn probe_cuda() -> CudaProbe {
    let dxg_present = Path::new("/dev/dxg").exists();
    let libcuda_path = find_libcuda();
    let (nvidia_smi_path, nvidia_smi_status, nvidia_smi_output) = run_nvidia_smi();

    if !dxg_present {
        return CudaProbe {
            status: Status::Fail,
            libcuda_path: libcuda_path.map(path_to_string),
            dxg_present,
            nvidia_smi_path: nvidia_smi_path.map(path_to_string),
            nvidia_smi_status,
            nvidia_smi_output,
            gpu: None,
            detail: "/dev/dxg is absent".to_string(),
        };
    }

    let Some(libcuda_path) = libcuda_path else {
        return CudaProbe {
            status: Status::Fail,
            libcuda_path: None,
            dxg_present,
            nvidia_smi_path: nvidia_smi_path.map(path_to_string),
            nvidia_smi_status,
            nvidia_smi_output,
            gpu: None,
            detail: "libcuda.so not found".to_string(),
        };
    };

    if nvidia_smi_output
        .as_deref()
        .is_some_and(|out| out.contains("GPU access blocked by the operating system"))
    {
        return CudaProbe {
            status: Status::Fail,
            libcuda_path: Some(path_to_string(libcuda_path)),
            dxg_present,
            nvidia_smi_path: nvidia_smi_path.map(path_to_string),
            nvidia_smi_status,
            nvidia_smi_output,
            gpu: None,
            detail: "nvidia-smi reported the GPU is blocked by the operating system".to_string(),
        };
    }

    match cuda_probe_via_lib() {
        Ok(gpu) => CudaProbe {
            status: Status::Ok,
            libcuda_path: Some(path_to_string(libcuda_path)),
            dxg_present,
            nvidia_smi_path: nvidia_smi_path.map(path_to_string),
            nvidia_smi_status,
            nvidia_smi_output,
            gpu: Some(gpu),
            detail: "ready".to_string(),
        },
        Err(detail) => CudaProbe {
            status: Status::Fail,
            libcuda_path: Some(path_to_string(libcuda_path)),
            dxg_present,
            nvidia_smi_path: nvidia_smi_path.map(path_to_string),
            nvidia_smi_status,
            nvidia_smi_output,
            gpu: None,
            detail,
        },
    }
}

/// Informative path of libcuda (best-effort, filesystem only). The actual verification
/// of CUDA usage is done by `ramshared_cuda::Cuda::load()` (audited FFI, isolated).
fn find_libcuda() -> Option<PathBuf> {
    [
        "/usr/lib/wsl/lib/libcuda.so.1",
        "/usr/lib/wsl/lib/libcuda.so",
        "/usr/lib/x86_64-linux-gnu/libcuda.so.1",
        "/usr/lib/x86_64-linux-gnu/libcuda.so",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|p| p.exists())
}

fn run_nvidia_smi() -> (Option<PathBuf>, Option<i32>, Option<String>) {
    let candidates = [
        PathBuf::from("/usr/lib/wsl/lib/nvidia-smi"),
        PathBuf::from("nvidia-smi"),
    ];

    for candidate in candidates {
        if candidate.is_absolute() && !candidate.exists() {
            continue;
        }

        if let Ok(output) = Command::new(&candidate).output() {
            let mut combined = String::new();
            combined.push_str(&String::from_utf8_lossy(&output.stdout));
            combined.push_str(&String::from_utf8_lossy(&output.stderr));

            return (
                Some(candidate),
                output.status.code(),
                Some(combined.trim().to_string()),
            );
        }
    }

    (None, None, None)
}

/// Real probe of CUDA via the audited `ramshared-cuda` crate (isolated FFI, RAII).
/// Replaces the duplicated FFI that lived here (Day-0: only `unsafe` in ramshared-cuda).
fn cuda_probe_via_lib() -> Result<GpuInfo, String> {
    let cuda = Cuda::load().map_err(|e| e.to_string())?;
    if cuda.device_count().map_err(|e| e.to_string())? < 1 {
        return Err("CUDA found no devices".to_string());
    }
    let dev = cuda.device(0).map_err(|e| e.to_string())?;
    let ctx = cuda.create_context(&dev).map_err(|e| e.to_string())?;
    let (free, total) = ctx.mem_info().map_err(|e| e.to_string())?;
    Ok(GpuInfo {
        name: dev.name().to_string(),
        total_bytes: total as u64,
        free_bytes: free as u64,
    })
}

fn print_text_report<W: Write + ?Sized>(
    report: &CheckReport,
    output: &mut W,
) -> std::io::Result<()> {
    print_basic_info(report, output)?;
    print_swap_info(report, output)?;
    print_backend_info(report, output)?;
    print_decision(report, output)?;
    print_details(report, output)?;
    print_issues(report, output)
}

fn print_basic_info<W: Write + ?Sized>(
    report: &CheckReport,
    output: &mut W,
) -> std::io::Result<()> {
    writeln!(
        output,
        "WSL2: {} ({})",
        report.wsl.status.as_str(),
        report.wsl.release
    )?;
    writeln!(
        output,
        "CUDA: {} ({})",
        report.cuda.status.as_str(),
        report.cuda.detail
    )?;

    match &report.cuda.gpu {
        Some(gpu) => writeln!(
            output,
            "GPU: {}, total={}MiB, free={}MiB",
            gpu.name,
            bytes_to_mib(gpu.total_bytes),
            bytes_to_mib(gpu.free_bytes)
        ),
        None => writeln!(output, "GPU: unavailable"),
    }
}

fn print_swap_info<W: Write + ?Sized>(report: &CheckReport, output: &mut W) -> std::io::Result<()> {
    match report.swaps.first() {
        Some(swap) => writeln!(
            output,
            "Current swap: {}, size={}MiB, used={}MiB, prio={}",
            swap.filename,
            kib_to_mib(swap.size_kib),
            kib_to_mib(swap.used_kib),
            swap.priority
        ),
        None => writeln!(output, "Current swap: none"),
    }
}

fn print_backend_info<W: Write + ?Sized>(
    report: &CheckReport,
    output: &mut W,
) -> std::io::Result<()> {
    writeln!(
        output,
        "Backends: nbd={}, ublk={}",
        report.backends.nbd_status.as_str(),
        report.backends.ublk_status.as_str()
    )?;
    writeln!(
        output,
        "Tiers (cascade): zram={}, vram=nbd({}), vhdx={}",
        if report.kernel.zram.is_some_and(KernelConfig::enabled) {
            "ok"
        } else {
            "fail"
        },
        report.backends.nbd_status.as_str(),
        report
            .swaps
            .first()
            .map(|s| s.filename.as_str())
            .unwrap_or("none")
    )
}

fn print_decision<W: Write + ?Sized>(report: &CheckReport, output: &mut W) -> std::io::Result<()> {
    writeln!(output, "Decision: {}", report.decision().as_str())
}

fn print_details<W: Write + ?Sized>(report: &CheckReport, output: &mut W) -> std::io::Result<()> {
    writeln!(output, "Details:")?;
    writeln!(
        output,
        "  config: {}",
        report.kernel.config_source.as_deref().unwrap_or("unknown")
    )?;
    writeln!(output, "  CONFIG_SWAP: {}", config_text(report.kernel.swap))?;
    writeln!(
        output,
        "  CONFIG_IO_URING: {}",
        config_text(report.kernel.io_uring)
    )?;
    writeln!(
        output,
        "  kernel.io_uring_disabled: {}",
        io_uring_runtime_text(report.kernel.io_uring_runtime)
    )?;
    writeln!(
        output,
        "  CONFIG_BLK_DEV_NBD: {}",
        config_text(report.kernel.nbd)
    )?;
    writeln!(
        output,
        "  CONFIG_BLK_DEV_UBLK: {}",
        config_text(report.kernel.ublk)
    )?;
    writeln!(output, "  CONFIG_ZRAM: {}", config_text(report.kernel.zram))?;
    writeln!(output, "  nbd: {}", report.backends.nbd_detail)?;
    writeln!(output, "  ublk: {}", report.backends.ublk_detail)?;
    writeln!(
        output,
        "  /dev/dxg: {}",
        if report.cuda.dxg_present {
            "present"
        } else {
            "missing"
        }
    )?;
    writeln!(
        output,
        "  libcuda: {}",
        report.cuda.libcuda_path.as_deref().unwrap_or("missing")
    )?;
    writeln!(
        output,
        "  nvidia-smi: {}",
        report.cuda.nvidia_smi_path.as_deref().unwrap_or("missing")
    )?;
    if let Some(code) = report.cuda.nvidia_smi_status {
        writeln!(output, "  nvidia-smi exit: {code}")?;
    }
    if let Some(nvidia_smi_output) = &report.cuda.nvidia_smi_output
        && !nvidia_smi_output.is_empty()
    {
        writeln!(
            output,
            "  nvidia-smi output: {}",
            one_line(nvidia_smi_output)
        )?;
    }
    Ok(())
}

fn print_issues<W: Write + ?Sized>(report: &CheckReport, output: &mut W) -> std::io::Result<()> {
    if !report.blockers.is_empty() {
        writeln!(output, "Blockers:")?;
        for blocker in &report.blockers {
            writeln!(output, "  - {blocker}")?;
        }
    }

    if !report.warnings.is_empty() {
        writeln!(output, "Warnings:")?;
        for warning in &report.warnings {
            writeln!(output, "  - {warning}")?;
        }
    }
    Ok(())
}

fn recommendations_for(report: &CheckReport) -> Vec<String> {
    let mut recommendations = Vec::new();

    if report.wsl.status == Status::Fail {
        recommendations.push(
            "Run this only in a WSL2 distro; this project must not run on bare-metal Linux in this mode"
                .to_string(),
        );
    }

    if !report.cuda.dxg_present {
        recommendations.push(
            "On Windows, update WSL with `wsl --update`; then run `wsl --shutdown` when you can interrupt the distro"
                .to_string(),
        );
        recommendations.push(
            "Update the NVIDIA driver on Windows; do not install the NVIDIA Linux driver inside WSL"
                .to_string(),
        );
        recommendations.push(
            "Reopen the distro and confirm that `/dev/dxg` exists before trying any VRAM test"
                .to_string(),
        );
    }

    if report
        .cuda
        .nvidia_smi_output
        .as_deref()
        .is_some_and(|output| output.contains("GPU access blocked by the operating system"))
    {
        recommendations.push(
            "The GPU is blocked by the host; close apps that may monopolize it, update Windows/the NVIDIA driver, and restart WSL manually"
                .to_string(),
        );
    }

    if report.cuda.libcuda_path.is_none() {
        recommendations.push(
            "Install only the WSL-compatible CUDA Toolkit if you need to compile; avoid `cuda`, `cuda-12-x`, or `cuda-drivers` packages inside WSL"
                .to_string(),
        );
    }

    if report.backends.nbd_detail.contains("module-not-loaded") {
        recommendations.push(
            "For a future start phase, the MVP backend must use `nbd`; loading the module with `modprobe nbd` must be a separate manual action"
                .to_string(),
        );
    }

    if report.backends.ublk_status == Status::Fail {
        recommendations.push(
            "`ublk` is unavailable in this kernel; ignore it for now and keep the MVP on `nbd`"
                .to_string(),
        );
    }

    if report.decision() == Decision::Ready {
        recommendations.push(
            "Environment ready for the bounded NBD preflight; keep pressure and boot activation disabled until status reports a guaranteed READY profile"
                .to_string(),
        );
    } else {
        recommendations.push(
            "Do not run `ramshared start`, `swapon`, memory-pressure tests, or auto-start until `ramshared check` returns `ready`"
                .to_string(),
        );
    }

    recommendations
}

fn print_recommendations<W: Write + ?Sized>(
    recommendations: &[String],
    output: &mut W,
) -> std::io::Result<()> {
    writeln!(output, "Recommendations:")?;
    for recommendation in recommendations {
        writeln!(output, "  - {recommendation}")?;
    }
    Ok(())
}

fn render_doctor_json(report: &CheckReport, recommendations: &[String]) -> String {
    format!(
        "{{\"check\":{},\"recommendations\":[{}]}}",
        render_json(report),
        json_array(recommendations)
    )
}

fn render_json(report: &CheckReport) -> String {
    let swaps = report
        .swaps
        .iter()
        .map(|swap| {
            format!(
                "{{\"filename\":\"{}\",\"type\":\"{}\",\"size_kib\":{},\"used_kib\":{},\"priority\":{}}}",
                json_escape(&swap.filename),
                json_escape(&swap.kind),
                swap.size_kib,
                swap.used_kib,
                swap.priority
            )
        })
        .collect::<Vec<_>>()
        .join(",");

    let gpu = match &report.cuda.gpu {
        Some(gpu) => format!(
            "{{\"name\":\"{}\",\"total_bytes\":{},\"free_bytes\":{}}}",
            json_escape(&gpu.name),
            gpu.total_bytes,
            gpu.free_bytes
        ),
        None => "null".to_string(),
    };

    format!(
        concat!(
            "{{",
            "\"wsl2\":{{\"status\":\"{}\",\"release\":\"{}\",\"version\":\"{}\"}},",
            "\"cuda\":{{\"status\":\"{}\",\"detail\":\"{}\",\"dxg_present\":{},",
            "\"libcuda_path\":{},\"nvidia_smi_path\":{},",
            "\"nvidia_smi_status\":{},\"nvidia_smi_output\":{},\"gpu\":{}}},",
            "\"swap\":[{}],",
            "\"kernel\":{{\"config_source\":{},\"CONFIG_SWAP\":{},",
            "\"CONFIG_IO_URING\":{},\"io_uring_disabled\":{},\"CONFIG_BLK_DEV_NBD\":{},",
            "\"CONFIG_BLK_DEV_UBLK\":{},\"CONFIG_ZRAM\":{}}},",
            "\"backends\":{{\"nbd\":\"{}\",\"nbd_detail\":\"{}\",",
            "\"ublk\":\"{}\",\"ublk_detail\":\"{}\"}},",
            "\"decision\":\"{}\",",
            "\"blockers\":[{}],",
            "\"warnings\":[{}]",
            "}}"
        ),
        report.wsl.status.as_str(),
        json_escape(&report.wsl.release),
        json_escape(&report.wsl.version),
        report.cuda.status.as_str(),
        json_escape(&report.cuda.detail),
        report.cuda.dxg_present,
        json_opt(report.cuda.libcuda_path.as_deref()),
        json_opt(report.cuda.nvidia_smi_path.as_deref()),
        report
            .cuda
            .nvidia_smi_status
            .map_or_else(|| "null".to_string(), |code| code.to_string()),
        json_opt(report.cuda.nvidia_smi_output.as_deref()),
        gpu,
        swaps,
        json_opt(report.kernel.config_source.as_deref()),
        json_config(report.kernel.swap),
        json_config(report.kernel.io_uring),
        json_io_uring_runtime(report.kernel.io_uring_runtime),
        json_config(report.kernel.nbd),
        json_config(report.kernel.ublk),
        json_config(report.kernel.zram),
        report.backends.nbd_status.as_str(),
        json_escape(&report.backends.nbd_detail),
        report.backends.ublk_status.as_str(),
        json_escape(&report.backends.ublk_detail),
        report.decision().as_str(),
        json_array(&report.blockers),
        json_array(&report.warnings)
    )
}

fn json_array(items: &[String]) -> String {
    items
        .iter()
        .map(|item| format!("\"{}\"", json_escape(item)))
        .collect::<Vec<_>>()
        .join(",")
}

fn json_opt(value: Option<&str>) -> String {
    value
        .map(|value| format!("\"{}\"", json_escape(value)))
        .unwrap_or_else(|| "null".to_string())
}

fn json_config(value: Option<KernelConfig>) -> String {
    value
        .map(|value| format!("\"{}\"", value.as_str()))
        .unwrap_or_else(|| "null".to_string())
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch if ch.is_control() => escaped.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => escaped.push(ch),
        }
    }
    escaped
}

fn read_to_string(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path).ok()
}

fn command_stdout(command: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(command).args(args).output().ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        None
    }
}

fn path_to_string(path: PathBuf) -> String {
    path.to_string_lossy().into_owned()
}

fn bytes_to_mib(value: u64) -> u64 {
    value / 1024 / 1024
}

fn kib_to_mib(value: u64) -> u64 {
    value / 1024
}

fn config_text(value: Option<KernelConfig>) -> &'static str {
    value.map_or("unknown", KernelConfig::as_str)
}

fn io_uring_runtime_text(value: Option<IoUringRuntime>) -> String {
    value.map_or_else(
        || "unknown".to_string(),
        |value| value.as_sysctl_value().to_string(),
    )
}

fn json_io_uring_runtime(value: Option<IoUringRuntime>) -> String {
    value.map_or_else(
        || "null".to_string(),
        |value| value.as_sysctl_value().to_string(),
    )
}

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl fmt::Display for KernelConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;

    #[derive(Default)]
    struct RecordingCliActions {
        calls: Vec<CliCommand>,
        exit: u8,
    }

    impl RecordingCliActions {
        fn with_exit(exit: u8) -> Self {
            Self {
                calls: Vec::new(),
                exit,
            }
        }

        fn result(&self) -> ExitCode {
            ExitCode::from(self.exit)
        }
    }

    impl CliActionRunner for RecordingCliActions {
        fn check(
            &mut self,
            json: bool,
            _stdout: &mut dyn std::io::Write,
            _stderr: &mut dyn std::io::Write,
        ) -> ExitCode {
            self.calls.push(CliCommand::Check { json });
            self.result()
        }

        fn doctor(
            &mut self,
            json: bool,
            _stdout: &mut dyn std::io::Write,
            _stderr: &mut dyn std::io::Write,
        ) -> ExitCode {
            self.calls.push(CliCommand::Doctor { json });
            self.result()
        }

        fn up(
            &mut self,
            args: &[String],
            _stdout: &mut dyn std::io::Write,
            _stderr: &mut dyn std::io::Write,
        ) -> ExitCode {
            self.calls.push(CliCommand::Up {
                args: args.to_vec(),
            });
            self.result()
        }

        fn down(
            &mut self,
            _stdout: &mut dyn std::io::Write,
            _stderr: &mut dyn std::io::Write,
        ) -> ExitCode {
            self.calls.push(CliCommand::Down);
            self.result()
        }

        fn status(
            &mut self,
            json: bool,
            _stdout: &mut dyn std::io::Write,
            _stderr: &mut dyn std::io::Write,
        ) -> ExitCode {
            self.calls.push(CliCommand::Status { json });
            self.result()
        }

        fn monitor(
            &mut self,
            options: &MonitorOptions,
            _stdout: &mut dyn std::io::Write,
            _stderr: &mut dyn std::io::Write,
        ) -> ExitCode {
            self.calls.push(CliCommand::Monitor {
                options: options.clone(),
            });
            self.result()
        }

        fn diagnose(
            &mut self,
            args: &[String],
            _stdout: &mut dyn std::io::Write,
            _stderr: &mut dyn std::io::Write,
        ) -> ExitCode {
            self.calls.push(CliCommand::Diagnose {
                args: args.to_vec(),
            });
            self.result()
        }

        fn run_workload(
            &mut self,
            args: &[String],
            _stdout: &mut dyn std::io::Write,
            _stderr: &mut dyn std::io::Write,
        ) -> ExitCode {
            self.calls.push(CliCommand::Run {
                args: args.to_vec(),
            });
            self.result()
        }
    }

    fn cli_args(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).to_string()).collect()
    }

    #[test]
    fn dispatch_refuses_invalid_flags_before_action() {
        for args in [
            cli_args(&["check", "--verbose"]),
            cli_args(&["doctor", "--json", "--json"]),
            cli_args(&["status", "--unsafe"]),
            cli_args(&["status", "--json", "--json"]),
            cli_args(&["down", "--force"]),
        ] {
            let mut actions = RecordingCliActions::default();
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let exit = run_from_args(&args, &mut actions, &mut stdout, &mut stderr);

            assert_eq!(exit, ExitCode::from(2));
            assert!(
                actions.calls.is_empty(),
                "{args:?} must not reach an action"
            );
            assert!(stdout.is_empty());
            assert!(
                String::from_utf8_lossy(&stderr).contains("invalid"),
                "{args:?} must explain the refusal"
            );
        }
    }

    #[test]
    fn version_is_public_and_side_effect_free() {
        let mut actions = RecordingCliActions::default();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit = run_from_args(
            &cli_args(&["--version"]),
            &mut actions,
            &mut stdout,
            &mut stderr,
        );
        assert_eq!(exit, ExitCode::SUCCESS);
        assert!(actions.calls.is_empty());
        assert_eq!(
            String::from_utf8(stdout).expect("version output is UTF-8"),
            format!("ramshared {}\n", env!("CARGO_PKG_VERSION"))
        );
        assert!(stderr.is_empty());
    }

    #[test]
    fn monitor_defaults_are_read_only_and_bounded() {
        let command = parse_cli_command(&cli_args(&["monitor"])).expect("monitor parses");
        assert_eq!(
            command,
            CliCommand::Monitor {
                options: MonitorOptions {
                    jsonl: false,
                    interval_ms: 2_000,
                    history_seconds: 300,
                    output: None,
                    heartbeat: None,
                    once: false,
                }
            }
        );
    }

    #[test]
    fn monitor_parses_machine_stream_outputs_without_mutation_flags() {
        let command = parse_cli_command(&cli_args(&[
            "monitor",
            "--jsonl",
            "--interval-ms",
            "2500",
            "--history-seconds",
            "600",
            "--output",
            "/tmp/health.jsonl",
            "--heartbeat",
            "/tmp/heartbeat.json",
            "--once",
        ]))
        .expect("monitor stream parses");
        assert_eq!(
            command,
            CliCommand::Monitor {
                options: MonitorOptions {
                    jsonl: true,
                    interval_ms: 2_500,
                    history_seconds: 600,
                    output: Some(PathBuf::from("/tmp/health.jsonl")),
                    heartbeat: Some(PathBuf::from("/tmp/heartbeat.json")),
                    once: true,
                }
            }
        );

        let error = parse_cli_command(&cli_args(&["monitor", "--activate"]))
            .expect_err("monitor has no mutation controls");
        assert!(matches!(error, CliParseError::InvalidOption { .. }));
    }

    #[test]
    fn dispatch_forwards_exact_status_and_diagnose_args() {
        let mut actions = RecordingCliActions::with_exit(1);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        assert_eq!(
            run_from_args(
                &cli_args(&["status", "--json"]),
                &mut actions,
                &mut stdout,
                &mut stderr,
            ),
            ExitCode::from(1)
        );
        assert_eq!(
            run_from_args(
                &cli_args(&["diagnose", "--events", "sample.jsonl", "--json"]),
                &mut actions,
                &mut stdout,
                &mut stderr,
            ),
            ExitCode::from(1)
        );
        assert_eq!(
            run_from_args(
                &cli_args(&["up", "--zram", "invalid"]),
                &mut actions,
                &mut stdout,
                &mut stderr,
            ),
            ExitCode::from(1)
        );

        assert_eq!(
            actions.calls,
            vec![
                CliCommand::Status { json: true },
                CliCommand::Diagnose {
                    args: cli_args(&["--events", "sample.jsonl", "--json"]),
                },
                CliCommand::Up {
                    args: cli_args(&["--zram", "invalid"]),
                },
            ]
        );
    }

    #[test]
    fn parses_proc_swaps() {
        let text = "\
Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n\
/dev/sdc                                partition\t8388608\t5643764\t-2\n";

        let swaps = parse_swaps(text);

        assert_eq!(swaps.len(), 1);
        assert_eq!(swaps[0].filename, "/dev/sdc");
        assert_eq!(swaps[0].kind, "partition");
        assert_eq!(swaps[0].size_kib, 8_388_608);
        assert_eq!(swaps[0].used_kib, 5_643_764);
        assert_eq!(swaps[0].priority, -2);
    }

    #[test]
    fn parses_kernel_config_values() {
        let text = "\
CONFIG_SWAP=y\n\
CONFIG_BLK_DEV_NBD=m\n\
# CONFIG_BLK_DEV_UBLK is not set\n";

        assert_eq!(
            parse_kernel_config(text, "CONFIG_SWAP"),
            Some(KernelConfig::BuiltIn)
        );
        assert_eq!(
            parse_kernel_config(text, "CONFIG_BLK_DEV_NBD"),
            Some(KernelConfig::Module)
        );
        assert_eq!(
            parse_kernel_config(text, "CONFIG_BLK_DEV_UBLK"),
            Some(KernelConfig::Disabled)
        );
        assert_eq!(parse_kernel_config(text, "CONFIG_IO_URING"), None);
    }

    #[test]
    fn parses_io_uring_disabled_runtime_values() {
        assert_eq!(parse_io_uring_runtime("0\n"), Some(IoUringRuntime::Enabled));
        assert_eq!(
            parse_io_uring_runtime("1\n"),
            Some(IoUringRuntime::Restricted)
        );
        assert_eq!(
            parse_io_uring_runtime("2\n"),
            Some(IoUringRuntime::Disabled)
        );
        assert_eq!(parse_io_uring_runtime("bad\n"), None);
    }

    #[test]
    fn ublk_backend_requires_runtime_io_uring_enabled() {
        let kernel = KernelFeatures {
            config_source: Some("/proc/config.gz".to_string()),
            swap: Some(KernelConfig::BuiltIn),
            io_uring: Some(KernelConfig::BuiltIn),
            io_uring_runtime: Some(IoUringRuntime::Disabled),
            nbd: Some(KernelConfig::Module),
            ublk: Some(KernelConfig::Module),
            zram: Some(KernelConfig::Module),
        };

        let backends = probe_backends_with_env(
            &kernel,
            BackendEnv {
                nbd_device_present: false,
                nbd_module_loaded: false,
                ublk_control_present: true,
                ublk_control_openable: true,
            },
        );

        assert_eq!(backends.ublk_status, Status::Fail);
        assert_eq!(backends.ublk_detail, "kernel.io_uring_disabled=2");
    }

    #[test]
    fn ublk_backend_requires_openable_control_node() {
        let kernel = KernelFeatures {
            config_source: Some("/proc/config.gz".to_string()),
            swap: Some(KernelConfig::BuiltIn),
            io_uring: Some(KernelConfig::BuiltIn),
            io_uring_runtime: Some(IoUringRuntime::Enabled),
            nbd: Some(KernelConfig::Module),
            ublk: Some(KernelConfig::Module),
            zram: Some(KernelConfig::Module),
        };

        let backends = probe_backends_with_env(
            &kernel,
            BackendEnv {
                nbd_device_present: false,
                nbd_module_loaded: false,
                ublk_control_present: true,
                ublk_control_openable: false,
            },
        );

        assert_eq!(backends.ublk_status, Status::RequiresPrivilege);
        assert_eq!(
            backends.ublk_detail,
            "/dev/ublk-control present; elevated capability probe required"
        );
    }

    #[test]
    fn escapes_json_strings() {
        assert_eq!(
            json_escape("a \"quoted\" path\\name\n"),
            "a \\\"quoted\\\" path\\\\name\\n"
        );
    }

    #[test]
    fn recommends_wsl_gpu_recovery_when_dxg_is_missing() {
        let mut report = CheckReport {
            wsl: WslProbe {
                status: Status::Ok,
                release: "6.6.87.2-microsoft-standard-WSL2".to_string(),
                version: "Linux version test".to_string(),
            },
            swaps: Vec::new(),
            kernel: KernelFeatures {
                config_source: Some("/proc/config.gz".to_string()),
                swap: Some(KernelConfig::BuiltIn),
                io_uring: Some(KernelConfig::BuiltIn),
                io_uring_runtime: Some(IoUringRuntime::Enabled),
                nbd: Some(KernelConfig::Module),
                ublk: Some(KernelConfig::Disabled),
                zram: Some(KernelConfig::Module),
            },
            cuda: CudaProbe {
                status: Status::Fail,
                libcuda_path: Some("/usr/lib/wsl/lib/libcuda.so.1".to_string()),
                dxg_present: false,
                nvidia_smi_path: Some("/usr/lib/wsl/lib/nvidia-smi".to_string()),
                nvidia_smi_status: Some(255),
                nvidia_smi_output: Some(
                    "Failed to initialize NVML: GPU access blocked by the operating system"
                        .to_string(),
                ),
                gpu: None,
                detail: "/dev/dxg is absent".to_string(),
            },
            backends: BackendProbe {
                nbd_status: Status::Ok,
                nbd_detail: "module-not-loaded".to_string(),
                ublk_status: Status::Fail,
                ublk_detail: "CONFIG_BLK_DEV_UBLK disabled or unknown".to_string(),
            },
            blockers: vec!["CUDA unavailable: /dev/dxg is absent".to_string()],
            warnings: Vec::new(),
        };

        let recommendations = recommendations_for(&report);

        assert!(
            recommendations
                .iter()
                .any(|item| item.contains("wsl --update"))
        );
        assert!(
            recommendations
                .iter()
                .any(|item| item.contains("do not install the NVIDIA Linux driver inside WSL"))
        );
        assert!(
            recommendations
                .iter()
                .any(|item| item.contains("Do not run `ramshared start`"))
        );

        report.blockers.clear();
        let ready = recommendations_for(&report);
        assert!(ready.iter().all(|item| !item.contains("implement and run")));
        assert!(
            ready
                .iter()
                .any(|item| item.contains("bounded NBD preflight"))
        );
    }
}
