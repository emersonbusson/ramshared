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
    use std::sync::OnceLock;
    use std::thread;
    use std::time::Duration;

    use windows_service::define_windows_service;
    use windows_service::service::{
        ServiceAccess, ServiceAction, ServiceActionType, ServiceControl, ServiceControlAccept,
        ServiceErrorControl, ServiceExitCode, ServiceFailureActions, ServiceFailureResetPeriod,
        ServiceInfo, ServiceSidType, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
    use windows_service::service_dispatcher;
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

    use ramshared_winsvc::config::WinDriveConfig;
    use ramshared_winsvc::runtime::{ProductCommand, RunMode, parse_product_cli};
    use ramshared_winsvc::windows_host::WindowsHostState;

    pub const SERVICE_NAME: &str = "RamSharedWinSvc";
    pub const SERVICE_DISPLAY: &str = "RamShared CUDA VRAM Disk Service";
    pub const BROKER_SERVICE_NAME: &str = "RamSharedBroker";
    pub const BROKER_SERVICE_DISPLAY: &str = "RamShared Local Broker Service";
    const PRODUCT_ROOT: &str = r"C:\Program Files\RamShared\versions";
    const PROGRAM_DATA: &str = r"C:\ProgramData\RamShared";
    static SCM_CONFIG: OnceLock<String> = OnceLock::new();

    define_windows_service!(ffi_service_main, service_main);

    pub fn entry(args: Vec<String>) -> i32 {
        let cmd_args: Vec<String> = if args.len() > 1 {
            args[1..].to_vec()
        } else {
            vec![]
        };
        match parse_product_cli(&cmd_args) {
            Ok(ProductCommand::Install { manifest }) | Ok(ProductCommand::Repair { manifest }) => {
                match install(&manifest) {
                    Ok(()) => {
                        println!("installed product manifest {manifest}");
                        0
                    }
                    Err(e) => {
                        eprintln!("install failed: {e}");
                        1
                    }
                }
            }
            Ok(ProductCommand::Status { json }) => match status(json) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("status failed: {e}");
                    1
                }
            },
            Ok(ProductCommand::Start) => match start() {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("start failed: {e}");
                    1
                }
            },
            Ok(ProductCommand::Stop) => match stop() {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("stop failed: {e}");
                    1
                }
            },
            Ok(ProductCommand::Uninstall) => match uninstall() {
                Ok(()) => {
                    println!("uninstalled {SERVICE_NAME}");
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
            Ok(ProductCommand::ScmDefault { config }) => {
                if let Some(config) = config
                    && SCM_CONFIG.set(config).is_err()
                {
                    eprintln!("SCM config was already selected");
                    return 2;
                }
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
        use std::sync::Mutex;
        use std::sync::atomic::{AtomicBool, Ordering};

        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_handler = Arc::clone(&stop);
        let stop_for_monitor = Arc::clone(&stop);
        let status_for_handler = Arc::new(Mutex::new(
            None::<service_control_handler::ServiceStatusHandle>,
        ));
        let status_slot = Arc::clone(&status_for_handler);
        let status_handle =
            service_control_handler::register(SERVICE_NAME, move |control| match control {
                ServiceControl::Stop | ServiceControl::Shutdown => {
                    if !stop_for_handler.swap(true, Ordering::SeqCst)
                        && let Ok(status) = status_slot.lock()
                        && let Some(handle) = *status
                    {
                        let stop_pending_result = handle.set_service_status(ServiceStatus {
                            service_type: ServiceType::OWN_PROCESS,
                            current_state: ServiceState::StopPending,
                            controls_accepted: ServiceControlAccept::empty(),
                            exit_code: ServiceExitCode::Win32(0),
                            checkpoint: 1,
                            wait_hint: Duration::from_secs(30),
                            process_id: None,
                        });
                        match stop_pending_result {
                            Ok(()) => diag_line("SCM status=StopPending"),
                            Err(error) => {
                                diag_line(&format!("SCM StopPending status failed: {error}"));
                            }
                        }
                    }
                    ServiceControlHandlerResult::NoError
                }
                ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
                _ => ServiceControlHandlerResult::NotImplemented,
            })?;
        *status_for_handler
            .lock()
            .map_err(|_| "SCM status handle lock poisoned")? = Some(status_handle);
        status_handle.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::StartPending,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 1,
            wait_hint: Duration::from_secs(30),
            process_id: None,
        })?;

        let cfg = load_scm_config().inspect_err(|_| {
            let _ = status_handle.set_service_status(ServiceStatus {
                service_type: ServiceType::OWN_PROCESS,
                current_state: ServiceState::Stopped,
                controls_accepted: ServiceControlAccept::empty(),
                exit_code: ServiceExitCode::ServiceSpecific(2),
                checkpoint: 0,
                wait_hint: Duration::default(),
                process_id: None,
            });
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

        let resume_notifier = Arc::new(AtomicBool::new(false));
        let monitor_done = Arc::new(AtomicBool::new(false));
        let notifier_for_monitor = Arc::clone(&resume_notifier);
        let done_for_monitor = Arc::clone(&monitor_done);
        let status_monitor = thread::spawn(move || {
            while !done_for_monitor.load(Ordering::Acquire) {
                if notifier_for_monitor.load(Ordering::Acquire) {
                    // Keep the one SCM STOP transaction pending. Briefly
                    // resume the I/O loop, then retry the safety gates.
                    stop_for_monitor.store(false, Ordering::Release);
                    notifier_for_monitor.store(false, Ordering::Release);
                    thread::sleep(Duration::from_secs(1));
                    stop_for_monitor.store(true, Ordering::Release);
                }
                thread::sleep(Duration::from_millis(5));
            }
        });

        // Blocks until stop is set (SCM Stop) then runs Gate A/B teardown inside.
        let result =
            run_product_online(&cfg, RunMode::Scm, Arc::clone(&stop), Some(resume_notifier));
        monitor_done.store(true, Ordering::Release);
        let _ = status_monitor.join();

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
            Err(e) => {
                diag_line(&format!("service runtime failed: {e}"));
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
        let path = SCM_CONFIG
            .get()
            .ok_or("SCM launch requires --config <absolute-version-owned-path>")?;
        verify_active_runtime_artifact(
            std::path::Path::new(path),
            ramshared_winsvc::package::ArtifactRole::WinsvcConfig,
        )?;
        load_product_config(path)
    }

    fn verify_active_runtime_artifact(
        selected: &std::path::Path,
        role: ramshared_winsvc::package::ArtifactRole,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let manifest = read_active_manifest()?;
        let root = std::path::Path::new(PRODUCT_ROOT).join(manifest.version_directory());
        let expected = root.join(&manifest.artifact(role)?.relative_path);
        if std::fs::canonicalize(selected)? != std::fs::canonicalize(&expected)? {
            return Err("SCM config does not match the active manifest".into());
        }
        ramshared_winsvc::package::verify_staged_files(&root, &manifest)?;
        Ok(())
    }

    fn load_product_config(path: &str) -> Result<WinDriveConfig, Box<dyn std::error::Error>> {
        if !WindowsHostState::is_elevated() {
            return Err("elevated token required".into());
        }
        Ok(WindowsHostState::read_owned_config(std::path::Path::new(
            path,
        ))?)
    }

    fn run_probe_cuda(config_path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let cfg = load_product_config(config_path)?;
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

        let cfg = load_product_config(config_path)?;
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
                match std::fs::symlink_metadata(&path) {
                    Ok(metadata)
                        if metadata.file_type().is_file() && !metadata.file_type().is_symlink() =>
                    {
                        match std::fs::remove_file(&path) {
                            Ok(()) => {
                                stop_c.store(true, Ordering::SeqCst);
                                // Unbuffered diagnostic: redirected stderr can lose last lines on kill.
                                diag_line("stop.request consumed; AtomicBool=true");
                                while stop_c.load(Ordering::Acquire) {
                                    thread::sleep(Duration::from_millis(50));
                                }
                                diag_line("stop flag cleared (resume Online or process exit)");
                            }
                            Err(e) => {
                                diag_line(&format!("stop.request remove refused: {e}"));
                            }
                        }
                    }
                    Ok(_) => diag_line("stop.request rejected: not a regular file"),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => diag_line(&format!("stop.request metadata failed: {e}")),
                }
                thread::sleep(Duration::from_millis(200));
            }
        });
        match run_product_online(&cfg, RunMode::Console, stop, None) {
            Ok(s) => {
                eprintln!("console stopped: {:?}", s);
                let _ = std::io::Write::flush(&mut std::io::stderr());
                Ok(s.exit_code)
            }
            Err(e) if e.code == 7 => {
                eprintln!("teardown refused (code 7): {e}");
                diag_line(&format!("teardown refused (code 7): {e}"));
                let _ = std::io::Write::flush(&mut std::io::stderr());
                Ok(7)
            }
            Err(e) => Err(e.into()),
        }
    }

    fn stop_request_path() -> std::path::PathBuf {
        std::path::PathBuf::from(r"C:\ProgramData\RamShared\stop.request")
    }

    /// Append one line to ProgramData diag log (create parents; best-effort).
    /// Survives force-kill better than redirected stderr buffers.
    fn diag_line(msg: &str) {
        use std::io::Write;
        let path = std::path::Path::new(r"C:\ProgramData\RamShared\teardown-diag.log");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let _ = writeln!(f, "{ts} {msg}");
            let _ = f.flush();
        }
        eprintln!("{msg}");
        let _ = std::io::Write::flush(&mut std::io::stderr());
    }

    fn install(manifest: &str) -> Result<(), Box<dyn std::error::Error>> {
        use ramshared_winsvc::package::{
            ArtifactRole, StartPolicy, validate_cross_config, verify_staged_files,
        };

        let _install_lock = ProductInstallMutex::acquire()?;
        let manifest_path = std::path::Path::new(manifest);
        if !manifest_path.is_absolute() {
            return Err("manifest path must be absolute".into());
        }
        let bytes = std::fs::read(manifest_path)?;
        let candidate = ramshared_winsvc::package::parse_manifest(&bytes)?;
        let source_root = manifest_path
            .parent()
            .ok_or("manifest has no package directory")?;
        let versions = std::path::Path::new(PRODUCT_ROOT);
        std::fs::create_dir_all(versions)?;
        std::fs::create_dir_all(PROGRAM_DATA)?;
        let final_root = versions.join(candidate.version_directory());
        if !final_root.exists() {
            let run_id = format!(
                "{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_nanos()
            );
            let staging = versions.join(format!(".staging-{run_id}"));
            std::fs::create_dir(&staging)?;
            let stage_result = (|| -> Result<(), Box<dyn std::error::Error>> {
                for artifact in &candidate.artifacts {
                    let source = source_root.join(&artifact.relative_path);
                    let target = staging.join(&artifact.relative_path);
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::copy(source, target)?;
                }
                std::fs::write(staging.join("product-manifest.json"), &bytes)?;
                verify_staged_files(&staging, &candidate)?;
                validate_installed_configs(&staging, &candidate)?;
                protect_version_directory(&staging)?;
                verify_staged_files(&staging, &candidate)?;
                std::fs::rename(&staging, &final_root)?;
                Ok(())
            })();
            if stage_result.is_err() {
                let _ = std::fs::remove_dir_all(&staging);
            }
            stage_result?;
        } else {
            verify_staged_files(&final_root, &candidate)?;
            validate_installed_configs(&final_root, &candidate)?;
        }

        stop_product_for_mutation()?;
        let old = read_active_manifest().ok();
        let register_result = (|| -> Result<(), Box<dyn std::error::Error>> {
            register_services(&final_root, &candidate, true)?;
            // Re-registering performs exact post-change queries for both definitions.
            verify_service_definitions(&final_root, &candidate)?;
            write_active_manifest(&candidate)?;
            Ok(())
        })();
        if let Err(error) = register_result {
            if let Some(old_manifest) = old {
                let old_root = versions.join(old_manifest.version_directory());
                register_services(&old_root, &old_manifest, false)?;
                verify_service_definitions(&old_root, &old_manifest)?;
                write_active_manifest(&old_manifest)?;
            } else {
                delete_service_if_present(SERVICE_NAME)?;
                delete_service_if_present(BROKER_SERVICE_NAME)?;
            }
            return Err(format!("candidate registration rolled back: {error}").into());
        }

        // Configs are parsed before SCM mutation and remain version-owned.
        let broker = ramshared_winbroker::BrokerConfigV1::from_toml(&std::fs::read(
            final_root.join(
                &candidate
                    .artifact(ArtifactRole::BrokerConfig)?
                    .relative_path,
            ),
        )?)?;
        let winsvc = WinDriveConfig::from_reader(&std::fs::read(
            final_root.join(
                &candidate
                    .artifact(ArtifactRole::WinsvcConfig)?
                    .relative_path,
            ),
        )?)?;
        validate_cross_config(&broker, &winsvc)?;
        match candidate.start_policy {
            StartPolicy::Demand | StartPolicy::Automatic => {}
        }
        Ok(())
    }

    fn validate_installed_configs(
        root: &std::path::Path,
        manifest: &ramshared_winsvc::package::ProductManifestV1,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use ramshared_winsvc::package::{ArtifactRole, validate_cross_config};
        let broker_path = root.join(&manifest.artifact(ArtifactRole::BrokerConfig)?.relative_path);
        let winsvc_path = root.join(&manifest.artifact(ArtifactRole::WinsvcConfig)?.relative_path);
        let broker = ramshared_winbroker::BrokerConfigV1::from_toml(&std::fs::read(broker_path)?)?;
        let winsvc = WinDriveConfig::from_reader(&std::fs::read(winsvc_path)?)?;
        validate_cross_config(&broker, &winsvc)?;
        Ok(())
    }

    fn protect_version_directory(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::Security::Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
        };
        use windows_sys::Win32::Security::{
            DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
            SetFileSecurityW,
        };

        let output = std::process::Command::new("sc.exe")
            .args(["showsid", BROKER_SERVICE_NAME])
            .output()?;
        if !output.status.success() {
            return Err("sc.exe showsid failed for broker service identity".into());
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let broker_sid = text
            .split_whitespace()
            .find(|part| part.starts_with("S-1-5-80-"))
            .ok_or("sc.exe showsid did not return a service SID")?;
        let sddl = format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGX;;;{broker_sid})");
        let sddl_w: Vec<u16> = std::ffi::OsStr::new(&sddl)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl_w.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut paths = Vec::new();
        collect_tree_paths(path, &mut paths)?;
        paths.push(path.to_path_buf());
        let result = paths.into_iter().try_for_each(|entry| {
            let wide: Vec<u16> = entry
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            if unsafe {
                SetFileSecurityW(
                    wide.as_ptr(),
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    descriptor,
                )
            } == 0
            {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
        unsafe {
            LocalFree(descriptor);
        }
        result.map_err(Into::into)
    }

    fn collect_tree_paths(
        root: &std::path::Path,
        output: &mut Vec<std::path::PathBuf>,
    ) -> Result<(), std::io::Error> {
        for entry in std::fs::read_dir(root)? {
            let path = entry?.path();
            if path.is_dir() {
                collect_tree_paths(&path, output)?;
            }
            output.push(path);
        }
        Ok(())
    }

    fn run_checked(program: &str, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
        let output = std::process::Command::new(program).args(args).output()?;
        if !output.status.success() {
            return Err(format!(
                "{program} failed exit={:?}: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        Ok(())
    }

    fn service_info(
        root: &std::path::Path,
        manifest: &ramshared_winsvc::package::ProductManifestV1,
        broker: bool,
    ) -> Result<ServiceInfo, Box<dyn std::error::Error>> {
        use ramshared_winsvc::package::{ArtifactRole, StartPolicy};
        let (name, display, exe_role, config_role, account, dependencies) = if broker {
            (
                BROKER_SERVICE_NAME,
                BROKER_SERVICE_DISPLAY,
                ArtifactRole::BrokerExe,
                ArtifactRole::BrokerConfig,
                Some(OsString::from(r"NT SERVICE\RamSharedBroker")),
                vec![],
            )
        } else {
            (
                SERVICE_NAME,
                SERVICE_DISPLAY,
                ArtifactRole::WinsvcExe,
                ArtifactRole::WinsvcConfig,
                None,
                vec![windows_service::service::ServiceDependency::Service(
                    OsString::from(BROKER_SERVICE_NAME),
                )],
            )
        };
        let start_type = match manifest.start_policy {
            StartPolicy::Demand => ServiceStartType::OnDemand,
            StartPolicy::Automatic => ServiceStartType::AutoStart,
        };
        Ok(ServiceInfo {
            name: OsString::from(name),
            display_name: OsString::from(display),
            service_type: ServiceType::OWN_PROCESS,
            start_type,
            error_control: ServiceErrorControl::Normal,
            executable_path: root.join(&manifest.artifact(exe_role)?.relative_path),
            launch_arguments: vec![
                OsString::from("--config"),
                root.join(&manifest.artifact(config_role)?.relative_path)
                    .into_os_string(),
            ],
            dependencies,
            account_name: account,
            account_password: None,
        })
    }

    fn register_one(
        manager: &ServiceManager,
        info: &ServiceInfo,
        broker: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let access = ServiceAccess::QUERY_CONFIG
            | ServiceAccess::QUERY_STATUS
            | ServiceAccess::CHANGE_CONFIG
            | ServiceAccess::START
            | ServiceAccess::STOP
            | ServiceAccess::DELETE;
        let service = match manager.open_service(&info.name, access) {
            Ok(service) => {
                service.change_config(info)?;
                service
            }
            Err(_) => manager.create_service(info, access)?,
        };
        service.set_config_service_sid_info(ServiceSidType::Unrestricted)?;
        if broker {
            service.update_failure_actions(ServiceFailureActions {
                reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(60)),
                reboot_msg: None,
                command: None,
                actions: Some(vec![
                    ServiceAction {
                        action_type: ServiceActionType::Restart,
                        delay: Duration::from_secs(5),
                    },
                    ServiceAction {
                        action_type: ServiceActionType::Restart,
                        delay: Duration::from_secs(15),
                    },
                    ServiceAction {
                        action_type: ServiceActionType::Restart,
                        delay: Duration::from_secs(40),
                    },
                    ServiceAction {
                        action_type: ServiceActionType::None,
                        delay: Duration::default(),
                    },
                ]),
            })?;
            service.set_failure_actions_on_non_crash_failures(false)?;
        } else {
            service.update_failure_actions(ServiceFailureActions {
                reset_period: ServiceFailureResetPeriod::Never,
                reboot_msg: None,
                command: None,
                actions: Some(vec![]),
            })?;
        }
        Ok(())
    }

    fn register_services(
        root: &std::path::Path,
        manifest: &ramshared_winsvc::package::ProductManifestV1,
        allow_manufactured_failure: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let manager = ServiceManager::local_computer(
            None::<&str>,
            ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
        )?;
        let broker = service_info(root, manifest, true)?;
        register_one(&manager, &broker, true)?;
        if allow_manufactured_failure
            && std::env::var_os("RAMSHARED_TEST_FAIL_AFTER_BROKER").is_some()
        {
            return Err("manufactured failure after broker registration".into());
        }
        let consumer = service_info(root, manifest, false)?;
        register_one(&manager, &consumer, false)?;
        if matches!(
            manifest.start_policy,
            ramshared_winsvc::package::StartPolicy::Automatic
        ) {
            run_checked(
                "sc.exe",
                &["config", SERVICE_NAME, "start=", "delayed-auto"],
            )?;
        }
        Ok(())
    }

    fn verify_service_definitions(
        root: &std::path::Path,
        manifest: &ramshared_winsvc::package::ProductManifestV1,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
        for broker in [true, false] {
            let expected = service_info(root, manifest, broker)?;
            let service = manager.open_service(&expected.name, ServiceAccess::QUERY_CONFIG)?;
            let actual = service.query_config()?;
            let command = actual.executable_path.to_string_lossy();
            if !command.contains(&expected.executable_path.to_string_lossy().to_string())
                || !expected
                    .launch_arguments
                    .iter()
                    .all(|argument| command.contains(&argument.to_string_lossy().to_string()))
                || actual.start_type != expected.start_type
                || actual.dependencies != expected.dependencies
                || service.get_config_service_sid_info()? != ServiceSidType::Unrestricted
            {
                return Err(format!("SCM definition mismatch for {:?}", expected.name).into());
            }
        }
        Ok(())
    }

    fn active_manifest_path() -> std::path::PathBuf {
        std::path::Path::new(PROGRAM_DATA).join("active-manifest.json")
    }

    fn read_active_manifest()
    -> Result<ramshared_winsvc::package::ProductManifestV1, Box<dyn std::error::Error>> {
        let bytes = std::fs::read(active_manifest_path())?;
        Ok(ramshared_winsvc::package::parse_manifest(&bytes)?)
    }

    fn write_active_manifest(
        manifest: &ramshared_winsvc::package::ProductManifestV1,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use std::io::Write;
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{REPLACEFILE_WRITE_THROUGH, ReplaceFileW};

        let active = active_manifest_path();
        let replacement = active.with_extension("new");
        let backup = active.with_file_name("active-manifest.bak");
        let mut file = std::fs::File::create(&replacement)?;
        file.write_all(&serde_json::to_vec_pretty(manifest)?)?;
        file.sync_all()?;
        drop(file);
        if active.exists() {
            let wide = |path: &std::path::Path| {
                path.as_os_str()
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect::<Vec<u16>>()
            };
            let active_w = wide(&active);
            let replacement_w = wide(&replacement);
            let backup_w = wide(&backup);
            let replaced = unsafe {
                ReplaceFileW(
                    active_w.as_ptr(),
                    replacement_w.as_ptr(),
                    backup_w.as_ptr(),
                    REPLACEFILE_WRITE_THROUGH,
                    std::ptr::null(),
                    std::ptr::null(),
                )
            };
            if replaced == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
        } else {
            std::fs::rename(replacement, active)?;
        }
        Ok(())
    }

    fn status(json: bool) -> Result<(), Box<dyn std::error::Error>> {
        use ramshared_winsvc::package::verify_staged_files;
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
        let consumer = manager.open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS)?;
        let broker = manager.open_service(BROKER_SERVICE_NAME, ServiceAccess::QUERY_STATUS)?;
        let consumer_status = consumer.query_status()?;
        let broker_status = broker.query_status()?;
        let consumer_state = format!("{:?}", consumer_status.current_state);
        let broker_state = format!("{:?}", broker_status.current_state);
        let manifest = read_active_manifest()?;
        let root = std::path::Path::new(PRODUCT_ROOT).join(manifest.version_directory());
        let hash_state = match verify_staged_files(&root, &manifest) {
            Ok(()) => "match",
            Err(_) => "mismatch",
        };
        if hash_state != "match" {
            return Err("installed artifact hash mismatch".into());
        }
        let (disk_count, pagefile_on_product_volume) = observe_host_residue(&root, &manifest)?;
        let live_broker = if broker_status.current_state == ServiceState::Running {
            Some(query_broker_status()?)
        } else {
            None
        };
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "schema": 1,
                    "active_version": manifest.version,
                    "active_commit": manifest.commit,
                    "artifact_hashes": hash_state,
                    "services": {
                        BROKER_SERVICE_NAME: {"state": broker_state},
                        SERVICE_NAME: {"state": consumer_state}
                    },
                    "host_residue": {
                        "ramshared_disk_count": disk_count,
                        "pagefile_on_product_volume": pagefile_on_product_volume
                    },
                    "broker_status": live_broker
                })
            );
        } else {
            println!(
                "version={} commit={} hashes={} {}={} {}={}",
                manifest.version,
                manifest.commit,
                hash_state,
                BROKER_SERVICE_NAME,
                broker_state,
                SERVICE_NAME,
                consumer_state
            );
        }
        Ok(())
    }

    fn observe_host_residue(
        root: &std::path::Path,
        manifest: &ramshared_winsvc::package::ProductManifestV1,
    ) -> Result<(u32, bool), Box<dyn std::error::Error>> {
        use ramshared_winsvc::package::ArtifactRole;
        let config = WinDriveConfig::from_reader(&std::fs::read(
            root.join(&manifest.artifact(ArtifactRole::WinsvcConfig)?.relative_path),
        )?)?;
        let script = build_residue_script(config.volume_letter)?;
        let mut child = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = child.try_wait()? {
                let output = child.wait_with_output()?;
                if !status.success() {
                    return Err(format!(
                        "host residue query failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    )
                    .into());
                }
                let stdout = String::from_utf8(output.stdout)?;
                let mut fields = stdout.trim().split('|');
                let disks = fields.next().ok_or("missing disk count")?.parse::<u32>()?;
                let pagefiles = fields
                    .next()
                    .ok_or("missing pagefile count")?
                    .parse::<u32>()?;
                if fields.next().is_some() {
                    return Err("ambiguous host residue output".into());
                }
                return Ok((disks, pagefiles != 0));
            }
            if std::time::Instant::now() >= deadline {
                child.kill()?;
                let _ = child.wait();
                return Err("host residue query timed out".into());
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    fn query_broker_status()
    -> Result<ramshared_winbroker::BrokerStatusV1, Box<dyn std::error::Error>> {
        use std::io::{Read, Write};
        let mut stream = ramshared_winsvc::ipc::NamedPipeBrokerStream::connect_status_pipe(
            std::time::Instant::now() + Duration::from_secs(2),
        )
        .map_err(|error| format!("broker status pipe: {error:?}"))?;
        stream.write_all(&serde_json::to_vec(
            &ramshared_winbroker::BrokerStatusRequestV1::Status,
        )?)?;
        stream.flush()?;
        let mut response = [0u8; 4097];
        let count = stream.read(&mut response)?;
        if count > 4096 {
            return Err("broker status response exceeds 4 KiB".into());
        }
        Ok(serde_json::from_slice(&response[..count])?)
    }

    fn start() -> Result<(), Box<dyn std::error::Error>> {
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
        let service = manager.open_service(SERVICE_NAME, ServiceAccess::START)?;
        service.start::<&str>(&[])?;
        Ok(())
    }

    fn stop() -> Result<(), Box<dyn std::error::Error>> {
        stop_product_for_mutation()
    }

    fn uninstall() -> Result<(), Box<dyn std::error::Error>> {
        let _install_lock = ProductInstallMutex::acquire()?;
        let manifest = read_active_manifest().map_err(|error| {
            format!("refuse uninstall with missing/corrupt active pointer: {error}")
        })?;
        let root = std::path::Path::new(PRODUCT_ROOT).join(manifest.version_directory());
        ramshared_winsvc::package::verify_staged_files(&root, &manifest)
            .map_err(|error| format!("refuse uninstall with ambiguous package state: {error}"))?;
        validate_installed_configs(&root, &manifest)
            .map_err(|error| format!("refuse uninstall with ambiguous config state: {error}"))?;
        stop_product_for_mutation()?;
        delete_service_if_present(SERVICE_NAME)?;
        delete_service_if_present(BROKER_SERVICE_NAME)?;
        std::fs::remove_dir_all(root)?;
        std::fs::remove_file(active_manifest_path())?;
        Ok(())
    }

    fn stop_product_for_mutation() -> Result<(), Box<dyn std::error::Error>> {
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
        for name in [SERVICE_NAME, BROKER_SERVICE_NAME] {
            let service = match manager
                .open_service(name, ServiceAccess::QUERY_STATUS | ServiceAccess::STOP)
            {
                Ok(service) => service,
                Err(_) => continue,
            };
            let state = service.query_status()?.current_state;
            if state != ServiceState::Stopped {
                service.stop()?;
                let deadline = std::time::Instant::now() + Duration::from_secs(45);
                loop {
                    if service.query_status()?.current_state == ServiceState::Stopped {
                        break;
                    }
                    if std::time::Instant::now() >= deadline {
                        return Err(format!("{name} did not reach Stopped safely").into());
                    }
                    thread::sleep(Duration::from_millis(200));
                }
            }
        }
        Ok(())
    }

    fn delete_service_if_present(name: &str) -> Result<(), Box<dyn std::error::Error>> {
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
        if let Ok(service) =
            manager.open_service(name, ServiceAccess::QUERY_STATUS | ServiceAccess::DELETE)
        {
            if service.query_status()?.current_state != ServiceState::Stopped {
                return Err(format!("refuse delete of non-stopped service {name}").into());
            }
            service.delete()?;
        }
        Ok(())
    }

    struct ProductInstallMutex {
        handle: windows_sys::Win32::Foundation::HANDLE,
    }

    impl ProductInstallMutex {
        fn acquire() -> Result<Self, Box<dyn std::error::Error>> {
            use std::os::windows::ffi::OsStrExt;
            use windows_sys::Win32::Foundation::{
                CloseHandle, LocalFree, WAIT_ABANDONED, WAIT_OBJECT_0,
            };
            use windows_sys::Win32::Security::Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
            };
            use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
            use windows_sys::Win32::System::Threading::{CreateMutexW, WaitForSingleObject};

            let wide = |value: &str| {
                std::ffi::OsStr::new(value)
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect::<Vec<u16>>()
            };
            let sddl = wide("D:P(A;;GA;;;SY)(A;;GA;;;BA)");
            let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
            if unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    std::ptr::null_mut(),
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
            let attributes = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor,
                bInheritHandle: 0,
            };
            let name = wide(r"Global\RamSharedProductInstall.v1");
            let handle = unsafe { CreateMutexW(&attributes, 0, name.as_ptr()) };
            unsafe {
                LocalFree(descriptor);
            }
            if handle.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            let wait = unsafe { WaitForSingleObject(handle, 30_000) };
            if wait != WAIT_OBJECT_0 && wait != WAIT_ABANDONED {
                unsafe {
                    CloseHandle(handle);
                }
                return Err(format!("installer mutex wait failed: {wait}").into());
            }
            Ok(Self { handle })
        }
    }

    impl Drop for ProductInstallMutex {
        fn drop(&mut self) {
            use windows_sys::Win32::Foundation::CloseHandle;
            use windows_sys::Win32::System::Threading::ReleaseMutex;
            unsafe {
                ReleaseMutex(self.handle);
                CloseHandle(self.handle);
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

pub fn build_residue_script(letter: char) -> Result<String, Box<dyn std::error::Error>> {
    let uppercase_letter = letter.to_ascii_uppercase();
    if !uppercase_letter.is_ascii_uppercase() {
        return Err("invalid volume letter for host residue query".into());
    }
    let escaped_letter = uppercase_letter.to_string().replace('\'', "''");
    Ok(format!(
        concat!(
            "$ErrorActionPreference='Stop';",
            "$d=@(Get-CimInstance Win32_DiskDrive|?{{",
            "$_.Model -match 'RAMSHARE|VRAMDISK' -or $_.Caption -match 'RAMSHARE|VRAMDISK'}});",
            "$p=@(Get-CimInstance Win32_PageFileUsage|?{{",
            "$_.Name -match '^{letter}:\\\\'}});",
            "Write-Output ($d.Count.ToString()+'|'+$p.Count.ToString())"
        ),
        letter = escaped_letter
    ))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn observe_host_residue_script_escaping() {
        let valid_script = build_residue_script('D').unwrap();
        assert!(valid_script.contains("$_.Name -match '^D:\\\\'"));

        let lowercase_script = build_residue_script('z').unwrap();
        assert!(lowercase_script.contains("$_.Name -match '^Z:\\\\'"));

        let invalid_char = build_residue_script('1');
        assert!(invalid_char.is_err());

        let injected_char = build_residue_script('\'');
        assert!(injected_char.is_err());
    }
}
