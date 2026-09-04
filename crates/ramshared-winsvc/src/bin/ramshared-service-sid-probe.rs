#[cfg(windows)]
mod windows_probe {
    use std::ffi::OsString;
    use std::io::{Read, Write};
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};

    use ramshared_winsvc::broker_tenant::BrokerTenant;
    use ramshared_winsvc::ipc::NamedPipeBrokerStream;
    use windows_service::define_windows_service;
    use windows_service::service::{
        ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
    use windows_service::service_dispatcher;

    const DEFAULT_SERVICE_NAME: &str = "RamSharedWinSvc";
    const RESULT_PATH: &str = r"C:\ramshared\autonomous-broker\service-sid-probe.json";
    static SERVICE_NAME: OnceLock<String> = OnceLock::new();
    static MODE: OnceLock<String> = OnceLock::new();
    static DENY_SID: OnceLock<String> = OnceLock::new();

    define_windows_service!(ffi_service_main, service_main);

    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let mut arguments: Vec<String> = std::env::args().skip(1).collect();
        let mut service_name = DEFAULT_SERVICE_NAME.to_string();
        let mut mode = "lease".to_string();
        let mut deny_sid = None;
        let mut index = 0;
        while index < arguments.len() {
            match arguments.get(index).map(String::as_str) {
                Some("--service-name") => {
                    let value = arguments
                        .get_mut(index + 1)
                        .filter(|value| !value.is_empty())
                        .ok_or("--service-name requires a value")?;
                    service_name = std::mem::take(value);
                }
                Some("--mode") => {
                    let value = arguments
                        .get_mut(index + 1)
                        .filter(|value| {
                            matches!(
                                value.as_str(),
                                "lease" | "oversized" | "partial" | "blocked-read" | "deny-only"
                            )
                        })
                        .ok_or("--mode must be lease, oversized, partial, or blocked-read")?;
                    mode = std::mem::take(value);
                }
                Some("--deny-sid") => {
                    let value = arguments
                        .get_mut(index + 1)
                        .filter(|value| value.starts_with("S-1-5-80-"))
                        .ok_or("--deny-sid requires a service SID")?;
                    deny_sid = Some(std::mem::take(value));
                }
                _ => return Err("unknown probe argument".into()),
            }
            index += 2;
        }
        SERVICE_NAME
            .set(service_name.clone())
            .map_err(|_| "service name already set")?;
        MODE.set(mode).map_err(|_| "probe mode already set")?;
        if let Some(sid) = deny_sid {
            DENY_SID.set(sid).map_err(|_| "deny SID already set")?;
        }
        service_dispatcher::start(service_name, ffi_service_main)?;
        Ok(())
    }

    fn service_main(_arguments: Vec<OsString>) {
        let result = run_probe();
        let _ = write_result(&result);
    }

    fn run_probe() -> Result<(), Box<dyn std::error::Error>> {
        let service_name = SERVICE_NAME.get().ok_or("service name unavailable")?;
        let status = service_control_handler::register(service_name, |_| {
            ServiceControlHandlerResult::NotImplemented
        })?;
        set_status(&status, ServiceState::StartPending, 1)?;
        let _deny_guard = if MODE.get().map(String::as_str) == Some("deny-only") {
            Some(impersonate_with_deny_only_service_sid(
                DENY_SID.get().ok_or("deny-only mode requires --deny-sid")?,
            )?)
        } else {
            None
        };
        let mut stream =
            NamedPipeBrokerStream::connect_product_pipe(Instant::now() + Duration::from_secs(10))
                .map_err(|error| format!("connect: {error:?}"))?;
        match MODE.get().map(String::as_str) {
            Some("oversized") => {
                stream.write_all(&vec![b'x'; ramshared_broker::protocol::MAX_LINE_BYTES + 2])?;
                let result = expect_peer_close(&mut stream);
                set_status(&status, ServiceState::Stopped, 0)?;
                return result;
            }
            Some("partial") => {
                stream.write_all(br#"{"type":"register""#)?;
                let result = expect_peer_close(&mut stream);
                set_status(&status, ServiceState::Stopped, 0)?;
                return result;
            }
            Some("blocked-read") => {
                set_status(&status, ServiceState::Running, 0)?;
                std::thread::sleep(Duration::from_secs(20));
                set_status(&status, ServiceState::Stopped, 0)?;
                return Ok(());
            }
            Some("deny-only") => {
                let mut tenant = BrokerTenant::new("windows-drive", Duration::from_secs(5));
                let result = tenant.register(&mut stream);
                set_status(&status, ServiceState::Stopped, 0)?;
                return if result.is_err() {
                    Ok(())
                } else {
                    Err("deny-only service SID was admitted".into())
                };
            }
            _ => {}
        }
        let mut tenant = BrokerTenant::new("windows-drive", Duration::from_secs(5));
        tenant.register(&mut stream)?;
        let lease = tenant.acquire(&mut stream, 64 * 1024 * 1024)?;
        set_status(&status, ServiceState::Running, 0)?;
        tenant.release(&mut stream)?;
        set_status(&status, ServiceState::Stopped, 0)?;
        if lease.bytes != 64 * 1024 * 1024 {
            return Err("unexpected lease size".into());
        }
        Ok(())
    }

    struct ImpersonationGuard;

    impl Drop for ImpersonationGuard {
        fn drop(&mut self) {
            unsafe {
                windows_sys::Win32::Security::RevertToSelf();
            }
        }
    }

    fn impersonate_with_deny_only_service_sid(
        sid: &str,
    ) -> Result<ImpersonationGuard, Box<dyn std::error::Error>> {
        use windows_sys::Win32::Foundation::{CloseHandle, LocalFree};
        use windows_sys::Win32::Security::Authorization::ConvertStringSidToSidW;
        use windows_sys::Win32::Security::{
            CreateRestrictedToken, ImpersonateLoggedOnUser, PSID, SID_AND_ATTRIBUTES,
            TOKEN_DUPLICATE, TOKEN_IMPERSONATE, TOKEN_QUERY,
        };
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

        let sid_w: Vec<u16> = sid.encode_utf16().chain(std::iter::once(0)).collect();
        let mut parsed_sid: PSID = std::ptr::null_mut();
        if unsafe { ConvertStringSidToSidW(sid_w.as_ptr(), &mut parsed_sid) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut source = std::ptr::null_mut();
        if unsafe {
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_DUPLICATE | TOKEN_IMPERSONATE | TOKEN_QUERY,
                &mut source,
            )
        } == 0
        {
            unsafe {
                LocalFree(parsed_sid);
            }
            return Err(std::io::Error::last_os_error().into());
        }
        let disabled = SID_AND_ATTRIBUTES {
            Sid: parsed_sid,
            Attributes: 0,
        };
        let mut restricted = std::ptr::null_mut();
        let created = unsafe {
            CreateRestrictedToken(
                source,
                0,
                1,
                &disabled,
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                &mut restricted,
            )
        };
        unsafe {
            CloseHandle(source);
            LocalFree(parsed_sid);
        }
        if created == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let impersonated = unsafe { ImpersonateLoggedOnUser(restricted) };
        unsafe {
            CloseHandle(restricted);
        }
        if impersonated == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(ImpersonationGuard)
    }

    fn expect_peer_close(
        stream: &mut NamedPipeBrokerStream,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut response = [0u8; 4096];
        match stream.read(&mut response) {
            Ok(0) | Err(_) => Ok(()),
            Ok(count)
                if String::from_utf8_lossy(&response[..count]).contains(r#""type":"error""#) =>
            {
                Ok(())
            }
            Ok(_) => Err("broker accepted malformed frame".into()),
        }
    }

    fn set_status(
        handle: &windows_service::service_control_handler::ServiceStatusHandle,
        state: ServiceState,
        checkpoint: u32,
    ) -> Result<(), windows_service::Error> {
        handle.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::Win32(0),
            checkpoint,
            wait_hint: if state == ServiceState::StartPending {
                Duration::from_secs(10)
            } else {
                Duration::ZERO
            },
            process_id: None,
        })
    }

    fn write_result(result: &Result<(), Box<dyn std::error::Error>>) -> std::io::Result<()> {
        let row = match result {
            Ok(()) => serde_json::json!({"schema":1,"verdict":"PASS"}),
            Err(error) => {
                serde_json::json!({"schema":1,"verdict":"FAIL","error":error.to_string()})
            }
        };
        let mut file = std::fs::File::create(RESULT_PATH)?;
        let bytes = serde_json::to_vec(&row).map_err(std::io::Error::other)?;
        if bytes.len() > 4096 {
            return Err(std::io::Error::other("probe result exceeds 4 KiB"));
        }
        file.write_all(&bytes)?;
        file.flush()
    }
}

#[cfg(windows)]
fn main() {
    if let Err(error) = windows_probe::run() {
        eprintln!("service SID probe failed: {error}");
        std::process::exit(2);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("ramshared-service-sid-probe requires Windows SCM");
    std::process::exit(2);
}
