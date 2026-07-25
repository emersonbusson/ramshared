use std::ffi::OsString;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ramshared_broker::protocol::{MAX_LINE_BYTES, Msg};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{
    self, ServiceControlHandlerResult, ServiceStatusHandle,
};

use crate::pipe::{AuthenticatedPipe, PipeAuthError, PipeServer};
use crate::{
    BrokerConfigV1, BrokerEffect, BrokerSessionCore, BrokerStatusRequestV1, BrokerStatusV1,
};

pub const SERVICE_NAME: &str = "RamSharedBroker";
pub const CONSUMER_SERVICE_ACCOUNT: &str = r"NT SERVICE\RamSharedWinSvc";
pub const BROKER_SERVICE_ACCOUNT: &str = r"NT SERVICE\RamSharedBroker";

windows_service::define_windows_service!(ffi_service_main, service_main);
static SERVICE_CONFIG: OnceLock<std::path::PathBuf> = OnceLock::new();

pub fn set_service_config(path: std::path::PathBuf) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("broker config path must be absolute".into());
    }
    SERVICE_CONFIG
        .set(path)
        .map_err(|_| "broker service config already set".into())
}

pub fn dispatch() -> Result<(), windows_service::Error> {
    windows_service::service_dispatcher::start(SERVICE_NAME, ffi_service_main)
}

pub fn service_main(_args: Vec<OsString>) {
    if let Err(error) = run_service_from_config() {
        eprintln!("RamSharedBroker service error: {error}");
        let _ = report_deterministic_start_failure(3);
    }
}

fn report_deterministic_start_failure(code: u32) -> Result<(), windows_service::Error> {
    let status = service_control_handler::register(SERVICE_NAME, |_| {
        ServiceControlHandlerResult::NotImplemented
    })?;
    set_status(&status, ServiceState::Stopped, 0, Duration::ZERO, code)
}

fn run_service_from_config() -> Result<(), Box<dyn std::error::Error>> {
    let path = SERVICE_CONFIG
        .get()
        .ok_or("SCM ImagePath must pass --config <absolute>")?;
    let bytes = std::fs::read(path)?;
    verify_active_config(path, &bytes)?;
    let config = BrokerConfigV1::from_toml(&bytes).map_err(io::Error::other)?;
    run_service(config)
}

#[derive(Deserialize)]
struct ActiveManifest {
    version: String,
    commit: String,
    artifacts: Vec<ActiveArtifact>,
}

#[derive(Deserialize)]
struct ActiveArtifact {
    role: String,
    relative_path: String,
    sha256: String,
}

fn verify_active_config(
    path: &std::path::Path,
    bytes: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let active: ActiveManifest = serde_json::from_slice(&std::fs::read(
        r"C:\ProgramData\RamShared\active-manifest.json",
    )?)?;
    if active.commit.len() < 12 {
        return Err("active manifest commit is too short".into());
    }
    let root = std::path::Path::new(r"C:\Program Files\RamShared\versions").join(format!(
        "{}-{}",
        active.version,
        &active.commit[..12]
    ));
    let artifact = active
        .artifacts
        .iter()
        .find(|artifact| artifact.role == "broker_config")
        .ok_or("active manifest has no broker_config")?;
    let expected = root.join(&artifact.relative_path);
    if std::fs::canonicalize(path)? != std::fs::canonicalize(expected)? {
        return Err("broker config does not match active manifest path".into());
    }
    if format!("{:X}", Sha256::digest(bytes)) != artifact.sha256 {
        return Err("broker config does not match active manifest hash".into());
    }
    Ok(())
}

pub fn run_service(config: BrokerConfigV1) -> Result<(), Box<dyn std::error::Error>> {
    let stop = Arc::new(AtomicBool::new(false));
    let handler_stop = Arc::clone(&stop);
    let status = service_control_handler::register(SERVICE_NAME, move |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            handler_stop.store(true, Ordering::Release);
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    })?;
    set_status(
        &status,
        ServiceState::StartPending,
        1,
        Duration::from_secs(30),
        0,
    )?;
    set_status(&status, ServiceState::Running, 0, Duration::ZERO, 0)?;
    let result = run_console(config, Arc::clone(&stop));
    let exit_code = if result.is_ok() { 0 } else { 3 };
    set_status(&status, ServiceState::Stopped, 0, Duration::ZERO, exit_code)?;
    result.map_err(Into::into)
}

fn set_status(
    handle: &ServiceStatusHandle,
    state: ServiceState,
    checkpoint: u32,
    wait_hint: Duration,
    exit_code: u32,
) -> Result<(), windows_service::Error> {
    handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: if state == ServiceState::Running {
            ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN
        } else {
            ServiceControlAccept::empty()
        },
        exit_code: if exit_code == 0 {
            ServiceExitCode::Win32(0)
        } else {
            ServiceExitCode::ServiceSpecific(exit_code)
        },
        checkpoint,
        wait_hint,
        process_id: None,
    })
}

pub fn run_console(config: BrokerConfigV1, stop: Arc<AtomicBool>) -> io::Result<()> {
    let instance_id = broker_instance_id()?;
    let evidence_path = config.evidence_path.clone();
    append_evidence(&evidence_path, &instance_id, "process_ready", None)?;
    let core = Arc::new(Mutex::new(BrokerSessionCore::new(
        config.capacity_bytes,
        config.allowed_tenant,
        instance_id.clone(),
    )));
    let status_core = Arc::clone(&core);
    let status_stop = Arc::clone(&stop);
    let status_thread = std::thread::spawn(move || serve_status(status_core, status_stop));
    let mut session_id = 1usize;
    while !stop.load(Ordering::Acquire) {
        let server =
            match PipeServer::bind_product(BROKER_SERVICE_ACCOUNT, CONSUMER_SERVICE_ACCOUNT) {
                Ok(server) => server,
                Err(PipeAuthError::Io(error)) => {
                    append_evidence(
                        &evidence_path,
                        &instance_id,
                        &format!("peer_auth_io_error_{}", error.raw_os_error().unwrap_or(-1)),
                        Some(session_id),
                    )?;
                    return Err(error);
                }
                Err(error) => return Err(io::Error::other(format!("{error:?}"))),
            };
        let pipe =
            match server.accept_authenticated(&stop, Instant::now() + Duration::from_secs(10)) {
                Ok(pipe) => pipe,
                Err(PipeAuthError::Stopping) if stop.load(Ordering::Acquire) => break,
                Err(PipeAuthError::Deadline) => continue,
                Err(PipeAuthError::Refused) => {
                    append_evidence(
                        &evidence_path,
                        &instance_id,
                        "peer_sid_refused",
                        Some(session_id),
                    )?;
                    continue;
                }
                Err(PipeAuthError::Io(error)) => {
                    append_evidence(
                        &evidence_path,
                        &instance_id,
                        &format!("peer_auth_io_error_{}", error.raw_os_error().unwrap_or(-1)),
                        Some(session_id),
                    )?;
                    return Err(error);
                }
                Err(error) => return Err(io::Error::other(format!("{error:?}"))),
            };
        let mut first_frame = [0u8; 4096];
        let first_read = match pipe.read_first_authenticated_stoppable(&mut first_frame, &stop) {
            Ok(read) => read,
            Err(PipeAuthError::Refused) => {
                append_evidence(
                    &evidence_path,
                    &instance_id,
                    "peer_sid_refused",
                    Some(session_id),
                )?;
                continue;
            }
            Err(PipeAuthError::Io(_)) if stop.load(Ordering::Acquire) => break,
            Err(PipeAuthError::Io(error)) => {
                append_evidence(
                    &evidence_path,
                    &instance_id,
                    &format!("peer_auth_io_error_{}", error.raw_os_error().unwrap_or(-1)),
                    Some(session_id),
                )?;
                return Err(error);
            }
            Err(error) => return Err(io::Error::other(format!("{error:?}"))),
        };
        serve_session(
            &core,
            session_id,
            &pipe,
            &stop,
            &evidence_path,
            &instance_id,
            &first_frame[..first_read],
        )?;
        let mut core_guard = core
            .lock()
            .map_err(|_| io::Error::other("broker core mutex poisoned"))?;
        for effect in core_guard.on_disconnect(session_id) {
            if let BrokerEffect::Audit(message) = effect {
                eprintln!("broker audit={message}");
                append_evidence(&evidence_path, &instance_id, &message, Some(session_id))?;
            }
        }
        session_id = session_id.saturating_add(1);
    }
    if status_thread.join().is_err() {
        return Err(io::Error::other("status worker panicked"));
    }
    Ok(())
}

fn serve_session(
    core: &Arc<Mutex<BrokerSessionCore>>,
    session_id: usize,
    pipe: &AuthenticatedPipe,
    stop: &AtomicBool,
    evidence_path: &std::path::Path,
    instance_id: &str,
    initial: &[u8],
) -> io::Result<()> {
    let mut frame = initial.to_vec();
    let mut chunk = [0u8; 4096];
    while !stop.load(Ordering::Acquire) {
        if frame.len() > MAX_LINE_BYTES {
            write_message(
                pipe,
                &Msg::Error {
                    reason: "frame_too_large".into(),
                },
            )?;
            break;
        }
        while let Some(position) = frame.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = frame.drain(..=position).collect();
            let message: Msg = serde_json::from_slice(&line[..line.len() - 1])
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            let effects = core
                .lock()
                .map_err(|_| io::Error::other("broker core mutex poisoned"))?
                .on_authenticated_msg(session_id, message);
            let mut close = false;
            for effect in effects {
                match effect {
                    BrokerEffect::Reply(reply) => write_message(pipe, &reply)?,
                    BrokerEffect::Close => close = true,
                    BrokerEffect::Audit(message) => {
                        eprintln!("broker audit={message}");
                        append_evidence(evidence_path, instance_id, &message, Some(session_id))?;
                    }
                    BrokerEffect::LeaseReleased(lease) => {
                        eprintln!("broker lease_released={lease}")
                    }
                }
            }
            if close {
                return Ok(());
            }
        }
        let read = match pipe.read_frame_stoppable(&mut chunk, stop) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            Err(error) if matches!(error.raw_os_error(), Some(109) | Some(233)) => break,
            Err(error) => return Err(error),
        };
        frame.extend_from_slice(&chunk[..read]);
    }
    Ok(())
}

fn append_evidence(
    path: &std::path::Path,
    instance_id: &str,
    transition: &str,
    session_id: Option<usize>,
) -> io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let timestamp_unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_millis();
    let mut row = serde_json::to_vec(&serde_json::json!({
        "schema": 1,
        "service": SERVICE_NAME,
        "broker_instance_id": instance_id,
        "pipe": crate::pipe::PRODUCT_PIPE,
        "protocol": 1,
        "transition": transition,
        "session_id": session_id,
        "timestamp_unix_ms": timestamp_unix_ms,
    }))
    .map_err(io::Error::other)?;
    row.push(b'\n');
    if row.len() > 16 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "broker lifecycle row exceeds 16 KiB",
        ));
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(&row)?;
    file.flush()
}

fn serve_status(core: Arc<Mutex<BrokerSessionCore>>, stop: Arc<AtomicBool>) -> io::Result<()> {
    while !stop.load(Ordering::Acquire) {
        let server = match PipeServer::bind_status(BROKER_SERVICE_ACCOUNT, CONSUMER_SERVICE_ACCOUNT)
        {
            Ok(server) => server,
            Err(PipeAuthError::Io(error)) => return Err(error),
            Err(error) => return Err(io::Error::other(format!("{error:?}"))),
        };
        let pipe =
            match server.accept_authenticated(&stop, Instant::now() + Duration::from_secs(10)) {
                Ok(pipe) => pipe,
                Err(PipeAuthError::Stopping) if stop.load(Ordering::Acquire) => break,
                Err(PipeAuthError::Deadline | PipeAuthError::Refused) => continue,
                Err(PipeAuthError::Io(error)) => return Err(error),
                Err(error) => return Err(io::Error::other(format!("{error:?}"))),
            };
        let mut frame = [0u8; crate::pipe::STATUS_BUFFER_BYTES as usize];
        let read = match pipe.read_frame_deadline(&mut frame) {
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::TimedOut => continue,
            Err(error) => return Err(error),
        };
        let request = serde_json::from_slice::<BrokerStatusRequestV1>(&frame[..read]);
        if !matches!(request, Ok(BrokerStatusRequestV1::Status)) {
            let refused = serde_json::to_vec(&serde_json::json!({
                "schema": 1,
                "error": "status_pipe_read_only"
            }))
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            pipe.write_frame_deadline(&refused)?;
            continue;
        }
        let status: BrokerStatusV1 = core
            .lock()
            .map_err(|_| io::Error::other("broker core mutex poisoned"))?
            .status();
        let response = serde_json::to_vec(&status)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if response.len() > crate::pipe::STATUS_BUFFER_BYTES as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "broker status exceeds 4 KiB",
            ));
        }
        pipe.write_frame_deadline(&response)?;
    }
    Ok(())
}

fn write_message(pipe: &AuthenticatedPipe, message: &Msg) -> io::Result<()> {
    let mut line = serde_json::to_vec(message)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    line.push(b'\n');
    if line.len() > MAX_LINE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "encoded protocol frame exceeds cap",
        ));
    }
    let mut written = 0;
    while written < line.len() {
        written += pipe.write_frame_deadline(&line[written..])?;
    }
    Ok(())
}

fn broker_instance_id() -> io::Result<String> {
    use windows_sys::Win32::Security::Cryptography::{
        BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
    };
    let mut bytes = [0u8; 16];
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status != 0 {
        return Err(io::Error::other(format!(
            "BCryptGenRandom failed: 0x{status:08X}"
        )));
    }
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
