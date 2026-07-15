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
    use std::thread;
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
    use ramshared_winsvc::runtime::{ProductCommand, RunMode, parse_product_cli};

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
        use ramshared_winsvc::product_online::run_product_online;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_handler = Arc::clone(&stop);
        let (shutdown_tx, shutdown_rx) = mpsc::channel();

        let status_handle =
            service_control_handler::register(SERVICE_NAME, move |control| match control {
                ServiceControl::Stop | ServiceControl::Shutdown => {
                    stop_for_handler.store(true, Ordering::SeqCst);
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
        cfg.validate()?;

        // Accept STOP while Online so the shared AtomicBool is honoured.
        status_handle.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::Running,
            controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        })?;

        // Blocks until stop is set (SCM Stop) then runs Gate A/B teardown inside.
        let result = run_product_online(&cfg, RunMode::Scm, Arc::clone(&stop));

        // Drain any leftover stop signal.
        let _ = shutdown_rx.try_recv();

        match result {
            Ok(summary) => {
                eprintln!("product stopped: {summary:?}");
                status_handle.set_service_status(ServiceStatus {
                    service_type: ServiceType::OWN_PROCESS,
                    current_state: ServiceState::Stopped,
                    controls_accepted: ServiceControlAccept::empty(),
                    exit_code: ServiceExitCode::Win32(0),
                    checkpoint: 0,
                    wait_hint: Duration::default(),
                    process_id: None,
                })?;
                Ok(())
            }
            Err(e) if e.code == 7 => {
                // DT-8: pagefile refusal — stay Running, STOP still accepted.
                eprintln!("teardown refused code 7: {e}");
                status_handle.set_service_status(ServiceStatus {
                    service_type: ServiceType::OWN_PROCESS,
                    current_state: ServiceState::Running,
                    controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
                    exit_code: ServiceExitCode::ServiceSpecific(7),
                    checkpoint: 0,
                    wait_hint: Duration::default(),
                    process_id: None,
                })?;
                // Wait for another stop after operator clears pagefile, then exit.
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
                Ok(())
            }
            Err(e) => {
                status_handle.set_service_status(ServiceStatus {
                    service_type: ServiceType::OWN_PROCESS,
                    current_state: ServiceState::Stopped,
                    controls_accepted: ServiceControlAccept::empty(),
                    exit_code: ServiceExitCode::ServiceSpecific(e.code as u32),
                    checkpoint: 0,
                    wait_hint: Duration::default(),
                    process_id: None,
                })?;
                Err(e.into())
            }
        }
    }

    fn load_scm_config() -> Result<WinDriveConfig, Box<dyn std::error::Error>> {
        let bytes = std::fs::read(SCM_CONFIG)?;
        Ok(WinDriveConfig::from_reader(&bytes)?)
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
        use ramshared_winsvc::product_online::run_product_online;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        let bytes = std::fs::read(config_path)?;
        let cfg = WinDriveConfig::from_reader(&bytes)?;
        cfg.validate()?;
        eprintln!(
            "console --storage-only: starting product Online size_bytes={}",
            cfg.size_bytes
        );
        eprintln!(
            "stop: create file {} or wait for process signal",
            stop_request_path().display()
        );
        let stop = Arc::new(AtomicBool::new(false));
        let stop_c = Arc::clone(&stop);
        // Lab stop path: poll stop.request file (no force-kill required).
        thread::spawn(move || {
            let path = stop_request_path();
            loop {
                if path.exists() {
                    let _ = std::fs::remove_file(&path);
                    stop_c.store(true, Ordering::SeqCst);
                    break;
                }
                thread::sleep(Duration::from_millis(200));
            }
        });
        match run_product_online(&cfg, RunMode::Console, stop) {
            Ok(s) => {
                eprintln!("console stopped: {:?}", s);
                Ok(s.exit_code)
            }
            Err(e) if e.code == 7 => {
                eprintln!("teardown refused (code 7): {e}");
                Ok(7)
            }
            Err(e) => Err(e.into()),
        }
    }

    fn stop_request_path() -> std::path::PathBuf {
        std::path::PathBuf::from(r"C:\ProgramData\RamShared\stop.request")
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
