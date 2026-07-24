//! ramshared CLI — preflight (check/doctor) and cascade orchestration (up/down).
//! No `unsafe`: the CUDA probe uses the audited `ramshared-cuda` crate (Day-0).
#![forbid(unsafe_code)]

use std::env;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use ramshared_cuda::Cuda;

mod cascade;
mod diagnose;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Status {
    Ok,
    Fail,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Status::Ok => "ok",
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

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let command = args.next();
    let rest = args.collect::<Vec<_>>();
    let json = rest.iter().any(|arg| arg == "--json");

    match command.as_deref() {
        Some("check") => {
            let report = run_check();
            if json {
                println!("{}", render_json(&report));
            } else {
                print_text_report(&report);
            }

            match report.decision() {
                Decision::Ready => ExitCode::SUCCESS,
                Decision::Blocked => ExitCode::from(1),
            }
        }
        Some("doctor") => {
            let report = run_check();
            let recommendations = recommendations_for(&report);
            if json {
                println!("{}", render_doctor_json(&report, &recommendations));
            } else {
                print_text_report(&report);
                print_recommendations(&recommendations);
            }

            match report.decision() {
                Decision::Ready => ExitCode::SUCCESS,
                Decision::Blocked => ExitCode::from(1),
            }
        }
        Some("up") => to_exit(cascade::up()),
        Some("down") => to_exit(cascade::down()),
        Some("status") => to_exit(cascade::status(json)),
        Some("diagnose") => to_exit(diagnose::run(&rest)),
        Some("-h") | Some("--help") | None => {
            print_usage();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("unsupported command: {other}");
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn to_exit<E: fmt::Display>(r: Result<(), E>) -> ExitCode {
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}

fn print_usage() {
    eprintln!("usage:");
    eprintln!("  ramshared check [--json]");
    eprintln!("  ramshared doctor [--json]");
    eprintln!("  ramshared diagnose --events PATH [--json]");
    eprintln!("  ramshared up [--vram MiB] [--zram MiB] [--daemon PATH]");
    eprintln!("      defaults: 1024 MiB each, or RAMSHARED_VRAM_MIB / RAMSHARED_ZRAM_MIB");
    eprintln!("      --zram 0  skip zram (VRAM/NBD only)");
    eprintln!("  ramshared status [--json]   # phase Armed/UsingVram/… + tiers");
    eprintln!("  ramshared down   # always swapoff before stopping the daemon (anti hang)");
    eprintln!();
    eprintln!("boot on WSL2 (opt-in, fail-closed):");
    eprintln!("  sudo bash scripts/safety/install-cascade-boot.sh --enable");
    eprintln!("  sudo bash scripts/safety/uninstall-cascade-boot.sh");
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
        blockers.push("kernel nao parece ser WSL2".to_string());
    }
    if cuda.status == Status::Fail {
        blockers.push(format!("CUDA indisponivel: {}", cuda.detail));
    }
    if backends.nbd_status == Status::Fail && backends.ublk_status == Status::Fail {
        blockers.push("nenhum backend de bloco disponivel sem kernel customizado".to_string());
    }
    if kernel.swap != Some(KernelConfig::BuiltIn) {
        match kernel.swap {
            Some(config) if config.enabled() => {}
            Some(_) => blockers.push("CONFIG_SWAP esta desabilitado".to_string()),
            None => warnings.push("nao foi possivel confirmar CONFIG_SWAP".to_string()),
        }
    }
    if kernel.io_uring != Some(KernelConfig::BuiltIn) {
        match kernel.io_uring {
            Some(config) if config.enabled() => {}
            Some(_) => warnings.push("CONFIG_IO_URING esta desabilitado".to_string()),
            None => warnings.push("nao foi possivel confirmar CONFIG_IO_URING".to_string()),
        }
    }
    if backends.nbd_detail.contains("module-not-loaded") {
        warnings.push(
            "CONFIG_BLK_DEV_NBD existe, mas /dev/nbd* nao esta presente; start podera exigir modprobe nbd"
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
    } else {
        Status::Fail
    };
    let ublk_detail = if !ublk_enabled {
        "CONFIG_BLK_DEV_UBLK disabled or unknown".to_string()
    } else if !env.ublk_control_present {
        "/dev/ublk-control missing".to_string()
    } else if !env.ublk_control_openable {
        "/dev/ublk-control not openable; run check as root".to_string()
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
            detail: "/dev/dxg ausente".to_string(),
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
            detail: "libcuda.so nao encontrada".to_string(),
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
            detail: "nvidia-smi reportou GPU bloqueada pelo sistema operacional".to_string(),
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
        return Err("CUDA nao encontrou devices".to_string());
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

fn print_text_report(report: &CheckReport) {
    print_basic_info(report);
    print_swap_info(report);
    print_backend_info(report);
    print_decision(report);
    print_details(report);
    print_issues(report);
}

fn print_basic_info(report: &CheckReport) {
    println!(
        "WSL2: {} ({})",
        report.wsl.status.as_str(),
        report.wsl.release
    );
    println!(
        "CUDA: {} ({})",
        report.cuda.status.as_str(),
        report.cuda.detail
    );

    match &report.cuda.gpu {
        Some(gpu) => println!(
            "GPU: {}, total={}MiB, livre={}MiB",
            gpu.name,
            bytes_to_mib(gpu.total_bytes),
            bytes_to_mib(gpu.free_bytes)
        ),
        None => println!("GPU: unavailable"),
    }
}

fn print_swap_info(report: &CheckReport) {
    match report.swaps.first() {
        Some(swap) => println!(
            "Swap atual: {}, size={}MiB, used={}MiB, prio={}",
            swap.filename,
            kib_to_mib(swap.size_kib),
            kib_to_mib(swap.used_kib),
            swap.priority
        ),
        None => println!("Swap atual: none"),
    }
}

fn print_backend_info(report: &CheckReport) {
    println!(
        "Backends: nbd={}, ublk={}",
        report.backends.nbd_status.as_str(),
        report.backends.ublk_status.as_str()
    );
    println!(
        "Tiers (cascata): zram={}, vram=nbd({}), vhdx={}",
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
    );
}

fn print_decision(report: &CheckReport) {
    println!("Decisao: {}", report.decision().as_str());
}

fn print_details(report: &CheckReport) {
    println!("Detalhes:");
    println!(
        "  config: {}",
        report.kernel.config_source.as_deref().unwrap_or("unknown")
    );
    println!("  CONFIG_SWAP: {}", config_text(report.kernel.swap));
    println!("  CONFIG_IO_URING: {}", config_text(report.kernel.io_uring));
    println!(
        "  kernel.io_uring_disabled: {}",
        io_uring_runtime_text(report.kernel.io_uring_runtime)
    );
    println!("  CONFIG_BLK_DEV_NBD: {}", config_text(report.kernel.nbd));
    println!("  CONFIG_BLK_DEV_UBLK: {}", config_text(report.kernel.ublk));
    println!("  CONFIG_ZRAM: {}", config_text(report.kernel.zram));
    println!("  nbd: {}", report.backends.nbd_detail);
    println!("  ublk: {}", report.backends.ublk_detail);
    println!(
        "  /dev/dxg: {}",
        if report.cuda.dxg_present {
            "present"
        } else {
            "missing"
        }
    );
    println!(
        "  libcuda: {}",
        report.cuda.libcuda_path.as_deref().unwrap_or("missing")
    );
    println!(
        "  nvidia-smi: {}",
        report.cuda.nvidia_smi_path.as_deref().unwrap_or("missing")
    );
    if let Some(code) = report.cuda.nvidia_smi_status {
        println!("  nvidia-smi exit: {code}");
    }
    if let Some(output) = &report.cuda.nvidia_smi_output
        && !output.is_empty()
    {
        println!("  nvidia-smi output: {}", one_line(output));
    }
}

fn print_issues(report: &CheckReport) {
    if !report.blockers.is_empty() {
        println!("Bloqueios:");
        for blocker in &report.blockers {
            println!("  - {blocker}");
        }
    }

    if !report.warnings.is_empty() {
        println!("Avisos:");
        for warning in &report.warnings {
            println!("  - {warning}");
        }
    }
}

fn recommendations_for(report: &CheckReport) -> Vec<String> {
    let mut recommendations = Vec::new();

    if report.wsl.status == Status::Fail {
        recommendations.push(
            "Execute isto apenas em uma distro WSL2; este projeto nao deve rodar em Linux bare-metal neste modo"
                .to_string(),
        );
    }

    if !report.cuda.dxg_present {
        recommendations.push(
            "No Windows, atualize WSL com `wsl --update`; depois execute `wsl --shutdown` quando puder interromper a distro"
                .to_string(),
        );
        recommendations.push(
            "Atualize o driver NVIDIA no Windows; nao instale driver NVIDIA Linux dentro do WSL"
                .to_string(),
        );
        recommendations.push(
            "Reabra a distro e confirme que `/dev/dxg` existe antes de tentar qualquer teste de VRAM"
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
            "A GPU esta bloqueada pelo host; feche apps que possam monopolizar a GPU, atualize Windows/driver NVIDIA e reinicie o WSL manualmente"
                .to_string(),
        );
    }

    if report.cuda.libcuda_path.is_none() {
        recommendations.push(
            "Instale apenas o CUDA Toolkit compativel com WSL se precisar compilar; evite pacotes `cuda`, `cuda-12-x` ou `cuda-drivers` dentro do WSL"
                .to_string(),
        );
    }

    if report.backends.nbd_detail.contains("module-not-loaded") {
        recommendations.push(
            "Para a fase de start futura, o backend MVP deve usar `nbd`; carregar o modulo com `modprobe nbd` deve ser uma acao manual e separada"
                .to_string(),
        );
    }

    if report.backends.ublk_status == Status::Fail {
        recommendations.push(
            "`ublk` esta indisponivel neste kernel; ignore por enquanto e mantenha o MVP em `nbd`"
                .to_string(),
        );
    }

    if report.decision() == Decision::Ready {
        recommendations.push(
            "Ambiente pronto para o proximo passo seguro: implementar e rodar um CUDA smoke test sem swap e com alocacao pequena"
                .to_string(),
        );
    } else {
        recommendations.push(
            "Nao execute `ramshared start`, `swapon`, testes de pressao de memoria ou auto-start ate `ramshared check` retornar `ready`"
                .to_string(),
        );
    }

    recommendations
}

fn print_recommendations(recommendations: &[String]) {
    println!("Recomendacoes:");
    for recommendation in recommendations {
        println!("  - {recommendation}");
    }
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
    use super::*;

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

        assert_eq!(backends.ublk_status, Status::Fail);
        assert_eq!(
            backends.ublk_detail,
            "/dev/ublk-control not openable; run check as root"
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
        let report = CheckReport {
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
                detail: "/dev/dxg ausente".to_string(),
            },
            backends: BackendProbe {
                nbd_status: Status::Ok,
                nbd_detail: "module-not-loaded".to_string(),
                ublk_status: Status::Fail,
                ublk_detail: "CONFIG_BLK_DEV_UBLK disabled or unknown".to_string(),
            },
            blockers: vec!["CUDA indisponivel: /dev/dxg ausente".to_string()],
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
                .any(|item| item.contains("nao instale driver NVIDIA Linux"))
        );
        assert!(
            recommendations
                .iter()
                .any(|item| item.contains("Nao execute `ramshared start`"))
        );
    }
}
