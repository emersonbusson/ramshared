//! ramshared-winsvc — Windows product entry for StorPort CUDA VRAM disk.
//!
//! SPEC DT-1: `probe-cuda`, `console --storage-only`, SCM default, `install|uninstall`.
//! Lab Start/Stop PS1 paths are not product entrypoints (see Install-RamSharedLabService.ps1).

#[cfg(not(windows))]
use ramshared_winsvc::WinDriveConfig;
#[cfg(not(windows))]
use ramshared_winsvc::runtime::{ProductCommand, RuntimeErrorClass, parse_product_cli};

#[cfg(windows)]
mod windows_svc {
    use std::ffi::OsString;
    use std::sync::mpsc;
    use std::time::Duration;

    use windows_service::define_windows_service;
    use windows_service::service::{
        ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode,
        ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
    use windows_service::service_dispatcher;
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

    use ramshared_winsvc::config::WinDriveConfig;
    use ramshared_winsvc::runtime::{ProductCommand, RunMode, RuntimeState, parse_product_cli};

    pub const SERVICE_NAME: &str = "RamSharedWinSvc";
    pub const SERVICE_DISPLAY: &str = "RamShared CUDA VRAM Disk Service";
    pub const SCM_CONFIG: &str = r"C:\ProgramData\RamShared\winsvc.toml";

    define_windows_service!(ffi_service_main, service_main);

    pub fn entry(args: Vec<String>) -> i32 {
        let cmd_args: Vec<String> = if args.len() > 1 {
            args[1..].to_vec()
        } else {
            vec![]
        };
        match parse_product_cli(&cmd_args) {
            Ok(ProductCommand::Install) => match install() {
                Ok(()) => {
                    eprintln!("installed service {SERVICE_NAME}");
                    0
                }
                Err(e) => {
                    eprintln!("install failed: {e}");
                    1
                }
            },
            Ok(ProductCommand::Uninstall) => match uninstall() {
                Ok(()) => {
                    eprintln!("uninstalled {SERVICE_NAME}");
                    0
                }
                Err(e) => {
                    eprintln!("uninstall failed: {e}");
                    1
                }
            },
            Ok(ProductCommand::ProbeCuda { config }) => match run_probe_cuda(&config) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("probe-cuda failed: {e}");
                    1
                }
            },
            Ok(ProductCommand::Console { config, .. }) => match run_console(&config) {
                Ok(code) => code,
                Err(e) => {
                    eprintln!("console failed: {e}");
                    1
                }
            },
            Ok(ProductCommand::ScmDefault) => {
                if let Err(e) = service_dispatcher::start(SERVICE_NAME, ffi_service_main) {
                    eprintln!("service_dispatcher failed: {e:?}");
                    eprintln!(
                        "usage: ramshared-winsvc [install|uninstall|probe-cuda --config <abs>|console --config <abs> --storage-only]"
                    );
                    1
                } else {
                    0
                }
            }
            Err(e) => {
                eprintln!("{e}");
                2
            }
        }
    }

    fn service_main(_args: Vec<OsString>) {
        if let Err(e) = run_service() {
            eprintln!("service error: {e}");
        }
    }

    fn run_service() -> Result<(), Box<dyn std::error::Error>> {
        let (shutdown_tx, shutdown_rx) = mpsc::channel();

        let status_handle =
            service_control_handler::register(SERVICE_NAME, move |control| match control {
                ServiceControl::Stop | ServiceControl::Shutdown => {
                    let _ = shutdown_tx.send(());
                    ServiceControlHandlerResult::NoError
                }
                ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
                _ => ServiceControlHandlerResult::NotImplemented,
            })?;

        status_handle.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::StartPending,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 1,
            wait_hint: Duration::from_secs(30),
            process_id: None,
        })?;

        let cfg = load_scm_config().map_err(|e| {
            let _ = status_handle.set_service_status(ServiceStatus {
                service_type: ServiceType::OWN_PROCESS,
                current_state: ServiceState::Stopped,
                controls_accepted: ServiceControlAccept::empty(),
                exit_code: ServiceExitCode::ServiceSpecific(2),
                checkpoint: 0,
                wait_hint: Duration::default(),
                process_id: None,
            });
            e
        })?;

        // Product runtime start (storage-only). Full Windows product wiring is
        // environment-bound; fail closed if CUDA/driver path is unavailable.
        if let Err(e) = start_product_runtime(&cfg) {
            status_handle.set_service_status(ServiceStatus {
                service_type: ServiceType::OWN_PROCESS,
                current_state: ServiceState::Stopped,
                controls_accepted: ServiceControlAccept::empty(),
                exit_code: ServiceExitCode::ServiceSpecific(1),
                checkpoint: 0,
                wait_hint: Duration::default(),
                process_id: None,
            })?;
            return Err(e);
        }

        status_handle.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::Running,
            controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        })?;

        let _ = shutdown_rx.recv();

        status_handle.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::StopPending,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 1,
            wait_hint: Duration::from_secs(30),
            process_id: None,
        })?;

        // DT-8 code 7: pagefile refusal returns to Running (SPEC main.rs).
        match stop_product_runtime(&cfg) {
            Ok(()) => {
                status_handle.set_service_status(ServiceStatus {
                    service_type: ServiceType::OWN_PROCESS,
                    current_state: ServiceState::Stopped,
                    controls_accepted: ServiceControlAccept::empty(),
                    exit_code: ServiceExitCode::Win32(0),
                    checkpoint: 0,
                    wait_hint: Duration::default(),
                    process_id: None,
                })?;
            }
            Err(code) if code == 7 => {
                // Refuse stop: remain Running, checkpoint 0, STOP still accepted.
                status_handle.set_service_status(ServiceStatus {
                    service_type: ServiceType::OWN_PROCESS,
                    current_state: ServiceState::Running,
                    controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
                    exit_code: ServiceExitCode::ServiceSpecific(7),
                    checkpoint: 0,
                    wait_hint: Duration::default(),
                    process_id: None,
                })?;
                // Block until process is terminated by operator after safety clear.
                let _ = shutdown_rx.recv();
                status_handle.set_service_status(ServiceStatus {
                    service_type: ServiceType::OWN_PROCESS,
                    current_state: ServiceState::Stopped,
                    controls_accepted: ServiceControlAccept::empty(),
                    exit_code: ServiceExitCode::ServiceSpecific(7),
                    checkpoint: 0,
                    wait_hint: Duration::default(),
                    process_id: None,
                })?;
            }
            Err(code) => {
                status_handle.set_service_status(ServiceStatus {
                    service_type: ServiceType::OWN_PROCESS,
                    current_state: ServiceState::Stopped,
                    controls_accepted: ServiceControlAccept::empty(),
                    exit_code: ServiceExitCode::ServiceSpecific(code as u32),
                    checkpoint: 0,
                    wait_hint: Duration::default(),
                    process_id: None,
                })?;
            }
        }

        Ok(())
    }

    fn load_scm_config() -> Result<WinDriveConfig, Box<dyn std::error::Error>> {
        let bytes = std::fs::read(SCM_CONFIG)?;
        Ok(WinDriveConfig::from_reader(&bytes)?)
    }

    fn start_product_runtime(cfg: &WinDriveConfig) -> Result<(), Box<dyn std::error::Error>> {
        // Validate product shape; full CUDA/driver composition is lab-bound.
        cfg.validate()?;
        let _state = RuntimeState::new(RunMode::Scm);
        eprintln!(
            "ramshared-winsvc: SCM product mode size_bytes={} cuda_device={} (full Online path requires lab GPU/driver)",
            cfg.size_bytes, cfg.cuda_device
        );
        // Without linked WindowsDriverLink + CUDA on this host, fail closed rather
        // than starting a false-green lab RAM backend.
        Err("product Online path requires Windows CUDA + StorPort lab; refuse false start".into())
    }

    fn stop_product_runtime(_cfg: &WinDriveConfig) -> Result<(), i32> {
        Ok(())
    }

    fn run_probe_cuda(config_path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let bytes = std::fs::read(config_path)?;
        let cfg = WinDriveConfig::from_reader(&bytes)?;
        cfg.validate()?;
        match try_probe_cuda(&cfg) {
            Ok(()) => {
                eprintln!("probe-cuda: PASS");
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    fn try_probe_cuda(cfg: &WinDriveConfig) -> Result<(), Box<dyn std::error::Error>> {
        use ramshared_winsvc::probe_cuda_allocates_roundtrips_and_restores;
        let report = probe_cuda_allocates_roundtrips_and_restores(cfg)?;
        eprintln!(
            "probe-cuda: device={} name={} size={} free_before={} free_after={} offsets={:?}",
            report.ordinal,
            report.device_name,
            report.size_bytes,
            report.free_before,
            report.free_after,
            report.offsets
        );
        Ok(())
    }

    fn run_console(config_path: &str) -> Result<i32, Box<dyn std::error::Error>> {
        let bytes = std::fs::read(config_path)?;
        let cfg = WinDriveConfig::from_reader(&bytes)?;
        cfg.validate()?;
        eprintln!(
            "console --storage-only: config OK size_bytes={} (product Online requires lab)",
            cfg.size_bytes
        );
        // Same product path as SCM — fail closed without lab stack.
        Err("console product Online path requires Windows CUDA + StorPort lab".into())
    }

    fn install() -> Result<(), Box<dyn std::error::Error>> {
        let exe = std::env::current_exe()?;
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
        let service_info = ServiceInfo {
            name: OsString::from(SERVICE_NAME),
            display_name: OsString::from(SERVICE_DISPLAY),
            service_type: ServiceType::OWN_PROCESS,
            start_type: ServiceStartType::OnDemand,
            error_control: ServiceErrorControl::Normal,
            executable_path: exe,
            launch_arguments: vec![],
            dependencies: vec![],
            account_name: None, // LocalSystem
            account_password: None,
        };
        match manager.create_service(
            &service_info,
            ServiceAccess::CHANGE_CONFIG | ServiceAccess::START,
        ) {
            Ok(_svc) => {
                // Disable failure auto-restart (SPEC).
                let _ = std::process::Command::new("sc.exe")
                    .args(["failure", SERVICE_NAME, "reset=", "0", "actions=", "="])
                    .status();
                Ok(())
            }
            Err(e) => Err(format!("create_service: {e:?} (try uninstall first)").into()),
        }
    }

    fn uninstall() -> Result<(), Box<dyn std::error::Error>> {
        // Safe stop first (SPEC): attempt stop; if code 7 refusal, do not delete.
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
        let service =
            manager.open_service(SERVICE_NAME, ServiceAccess::STOP | ServiceAccess::DELETE)?;
        match service.stop() {
            Ok(_) => {
                service.delete()?;
                Ok(())
            }
            Err(e) => {
                // If still running after safety refusal, refuse uninstall.
                Err(format!("safe stop failed, refuse uninstall: {e:?}").into())
            }
        }
    }
}

#[cfg(windows)]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let code = windows_svc::entry(args);
    std::process::exit(code);
}

#[cfg(not(windows))]
fn main() {
    use ramshared_winsvc::probe_cuda_allocates_roundtrips_and_restores;

    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse_product_cli(&args) {
        Ok(ProductCommand::ProbeCuda { config }) => match std::fs::read(&config) {
            Ok(bytes) => match WinDriveConfig::from_reader(&bytes) {
                Ok(cfg) => match probe_cuda_allocates_roundtrips_and_restores(&cfg) {
                    Ok(report) => {
                        eprintln!(
                            "probe-cuda: PASS (WSL/Linux libcuda evidence) ordinal={} name={} size={} free_before={} free_after={}",
                            report.ordinal,
                            report.device_name,
                            report.size_bytes,
                            report.free_before,
                            report.free_after
                        );
                        eprintln!(
                            "note: product path is Windows nvcuda.dll + StorPort; this run proves DT-3 allocate/pattern/free on available CUDA"
                        );
                        std::process::exit(0);
                    }
                    Err(e) => {
                        eprintln!("probe-cuda failed: {e}");
                        std::process::exit(1);
                    }
                },
                Err(e) => {
                    eprintln!("config error: {e}");
                    std::process::exit(2);
                }
            },
            Err(e) => {
                eprintln!("read config: {e}");
                std::process::exit(2);
            }
        },
        Ok(cmd) => {
            eprintln!("ramshared-winsvc: Windows product binary (Linux stub for non-probe cmds)");
            eprintln!("parsed command: {cmd:?}");
            eprintln!("lib APIs are testable via `cargo test -p ramshared-winsvc`");
            std::process::exit(2);
        }
        Err(e) => {
            eprintln!("{e}");
            let code = if e.class == RuntimeErrorClass::Config {
                2
            } else {
                1
            };
            std::process::exit(code);
        }
    }
}
