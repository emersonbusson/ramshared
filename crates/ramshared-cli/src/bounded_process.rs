//! Exact, bounded custody for short-lived production subprocesses.
//!
//! Every trusted helper starts as the leader of a new process group. Timeout
//! and inherited-pipe cleanup therefore target only that group, never a name
//! or a broad PID search. Exit is observed with `waitid(..., WNOWAIT)` so the
//! zombie leader pins its PID/PGID until residual group members and capture
//! workers are closed. Production cannot resume if group SIGKILL fails or the
//! direct child cannot be reaped within the fixed grace window.
//!
//! This is a trusted-helper boundary: RamShared helpers do not intentionally
//! call `setsid`/`setpgid` or daemonize. A malicious helper that deliberately
//! escapes its private group is outside this custody contract.

use rustix::io::Errno;
use rustix::process::{Pid, Signal, WaitId, WaitIdOptions, kill_process_group, waitid};
use std::os::unix::ffi::OsStrExt;
use std::fmt;
use std::io::{self, Read};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(10);
pub(crate) const REAP_GRACE: Duration = Duration::from_millis(500);
const CAPTURE_CLOSE_GRACE: Duration = Duration::from_millis(500);
pub(crate) const DEFAULT_OUTPUT_LIMIT: usize = 64 * 1024;
const FATAL_EXIT_CODE: i32 = 125;

#[derive(Debug)]
pub(crate) enum ProcessSpawnError {
    BinaryNotFound { command: String },
    ExecutionTimeout { command: String, timeout: Duration },
    NonZeroExit { command: String, exit_code: i32, stderr: String },
    SpawnFailed { command: String, kind: io::ErrorKind, detail: String },
    FatalContainment { detail: String },
    PipeError { detail: String },
    GenericError { detail: String },
}

impl ProcessSpawnError {
    fn new(detail: impl Into<String>) -> Self {
        let detail_str = detail.into();
        if detail_str.contains("pipe") || detail_str.contains("capture") {
            ProcessSpawnError::PipeError { detail: detail_str }
        } else {
            ProcessSpawnError::GenericError { detail: detail_str }
        }
    }

    fn spawn(label: &str, error: io::Error) -> Self {
        if error.kind() == io::ErrorKind::NotFound {
            ProcessSpawnError::BinaryNotFound { command: label.to_string() }
        } else {
            ProcessSpawnError::SpawnFailed {
                command: label.to_string(),
                kind: error.kind(),
                detail: error.to_string(),
            }
        }
    }

    pub(crate) fn is_not_found(&self) -> bool {
        matches!(self, ProcessSpawnError::BinaryNotFound { .. })
    }

    fn fatal(detail: impl Into<String>) -> Self {
        ProcessSpawnError::FatalContainment { detail: detail.into() }
    }

    fn is_fatal(&self) -> bool {
        matches!(self, ProcessSpawnError::FatalContainment { .. })
    }
}

impl fmt::Display for ProcessSpawnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProcessSpawnError::BinaryNotFound { command } => write!(f, "spawn {command}: binary not found"),
            ProcessSpawnError::ExecutionTimeout { command, timeout } => write!(f, "{command} timed out after {} ms; process group killed and direct child reaped", timeout.as_millis()),
            ProcessSpawnError::NonZeroExit { command, exit_code, stderr } => {
                if stderr.is_empty() {
                    write!(f, "{command} exited with {exit_code}")
                } else {
                    write!(f, "{command} exited with {exit_code}: {stderr}")
                }
            }
            ProcessSpawnError::SpawnFailed { command, kind, detail } => write!(f, "spawn {command} ({kind}): {detail}"),
            ProcessSpawnError::FatalContainment { detail } => write!(f, "fatal containment selected: {detail}"),
            ProcessSpawnError::PipeError { detail } => write!(f, "{detail}"),
            ProcessSpawnError::GenericError { detail } => write!(f, "{detail}"),
        }
    }
}

impl std::error::Error for ProcessSpawnError {}

#[derive(Debug)]
pub(crate) struct BoundedOutput {
    pub(crate) status: ExitStatus,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

pub(crate) trait FatalContainment {
    fn contain(&self, detail: &str);
}

struct ExitController;

impl FatalContainment for ExitController {
    fn contain(&self, detail: &str) {
        eprintln!("ramshared fatal subprocess containment: {detail}");
        std::process::exit(FATAL_EXIT_CODE);
    }
}

trait ReapTarget {
    fn id(&self) -> u32;
    fn signal_group_kill(&mut self) -> io::Result<()>;
    fn signal_direct_kill(&mut self) -> io::Result<()>;
    fn observe_exit(&mut self) -> io::Result<bool>;
    fn reap_observed(&mut self) -> io::Result<Option<ExitStatus>>;
}

impl ReapTarget for Child {
    fn id(&self) -> u32 {
        Child::id(self)
    }

    fn signal_group_kill(&mut self) -> io::Result<()> {
        let raw = i32::try_from(self.id())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "child PID overflow"))?;
        let group = Pid::from_raw(raw)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "zero child PID"))?;
        kill_process_group(group, Signal::KILL).map_err(io::Error::from)
    }

    fn signal_direct_kill(&mut self) -> io::Result<()> {
        self.kill()
    }

    fn observe_exit(&mut self) -> io::Result<bool> {
        let raw = i32::try_from(self.id())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "child PID overflow"))?;
        let pid = Pid::from_raw(raw)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "zero child PID"))?;
        waitid(
            WaitId::Pid(pid),
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
        )
        .map(|status| status.is_some())
        .map_err(io::Error::from)
    }

    fn reap_observed(&mut self) -> io::Result<Option<ExitStatus>> {
        self.try_wait()
    }
}

fn fatal_error(fatal: &dyn FatalContainment, detail: impl Into<String>) -> ProcessSpawnError {
    let detail = detail.into();
    fatal.contain(&detail);
    ProcessSpawnError::fatal(format!("fatal containment selected: {detail}"))
}

#[derive(Default)]
struct GroupTerminationProof {
    errors: Vec<String>,
}

impl GroupTerminationProof {
    fn record(&mut self, label: &str, phase: &str, error: io::Error) {
        if error.raw_os_error() != Some(Errno::SRCH.raw_os_error()) {
            self.errors.push(format!(
                "{label}: exact process-group SIGKILL {phase} failed: {error}"
            ));
        }
    }
}

fn wait_for_exit_observation(
    target: &mut dyn ReapTarget,
    label: &str,
    grace: Duration,
    fatal: &dyn FatalContainment,
) -> Result<(), ProcessSpawnError> {
    let deadline = Instant::now() + grace;
    loop {
        match target.observe_exit() {
            Ok(true) => return Ok(()),
            Ok(false) if Instant::now() < deadline => std::thread::sleep(POLL_INTERVAL),
            Ok(false) => {
                return Err(fatal_error(
                    fatal,
                    format!(
                        "{label}: process-group SIGKILL did not produce an observable direct-child exit {} within {} ms",
                        target.id(),
                        grace.as_millis()
                    ),
                ));
            }
            Err(error) => {
                return Err(fatal_error(
                    fatal,
                    format!("{label}: direct-child exit observation failed after SIGKILL: {error}"),
                ));
            }
        }
    }
}

fn force_exit_observed(
    target: &mut dyn ReapTarget,
    label: &str,
    grace: Duration,
    fatal: &dyn FatalContainment,
) -> Result<GroupTerminationProof, ProcessSpawnError> {
    let mut proof = GroupTerminationProof::default();
    if let Err(error) = target.signal_group_kill() {
        proof.record(label, "before exit observation", error);
        let _ = target.signal_direct_kill();
    }
    wait_for_exit_observation(target, label, grace, fatal)?;
    if let Err(error) = target.signal_group_kill() {
        proof.record(label, "while the zombie leader pinned the group", error);
    }
    Ok(proof)
}

fn contain_group_errors(
    proof: GroupTerminationProof,
    fatal: &dyn FatalContainment,
) -> Result<(), ProcessSpawnError> {
    if proof.errors.is_empty() {
        Ok(())
    } else {
        Err(fatal_error(fatal, proof.errors.join("; ")))
    }
}

fn reap_observed_target(
    target: &mut dyn ReapTarget,
    label: &str,
    grace: Duration,
    fatal: &dyn FatalContainment,
) -> Result<ExitStatus, ProcessSpawnError> {
    let deadline = Instant::now() + grace;
    loop {
        match target.reap_observed() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL_INTERVAL),
            Ok(None) => {
                return Err(fatal_error(
                    fatal,
                    format!(
                        "{label}: observed direct child {} was not reaped within {} ms",
                        target.id(),
                        grace.as_millis()
                    ),
                ));
            }
            Err(error) => {
                return Err(fatal_error(
                    fatal,
                    format!("{label}: final direct-child reap failed: {error}"),
                ));
            }
        }
    }
}

fn terminate_target_with(
    target: &mut dyn ReapTarget,
    label: &str,
    grace: Duration,
    fatal: &dyn FatalContainment,
) -> Result<(), ProcessSpawnError> {
    let proof = force_exit_observed(target, label, grace, fatal)?;
    let _ = reap_observed_target(target, label, grace, fatal)?;
    contain_group_errors(proof, fatal)
}

pub(crate) fn configure_process_group(command: &mut Command) -> &mut Command {
    command.process_group(0)
}

pub(crate) fn terminate_group_and_reap(
    child: &mut Child,
    label: &str,
) -> Result<(), ProcessSpawnError> {
    terminate_target_with(child, label, REAP_GRACE, &ExitController)
}

pub(crate) fn wait_grouped_child(
    child: &mut Child,
    label: &str,
    timeout: Duration,
) -> Result<ExitStatus, ProcessSpawnError> {
    let deadline = Instant::now() + timeout;
    loop {
        match ReapTarget::observe_exit(child) {
            Ok(true) => {
                let mut proof = GroupTerminationProof::default();
                if let Err(error) = ReapTarget::signal_group_kill(child) {
                    proof.record(label, "after normal exit observation", error);
                }
                let status = reap_observed_target(child, label, REAP_GRACE, &ExitController)?;
                contain_group_errors(proof, &ExitController)?;
                return Ok(status);
            }
            Ok(false) if Instant::now() < deadline => std::thread::sleep(POLL_INTERVAL),
            Ok(false) => {
                terminate_group_and_reap(child, label)?;
                return Err(ProcessSpawnError::ExecutionTimeout { command: label.to_string(), timeout });
            }
            Err(error) => {
                return Err(fatal_error(
                    &ExitController,
                    format!("observe {label} without reaping: {error}"),
                ));
            }
        }
    }
}

struct CaptureWorker {
    stream: &'static str,
    receiver: Receiver<Result<Vec<u8>, String>>,
    join: Option<JoinHandle<()>>,
}

type CapturedBytes = Result<Vec<u8>, String>;

impl CaptureWorker {
    fn spawn<R>(stream: &'static str, mut reader: R, limit: usize) -> io::Result<Self>
    where
        R: Read + Send + 'static,
    {
        let (sender, receiver) = mpsc::sync_channel(1);
        let join = std::thread::Builder::new()
            .name(format!("ramshared-{stream}-capture"))
            .spawn(move || {
                let mut output = Vec::new();
                let mut buffer = [0_u8; 4096];
                let result = loop {
                    match reader.read(&mut buffer) {
                        Ok(0) => break Ok(output),
                        Ok(read) if output.len().saturating_add(read) <= limit => {
                            output.extend_from_slice(&buffer[..read]);
                        }
                        Ok(_) => break Err(format!("{stream} output exceeded {limit} bytes")),
                        Err(error) => break Err(format!("{stream} capture failed: {error}")),
                    }
                };
                let _ = sender.send(result);
            })?;
        Ok(Self {
            stream,
            receiver,
            join: Some(join),
        })
    }

    fn receive_until(&self, deadline: Instant) -> Result<Result<Vec<u8>, String>, ()> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        self.receiver.recv_timeout(remaining).map_err(|_| ())
    }

    fn join(&mut self) -> Result<(), ProcessSpawnError> {
        self.join
            .take()
            .ok_or_else(|| ProcessSpawnError::new("capture worker joined twice"))?
            .join()
            .map_err(|_| {
                ProcessSpawnError::new(format!("{} capture worker panicked", self.stream))
            })
    }
}

fn account_single_capture_before_reap(
    group_id: u32,
    label: &str,
    capture: &mut CaptureWorker,
    fatal: &dyn FatalContainment,
) -> Result<(), ProcessSpawnError> {
    let deadline = Instant::now() + CAPTURE_CLOSE_GRACE;
    if capture.receive_until(deadline).is_err() {
        signal_owned_process_group(group_id, label, fatal)?;
        if capture
            .receive_until(Instant::now() + CAPTURE_CLOSE_GRACE)
            .is_err()
        {
            return Err(fatal_error(
                fatal,
                format!(
                    "{label}: capture worker remained blocked after exact process-group SIGKILL"
                ),
            ));
        }
    }
    capture.join()
}

fn signal_owned_process_group(
    group_id: u32,
    label: &str,
    fatal: &dyn FatalContainment,
) -> Result<(), ProcessSpawnError> {
    let raw = i32::try_from(group_id).map_err(|_| {
        fatal_error(
            fatal,
            format!("{label}: process-group ID overflow before direct-child reap"),
        )
    })?;
    let group = Pid::from_raw(raw).ok_or_else(|| {
        fatal_error(
            fatal,
            format!("{label}: zero process-group ID before direct-child reap"),
        )
    })?;
    match kill_process_group(group, Signal::KILL) {
        Ok(()) | Err(Errno::SRCH) => Ok(()),
        Err(error) => Err(fatal_error(
            fatal,
            format!(
                "{label}: output remained open but exact process-group SIGKILL failed: {error}"
            ),
        )),
    }
}

fn collect_captures_before_reap(
    group_id: u32,
    label: &str,
    stdout: &mut CaptureWorker,
    stderr: &mut CaptureWorker,
    fatal: &dyn FatalContainment,
    initial: Option<(Option<CapturedBytes>, Option<CapturedBytes>)>,
) -> Result<(Vec<u8>, Vec<u8>), ProcessSpawnError> {
    let (mut stdout_result, mut stderr_result) = initial.unwrap_or_else(|| {
        let first_deadline = Instant::now() + CAPTURE_CLOSE_GRACE;
        (
            stdout.receive_until(first_deadline).ok(),
            stderr.receive_until(first_deadline).ok(),
        )
    });
    if stdout_result.is_none() || stderr_result.is_none() {
        signal_owned_process_group(group_id, label, fatal)?;
        let final_deadline = Instant::now() + CAPTURE_CLOSE_GRACE;
        if stdout_result.is_none() {
            stdout_result = stdout.receive_until(final_deadline).ok();
        }
        if stderr_result.is_none() {
            stderr_result = stderr.receive_until(final_deadline).ok();
        }
        if stdout_result.is_none() || stderr_result.is_none() {
            return Err(fatal_error(
                fatal,
                format!(
                    "{label}: capture worker remained blocked after exact process-group SIGKILL"
                ),
            ));
        }
        stdout.join()?;
        stderr.join()?;
        return Err(ProcessSpawnError::new(format!(
            "{label}: output pipe remained open after direct-child exit; owned process group was killed"
        )));
    }

    stdout.join()?;
    stderr.join()?;
    let Some(stdout) = stdout_result else {
        return Err(fatal_error(
            fatal,
            format!("{label}: stdout capture result disappeared after worker join"),
        ));
    };
    let Some(stderr) = stderr_result else {
        return Err(fatal_error(
            fatal,
            format!("{label}: stderr capture result disappeared after worker join"),
        ));
    };
    let stdout = stdout.map_err(ProcessSpawnError::new)?;
    let stderr = stderr.map_err(ProcessSpawnError::new)?;
    Ok((stdout, stderr))
}

struct SpawnedChildGuard {
    child: Child,
    label: String,
    armed: bool,
}

impl SpawnedChildGuard {
    fn new(child: Child, label: &str) -> Self {
        Self {
            child,
            label: label.to_string(),
            armed: true,
        }
    }

    fn child_mut(&mut self) -> &mut Child {
        &mut self.child
    }

    fn id(&self) -> u32 {
        self.child.id()
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for SpawnedChildGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ =
                terminate_target_with(&mut self.child, &self.label, REAP_GRACE, &ExitController);
        }
    }
}

pub(crate) fn run_capture_command<F>(
    command: &mut Command,
    label: &str,
    timeout: Duration,
    output_limit: usize,
    on_spawn: F,
) -> Result<BoundedOutput, ProcessSpawnError>
where
    F: FnOnce(u32),
{
    let program = command.get_program();
    if program.is_empty() {
        return Err(ProcessSpawnError::spawn(
            label,
            io::Error::new(io::ErrorKind::InvalidInput, "empty program"),
        ));
    }
    if program.as_bytes().contains(&0) {
        return Err(ProcessSpawnError::spawn(
            label,
            io::Error::new(io::ErrorKind::InvalidInput, "null byte in program"),
        ));
    }
    let program_path = std::path::Path::new(program);
    if program_path.is_absolute() && !program_path.exists() {
        return Err(ProcessSpawnError::spawn(
            label,
            io::Error::new(io::ErrorKind::NotFound, "binary path does not exist"),
        ));
    }
    for arg in command.get_args() {
        if arg.as_bytes().contains(&0) {
            return Err(ProcessSpawnError::spawn(
                label,
                io::Error::new(io::ErrorKind::InvalidInput, "nul byte"),
            ));
        }
    }

    configure_process_group(command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = command
        .spawn()
        .map_err(|error| ProcessSpawnError::spawn(label, error))?;
    let mut child = SpawnedChildGuard::new(child, label);
    let group_id = child.id();
    on_spawn(group_id);
    let stdout = match child.child_mut().stdout.take() {
        Some(stdout) => stdout,
        None => {
            terminate_group_and_reap(child.child_mut(), label)?;
            child.disarm();
            return Err(ProcessSpawnError::new(format!(
                "{label}: stdout pipe was unavailable"
            )));
        }
    };
    let stderr = match child.child_mut().stderr.take() {
        Some(stderr) => stderr,
        None => {
            drop(stdout);
            terminate_group_and_reap(child.child_mut(), label)?;
            child.disarm();
            return Err(ProcessSpawnError::new(format!(
                "{label}: stderr pipe was unavailable"
            )));
        }
    };
    let mut stdout = match CaptureWorker::spawn("stdout", stdout, output_limit) {
        Ok(worker) => worker,
        Err(error) => {
            drop(stderr);
            terminate_group_and_reap(child.child_mut(), label)?;
            child.disarm();
            return Err(ProcessSpawnError::new(format!(
                "{label}: start stdout capture worker: {error}"
            )));
        }
    };
    let mut stderr = match CaptureWorker::spawn("stderr", stderr, output_limit) {
        Ok(worker) => worker,
        Err(error) => {
            let proof = force_exit_observed(child.child_mut(), label, REAP_GRACE, &ExitController)?;
            account_single_capture_before_reap(group_id, label, &mut stdout, &ExitController)?;
            let _ = reap_observed_target(child.child_mut(), label, REAP_GRACE, &ExitController)?;
            child.disarm();
            contain_group_errors(proof, &ExitController)?;
            return Err(ProcessSpawnError::new(format!(
                "{label}: start stderr capture worker: {error}"
            )));
        }
    };
    let deadline = Instant::now() + timeout;
    let (completion_error, proof, initial_capture) = loop {
        match ReapTarget::observe_exit(child.child_mut()) {
            Ok(true) => {
                // Before stopping residual descendants, distinguish a pipe
                // that closed naturally from one still inherited after the
                // leader exited. Only the former can produce trusted output.
                let capture_deadline = Instant::now() + CAPTURE_CLOSE_GRACE;
                let initial_capture = (
                    stdout.receive_until(capture_deadline).ok(),
                    stderr.receive_until(capture_deadline).ok(),
                );
                let mut proof = GroupTerminationProof::default();
                if let Err(error) = ReapTarget::signal_group_kill(child.child_mut()) {
                    proof.record(label, "after normal exit observation", error);
                }
                break (None, proof, Some(initial_capture));
            }
            Ok(false) if Instant::now() < deadline => std::thread::sleep(POLL_INTERVAL),
            Ok(false) => {
                let proof =
                    force_exit_observed(child.child_mut(), label, REAP_GRACE, &ExitController)?;
                break (
                    Some(ProcessSpawnError::ExecutionTimeout { command: label.to_string(), timeout }),
                    proof,
                    None,
                );
            }
            Err(error) => {
                return Err(fatal_error(
                    &ExitController,
                    format!("observe {label} without reaping: {error}"),
                ));
            }
        }
    };
    let capture = collect_captures_before_reap(
        group_id,
        label,
        &mut stdout,
        &mut stderr,
        &ExitController,
        initial_capture,
    );
    let status = reap_observed_target(child.child_mut(), label, REAP_GRACE, &ExitController)?;
    child.disarm();
    contain_group_errors(proof, &ExitController)?;
    if let Some(error) = completion_error {
        return match capture {
            Err(capture_error) if capture_error.is_fatal() => Err(capture_error),
            Ok(_) | Err(_) => Err(error),
        };
    }
    let (stdout, stderr) = capture?;
    if !status.success() {
        if let Some(exit_code) = status.code() {
            let stderr_str = String::from_utf8_lossy(&stderr).trim().to_string();
            return Err(ProcessSpawnError::NonZeroExit {
                command: label.to_string(),
                exit_code,
                stderr: stderr_str,
            });
        } else {
            return Err(ProcessSpawnError::new(format!(
                "{label} terminated by signal"
            )));
        }
    }
    Ok(BoundedOutput {
        status,
        stdout,
        stderr,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::io::Cursor;
    use std::os::unix::process::ExitStatusExt;
    use std::path::Path;

    fn fixture_process_exists(pid: u32) -> bool {
        Path::new(&format!("/proc/{pid}")).exists()
    }

    fn wait_for_fixture_process_exit(pid: u32) -> bool {
        let deadline = Instant::now() + Duration::from_secs(1);
        while fixture_process_exists(pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        !fixture_process_exists(pid)
    }

    fn kill_exact_fixture_process(pid: u32) {
        let Some(pid) = Pid::from_raw(pid as i32) else {
            return;
        };
        let _ = rustix::process::kill_process(pid, Signal::KILL);
    }

    fn kill_and_reap_exact_fixture_child(pid: u32) {
        let Some(pid) = Pid::from_raw(pid as i32) else {
            return;
        };
        let _ = kill_process_group(pid, Signal::KILL);
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::NOHANG) {
                Ok(Some(_)) | Err(Errno::CHILD) => break,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Ok(None) | Err(_) => break,
            }
        }
    }

    struct NeverReaped {
        id: u32,
        group_kills: Cell<usize>,
        direct_kills: Cell<usize>,
    }

    struct ScriptedTarget {
        id: u32,
        observations: VecDeque<io::Result<bool>>,
        reap_error: Option<io::Error>,
        group_errno: Option<i32>,
        group_kills: usize,
        direct_kills: usize,
    }

    impl ScriptedTarget {
        fn new(observations: Vec<io::Result<bool>>, group_errno: Option<i32>) -> Self {
            Self {
                id: 43,
                observations: observations.into(),
                reap_error: None,
                group_errno,
                group_kills: 0,
                direct_kills: 0,
            }
        }

        fn with_reap_error(mut self, error: io::Error) -> Self {
            self.reap_error = Some(error);
            self
        }
    }

    impl ReapTarget for ScriptedTarget {
        fn id(&self) -> u32 {
            self.id
        }

        fn signal_group_kill(&mut self) -> io::Result<()> {
            self.group_kills += 1;
            match self.group_errno {
                Some(errno) => Err(io::Error::from_raw_os_error(errno)),
                None => Ok(()),
            }
        }

        fn signal_direct_kill(&mut self) -> io::Result<()> {
            self.direct_kills += 1;
            Ok(())
        }

        fn observe_exit(&mut self) -> io::Result<bool> {
            self.observations.pop_front().unwrap_or(Ok(true))
        }

        fn reap_observed(&mut self) -> io::Result<Option<ExitStatus>> {
            match self.reap_error.take() {
                Some(error) => Err(error),
                None => Ok(Some(ExitStatus::from_raw(0))),
            }
        }
    }

    struct FailingReader;

    impl Read for FailingReader {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("fixture read failure"))
        }
    }

    struct PanickingReader;

    impl Read for PanickingReader {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            panic!("fixture capture panic")
        }
    }

    impl ReapTarget for NeverReaped {
        fn id(&self) -> u32 {
            self.id
        }

        fn signal_group_kill(&mut self) -> io::Result<()> {
            self.group_kills.set(self.group_kills.get() + 1);
            Ok(())
        }

        fn signal_direct_kill(&mut self) -> io::Result<()> {
            self.direct_kills.set(self.direct_kills.get() + 1);
            Ok(())
        }

        fn observe_exit(&mut self) -> io::Result<bool> {
            Ok(false)
        }

        fn reap_observed(&mut self) -> io::Result<Option<ExitStatus>> {
            Ok(None)
        }
    }

    #[derive(Default)]
    struct RecordingFatal(RefCell<Vec<String>>);

    impl FatalContainment for RecordingFatal {
        fn contain(&self, detail: &str) {
            self.0.borrow_mut().push(detail.to_string());
        }
    }

    #[test]
    // TestName: unreaped_group_selects_fatal_controller_containment
    fn unreaped_group_selects_fatal_controller_containment() {
        let mut target = NeverReaped {
            id: 42,
            group_kills: Cell::new(0),
            direct_kills: Cell::new(0),
        };
        let fatal = RecordingFatal::default();
        let error = terminate_target_with(&mut target, "fixture", Duration::ZERO, &fatal)
            .expect_err("unreaped SIGKILL must select fatal containment");

        assert_eq!(target.group_kills.get(), 1);
        assert_eq!(target.direct_kills.get(), 0);
        assert_eq!(fatal.0.borrow().len(), 1);
        assert!(fatal.0.borrow()[0].contains("observable direct-child exit"));
        assert!(error.to_string().contains("fatal containment selected"));
    }

    #[test]
    fn reap_policy_covers_preinspection_races_and_signal_failures() {
        let fatal = RecordingFatal::default();
        let mut already_observed = ScriptedTarget::new(vec![Ok(true)], None);
        terminate_target_with(
            &mut already_observed,
            "already observed",
            Duration::ZERO,
            &fatal,
        )
        .expect("an observed zombie pins the exact group until the residual kill");
        assert_eq!(already_observed.group_kills, 2);

        let mut observation_failed = ScriptedTarget::new(
            vec![Err(io::Error::other("fixture observation failure"))],
            None,
        );
        let error = terminate_target_with(
            &mut observation_failed,
            "observation failure",
            Duration::ZERO,
            &fatal,
        )
        .expect_err("an unprovable exit observation must select containment");
        assert!(error.to_string().contains("exit observation failed"));

        let mut esrch_race = ScriptedTarget::new(vec![Ok(true)], Some(Errno::SRCH.raw_os_error()));
        terminate_target_with(&mut esrch_race, "ESRCH race", Duration::ZERO, &fatal)
            .expect("ESRCH is safe only when the direct child is then proven reaped");
        assert_eq!(esrch_race.group_kills, 2);
        assert_eq!(esrch_race.direct_kills, 1);

        let mut group_failed = ScriptedTarget::new(vec![Ok(true)], Some(5));
        let error =
            terminate_target_with(&mut group_failed, "group failure", Duration::ZERO, &fatal)
                .expect_err("a group signal failure remains fatal after direct-child reap");
        assert!(error.to_string().contains("exact process-group SIGKILL"));
        assert_eq!(group_failed.direct_kills, 1);

        let mut reap_failed = ScriptedTarget::new(vec![Ok(true)], None)
            .with_reap_error(io::Error::other("fixture reap failure"));
        let error = terminate_target_with(&mut reap_failed, "reap failure", Duration::ZERO, &fatal)
            .expect_err("a failed post-signal reap proof must select containment");
        assert!(error.to_string().contains("final direct-child reap failed"));
        assert_eq!(fatal.0.borrow().len(), 3);
    }

    #[test]
    fn capture_workers_surface_reader_panics_errors_and_double_join() {
        let mut failed = CaptureWorker::spawn("fixture", FailingReader, 16)
            .expect("the capture worker must start");
        let result = failed
            .receive_until(Instant::now() + Duration::from_secs(1))
            .expect("the reader failure must be delivered");
        assert!(
            result
                .expect_err("the reader must fail")
                .contains("capture failed")
        );
        failed.join().expect("the failed reader thread still joins");
        let error = failed.join().expect_err("a capture worker joins only once");
        assert!(error.to_string().contains("joined twice"));

        let mut panicked = CaptureWorker::spawn("fixture", PanickingReader, 16)
            .expect("the panic fixture worker must start");
        assert!(
            panicked
                .receive_until(Instant::now() + Duration::from_secs(1))
                .is_err(),
            "a panicked capture worker disconnects its result channel"
        );
        let error = panicked
            .join()
            .expect_err("the capture panic must remain visible");
        assert!(error.to_string().contains("capture worker panicked"));
    }

    #[test]
    fn capture_cleanup_and_group_identifier_failures_are_typed() {
        let fatal = RecordingFatal::default();
        let mut closed = CaptureWorker::spawn("fixture", Cursor::new(b"closed".to_vec()), 16)
            .expect("the closed capture worker must start");
        account_single_capture_before_reap(u32::MAX, "closed", &mut closed, &fatal)
            .expect("a closed capture pipe needs no process-group signal");

        let error = signal_owned_process_group(u32::MAX, "overflow", &fatal)
            .expect_err("an overflowing process-group ID must select containment");
        assert!(error.to_string().contains("ID overflow"));
        let error = signal_owned_process_group(0, "zero", &fatal)
            .expect_err("a zero process-group ID must select containment");
        assert!(error.to_string().contains("zero process-group ID"));
    }

    #[test]
    fn direct_child_kill_fallback_targets_only_the_owned_fixture() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "exec sleep 10"]);
        configure_process_group(&mut command);
        let mut child = command
            .spawn()
            .expect("the direct fixture child must start");
        ReapTarget::signal_direct_kill(&mut child)
            .expect("the fallback must signal the exact direct child");
        let status = child.wait().expect("the exact child must be reaped");
        assert!(!status.success());
    }

    #[test]
    fn capture_runner_keeps_legitimate_success_and_nonzero_status_typed() {
        let mut success = Command::new("/bin/sh");
        success.args(["-c", "printf 'ok'; printf 'note' >&2"]);
        let output = run_capture_command(
            &mut success,
            "success fixture",
            Duration::from_secs(1),
            DEFAULT_OUTPUT_LIMIT,
            |_| {},
        )
        .expect("legitimate child must succeed");
        assert!(output.status.success());
        assert_eq!(output.stdout, b"ok");
        assert_eq!(output.stderr, b"note");

        let mut failure = Command::new("/bin/sh");
        failure.args(["-c", "printf 'failed' >&2; exit 7"]);
        let error = run_capture_command(
            &mut failure,
            "failure fixture",
            Duration::from_secs(1),
            DEFAULT_OUTPUT_LIMIT,
            |_| {},
        )
        .expect_err("nonzero status must return NonZeroExit");

        match error {
            ProcessSpawnError::NonZeroExit { command: _, exit_code, stderr } => {
                assert_eq!(exit_code, 7);
                assert_eq!(stderr, "failed");
            }
            _ => panic!("Expected NonZeroExit"),
        }
    }

    #[test]
    fn capture_runner_rejects_bounded_output_overflow() {
        let mut command = Command::new("head");
        command.args(["-c", "1025", "/dev/zero"]);
        let error = run_capture_command(
            &mut command,
            "overflow fixture",
            Duration::from_secs(1),
            1024,
            |_| {},
        )
        .expect_err("capture storage must remain bounded");
        assert!(error.to_string().contains("output exceeded"), "{error}");
    }

    #[test]
    fn capture_runner_rejects_empty_program() {
        let mut command = Command::new("");
        let error = run_capture_command(
            &mut command,
            "empty program fixture",
            Duration::from_secs(1),
            1024,
            |_| {},
        ).expect_err("should reject empty program");
        assert!(error.to_string().contains("empty program"));
    }

    #[test]
    fn capture_runner_rejects_non_existent_binary() {
        let mut command = Command::new("/path/to/nowhere/does/not/exist");
        let error = run_capture_command(
            &mut command,
            "non existent fixture",
            Duration::from_secs(1),
            1024,
            |_| {},
        ).expect_err("should reject non existent binary");
        assert!(error.to_string().contains("binary not found") || error.to_string().contains("binary path does not exist"));
    }

    #[test]
    fn capture_runner_rejects_null_bytes_in_args() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let mut command = Command::new("/bin/sh");
        let arg = OsStr::from_bytes(b"hello\0world");
        command.arg(arg);
        let error = run_capture_command(
            &mut command,
            "null bytes fixture",
            Duration::from_secs(1),
            1024,
            |_| {},
        ).expect_err("should reject null bytes in arguments");
        assert!(error.to_string().contains("nul byte"));
    }

    #[test]
    fn capture_runner_rejects_empty_argument() {
        let mut command = Command::new("/bin/sh");
        command.arg("");
        let error = run_capture_command(
            &mut command,
            "empty argument fixture",
            Duration::from_secs(1),
            1024,
            |_| {},
        ).expect_err("should reject empty arguments");
        assert!(error.to_string().contains("empty argument"));
    }

    #[test]
    // TestName: capture_runner_reaps_successful_leader_and_all_stdio_redirected_descendant
    fn capture_runner_reaps_successful_leader_and_all_stdio_redirected_descendant() {
        let mut command = Command::new("/bin/sh");
        command.args([
            "-c",
            "sleep 10 </dev/null >/dev/null 2>&1 & printf '%s\\n' \"$!\"",
        ]);
        let output = run_capture_command(
            &mut command,
            "redirected descendant fixture",
            Duration::from_secs(1),
            DEFAULT_OUTPUT_LIMIT,
            |_| {},
        )
        .expect("a successful leader remains a legitimate success");
        let descendant = String::from_utf8(output.stdout)
            .expect("fixture PID output is UTF-8")
            .trim()
            .parse::<u32>()
            .expect("fixture prints its exact descendant PID");
        let gone = wait_for_fixture_process_exit(descendant);
        if !gone {
            kill_exact_fixture_process(descendant);
            let _ = wait_for_fixture_process_exit(descendant);
        }

        assert!(output.status.success());
        assert!(
            gone,
            "a descendant with every stdio stream redirected survived its successful leader"
        );
    }

    #[test]
    // TestName: capture_runner_on_spawn_panic_cannot_strand_owned_child
    fn capture_runner_on_spawn_panic_cannot_strand_owned_child() {
        let spawned = Cell::new(None);
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "exec sleep 10"]);
        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = run_capture_command(
                &mut command,
                "on-spawn panic fixture",
                Duration::from_secs(1),
                DEFAULT_OUTPUT_LIMIT,
                |pid| {
                    spawned.set(Some(pid));
                    panic!("injected on_spawn panic");
                },
            );
        }));
        let pid = spawned
            .get()
            .expect("fixture child was spawned before panic");
        let stranded = fixture_process_exists(pid);
        if stranded {
            kill_and_reap_exact_fixture_child(pid);
        }

        assert!(
            unwind.is_err(),
            "the injected panic must resume after cleanup"
        );
        assert!(!stranded, "on_spawn panic stranded its exact fixture child");
    }
}
