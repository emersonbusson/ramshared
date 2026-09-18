use std::ffi::OsString;
use std::io;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;

#[cfg(windows)]
use std::sync::Mutex;
#[cfg(windows)]
use std::sync::atomic::Ordering;
#[cfg(windows)]
use std::time::Duration;
#[cfg(windows)]
use std::time::Instant;

#[cfg(windows)]
use ramshared_broker::protocol::{MAX_LINE_BYTES, Msg};
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[cfg(windows)]
use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
#[cfg(windows)]
use windows_service::service_control_handler::{
    self, ServiceControlHandlerResult, ServiceStatusHandle,
};

#[cfg(windows)]
use crate::pipe::{AuthenticatedPipe, PIPE_OPERATION_TIMEOUT, PipeAuthError, PipeServer};
#[cfg(windows)]
use crate::{
    BrokerEffect, BrokerSessionCore, BrokerStatusRequestV1, BrokerStatusV1,
};
use crate::BrokerConfigV1;

pub const SERVICE_NAME: &str = "RamSharedBroker";
pub const CONSUMER_SERVICE_ACCOUNT: &str = r"NT SERVICE\RamSharedWinSvc";
pub const BROKER_SERVICE_ACCOUNT: &str = r"NT SERVICE\RamSharedBroker";

#[cfg(windows)]
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

#[cfg(windows)]
pub fn dispatch() -> Result<(), windows_service::Error> {
    windows_service::service_dispatcher::start(SERVICE_NAME, ffi_service_main)
}

#[cfg(not(windows))]
pub fn dispatch() -> Result<(), String> {
    Err("dispatch is only supported on Windows".into())
}

pub fn service_main(_args: Vec<OsString>) {
    if let Err(error) = run_service_from_config() {
        eprintln!("RamSharedBroker service error: {error}");
        let _ = report_deterministic_start_failure(3);
    }
}

#[cfg(windows)]
fn report_deterministic_start_failure(code: u32) -> Result<(), windows_service::Error> {
    let status = service_control_handler::register(SERVICE_NAME, |_| {
        ServiceControlHandlerResult::NotImplemented
    })?;
    set_status(&status, ServiceState::Stopped, 0, Duration::ZERO, code)
}

#[cfg(not(windows))]
fn report_deterministic_start_failure(_code: u32) -> Result<(), String> {
    Err("SCM service status reporting is only supported on Windows".into())
}

pub fn run_service_from_config() -> Result<(), Box<dyn std::error::Error>> {
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
    verify_active_config_with_paths(
        std::path::Path::new(r"C:\ProgramData\RamShared\active-manifest.json"),
        std::path::Path::new(r"C:\Program Files\RamShared\versions"),
        path,
        bytes,
    )
}

fn verify_active_config_with_paths(
    manifest_path: &std::path::Path,
    version_root: &std::path::Path,
    path: &std::path::Path,
    bytes: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let active: ActiveManifest = serde_json::from_slice(&std::fs::read(manifest_path)?)?;
    if active.commit.len() < 12 {
        return Err("active manifest commit is too short".into());
    }
    let root = version_root.join(format!(
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
    let hash_hex: String = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect();
    if hash_hex != artifact.sha256 {
        return Err("broker config does not match active manifest hash".into());
    }
    Ok(())
}

#[cfg(windows)]
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

#[cfg(not(windows))]
pub fn run_service(_config: BrokerConfigV1) -> Result<(), Box<dyn std::error::Error>> {
    Err("run_service is only supported on Windows".into())
}

#[cfg(windows)]
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

#[cfg(windows)]
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
        let expiry_effects = core
            .lock()
            .map_err(|_| io::Error::other("broker core mutex poisoned"))?
            .on_tick(Instant::now());
        for effect in expiry_effects {
            match effect {
                BrokerEffect::Audit(message) => {
                    eprintln!("broker audit={message}");
                    append_evidence(&evidence_path, &instance_id, &message, Some(session_id))?;
                }
                BrokerEffect::LeaseReleased(lease) => {
                    eprintln!("broker lease_released={lease}");
                }
                BrokerEffect::Reply(_) | BrokerEffect::Close => {
                    return Err(io::Error::other("unexpected detached broker effect"));
                }
            }
        }
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
        let pipe = match server.accept_authenticated(&stop, Instant::now() + PIPE_OPERATION_TIMEOUT)
        {
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

#[cfg(not(windows))]
pub fn run_console(_config: BrokerConfigV1, _stop: Arc<AtomicBool>) -> io::Result<()> {
    Err(io::Error::other("run_console is only supported on Windows"))
}

#[cfg(windows)]
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
                .on_authenticated_msg_at(session_id, message, Instant::now());
            if deliver_session_effects(pipe, effects, evidence_path, instance_id, session_id)? {
                return Ok(());
            }
        }
        let read = match pipe.read_frame_stoppable(&mut chunk, stop) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::TimedOut => {
                let effects = core
                    .lock()
                    .map_err(|_| io::Error::other("broker core mutex poisoned"))?
                    .on_tick(Instant::now());
                if deliver_session_effects(pipe, effects, evidence_path, instance_id, session_id)? {
                    return Ok(());
                }
                continue;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if matches!(error.raw_os_error(), Some(109) | Some(232) | Some(233)) => {
                break;
            }
            Err(error) => return Err(error),
        };
        frame.extend_from_slice(&chunk[..read]);
    }
    Ok(())
}

#[cfg(windows)]
fn deliver_session_effects(
    pipe: &AuthenticatedPipe,
    effects: Vec<BrokerEffect>,
    evidence_path: &std::path::Path,
    instance_id: &str,
    session_id: usize,
) -> io::Result<bool> {
    let mut close = false;
    for effect in effects {
        match effect {
            BrokerEffect::Reply(reply) => write_message(pipe, &reply)?,
            BrokerEffect::Close => close = true,
            BrokerEffect::Audit(message) => {
                eprintln!("broker audit={message}");
                append_evidence(evidence_path, instance_id, &message, Some(session_id))?;
            }
            BrokerEffect::LeaseReleased(lease) => eprintln!("broker lease_released={lease}"),
        }
    }
    Ok(close)
}

#[cfg_attr(not(windows), allow(dead_code))]
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
        "pipe": r"\\.\pipe\RamSharedBroker.v1",
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
    file.flush()?;
    emit_event(transition, instance_id);
    Ok(())
}

#[cfg(windows)]
fn emit_event(transition: &str, instance_id: &str) {
    use std::ptr;
    use windows_sys::Win32::System::EventLog::{
        DeregisterEventSource, EVENTLOG_INFORMATION_TYPE, RegisterEventSourceW, ReportEventW,
    };

    let transition: String = transition
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        .take(80)
        .collect();
    let instance_id: String = instance_id
        .chars()
        .filter(char::is_ascii_hexdigit)
        .take(32)
        .collect();
    let source: Vec<u16> = "RamSharedBroker\0".encode_utf16().collect();
    let message: Vec<u16> = format!("transition={transition} broker_instance_id={instance_id}\0")
        .encode_utf16()
        .collect();
    unsafe {
        let handle = RegisterEventSourceW(ptr::null(), source.as_ptr());
        if !handle.is_null() {
            let strings = [message.as_ptr()];
            let _ = ReportEventW(
                handle,
                EVENTLOG_INFORMATION_TYPE,
                0,
                1000,
                ptr::null_mut(),
                1,
                0,
                strings.as_ptr(),
                ptr::null(),
            );
            let _ = DeregisterEventSource(handle);
        }
    }
}

#[cfg(not(windows))]
fn emit_event(_transition: &str, _instance_id: &str) {}

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(not(windows))]
#[cfg_attr(not(windows), allow(dead_code))]
fn broker_instance_id() -> io::Result<String> {
    use sha2::{Digest, Sha256};
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let hash = Sha256::digest(time.to_le_bytes());
    Ok(hash[..16].iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use sha2::{Digest, Sha256};
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn constants_match_spec() {
        assert_eq!(SERVICE_NAME, "RamSharedBroker");
        assert_eq!(CONSUMER_SERVICE_ACCOUNT, r"NT SERVICE\RamSharedWinSvc");
        assert_eq!(BROKER_SERVICE_ACCOUNT, r"NT SERVICE\RamSharedBroker");
    }

    #[test]
    fn service_config_path_validation() {
        let relative = PathBuf::from("relative/config.toml");
        assert_eq!(
            set_service_config(relative).unwrap_err(),
            "broker config path must be absolute"
        );
    }

    #[test]
    fn run_service_from_config_fails_when_unset() {
        let err = run_service_from_config().unwrap_err();
        assert!(err.to_string().contains("SCM ImagePath must pass --config <absolute>"));
    }

    #[test]
    fn verify_active_config_rejects_short_commit() {
        let temp_dir = std::env::temp_dir().join("winbroker_test_short_commit");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let manifest_path = temp_dir.join("active-manifest.json");
        let manifest_json = serde_json::json!({
            "version": "1.0.0",
            "commit": "12345678901", // 11 chars
            "artifacts": [{
                "role": "broker_config",
                "relative_path": "broker.toml",
                "sha256": "ABCD"
            }]
        });
        fs::write(&manifest_path, serde_json::to_vec(&manifest_json).unwrap()).unwrap();

        let config_path = temp_dir.join("broker.toml");
        let err = verify_active_config_with_paths(
            &manifest_path,
            &temp_dir,
            &config_path,
            b"test config",
        )
        .unwrap_err();

        assert_eq!(err.to_string(), "active manifest commit is too short");
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn verify_active_config_rejects_missing_broker_config_artifact() {
        let temp_dir = std::env::temp_dir().join("winbroker_test_missing_artifact");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let manifest_path = temp_dir.join("active-manifest.json");
        let manifest_json = serde_json::json!({
            "version": "1.0.0",
            "commit": "123456789012", // 12 chars
            "artifacts": [{
                "role": "other_artifact",
                "relative_path": "other.toml",
                "sha256": "ABCD"
            }]
        });
        fs::write(&manifest_path, serde_json::to_vec(&manifest_json).unwrap()).unwrap();

        let config_path = temp_dir.join("broker.toml");
        let err = verify_active_config_with_paths(
            &manifest_path,
            &temp_dir,
            &config_path,
            b"test config",
        )
        .unwrap_err();

        assert_eq!(err.to_string(), "active manifest has no broker_config");
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn verify_active_config_rejects_mismatched_hash() {
        let temp_dir = std::env::temp_dir().join("winbroker_test_mismatched_hash");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let version_root = temp_dir.join("versions");
        let version_dir = version_root.join("1.0.0-123456789012");
        fs::create_dir_all(&version_dir).unwrap();

        let config_path = version_dir.join("broker.toml");
        let config_bytes = b"test config content";
        fs::write(&config_path, config_bytes).unwrap();

        let manifest_path = temp_dir.join("active-manifest.json");
        let manifest_json = serde_json::json!({
            "version": "1.0.0",
            "commit": "123456789012",
            "artifacts": [{
                "role": "broker_config",
                "relative_path": "broker.toml",
                "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
            }]
        });
        fs::write(&manifest_path, serde_json::to_vec(&manifest_json).unwrap()).unwrap();

        let err = verify_active_config_with_paths(
            &manifest_path,
            &version_root,
            &config_path,
            config_bytes,
        )
        .unwrap_err();

        assert_eq!(err.to_string(), "broker config does not match active manifest hash");
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn verify_active_config_succeeds_matching() {
        let temp_dir = std::env::temp_dir().join("winbroker_test_matching");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let version_root = temp_dir.join("versions");
        let version_dir = version_root.join("1.0.0-123456789012");
        fs::create_dir_all(&version_dir).unwrap();

        let config_path = version_dir.join("broker.toml");
        let config_bytes = b"test config content";
        fs::write(&config_path, config_bytes).unwrap();

        let hash_hex: String = Sha256::digest(config_bytes)
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect();

        let manifest_path = temp_dir.join("active-manifest.json");
        let manifest_json = serde_json::json!({
            "version": "1.0.0",
            "commit": "123456789012",
            "artifacts": [{
                "role": "broker_config",
                "relative_path": "broker.toml",
                "sha256": hash_hex
            }]
        });
        fs::write(&manifest_path, serde_json::to_vec(&manifest_json).unwrap()).unwrap();

        assert!(
            verify_active_config_with_paths(
                &manifest_path,
                &version_root,
                &config_path,
                config_bytes,
            )
            .is_ok()
        );
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn append_evidence_writes_valid_json_line() {
        let temp_dir = std::env::temp_dir().join("winbroker_test_evidence");
        let _ = fs::remove_dir_all(&temp_dir);
        let evidence_file = temp_dir.join("evidence.log");

        append_evidence(&evidence_file, "0123456789abcdef", "process_ready", Some(1)).unwrap();

        let content = fs::read_to_string(&evidence_file).unwrap();
        assert!(content.ends_with('\n'));

        let parsed: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(parsed["schema"], 1);
        assert_eq!(parsed["service"], SERVICE_NAME);
        assert_eq!(parsed["broker_instance_id"], "0123456789abcdef");
        assert_eq!(parsed["transition"], "process_ready");
        assert_eq!(parsed["session_id"], 1);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn broker_instance_id_generates_hex_string() {
        let instance_id = broker_instance_id().unwrap();
        assert_eq!(instance_id.len(), 32);
        assert!(instance_id.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
