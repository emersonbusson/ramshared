//! ramshared-winsvc — Windows SCM entry for StorPort VRAM disk (SPEC ITEM-3/6/7).
//!
//! On non-Windows builds this is a stub so `cargo test --workspace` stays green.
//!
//! Lab mode (win11-drill / no CUDA): service start/stop orchestrates
//! `Start-RamSharedLab.ps1` / `Stop-RamSharedLab.ps1` (DT-9 ordered kill).
//! Product CUDA provision remains in the library (`provision_after_lease`).

#[cfg(windows)]
mod windows_svc {
    use std::ffi::OsString;
    use std::process::Command;
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

    pub const SERVICE_NAME: &str = "RamSharedWinSvc";
    pub const SERVICE_DISPLAY: &str = "RamShared VRAM Disk Service";
    /// Default lab scripts (deployed by install harness).
    const START_PS1: &str = r"C:\ramshared\bin\Start-RamSharedLab.ps1";
    const STOP_PS1: &str = r"C:\ramshared\bin\Stop-RamSharedLab.ps1";

    define_windows_service!(ffi_service_main, service_main);

    pub fn entry(args: Vec<String>) -> i32 {
        match args.get(1).map(String::as_str) {
            Some("install") => match install() {
                Ok(()) => {
                    eprintln!("installed service {SERVICE_NAME} (auto-start)");
                    0
                }
                Err(e) => {
                    eprintln!("install failed: {e}");
                    1
                }
            },
            Some("uninstall") => match uninstall() {
                Ok(()) => {
                    eprintln!("uninstalled {SERVICE_NAME}");
                    0
                }
                Err(e) => {
                    eprintln!("uninstall failed: {e}");
                    1
                }
            },
            Some("console") | Some("run") => {
                // Interactive lab path (not under SCM).
                if let Err(e) = run_start_scripts() {
                    eprintln!("console start failed: {e}");
                    return 1;
                }
                eprintln!("console: backend started; Ctrl+C then Stop-RamSharedLab for DT-9 stop");
                // Block until killed — operator uses sc stop / Stop-RamSharedLab.
                loop {
                    std::thread::sleep(Duration::from_secs(60));
                }
            }
            Some("stop-console") => match run_stop_scripts() {
                Ok(code) => code,
                Err(e) => {
                    eprintln!("stop-console failed: {e}");
                    1
                }
            },
            _ => {
                // Default: SCM dispatcher.
                if let Err(e) = service_dispatcher::start(SERVICE_NAME, ffi_service_main) {
                    eprintln!("service_dispatcher failed: {e:?}");
                    eprintln!("usage: ramshared-winsvc [install|uninstall|console|stop-console]");
                    1
                } else {
                    0
                }
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

        // Lab provision: start backend (StorPort path). Fail-closed if scripts missing.
        if let Err(e) = run_start_scripts() {
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

        // Wait for stop.
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

        // DT-9 ordered stop — if pagefile still hot, Stop-RamSharedLab exits 2 (refuse).
        let stop_code = run_stop_scripts().unwrap_or(1);

        status_handle.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::Stopped,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: if stop_code == 0 {
                ServiceExitCode::Win32(0)
            } else {
                ServiceExitCode::ServiceSpecific(stop_code as u32)
            },
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        })?;

        Ok(())
    }

    fn run_start_scripts() -> Result<(), Box<dyn std::error::Error>> {
        let status = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                START_PS1,
                "-FormatIfNeeded",
            ])
            .status()?;
        if !status.success() {
            return Err(format!("Start-RamSharedLab exit {status:?}").into());
        }
        Ok(())
    }

    fn run_stop_scripts() -> Result<i32, Box<dyn std::error::Error>> {
        let status = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                STOP_PS1,
                "-Drive",
                "D",
            ])
            .status()?;
        Ok(status.code().unwrap_or(1))
    }

    fn install() -> Result<(), Box<dyn std::error::Error>> {
        let exe = std::env::current_exe()?;
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
        // CREATE already may fail if exists — try open+change or create.
        let service_info = ServiceInfo {
            name: OsString::from(SERVICE_NAME),
            display_name: OsString::from(SERVICE_DISPLAY),
            service_type: ServiceType::OWN_PROCESS,
            start_type: ServiceStartType::Auto,
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
                // Prefer delayed auto-start so StorPort/PnP can settle (sc config).
                let _ = Command::new("sc.exe")
                    .args(["config", SERVICE_NAME, "start=", "delayed-auto"])
                    .status();
                Ok(())
            }
            Err(e) => {
                // Already exists: update binary path via delete+create is heavy; report.
                Err(format!("create_service: {e:?} (try uninstall first)").into())
            }
        }
    }

    fn uninstall() -> Result<(), Box<dyn std::error::Error>> {
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
        let service =
            manager.open_service(SERVICE_NAME, ServiceAccess::STOP | ServiceAccess::DELETE)?;
        let _ = service.stop();
        service.delete()?;
        Ok(())
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
    eprintln!("ramshared-winsvc: Windows-only binary (stub on this host)");
    eprintln!(
        "lib APIs (provision/teardown/DT-9) are testable via `cargo test -p ramshared-winsvc`"
    );
    std::process::exit(2);
}
