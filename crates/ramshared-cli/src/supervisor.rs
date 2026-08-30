//! Preventive control-plane pressure state machine.

use crate::bounded_process;
use crate::workload::{self, OwnerIdentity};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::ops::Deref;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CONTROL_REQUEST_MAX_AGE_MS: u64 = 15_000;
const SYSTEMCTL_IDENTITY_TIMEOUT: Duration = Duration::from_secs(1);
const EMERGENCY_TERM_GRACE_MS: u64 = 5_000;
const ACTION_ERROR_MAX_BYTES: usize = 1_024;
const ACTION_ERROR_TRUNCATION_MARKER: &str = " [truncated]";
static RUNTIME_FILE_NONCE: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
type RuntimeWriteHook = Box<dyn Fn(&str, &serde_json::Value) -> Result<(), String>>;

#[cfg(test)]
thread_local! {
    static RUNTIME_WRITE_HOOK: std::cell::RefCell<Option<RuntimeWriteHook>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn invoke_runtime_write_hook(name: &str, value: &serde_json::Value) -> Result<(), String> {
    RUNTIME_WRITE_HOOK.with(|hook| match hook.borrow().as_ref() {
        Some(hook) => hook(name, value),
        None => Ok(()),
    })
}

#[cfg(not(test))]
fn invoke_runtime_write_hook(_name: &str, _value: &serde_json::Value) -> Result<(), String> {
    Ok(())
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SupervisorState {
    Healthy,
    Guarded,
    Critical,
    Emergency,
}

impl SupervisorState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "HEALTHY",
            Self::Guarded => "GUARDED",
            Self::Critical => "CRITICAL",
            Self::Emergency => "EMERGENCY",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PressureSample {
    pub memory_available_bytes: u64,
    pub control_reserve_bytes: u64,
    pub psi_full_avg10: f64,
    pub sample_delay_ms: u64,
    pub elapsed_ms: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorAction {
    CloseAdmission,
    ReduceVramCache,
    FreezeDiscardable,
    ThawDiscardable,
    RequestReclaim,
    TerminateDiscardable,
    KillDiscardable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorActionStatus {
    Succeeded,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SupervisorActionError(String);

impl SupervisorActionError {
    fn new(raw: String) -> Self {
        let content_limit = ACTION_ERROR_MAX_BYTES - ACTION_ERROR_TRUNCATION_MARKER.len();
        let mut sanitized = String::with_capacity(raw.len().min(ACTION_ERROR_MAX_BYTES));
        let mut truncated = false;
        for character in raw.chars() {
            let character = if character.is_control() {
                ' '
            } else {
                character
            };
            if sanitized.len() + character.len_utf8() > content_limit {
                truncated = true;
                break;
            }
            sanitized.push(character);
        }
        if truncated {
            sanitized.push_str(ACTION_ERROR_TRUNCATION_MARKER);
        }
        Self(sanitized)
    }

    fn is_canonical(value: &str) -> bool {
        value.len() <= ACTION_ERROR_MAX_BYTES
            && !value.chars().any(char::is_control)
            && Self::new(value.to_string()).0 == value
    }
}

impl Deref for SupervisorActionError {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for SupervisorActionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for SupervisorActionError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SupervisorActionError {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if Self::is_canonical(&value) {
            Ok(Self(value))
        } else {
            Err(serde::de::Error::custom(
                "supervisor action error is not canonical",
            ))
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SupervisorActionResult {
    pub action: SupervisorAction,
    pub status: SupervisorActionStatus,
    pub error: Option<SupervisorActionError>,
}

impl SupervisorActionResult {
    fn from_result(action: SupervisorAction, result: Result<(), String>) -> Self {
        match result {
            Ok(()) => Self {
                action,
                status: SupervisorActionStatus::Succeeded,
                error: None,
            },
            Err(error) => Self {
                action,
                status: SupervisorActionStatus::Failed,
                error: Some(SupervisorActionError::new(error)),
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SupervisorDecision {
    pub state: SupervisorState,
    pub actions: Vec<SupervisorAction>,
    pub healthy_samples: u32,
}

#[derive(Debug)]
pub struct Supervisor {
    state: SupervisorState,
    guarded_psi_samples: u32,
    critical_psi_samples: u32,
    emergency_psi_samples: u32,
    healthy_samples: u32,
    emergency_kill_eligible_ms: Option<u64>,
    emergency_kill_sent: bool,
    discardable_frozen: bool,
    discardable_freeze_pending: bool,
}

impl Default for Supervisor {
    fn default() -> Self {
        Self {
            state: SupervisorState::Healthy,
            guarded_psi_samples: 0,
            critical_psi_samples: 0,
            emergency_psi_samples: 0,
            healthy_samples: 0,
            emergency_kill_eligible_ms: None,
            emergency_kill_sent: false,
            discardable_frozen: false,
            discardable_freeze_pending: false,
        }
    }
}

impl Supervisor {
    pub fn observe(&mut self, sample: PressureSample) -> SupervisorDecision {
        self.guarded_psi_samples = streak(self.guarded_psi_samples, sample.psi_full_avg10 >= 2.0);
        self.critical_psi_samples = streak(self.critical_psi_samples, sample.psi_full_avg10 >= 5.0);
        self.emergency_psi_samples =
            streak(self.emergency_psi_samples, sample.psi_full_avg10 >= 10.0);

        let emergency = sample.memory_available_bytes
            < sample.control_reserve_bytes.saturating_div(2)
            || self.emergency_psi_samples >= 5
            || sample.sample_delay_ms >= 3_000;
        let critical = sample.memory_available_bytes
            < sample
                .control_reserve_bytes
                .saturating_mul(3)
                .saturating_div(4)
            || self.critical_psi_samples >= 5;
        let guarded = sample.memory_available_bytes < sample.control_reserve_bytes
            || self.guarded_psi_samples >= 3;
        let fully_healthy = sample.memory_available_bytes
            >= sample
                .control_reserve_bytes
                .saturating_add(1024 * 1024 * 1024)
            && sample.psi_full_avg10 < 1.0
            && sample.sample_delay_ms < 3_000;

        let requested = if emergency {
            SupervisorState::Emergency
        } else if critical {
            SupervisorState::Critical
        } else if guarded {
            SupervisorState::Guarded
        } else {
            SupervisorState::Healthy
        };

        let previous_state = self.state;
        if requested == SupervisorState::Healthy {
            self.healthy_samples = streak(self.healthy_samples, fully_healthy);
            if previous_state == SupervisorState::Healthy || self.healthy_samples >= 60 {
                self.state = SupervisorState::Healthy;
            } else {
                // Keep admission closed while the 60-second recovery window is
                // accumulating, but stop destructive actions immediately.
                self.state = SupervisorState::Guarded;
            }
        } else {
            self.healthy_samples = 0;
            self.state = requested;
        }

        if requested != SupervisorState::Emergency {
            self.emergency_kill_eligible_ms = None;
            self.emergency_kill_sent = false;
        }

        let mut actions = Vec::new();
        if (self.discardable_frozen || self.discardable_freeze_pending)
            && self.state != SupervisorState::Critical
        {
            actions.push(SupervisorAction::ThawDiscardable);
        }
        match self.state {
            SupervisorState::Healthy => {}
            SupervisorState::Guarded => actions.push(SupervisorAction::CloseAdmission),
            SupervisorState::Critical => {
                actions.extend([
                    SupervisorAction::CloseAdmission,
                    SupervisorAction::ReduceVramCache,
                ]);
                if !self.discardable_frozen {
                    actions.push(SupervisorAction::FreezeDiscardable);
                }
                actions.push(SupervisorAction::RequestReclaim);
            }
            SupervisorState::Emergency => {
                actions.extend([
                    SupervisorAction::CloseAdmission,
                    SupervisorAction::ReduceVramCache,
                    SupervisorAction::RequestReclaim,
                ]);
                if self
                    .emergency_kill_eligible_ms
                    .is_some_and(|eligible| sample.elapsed_ms >= eligible)
                {
                    if !self.emergency_kill_sent {
                        actions.push(SupervisorAction::KillDiscardable);
                    }
                } else if self.emergency_kill_eligible_ms.is_none() {
                    actions.push(SupervisorAction::TerminateDiscardable);
                }
            }
        }

        SupervisorDecision {
            state: self.state,
            actions,
            healthy_samples: self.healthy_samples,
        }
    }

    fn commit_action_results(
        &mut self,
        action_results: &[SupervisorActionResult],
        elapsed_ms: u64,
    ) {
        for result in action_results {
            if result.status != SupervisorActionStatus::Succeeded {
                continue;
            }
            match result.action {
                SupervisorAction::FreezeDiscardable => {
                    self.discardable_frozen = true;
                    self.discardable_freeze_pending = false;
                }
                SupervisorAction::ThawDiscardable => {
                    self.discardable_frozen = false;
                    self.discardable_freeze_pending = false;
                }
                SupervisorAction::TerminateDiscardable => {
                    self.emergency_kill_eligible_ms =
                        Some(elapsed_ms.saturating_add(EMERGENCY_TERM_GRACE_MS));
                }
                SupervisorAction::KillDiscardable => self.emergency_kill_sent = true,
                SupervisorAction::CloseAdmission
                | SupervisorAction::ReduceVramCache
                | SupervisorAction::RequestReclaim => {}
            }
        }
    }

    fn recover_action_state(
        &mut self,
        runtime: &Path,
        now_unix_ms: u64,
        elapsed_ms: u64,
    ) -> Result<(), String> {
        match read_frozen_target(runtime)? {
            Some((FrozenPhase::Pending, _)) => {
                self.discardable_frozen = false;
                self.discardable_freeze_pending = true;
            }
            Some((FrozenPhase::Applied, _)) => {
                self.discardable_frozen = true;
                self.discardable_freeze_pending = false;
            }
            None => {
                self.discardable_frozen = false;
                self.discardable_freeze_pending = false;
            }
        }
        match read_emergency_target(runtime)? {
            Some(record) if record.phase == EmergencyPhase::Termed => {
                let term_time = record
                    .term_succeeded_at_unix_ms
                    .ok_or("durable TERM record omitted its successful time")?;
                if term_time > now_unix_ms {
                    return Err("durable TERM record is from the future".into());
                }
                let age = now_unix_ms.saturating_sub(term_time);
                self.emergency_kill_eligible_ms =
                    Some(elapsed_ms.saturating_add(EMERGENCY_TERM_GRACE_MS.saturating_sub(age)));
                self.emergency_kill_sent = false;
            }
            Some(record) if record.phase == EmergencyPhase::Killed => {
                self.emergency_kill_eligible_ms = Some(elapsed_ms);
                self.emergency_kill_sent = true;
            }
            Some(_) | None => {
                self.emergency_kill_eligible_ms = None;
                self.emergency_kill_sent = false;
            }
        }
        Ok(())
    }
}

fn streak(current: u32, condition: bool) -> u32 {
    if condition {
        current.saturating_add(1)
    } else {
        0
    }
}

pub fn discard_priority(class: &str) -> Option<u8> {
    match class {
        "batch" => Some(0),
        "browser-test" => Some(1),
        "build" => Some(2),
        "interactive" => Some(3),
        _ => None,
    }
}

fn valid_scope_unit(unit: &str) -> bool {
    unit.starts_with("ramshared-")
        && unit.ends_with(".scope")
        && unit
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'@'))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct VictimIdentity {
    unit: String,
    invocation_id: String,
}

fn select_victim(path: &Path) -> Result<Option<VictimIdentity>, String> {
    let mut candidates = workload::read_reservation_ledger(path)?
        .into_iter()
        .filter_map(|reservation| {
            let priority = discard_priority(reservation.class.as_str())?;
            let invocation_id = reservation
                .invocation_id
                .filter(|value| workload::valid_systemd_invocation_id(value))?;
            valid_scope_unit(&reservation.unit).then_some((
                priority,
                std::cmp::Reverse(reservation.memory_bytes),
                reservation.issued_ordinal,
                reservation.unit,
                invocation_id,
            ))
        })
        .collect::<Vec<_>>();
    candidates.sort();
    Ok(candidates
        .into_iter()
        .next()
        .map(|(_, _, _, unit, invocation_id)| VictimIdentity {
            unit,
            invocation_id,
        }))
}

fn required_victim<'a>(
    victim: &'a Result<Option<VictimIdentity>, String>,
    missing_reason: &'static str,
) -> Result<&'a VictimIdentity, String> {
    match victim {
        Ok(Some(victim)) => Ok(victim),
        Ok(None) => Err(missing_reason.into()),
        Err(error) => Err(error.clone()),
    }
}

fn write_runtime_request_at(
    parent: &Path,
    name: &str,
    value: serde_json::Value,
) -> Result<(), String> {
    invoke_runtime_write_hook(name, &value)?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let path = parent.join(name);
    let nonce = RUNTIME_FILE_NONCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(".{name}.{}-{nonce}.tmp", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        file.write_all(format!("{value}\n").as_bytes())
            .map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        fs::rename(&temporary, &path).map_err(|error| error.to_string())?;
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn remove_runtime_record_at(parent: &Path, name: &str) -> Result<(), String> {
    fs::remove_file(parent.join(name)).map_err(|error| error.to_string())?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| error.to_string())
}

fn read_runtime_record_at<T>(parent: &Path, name: &str) -> Result<Option<T>, String>
where
    T: serde::de::DeserializeOwned,
{
    const MAX_RUNTIME_RECORD_BYTES: u64 = 16 * 1024;
    let path = parent.join(name);
    let opened = match rustix::fs::open(
        &path,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    ) {
        Ok(opened) => opened,
        Err(rustix::io::Errno::NOENT) => return Ok(None),
        Err(error) => return Err(format!("open {name} without following links: {error}")),
    };
    let mut file = File::from(opened);
    let before = file
        .metadata()
        .map_err(|error| format!("inspect {name}: {error}"))?;
    if !before.is_file()
        || before.nlink() != 1
        || before.uid() != rustix::process::geteuid().as_raw()
        || before.mode() & 0o022 != 0
        || before.len() > MAX_RUNTIME_RECORD_BYTES
    {
        return Err(format!("{name} has unsafe metadata"));
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_RUNTIME_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("read {name}: {error}"))?;
    if bytes.len() as u64 > MAX_RUNTIME_RECORD_BYTES {
        return Err(format!("{name} exceeds its bounded size"));
    }
    let after = file
        .metadata()
        .map_err(|error| format!("reinspect {name}: {error}"))?;
    let named =
        fs::symlink_metadata(&path).map_err(|error| format!("reinspect named {name}: {error}"))?;
    if !named.is_file()
        || before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.len() != after.len()
        || before.dev() != named.dev()
        || before.ino() != named.ino()
        || before.len() != named.len()
    {
        return Err(format!("{name} identity or size changed during read"));
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| format!("{name} is malformed"))
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum FrozenPhase {
    Pending,
    Applied,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct FrozenTargetRecord {
    schema_version: u32,
    phase: FrozenPhase,
    unit: String,
    invocation_id: String,
    updated_at_unix_ms: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum EmergencyPhase {
    PendingTerm,
    Termed,
    Killed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct EmergencyTargetRecord {
    schema_version: u32,
    phase: EmergencyPhase,
    unit: String,
    invocation_id: String,
    term_succeeded_at_unix_ms: Option<u64>,
    updated_at_unix_ms: u64,
}

fn validate_record_victim(unit: String, invocation_id: String) -> Result<VictimIdentity, String> {
    if !valid_scope_unit(&unit) || !workload::valid_systemd_invocation_id(&invocation_id) {
        return Err("durable scope target identity is invalid".into());
    }
    Ok(VictimIdentity {
        unit,
        invocation_id,
    })
}

fn read_frozen_target(runtime: &Path) -> Result<Option<(FrozenPhase, VictimIdentity)>, String> {
    let Some(record) = read_runtime_record_at::<FrozenTargetRecord>(runtime, "frozen-scope.json")?
    else {
        return Ok(None);
    };
    if record.schema_version != 3 || record.updated_at_unix_ms == 0 {
        return Err("frozen scope evidence has an unsupported schema or timestamp".into());
    }
    Ok(Some((
        record.phase,
        validate_record_victim(record.unit, record.invocation_id)?,
    )))
}

fn write_frozen_target(
    runtime: &Path,
    phase: FrozenPhase,
    victim: &VictimIdentity,
    now_ms: u64,
) -> Result<(), String> {
    write_runtime_request_at(
        runtime,
        "frozen-scope.json",
        serde_json::to_value(FrozenTargetRecord {
            schema_version: 3,
            phase,
            unit: victim.unit.clone(),
            invocation_id: victim.invocation_id.clone(),
            updated_at_unix_ms: now_ms,
        })
        .map_err(|error| error.to_string())?,
    )
}

fn read_emergency_target(runtime: &Path) -> Result<Option<EmergencyTargetRecord>, String> {
    let Some(record) =
        read_runtime_record_at::<EmergencyTargetRecord>(runtime, "emergency-target.json")?
    else {
        return Ok(None);
    };
    if record.schema_version != 1
        || record.updated_at_unix_ms == 0
        || validate_record_victim(record.unit.clone(), record.invocation_id.clone()).is_err()
        || matches!(record.phase, EmergencyPhase::PendingTerm)
            && record.term_succeeded_at_unix_ms.is_some()
        || matches!(
            record.phase,
            EmergencyPhase::Termed | EmergencyPhase::Killed
        ) && record.term_succeeded_at_unix_ms.is_none()
    {
        return Err("emergency target evidence is invalid".into());
    }
    Ok(Some(record))
}

fn emergency_record_victim(record: &EmergencyTargetRecord) -> Result<VictimIdentity, String> {
    validate_record_victim(record.unit.clone(), record.invocation_id.clone())
}

fn write_emergency_target(
    runtime: &Path,
    phase: EmergencyPhase,
    victim: &VictimIdentity,
    term_succeeded_at_unix_ms: Option<u64>,
    now_ms: u64,
) -> Result<(), String> {
    write_runtime_request_at(
        runtime,
        "emergency-target.json",
        serde_json::to_value(EmergencyTargetRecord {
            schema_version: 1,
            phase,
            unit: victim.unit.clone(),
            invocation_id: victim.invocation_id.clone(),
            term_succeeded_at_unix_ms,
            updated_at_unix_ms: now_ms,
        })
        .map_err(|error| error.to_string())?,
    )
}

fn unix_time_ms() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .map_err(|error| format!("system clock is before the Unix epoch: {error}"))
}

fn valid_daemon_instance_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn request_daemon_instance_at(runtime: &Path, now_ms: u64) -> Result<String, String> {
    let path = runtime.join("cache-status.json");
    let text = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| error.to_string())?;
    let schema_version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or("cache status schema is missing")?;
    let daemon_instance_id = value
        .get("daemon_instance_id")
        .and_then(serde_json::Value::as_str)
        .filter(|value| valid_daemon_instance_id(value))
        .ok_or("cache status daemon instance is missing")?;
    let written_at_ms = value
        .get("written_at_unix_ms")
        .and_then(serde_json::Value::as_u64)
        .ok_or("cache status freshness is missing")?;
    if schema_version != 1
        || now_ms < written_at_ms
        || now_ms.saturating_sub(written_at_ms) > CONTROL_REQUEST_MAX_AGE_MS
    {
        return Err("cache status is stale or invalid".into());
    }
    Ok(daemon_instance_id.to_string())
}

fn critical_request_value(
    daemon_instance_id: &str,
    issued_at_unix_ms: u64,
    target_bytes: Option<u64>,
) -> serde_json::Value {
    let mut request = serde_json::json!({
        "schema_version": 1,
        "reason": "control_pressure",
        "daemon_instance_id": daemon_instance_id,
        "issued_at_unix_ms": issued_at_unix_ms,
    });
    if let Some(target_bytes) = target_bytes {
        request["target_bytes"] = serde_json::json!(target_bytes);
    }
    request
}

fn run_systemctl_bounded(args: &[&str]) -> Result<(), String> {
    run_systemctl_bounded_for(Path::new("systemctl"), args, Duration::from_secs(1))
}

fn run_systemctl_bounded_for(
    command: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<(), String> {
    let mut command = Command::new(command);
    command.args(args);
    let output = bounded_process::run_capture_command(
        &mut command,
        "systemctl action",
        timeout,
        bounded_process::DEFAULT_OUTPUT_LIMIT,
        |_| {},
    )
    .map_err(|error| format!("systemctl action failed: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            Err(format!("systemctl exited with {}", output.status))
        } else {
            Err(format!("systemctl exited with {}: {stderr}", output.status))
        }
    }
}

trait UnitActionRunner {
    fn current_invocation_id(&self, unit: &str) -> Result<String, String>;
    fn run(&self, args: &[&str]) -> Result<(), String>;
}

struct SystemUnitActionRunner;

impl UnitActionRunner for SystemUnitActionRunner {
    fn current_invocation_id(&self, unit: &str) -> Result<String, String> {
        query_unit_invocation_id(unit)
    }

    fn run(&self, args: &[&str]) -> Result<(), String> {
        run_systemctl_bounded(args)
    }
}

fn parse_unit_invocation_id(unit: &str, output: &str) -> Result<String, String> {
    let mut id = None;
    let mut invocation_id = None;
    for line in output.lines() {
        let (name, value) = line
            .split_once('=')
            .ok_or("malformed systemd identity response")?;
        let target = match name {
            "Id" => &mut id,
            "InvocationID" => &mut invocation_id,
            _ => return Err("unexpected systemd identity field".into()),
        };
        if target.replace(value.to_string()).is_some() {
            return Err("duplicate systemd identity field".into());
        }
    }
    if id.as_deref() != Some(unit) {
        return Err("systemd unit identity changed".into());
    }
    invocation_id
        .filter(|value| workload::valid_systemd_invocation_id(value))
        .ok_or_else(|| "systemd InvocationID is missing or invalid".into())
}

fn query_unit_invocation_id(unit: &str) -> Result<String, String> {
    if !valid_scope_unit(unit) {
        return Err("invalid managed scope unit".into());
    }
    let mut command = Command::new("systemctl");
    command.args(["show", "--property=Id", "--property=InvocationID", unit]);
    let output = bounded_process::run_capture_command(
        &mut command,
        "systemd identity query",
        SYSTEMCTL_IDENTITY_TIMEOUT,
        bounded_process::DEFAULT_OUTPUT_LIMIT,
        |_| {},
    )
    .map_err(|error| format!("bounded systemd identity query failed: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "systemd identity query exited with {}",
            output.status
        ));
    }
    let output = String::from_utf8(output.stdout)
        .map_err(|error| format!("systemd identity query returned non-UTF-8 output: {error}"))?;
    parse_unit_invocation_id(unit, &output)
}

fn run_identity_bound_action(
    runner: &dyn UnitActionRunner,
    victim: &VictimIdentity,
    args: &[&str],
) -> Result<(), String> {
    let current = runner.current_invocation_id(&victim.unit)?;
    if current != victim.invocation_id {
        return Err("managed scope InvocationID changed; action refused".into());
    }
    runner.run(args)
}

struct ActionPaths<'a> {
    runtime: &'a Path,
    ledger: &'a Path,
    daemon_instance_id: Option<&'a str>,
    issued_at_unix_ms: Option<u64>,
}

fn execute_freeze(
    paths: &ActionPaths<'_>,
    selected: &Result<Option<VictimIdentity>, String>,
    runner: &dyn UnitActionRunner,
) -> Result<(), String> {
    let now_ms = paths
        .issued_at_unix_ms
        .ok_or("freeze evidence timestamp is unavailable")?;
    let existing = read_frozen_target(paths.runtime)?;
    if matches!(&existing, Some((FrozenPhase::Applied, _))) {
        return Ok(());
    }
    let victim = match existing {
        Some((FrozenPhase::Pending, victim)) => victim,
        Some((FrozenPhase::Applied, _)) => unreachable!("applied freeze returned above"),
        None => required_victim(selected, "no_discardable_scope")?.clone(),
    };
    write_frozen_target(paths.runtime, FrozenPhase::Pending, &victim, now_ms)?;
    run_identity_bound_action(runner, &victim, &["freeze", &victim.unit])?;
    write_frozen_target(paths.runtime, FrozenPhase::Applied, &victim, now_ms)
}

fn execute_thaw(paths: &ActionPaths<'_>, runner: &dyn UnitActionRunner) -> Result<(), String> {
    let Some((_, victim)) = read_frozen_target(paths.runtime)? else {
        return Err("no_frozen_scope".into());
    };
    run_identity_bound_action(runner, &victim, &["thaw", &victim.unit])?;
    remove_runtime_record_at(paths.runtime, "frozen-scope.json")
}

fn execute_term(
    paths: &ActionPaths<'_>,
    selected: &Result<Option<VictimIdentity>, String>,
    runner: &dyn UnitActionRunner,
) -> Result<(), String> {
    let now_ms = paths
        .issued_at_unix_ms
        .ok_or("TERM outcome timestamp is unavailable")?;
    let existing = read_emergency_target(paths.runtime)?;
    let victim = match existing.as_ref() {
        Some(record) if record.phase == EmergencyPhase::Termed => return Ok(()),
        Some(record) if record.phase == EmergencyPhase::Killed => {
            return Err("emergency target is already durably killed".into());
        }
        Some(record) => emergency_record_victim(record)?,
        None => required_victim(selected, "no_discardable_scope")?.clone(),
    };
    write_emergency_target(
        paths.runtime,
        EmergencyPhase::PendingTerm,
        &victim,
        None,
        now_ms,
    )?;
    run_identity_bound_action(
        runner,
        &victim,
        &["kill", "--kill-whom=all", "--signal=TERM", &victim.unit],
    )?;
    write_emergency_target(
        paths.runtime,
        EmergencyPhase::Termed,
        &victim,
        Some(now_ms),
        now_ms,
    )
}

fn execute_kill(paths: &ActionPaths<'_>, runner: &dyn UnitActionRunner) -> Result<(), String> {
    let now_ms = paths
        .issued_at_unix_ms
        .ok_or("KILL outcome timestamp is unavailable")?;
    let record = read_emergency_target(paths.runtime)?
        .ok_or("no successfully terminated emergency target")?;
    if record.phase == EmergencyPhase::Killed {
        return Ok(());
    }
    if record.phase != EmergencyPhase::Termed {
        return Err("emergency target has no successful TERM outcome".into());
    }
    let term_succeeded_at = record
        .term_succeeded_at_unix_ms
        .ok_or("emergency target TERM timestamp is unavailable")?;
    if now_ms < term_succeeded_at.saturating_add(EMERGENCY_TERM_GRACE_MS) {
        return Err("emergency target TERM grace has not elapsed".into());
    }
    let victim = emergency_record_victim(&record)?;
    run_identity_bound_action(
        runner,
        &victim,
        &["kill", "--kill-whom=all", "--signal=KILL", &victim.unit],
    )?;
    write_emergency_target(
        paths.runtime,
        EmergencyPhase::Killed,
        &victim,
        Some(term_succeeded_at),
        now_ms,
    )
}

fn execute_actions_with<F>(
    decision: &SupervisorDecision,
    paths: &ActionPaths<'_>,
    runner: &dyn UnitActionRunner,
    mut close_admission: F,
) -> Vec<SupervisorActionResult>
where
    F: FnMut() -> Result<(), String>,
{
    let victim = select_victim(paths.ledger);
    let mut outcomes = Vec::with_capacity(decision.actions.len());
    for action in &decision.actions {
        let result = match action {
            SupervisorAction::CloseAdmission => close_admission(),
            SupervisorAction::ReduceVramCache => paths
                .daemon_instance_id
                .zip(paths.issued_at_unix_ms)
                .ok_or_else(|| "daemon_instance_unavailable".to_string())
                .and_then(|(daemon_instance_id, issued_at_unix_ms)| {
                    write_runtime_request_at(
                        paths.runtime,
                        "cache-target.json",
                        critical_request_value(daemon_instance_id, issued_at_unix_ms, Some(0)),
                    )
                }),
            SupervisorAction::RequestReclaim => paths
                .daemon_instance_id
                .zip(paths.issued_at_unix_ms)
                .ok_or_else(|| "daemon_instance_unavailable".to_string())
                .and_then(|(daemon_instance_id, issued_at_unix_ms)| {
                    write_runtime_request_at(
                        paths.runtime,
                        "reclaim-request.json",
                        critical_request_value(daemon_instance_id, issued_at_unix_ms, None),
                    )
                }),
            SupervisorAction::FreezeDiscardable => execute_freeze(paths, &victim, runner),
            SupervisorAction::ThawDiscardable => execute_thaw(paths, runner),
            SupervisorAction::TerminateDiscardable => execute_term(paths, &victim, runner),
            SupervisorAction::KillDiscardable => execute_kill(paths, runner),
        };
        outcomes.push(SupervisorActionResult::from_result(*action, result));
    }
    outcomes
}

fn parse_meminfo(text: &str) -> Option<(u64, u64)> {
    let value = |name: &str| {
        text.lines().find_map(|line| {
            let (key, rest) = line.split_once(':')?;
            (key == name)
                .then(|| rest.split_whitespace().next()?.parse::<u64>().ok())
                .flatten()
        })
    };
    Some((value("MemTotal")? * 1024, value("MemAvailable")? * 1024))
}

fn parse_psi_full_avg10(text: &str) -> Option<f64> {
    text.lines()
        .find(|line| line.starts_with("full "))?
        .split_whitespace()
        .find_map(|field| field.strip_prefix("avg10=")?.parse().ok())
}

fn publish_at(
    path: &Path,
    decision: &SupervisorDecision,
    action_results: &[SupervisorActionResult],
    daemon_instance_id: Option<&str>,
    supervisor_identity: &OwnerIdentity,
    written_at_unix_ms: u64,
) -> Result<(), String> {
    let parent = path.parent().ok_or("supervisor state parent missing")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary = parent.join(format!(".supervisor-{}.tmp", std::process::id()));
    let value = serde_json::json!({
        "schema_version": 3,
        "control_state": decision.state,
        "healthy_samples": decision.healthy_samples,
        "action_results": action_results,
        "daemon_instance_id": daemon_instance_id,
        "supervisor_identity": supervisor_identity,
        "written_at_unix_ms": written_at_unix_ms,
    });
    fs::write(&temporary, format!("{value}\n")).map_err(|error| error.to_string())?;
    fs::rename(temporary, path).map_err(|error| error.to_string())
}

#[derive(Debug)]
struct DecisionApplyError {
    detail: String,
    action_results: Vec<SupervisorActionResult>,
}

impl fmt::Display for DecisionApplyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for DecisionApplyError {}

#[allow(clippy::too_many_arguments)]
fn apply_decision_with(
    state_path: &Path,
    ledger_root: &Path,
    decision: &SupervisorDecision,
    action_paths: &ActionPaths<'_>,
    runner: &dyn UnitActionRunner,
    supervisor_identity: &OwnerIdentity,
    written_at_unix_ms: u64,
) -> Result<Vec<SupervisorActionResult>, DecisionApplyError> {
    if decision.state == SupervisorState::Healthy {
        let action_results = execute_actions_with(decision, action_paths, runner, || {
            Err("CloseAdmission is invalid in a healthy decision".into())
        });
        publish_at(
            state_path,
            decision,
            &action_results,
            action_paths.daemon_instance_id,
            supervisor_identity,
            written_at_unix_ms,
        )
        .map_err(|detail| DecisionApplyError {
            detail,
            action_results: action_results.clone(),
        })?;
        workload::publish_admission_state_at(
            ledger_root,
            supervisor_identity,
            decision.state.as_str(),
            written_at_unix_ms,
        )
        .map_err(|detail| DecisionApplyError {
            detail,
            action_results: action_results.clone(),
        })?;
        return Ok(action_results);
    }

    // Close admission exactly when its action is executed. A refusal remains
    // that action's definitive typed result; later pressure actions still run
    // and every outcome is published in execution order.
    let action_results = execute_actions_with(decision, action_paths, runner, || {
        workload::publish_admission_state_at(
            ledger_root,
            supervisor_identity,
            decision.state.as_str(),
            written_at_unix_ms,
        )
    });
    let state_result = publish_at(
        state_path,
        decision,
        &action_results,
        action_paths.daemon_instance_id,
        supervisor_identity,
        written_at_unix_ms,
    );
    if let Err(state) = state_result {
        return Err(DecisionApplyError {
            detail: format!(
                "pressure actions ran but their definitive state publication failed: {state}"
            ),
            action_results,
        });
    }
    if let Some(close) = action_results.iter().find(|result| {
        result.action == SupervisorAction::CloseAdmission
            && result.status == SupervisorActionStatus::Failed
    }) {
        let detail = close
            .error
            .as_deref()
            .unwrap_or("canonical admission close failed without an error detail");
        return Err(DecisionApplyError {
            detail: format!(
                "admission close failed after pressure actions were recorded: {detail}"
            ),
            action_results,
        });
    }
    Ok(action_results)
}

fn collect_sample(start: Instant, previous: Instant) -> Result<PressureSample, String> {
    let meminfo = fs::read_to_string("/proc/meminfo").map_err(|error| error.to_string())?;
    let pressure =
        fs::read_to_string("/proc/pressure/memory").map_err(|error| error.to_string())?;
    let (total, available) = parse_meminfo(&meminfo).ok_or("invalid /proc/meminfo")?;
    Ok(PressureSample {
        memory_available_bytes: available,
        control_reserve_bytes: (total.div_ceil(4)).max(4 * 1024 * 1024 * 1024),
        psi_full_avg10: parse_psi_full_avg10(&pressure).ok_or("invalid memory PSI")?,
        sample_delay_ms: previous.elapsed().as_millis().saturating_sub(1_000) as u64,
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
}

pub fn run(args: &[String]) -> Result<(), String> {
    let once = match args {
        [] => false,
        [option] if option == "--once" => true,
        _ => return Err("usage: ramshared supervise [--once]".into()),
    };
    let start = Instant::now();
    let mut previous = Instant::now();
    let mut supervisor = Supervisor::default();
    let supervisor_identity = OwnerIdentity::current()?;
    loop {
        let sample = collect_sample(start, previous)?;
        previous = Instant::now();
        let written_at_unix_ms = unix_time_ms()?;
        supervisor.recover_action_state(
            Path::new("/run/ramshared"),
            written_at_unix_ms,
            sample.elapsed_ms,
        )?;
        let decision = supervisor.observe(sample);
        let daemon_instance_id =
            request_daemon_instance_at(Path::new("/run/ramshared"), written_at_unix_ms).ok();
        let application = apply_decision_with(
            Path::new("/run/ramshared/supervisor-state.json"),
            Path::new("/run/ramshared/admission"),
            &decision,
            &ActionPaths {
                runtime: Path::new("/run/ramshared"),
                ledger: Path::new("/run/ramshared/admission/reservations.json"),
                daemon_instance_id: daemon_instance_id.as_deref(),
                issued_at_unix_ms: Some(written_at_unix_ms),
            },
            &SystemUnitActionRunner,
            &supervisor_identity,
            written_at_unix_ms,
        );
        let (action_results, application_error) = match application {
            Ok(action_results) => (action_results, None),
            Err(error) => (error.action_results, Some(error.detail)),
        };
        supervisor.commit_action_results(&action_results, sample.elapsed_ms);
        if let Some(error) = application_error {
            return Err(error);
        }
        if once {
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use std::cell::RefCell;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const GIB: u64 = 1024 * 1024 * 1024;
    static TEMP: AtomicUsize = AtomicUsize::new(0);

    struct RuntimeWriteHookGuard;

    impl RuntimeWriteHookGuard {
        fn install(hook: RuntimeWriteHook) -> Self {
            RUNTIME_WRITE_HOOK.with(|slot| {
                assert!(
                    slot.borrow().is_none(),
                    "runtime write hook already installed"
                );
                *slot.borrow_mut() = Some(hook);
            });
            Self
        }
    }

    impl Drop for RuntimeWriteHookGuard {
        fn drop(&mut self) {
            RUNTIME_WRITE_HOOK.with(|slot| *slot.borrow_mut() = None);
        }
    }

    struct FakeUnitRunner {
        calls: RefCell<Vec<Vec<String>>>,
        fail: bool,
    }

    struct StaleIdentityRunner {
        calls: RefCell<Vec<Vec<String>>>,
        identity_queries: RefCell<usize>,
    }

    impl UnitActionRunner for FakeUnitRunner {
        fn current_invocation_id(&self, _unit: &str) -> Result<String, String> {
            Ok("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into())
        }

        fn run(&self, args: &[&str]) -> Result<(), String> {
            self.calls
                .borrow_mut()
                .push(args.iter().map(|value| (*value).to_string()).collect());
            if self.fail {
                Err("fixture systemctl refusal".into())
            } else {
                Ok(())
            }
        }
    }

    impl UnitActionRunner for StaleIdentityRunner {
        fn current_invocation_id(&self, _unit: &str) -> Result<String, String> {
            *self.identity_queries.borrow_mut() += 1;
            Ok("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into())
        }

        fn run(&self, args: &[&str]) -> Result<(), String> {
            self.calls
                .borrow_mut()
                .push(args.iter().map(|value| (*value).to_string()).collect());
            Ok(())
        }
    }

    fn fixture() -> std::path::PathBuf {
        let parent = if std::path::Path::new("/dev/shm").is_dir() {
            std::path::PathBuf::from("/dev/shm")
        } else {
            std::env::temp_dir()
        };
        for _ in 0..1024 {
            let number = TEMP.fetch_add(1, Ordering::SeqCst);
            let path = parent.join(format!(
                "ramshared-supervisor-{}-{number}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return path,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create isolated supervisor fixture: {error}"),
            }
        }
        panic!("exhausted isolated supervisor fixture names");
    }

    #[test]
    // TestName: supervisor_fixture_roots_are_unique_and_avoid_shared_disk_sync
    fn supervisor_fixture_roots_are_unique_and_avoid_shared_disk_sync() {
        let first = fixture();
        let second = fixture();
        assert_ne!(first, second);
        if std::path::Path::new("/dev/shm").is_dir() {
            assert_eq!(first.parent(), Some(std::path::Path::new("/dev/shm")));
            assert_eq!(second.parent(), Some(std::path::Path::new("/dev/shm")));
        }
        fs::remove_dir_all(first).unwrap();
        fs::remove_dir_all(second).unwrap();
    }

    fn run_isolated_guarded_transition_fixture() {
        let root = fixture();
        let state_path = root.join("supervisor-state.json");
        let ledger_root = root.join("admission");
        let identity = OwnerIdentity::current().unwrap();
        let decision = SupervisorDecision {
            state: SupervisorState::Guarded,
            actions: vec![SupervisorAction::CloseAdmission],
            healthy_samples: 0,
        };
        let runner = FakeUnitRunner {
            calls: RefCell::new(Vec::new()),
            fail: false,
        };
        let actions = apply_decision_with(
            &state_path,
            &ledger_root,
            &decision,
            &ActionPaths {
                runtime: &root.join("runtime"),
                ledger: &ledger_root.join("reservations.json"),
                daemon_instance_id: Some("fixture-daemon"),
                issued_at_unix_ms: Some(1_000),
            },
            &runner,
            &identity,
            1_000,
        )
        .unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action, SupervisorAction::CloseAdmission);
        assert_eq!(actions[0].status, SupervisorActionStatus::Succeeded);
        let gate: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(ledger_root.join("admission-state.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(gate["admission_open"], false);
        assert!(runner.calls.borrow().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    // TestName: supervisor_parallel_guarded_transitions_stay_durable_and_isolated
    fn supervisor_parallel_guarded_transitions_stay_durable_and_isolated() {
        std::thread::scope(|scope| {
            let workers = (0..8)
                .map(|_| scope.spawn(run_isolated_guarded_transition_fixture))
                .collect::<Vec<_>>();
            for worker in workers {
                worker.join().unwrap();
            }
        });
    }

    struct TestDir {
        path: std::path::PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            Self { path: fixture() }
        }

        fn program(&self, name: &str, source: &str) -> std::path::PathBuf {
            let path = self.path.join(name);
            fs::write(&path, source).unwrap();
            let mut permissions = fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(&path, permissions).unwrap();
            path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn ledger_fixture(rows: &[(&str, u64, &str)]) -> String {
        let reservations = rows
            .iter()
            .enumerate()
            .map(|(index, (class, memory_bytes, unit))| {
                serde_json::json!({
                    "id": format!("fixture-{index}"),
                    "owner": {
                        "boot_id": "fixture-boot",
                        "pid": index as u32 + 1,
                        "start_time": index as u64 + 1,
                        "nonce": format!("fixture-{index}"),
                    },
                    "class": class,
                    "memory_bytes": memory_bytes,
                    "unit": unit,
                    "invocation_id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "issued_ordinal": index as u64 + 1,
                })
            })
            .collect::<Vec<_>>();
        serde_json::json!({
            "schema_version": workload::RESERVATION_LEDGER_SCHEMA_VERSION,
            "next_ordinal": reservations.len() as u64 + 1,
            "reservations": reservations,
        })
        .to_string()
    }

    fn sample(
        available_gib: u64,
        reserve_gib: u64,
        psi: f64,
        delay: u64,
        time: u64,
    ) -> PressureSample {
        PressureSample {
            memory_available_bytes: available_gib * GIB,
            control_reserve_bytes: reserve_gib * GIB,
            psi_full_avg10: psi,
            sample_delay_ms: delay,
            elapsed_ms: time,
        }
    }

    #[test]
    fn supervisor_transitions_and_hysteresis_are_exact() {
        let mut supervisor = Supervisor::default();
        for index in 0..2 {
            assert_eq!(
                supervisor.observe(sample(6, 4, 2.0, 0, index * 1000)).state,
                SupervisorState::Healthy
            );
        }
        assert_eq!(
            supervisor.observe(sample(6, 4, 2.0, 0, 2000)).state,
            SupervisorState::Guarded
        );
        for index in 3..7 {
            supervisor.observe(sample(6, 4, 5.0, 0, index * 1000));
        }
        assert_eq!(
            supervisor.observe(sample(6, 4, 5.0, 0, 7000)).state,
            SupervisorState::Critical
        );
        for index in 8..12 {
            supervisor.observe(sample(6, 4, 10.0, 0, index * 1000));
        }
        assert_eq!(
            supervisor.observe(sample(6, 4, 10.0, 0, 12000)).state,
            SupervisorState::Emergency
        );
        for index in 0..59 {
            assert_eq!(
                supervisor
                    .observe(sample(6, 4, 0.0, 0, 13000 + index * 1000))
                    .state,
                SupervisorState::Guarded
            );
        }
        assert_eq!(
            supervisor.observe(sample(6, 4, 0.0, 0, 72000)).state,
            SupervisorState::Healthy
        );
    }

    #[test]
    fn supervisor_delay_enters_emergency() {
        let mut supervisor = Supervisor::default();
        let decision = supervisor.observe(sample(6, 4, 0.0, 3000, 0));
        assert_eq!(decision.state, SupervisorState::Emergency);
        assert!(
            decision
                .actions
                .contains(&SupervisorAction::ReduceVramCache)
        );
        assert!(decision.actions.contains(&SupervisorAction::RequestReclaim));
        assert!(
            decision
                .actions
                .contains(&SupervisorAction::TerminateDiscardable)
        );
        supervisor.commit_action_results(
            &[SupervisorActionResult::from_result(
                SupervisorAction::TerminateDiscardable,
                Ok(()),
            )],
            0,
        );
        let decision = supervisor.observe(sample(6, 4, 0.0, 3000, 5000));
        assert!(
            decision
                .actions
                .contains(&SupervisorAction::KillDiscardable)
        );
        assert!(
            decision
                .actions
                .contains(&SupervisorAction::ReduceVramCache)
        );
        assert!(decision.actions.contains(&SupervisorAction::RequestReclaim));
    }

    #[test]
    // TestName: supervisor_state_advances_only_from_successful_action_results
    fn supervisor_state_advances_only_from_successful_action_results() {
        let mut supervisor = Supervisor::default();
        let first = supervisor.observe(sample(1, 4, 0.0, 0, 0));
        assert!(
            first
                .actions
                .contains(&SupervisorAction::TerminateDiscardable)
        );

        let retry = supervisor.observe(sample(1, 4, 0.0, 0, 1_000));
        assert!(
            retry
                .actions
                .contains(&SupervisorAction::TerminateDiscardable),
            "an uncommitted TERM intent suppressed the required retry"
        );

        let mut freeze = Supervisor::default();
        let critical = freeze.observe(sample(2, 4, 0.0, 0, 0));
        assert!(
            critical
                .actions
                .contains(&SupervisorAction::FreezeDiscardable)
        );
        let emergency = freeze.observe(sample(1, 4, 0.0, 0, 1_000));
        assert!(
            !emergency
                .actions
                .contains(&SupervisorAction::ThawDiscardable),
            "an uncommitted freeze intent created a synthetic thaw"
        );
    }

    #[test]
    fn sustained_emergency_renews_zero_cache_and_reclaim_requests() {
        let root = fixture();
        let runtime = root.join("runtime");
        let ledger = root.join("reservations.json");
        let runner = FakeUnitRunner {
            calls: RefCell::new(Vec::new()),
            fail: false,
        };
        let mut supervisor = Supervisor::default();

        let first = supervisor.observe(sample(6, 4, 0.0, 3_000, 0));
        execute_actions_with(
            &first,
            &ActionPaths {
                runtime: &runtime,
                ledger: &ledger,
                daemon_instance_id: Some("fixture-daemon"),
                issued_at_unix_ms: Some(1_000),
            },
            &runner,
            || Ok(()),
        );

        let sustained = supervisor.observe(sample(6, 4, 0.0, 3_000, 5_000));
        assert!(
            sustained
                .actions
                .contains(&SupervisorAction::ReduceVramCache)
        );
        assert!(
            sustained
                .actions
                .contains(&SupervisorAction::RequestReclaim)
        );
        execute_actions_with(
            &sustained,
            &ActionPaths {
                runtime: &runtime,
                ledger: &ledger,
                daemon_instance_id: Some("fixture-daemon"),
                issued_at_unix_ms: Some(2_000),
            },
            &runner,
            || Ok(()),
        );

        let target: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(runtime.join("cache-target.json"))
                .unwrap_or_else(|error| panic!("read renewed target: {error}")),
        )
        .unwrap_or_else(|error| panic!("parse renewed target: {error}"));
        let reclaim: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(runtime.join("reclaim-request.json"))
                .unwrap_or_else(|error| panic!("read renewed reclaim: {error}")),
        )
        .unwrap_or_else(|error| panic!("parse renewed reclaim: {error}"));
        assert_eq!(target["target_bytes"], 0);
        assert_eq!(target["issued_at_unix_ms"], 2_000);
        assert_eq!(reclaim["issued_at_unix_ms"], 2_000);
        fs::remove_dir_all(root).unwrap_or_else(|error| panic!("remove fixture: {error}"));
    }

    #[test]
    fn recovery_samples_never_repeat_destructive_actions() {
        let mut supervisor = Supervisor::default();
        let first = supervisor.observe(sample(6, 4, 0.0, 3000, 0));
        assert!(
            first
                .actions
                .contains(&SupervisorAction::TerminateDiscardable)
        );

        let recovering = supervisor.observe(sample(6, 4, 0.0, 0, 6_000));
        assert_eq!(recovering.state, SupervisorState::Guarded);
        assert_eq!(recovering.actions, vec![SupervisorAction::CloseAdmission]);

        for index in 1..59 {
            let decision = supervisor.observe(sample(6, 4, 0.0, 0, 6_000 + index * 1_000));
            assert!(!decision.actions.iter().any(|action| matches!(
                action,
                SupervisorAction::TerminateDiscardable | SupervisorAction::KillDiscardable
            )));
        }
        let recovered = supervisor.observe(sample(6, 4, 0.0, 0, 65_000));
        assert_eq!(recovered.state, SupervisorState::Healthy);
        assert!(recovered.actions.is_empty());
    }

    #[test]
    fn frozen_scope_is_thawed_before_emergency_termination() {
        let mut supervisor = Supervisor::default();
        let critical = supervisor.observe(sample(2, 4, 0.0, 0, 0));
        assert!(
            critical
                .actions
                .contains(&SupervisorAction::FreezeDiscardable)
        );
        supervisor.commit_action_results(
            &[SupervisorActionResult::from_result(
                SupervisorAction::FreezeDiscardable,
                Ok(()),
            )],
            0,
        );

        let emergency = supervisor.observe(sample(1, 4, 0.0, 0, 1_000));
        let thaw = emergency
            .actions
            .iter()
            .position(|action| action == &SupervisorAction::ThawDiscardable);
        let term = emergency
            .actions
            .iter()
            .position(|action| action == &SupervisorAction::TerminateDiscardable);
        assert!(matches!((thaw, term), (Some(thaw), Some(term)) if thaw < term));
    }

    #[test]
    fn discard_priority_is_deterministic() {
        let mut classes = ["interactive", "batch", "build", "browser-test"];
        classes.sort_by_key(|class| discard_priority(class));
        assert_eq!(classes, ["batch", "browser-test", "build", "interactive"]);
    }

    #[test]
    fn exact_memory_thresholds_take_priority_over_psi_streaks() {
        let mut supervisor = Supervisor::default();
        assert_eq!(
            supervisor.observe(sample(3, 4, 0.0, 0, 0)).state,
            SupervisorState::Guarded
        );
        assert_eq!(
            supervisor.observe(sample(2, 4, 0.0, 0, 1000)).state,
            SupervisorState::Critical
        );
        assert_eq!(
            supervisor.observe(sample(1, 4, 0.0, 0, 2000)).state,
            SupervisorState::Emergency
        );
    }

    #[test]
    fn sanitized_victim_selection_uses_class_then_size() {
        let root = fixture();
        let ledger = root.join("reservations.json");
        fs::write(
            &ledger,
            ledger_fixture(&[
                ("build", 900, "ramshared-build-a.scope"),
                ("batch", 100, "ramshared-batch-small.scope"),
                ("batch", 800, "ramshared-batch-large.scope"),
                ("batch", 999, "foreign.service"),
            ]),
        )
        .unwrap();
        assert_eq!(
            select_victim(&ledger).unwrap().map(|victim| victim.unit),
            Some("ramshared-batch-large.scope".into())
        );
        assert!(valid_scope_unit("ramshared-build-a.scope"));
        assert!(!valid_scope_unit("../ramshared-build-a.scope"));
        assert!(!valid_scope_unit("foreign.service"));
        assert_eq!(discard_priority("unknown"), None);
        assert!(
            select_victim(&root.join("missing/reservations.json"))
                .unwrap_err()
                .contains("reservation ledger is unavailable")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    // TestName: discard_priority_uses_durable_older_reservation_ordinal_tie_break
    fn discard_priority_uses_durable_older_reservation_ordinal_tie_break() {
        let root = fixture();
        let ledger = root.join("reservations.json");
        let reservation = |id: &str, unit: &str, issued_ordinal: u64| {
            serde_json::json!({
                "id": id,
                "owner": {
                    "boot_id": "fixture-boot",
                    "pid": issued_ordinal as u32 + 1,
                    "start_time": issued_ordinal + 1,
                    "nonce": format!("fixture-{issued_ordinal}"),
                },
                "class": "batch",
                "memory_bytes": 4096,
                "unit": unit,
                "invocation_id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "issued_ordinal": issued_ordinal,
            })
        };
        fs::write(
            &ledger,
            serde_json::json!({
                "schema_version": workload::RESERVATION_LEDGER_SCHEMA_VERSION,
                "next_ordinal": 3,
                "reservations": [
                    reservation("older", "ramshared-batch-z.scope", 1),
                    reservation("newer", "ramshared-batch-a.scope", 2),
                ],
            })
            .to_string(),
        )
        .unwrap();
        let selected = select_victim(&ledger).unwrap().unwrap().unit;
        fs::remove_dir_all(root).unwrap();

        assert_eq!(
            selected, "ramshared-batch-z.scope",
            "unit spelling replaced the durable older-token tie break"
        );
    }

    #[test]
    fn action_executor_covers_cache_freeze_reclaim_thaw_and_kill() {
        let root = fixture();
        let runtime = root.join("runtime");
        let ledger = root.join("reservations.json");
        fs::write(
            &ledger,
            ledger_fixture(&[("batch", 4096, "ramshared-batch-fixture.scope")]),
        )
        .unwrap();
        let runner = FakeUnitRunner {
            calls: RefCell::new(Vec::new()),
            fail: false,
        };
        let decision = SupervisorDecision {
            state: SupervisorState::Emergency,
            healthy_samples: 0,
            actions: vec![
                SupervisorAction::CloseAdmission,
                SupervisorAction::ReduceVramCache,
                SupervisorAction::FreezeDiscardable,
                SupervisorAction::RequestReclaim,
                SupervisorAction::ThawDiscardable,
                SupervisorAction::TerminateDiscardable,
            ],
        };
        let outcomes = execute_actions_with(
            &decision,
            &ActionPaths {
                runtime: &runtime,
                ledger: &ledger,
                daemon_instance_id: Some("fixture-daemon"),
                issued_at_unix_ms: Some(1_000),
            },
            &runner,
            || Ok(()),
        );
        assert_eq!(outcomes.len(), decision.actions.len());
        assert!(
            outcomes
                .iter()
                .all(|outcome| outcome.status == SupervisorActionStatus::Succeeded)
        );
        assert_eq!(
            outcomes.last().map(|outcome| outcome.action),
            Some(SupervisorAction::TerminateDiscardable)
        );
        let kill = execute_actions_with(
            &SupervisorDecision {
                state: SupervisorState::Emergency,
                healthy_samples: 0,
                actions: vec![SupervisorAction::KillDiscardable],
            },
            &ActionPaths {
                runtime: &runtime,
                ledger: &ledger,
                daemon_instance_id: None,
                issued_at_unix_ms: Some(6_000),
            },
            &runner,
            || Ok(()),
        );
        assert_eq!(kill[0].status, SupervisorActionStatus::Succeeded);
        let cache_target: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(runtime.join("cache-target.json")).unwrap())
                .unwrap();
        assert_eq!(cache_target["target_bytes"], 0);
        assert_eq!(cache_target["daemon_instance_id"], "fixture-daemon");
        assert_eq!(cache_target["issued_at_unix_ms"], 1_000);
        let reclaim_request: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(runtime.join("reclaim-request.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(reclaim_request["daemon_instance_id"], "fixture-daemon");
        assert_eq!(reclaim_request["issued_at_unix_ms"], 1_000);
        assert!(!runtime.join("frozen-scope.json").exists());
        let calls = runner.calls.borrow();
        assert_eq!(calls.len(), 4);
        assert_eq!(calls[0][0], "freeze");
        assert_eq!(calls[1][0], "thaw");
        assert_eq!(calls[2][2], "--signal=TERM");
        assert_eq!(calls[3][2], "--signal=KILL");
        fs::remove_dir_all(root).unwrap();
    }

    struct FreezeEvidenceRunner {
        frozen_path: std::path::PathBuf,
        saw_pending: RefCell<bool>,
    }

    impl UnitActionRunner for FreezeEvidenceRunner {
        fn current_invocation_id(&self, _unit: &str) -> Result<String, String> {
            Ok("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into())
        }

        fn run(&self, _args: &[&str]) -> Result<(), String> {
            let pending = fs::read_to_string(&self.frozen_path)
                .ok()
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .is_some_and(|value| {
                    value["phase"] == "pending"
                        && value["unit"] == "ramshared-batch-fixture.scope"
                        && value["invocation_id"] == "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                });
            *self.saw_pending.borrow_mut() = pending;
            Err("injected freeze refusal".into())
        }
    }

    #[test]
    // TestName: freeze_persists_exact_pending_identity_before_effect_and_never_falls_back
    fn freeze_persists_exact_pending_identity_before_effect_and_never_falls_back() {
        let root = fixture();
        let runtime = root.join("runtime");
        let frozen_path = runtime.join("frozen-scope.json");
        let ledger = root.join("reservations.json");
        fs::write(
            &ledger,
            ledger_fixture(&[("batch", 4096, "ramshared-batch-fixture.scope")]),
        )
        .unwrap();
        let freeze_runner = FreezeEvidenceRunner {
            frozen_path: frozen_path.clone(),
            saw_pending: RefCell::new(false),
        };
        let freeze = execute_actions_with(
            &SupervisorDecision {
                state: SupervisorState::Critical,
                healthy_samples: 0,
                actions: vec![SupervisorAction::FreezeDiscardable],
            },
            &ActionPaths {
                runtime: &runtime,
                ledger: &ledger,
                daemon_instance_id: None,
                issued_at_unix_ms: Some(1_000),
            },
            &freeze_runner,
            || Ok(()),
        );
        let pending_survived = frozen_path.is_file();

        if frozen_path.exists() {
            fs::remove_file(&frozen_path).unwrap();
        }
        let thaw_runner = FakeUnitRunner {
            calls: RefCell::new(Vec::new()),
            fail: false,
        };
        let thaw = execute_actions_with(
            &SupervisorDecision {
                state: SupervisorState::Emergency,
                healthy_samples: 0,
                actions: vec![SupervisorAction::ThawDiscardable],
            },
            &ActionPaths {
                runtime: &runtime,
                ledger: &ledger,
                daemon_instance_id: None,
                issued_at_unix_ms: Some(2_000),
            },
            &thaw_runner,
            || Ok(()),
        );
        let thaw_calls = thaw_runner.calls.borrow().len();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(freeze[0].status, SupervisorActionStatus::Failed);
        assert!(
            *freeze_runner.saw_pending.borrow(),
            "freeze effect ran before durable pending identity evidence"
        );
        assert!(
            pending_survived,
            "failed freeze discarded the only exact recovery identity"
        );
        assert_eq!(thaw[0].status, SupervisorActionStatus::Failed);
        assert_eq!(
            thaw_calls, 0,
            "thaw fell back to a current ledger victim without frozen identity evidence"
        );
    }

    #[test]
    // TestName: freeze_applied_write_failure_recovers_exact_pending_identity_after_restart
    fn freeze_applied_write_failure_recovers_exact_pending_identity_after_restart() {
        let root = fixture();
        let runtime = root.join("runtime");
        let ledger = root.join("reservations.json");
        fs::write(
            &ledger,
            ledger_fixture(&[("batch", 4096, "ramshared-batch-original.scope")]),
        )
        .unwrap();
        let freeze_runner = FakeUnitRunner {
            calls: RefCell::new(Vec::new()),
            fail: false,
        };
        let write_hook = RuntimeWriteHookGuard::install(Box::new(|name, value| {
            if name == "frozen-scope.json" && value["phase"] == "applied" {
                Err("injected applied-record durability failure".into())
            } else {
                Ok(())
            }
        }));
        let freeze = execute_actions_with(
            &SupervisorDecision {
                state: SupervisorState::Critical,
                healthy_samples: 0,
                actions: vec![SupervisorAction::FreezeDiscardable],
            },
            &ActionPaths {
                runtime: &runtime,
                ledger: &ledger,
                daemon_instance_id: None,
                issued_at_unix_ms: Some(1_000),
            },
            &freeze_runner,
            || Ok(()),
        );
        drop(write_hook);
        assert_eq!(freeze[0].status, SupervisorActionStatus::Failed);
        let pending: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(runtime.join("frozen-scope.json")).unwrap())
                .unwrap();
        assert_eq!(pending["phase"], "pending");
        assert_eq!(pending["unit"], "ramshared-batch-original.scope");

        fs::write(
            &ledger,
            ledger_fixture(&[("batch", 8192, "ramshared-batch-replacement.scope")]),
        )
        .unwrap();
        let mut reconstructed = Supervisor::default();
        reconstructed
            .recover_action_state(&runtime, 2_000, 100)
            .unwrap();
        let decision = reconstructed.observe(sample(8, 4, 0.0, 0, 100));
        assert_eq!(decision.actions, vec![SupervisorAction::ThawDiscardable]);
        let thaw_runner = FakeUnitRunner {
            calls: RefCell::new(Vec::new()),
            fail: false,
        };
        let thaw = execute_actions_with(
            &decision,
            &ActionPaths {
                runtime: &runtime,
                ledger: &ledger,
                daemon_instance_id: None,
                issued_at_unix_ms: Some(2_000),
            },
            &thaw_runner,
            || Ok(()),
        );
        let calls = thaw_runner.calls.borrow();
        assert_eq!(thaw[0].status, SupervisorActionStatus::Succeeded);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0][0], "thaw");
        assert_eq!(calls[0][1], "ramshared-batch-original.scope");
        assert!(!runtime.join("frozen-scope.json").exists());
        drop(calls);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    // TestName: emergency_kill_is_bound_to_the_successfully_termed_identity_across_ledger_change
    fn emergency_kill_is_bound_to_the_successfully_termed_identity_across_ledger_change() {
        let root = fixture();
        let runtime = root.join("runtime");
        let ledger = root.join("reservations.json");
        fs::write(
            &ledger,
            ledger_fixture(&[("batch", 4096, "ramshared-batch-first.scope")]),
        )
        .unwrap();
        let runner = FakeUnitRunner {
            calls: RefCell::new(Vec::new()),
            fail: false,
        };
        let paths = ActionPaths {
            runtime: &runtime,
            ledger: &ledger,
            daemon_instance_id: None,
            issued_at_unix_ms: Some(1_000),
        };
        let term = execute_actions_with(
            &SupervisorDecision {
                state: SupervisorState::Emergency,
                healthy_samples: 0,
                actions: vec![SupervisorAction::TerminateDiscardable],
            },
            &paths,
            &runner,
            || Ok(()),
        );
        let durable_target = runtime.join("emergency-target.json").is_file();
        fs::write(
            &ledger,
            ledger_fixture(&[("batch", 8192, "ramshared-batch-replacement.scope")]),
        )
        .unwrap();
        let kill = execute_actions_with(
            &SupervisorDecision {
                state: SupervisorState::Emergency,
                healthy_samples: 0,
                actions: vec![SupervisorAction::KillDiscardable],
            },
            &ActionPaths {
                runtime: &runtime,
                ledger: &ledger,
                daemon_instance_id: None,
                issued_at_unix_ms: Some(7_000),
            },
            &runner,
            || Ok(()),
        );
        let calls = runner.calls.borrow().clone();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(term[0].status, SupervisorActionStatus::Succeeded);
        assert!(
            durable_target,
            "successful TERM did not persist its exact target"
        );
        assert_eq!(kill[0].status, SupervisorActionStatus::Succeeded);
        assert_eq!(calls.len(), 2);
        assert_eq!(
            calls[0].last().map(String::as_str),
            Some("ramshared-batch-first.scope")
        );
        assert_eq!(
            calls[1].last().map(String::as_str),
            Some("ramshared-batch-first.scope"),
            "KILL retargeted a newly selected ledger victim"
        );
    }

    #[test]
    // TestName: reconstructed_supervisor_kills_only_durable_termed_identity_after_grace
    fn reconstructed_supervisor_kills_only_durable_termed_identity_after_grace() {
        let root = fixture();
        let runtime = root.join("runtime");
        let ledger = root.join("reservations.json");
        let durable_victim = VictimIdentity {
            unit: "ramshared-batch-before-restart.scope".into(),
            invocation_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        };
        write_emergency_target(
            &runtime,
            EmergencyPhase::Termed,
            &durable_victim,
            Some(1_000),
            1_000,
        )
        .unwrap();
        fs::write(
            &ledger,
            ledger_fixture(&[("batch", 8192, "ramshared-batch-after-restart.scope")]),
        )
        .unwrap();

        let mut reconstructed = Supervisor::default();
        reconstructed
            .recover_action_state(&runtime, 7_000, 250)
            .unwrap();
        let decision = reconstructed.observe(sample(0, 4, 0.0, 0, 250));
        assert!(
            decision
                .actions
                .contains(&SupervisorAction::KillDiscardable),
            "elapsed durable TERM grace did not reconstruct KILL eligibility"
        );
        assert!(
            !decision
                .actions
                .contains(&SupervisorAction::TerminateDiscardable)
        );
        let runner = FakeUnitRunner {
            calls: RefCell::new(Vec::new()),
            fail: false,
        };
        let results = execute_actions_with(
            &decision,
            &ActionPaths {
                runtime: &runtime,
                ledger: &ledger,
                daemon_instance_id: None,
                issued_at_unix_ms: Some(7_000),
            },
            &runner,
            || Ok(()),
        );
        let kill = results
            .iter()
            .find(|result| result.action == SupervisorAction::KillDiscardable)
            .unwrap();
        assert_eq!(kill.status, SupervisorActionStatus::Succeeded);
        let calls = runner.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].last().map(String::as_str),
            Some("ramshared-batch-before-restart.scope")
        );
        drop(calls);
        fs::remove_dir_all(root).unwrap();
    }

    struct HugeErrorRunner;

    impl UnitActionRunner for HugeErrorRunner {
        fn current_invocation_id(&self, _unit: &str) -> Result<String, String> {
            Ok("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into())
        }

        fn run(&self, _args: &[&str]) -> Result<(), String> {
            Err(format!(
                "{}\n\0\u{001b}\u{0085}tail",
                "x".repeat(1024 * 1024)
            ))
        }
    }

    #[test]
    // TestName: supervisor_action_errors_are_bounded_single_line_and_control_free
    fn supervisor_action_errors_are_bounded_single_line_and_control_free() {
        let root = fixture();
        let ledger = root.join("reservations.json");
        fs::write(
            &ledger,
            ledger_fixture(&[("batch", 4096, "ramshared-batch-fixture.scope")]),
        )
        .unwrap();
        let results = execute_actions_with(
            &SupervisorDecision {
                state: SupervisorState::Critical,
                healthy_samples: 0,
                actions: vec![SupervisorAction::FreezeDiscardable],
            },
            &ActionPaths {
                runtime: &root.join("runtime"),
                ledger: &ledger,
                daemon_instance_id: None,
                issued_at_unix_ms: Some(1_000),
            },
            &HugeErrorRunner,
            || Ok(()),
        );
        let error = results[0].error.as_deref().unwrap().to_string();
        fs::remove_dir_all(root).unwrap();

        assert!(
            error.len() <= 1_024,
            "action error exceeded its type ceiling"
        );
        assert!(
            error.contains("[truncated]"),
            "truncated action error omitted its explicit marker"
        );
        assert!(
            !error.chars().any(char::is_control),
            "action error retained a control character"
        );
        assert_eq!(error.lines().count(), 1);
    }

    #[test]
    // TestName: supervisor_publishes_every_action_outcome_in_execution_order
    fn supervisor_publishes_every_action_outcome_in_execution_order() {
        let root = fixture();
        let runner = FakeUnitRunner {
            calls: RefCell::new(Vec::new()),
            fail: false,
        };
        let decision = SupervisorDecision {
            state: SupervisorState::Critical,
            healthy_samples: 0,
            actions: vec![
                SupervisorAction::ReduceVramCache,
                SupervisorAction::CloseAdmission,
            ],
        };
        let action_results = execute_actions_with(
            &decision,
            &ActionPaths {
                runtime: &root.join("runtime"),
                ledger: &root.join("missing-ledger.json"),
                daemon_instance_id: None,
                issued_at_unix_ms: None,
            },
            &runner,
            || Ok(()),
        );
        let state = root.join("supervisor-state.json");
        let identity = OwnerIdentity::current().unwrap();
        publish_at(&state, &decision, &action_results, None, &identity, 1_000).unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&state).unwrap()).unwrap();

        assert_eq!(value["schema_version"], 3);
        assert!(
            value.get("last_action").is_none(),
            "the lossy compatibility field must be removed"
        );
        let results = value["action_results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["action"], "reduce_vram_cache");
        assert_eq!(results[0]["status"], "failed");
        assert!(results[0]["error"].as_str().unwrap().contains("daemon"));
        assert_eq!(results[1]["action"], "close_admission");
        assert_eq!(results[1]["status"], "succeeded");
        assert!(results[1]["error"].is_null());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    // TestName: stale_invocation_id_refuses_all_systemctl_actions
    fn stale_invocation_id_refuses_all_systemctl_actions() {
        let root = fixture();
        let runtime = root.join("runtime");
        let ledger = root.join("reservations.json");
        fs::create_dir_all(&runtime).unwrap();
        fs::write(
            &ledger,
            ledger_fixture(&[("batch", 4096, "ramshared-batch-fixture.scope")]),
        )
        .unwrap();
        fs::write(
            runtime.join("frozen-scope.json"),
            serde_json::json!({
                "schema_version": 3,
                "phase": "pending",
                "unit": "ramshared-batch-fixture.scope",
                "invocation_id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "updated_at_unix_ms": 1_000,
            })
            .to_string(),
        )
        .unwrap();
        let runner = StaleIdentityRunner {
            calls: RefCell::new(Vec::new()),
            identity_queries: RefCell::new(0),
        };
        let decision = SupervisorDecision {
            state: SupervisorState::Emergency,
            healthy_samples: 0,
            actions: vec![
                SupervisorAction::FreezeDiscardable,
                SupervisorAction::ThawDiscardable,
                SupervisorAction::TerminateDiscardable,
                SupervisorAction::KillDiscardable,
            ],
        };

        let results = execute_actions_with(
            &decision,
            &ActionPaths {
                runtime: &runtime,
                ledger: &ledger,
                daemon_instance_id: None,
                issued_at_unix_ms: Some(1_000),
            },
            &runner,
            || Ok(()),
        );

        assert_eq!(results.len(), 4);
        assert!(
            results
                .iter()
                .all(|result| result.status == SupervisorActionStatus::Failed)
        );
        assert!(results[..3].iter().all(|result| {
            result
                .error
                .as_deref()
                .is_some_and(|error| error.contains("InvocationID changed"))
        }));
        assert!(
            results[3]
                .error
                .as_deref()
                .is_some_and(|error| error.contains("successful TERM outcome"))
        );
        assert_eq!(*runner.identity_queries.borrow(), 3);
        assert!(runner.calls.borrow().is_empty());
        assert!(runtime.join("frozen-scope.json").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    // TestName: unsupported_ledger_schema_refuses_scope_cleanup_without_runner_action
    fn unsupported_ledger_schema_refuses_scope_cleanup_without_runner_action() {
        let root = fixture();
        let ledger = root.join("reservations.json");
        let contents = serde_json::json!({
            "schema_version": 999,
            "next_ordinal": 2,
            "reservations": [{
                "id": "fixture-reservation",
                "owner": {
                    "boot_id": "fixture-boot",
                    "pid": 1,
                    "start_time": 1,
                    "nonce": "fixture-nonce",
                },
                "class": "batch",
                "memory_bytes": 4096,
                "unit": "ramshared-batch-fixture.scope",
                "invocation_id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "issued_ordinal": 1,
            }],
        })
        .to_string();
        fs::write(&ledger, &contents).unwrap();
        let runner = FakeUnitRunner {
            calls: RefCell::new(Vec::new()),
            fail: false,
        };
        let decision = SupervisorDecision {
            state: SupervisorState::Emergency,
            healthy_samples: 0,
            actions: vec![
                SupervisorAction::FreezeDiscardable,
                SupervisorAction::TerminateDiscardable,
                SupervisorAction::KillDiscardable,
            ],
        };
        let results = execute_actions_with(
            &decision,
            &ActionPaths {
                runtime: &root.join("runtime"),
                ledger: &ledger,
                daemon_instance_id: None,
                issued_at_unix_ms: Some(1_000),
            },
            &runner,
            || Ok(()),
        );
        assert_eq!(results.len(), 3);
        assert!(
            results
                .iter()
                .all(|result| result.status == SupervisorActionStatus::Failed)
        );
        assert!(results[..2].iter().all(|result| {
            result
                .error
                .as_deref()
                .is_some_and(|error| error.contains("unsupported reservation ledger schema"))
        }));
        assert!(
            results[2]
                .error
                .as_deref()
                .is_some_and(|error| error.contains("successfully terminated"))
        );
        assert!(runner.calls.borrow().is_empty());
        assert_eq!(fs::read_to_string(&ledger).unwrap(), contents);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn critical_requests_require_a_fresh_daemon_cache_status() {
        let root = fixture();
        let runtime = root.join("runtime");
        fs::create_dir_all(&runtime).unwrap();
        fs::write(
            runtime.join("cache-status.json"),
            r#"{"schema_version":1,"daemon_instance_id":"daemon-42","written_at_unix_ms":1000}"#,
        )
        .unwrap();
        assert_eq!(
            request_daemon_instance_at(&runtime, 1_001).unwrap(),
            "daemon-42"
        );
        assert!(request_daemon_instance_at(&runtime, 16_001).is_err());
        fs::write(runtime.join("cache-status.json"), "not-json").unwrap();
        assert!(request_daemon_instance_at(&runtime, 1_001).is_err());
        assert!(!valid_daemon_instance_id("foreign/daemon"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn action_executor_records_refusals_without_touching_a_unit() {
        let root = fixture();
        let runner = FakeUnitRunner {
            calls: RefCell::new(Vec::new()),
            fail: true,
        };
        let decision = SupervisorDecision {
            state: SupervisorState::Critical,
            healthy_samples: 0,
            actions: vec![SupervisorAction::FreezeDiscardable],
        };
        let missing_ledger = root.join("missing/reservations.json");
        let paths = ActionPaths {
            runtime: &root,
            ledger: &missing_ledger,
            daemon_instance_id: None,
            issued_at_unix_ms: Some(1_000),
        };
        let missing = execute_actions_with(&decision, &paths, &runner, || Ok(()));
        assert_eq!(missing.len(), 1);
        assert!(
            missing[0]
                .error
                .as_deref()
                .is_some_and(|error| error.contains("reservation ledger is unavailable"))
        );

        let ledger = root.join("reservations.json");
        fs::write(
            &ledger,
            ledger_fixture(&[("batch", 1, "ramshared-batch-fixture.scope")]),
        )
        .unwrap();
        let failed = execute_actions_with(
            &decision,
            &ActionPaths {
                runtime: &root,
                ledger: &ledger,
                daemon_instance_id: None,
                issued_at_unix_ms: Some(1_000),
            },
            &runner,
            || Ok(()),
        );
        assert_eq!(failed.len(), 1);
        assert!(
            failed[0]
                .error
                .as_deref()
                .is_some_and(|error| error.contains("fixture systemctl refusal"))
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sample_parsers_and_atomic_publication_are_bounded() {
        assert_eq!(
            parse_meminfo("MemTotal: 16384 kB\nMemAvailable: 8192 kB\n"),
            Some((16 * 1024 * 1024, 8 * 1024 * 1024))
        );
        assert!(parse_meminfo("MemTotal: no\n").is_none());
        assert_eq!(
            parse_psi_full_avg10("some avg10=3.00\nfull avg10=2.50 avg60=1.00\n"),
            Some(2.5)
        );
        assert!(parse_psi_full_avg10("some avg10=1.0\n").is_none());

        let root = fixture();
        let state = root.join("nested/supervisor.json");
        let decision = SupervisorDecision {
            state: SupervisorState::Guarded,
            actions: vec![SupervisorAction::CloseAdmission],
            healthy_samples: 17,
        };
        let supervisor_identity = OwnerIdentity::current().unwrap();
        let action_results = [SupervisorActionResult::from_result(
            SupervisorAction::CloseAdmission,
            Ok(()),
        )];
        publish_at(
            &state,
            &decision,
            &action_results,
            Some("fixture-daemon"),
            &supervisor_identity,
            1_000,
        )
        .unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&state).unwrap()).unwrap();
        assert_eq!(value["control_state"], "GUARDED");
        assert_eq!(value["healthy_samples"], 17);
        assert_eq!(value["action_results"][0]["action"], "close_admission");
        assert_eq!(value["daemon_instance_id"], "fixture-daemon");
        assert_eq!(value["supervisor_identity"]["pid"], supervisor_identity.pid);
        assert_eq!(value["written_at_unix_ms"], 1_000);
        assert!(collect_sample(Instant::now(), Instant::now()).is_ok());
        assert!(run(&["--invalid".into()]).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    // TestName: close_admission_publishes_a_serialized_closed_gate_before_guarded_state
    fn close_admission_publishes_a_serialized_closed_gate_before_guarded_state() {
        let root = fixture();
        let state_path = root.join("supervisor-state.json");
        let ledger_root = root.join("admission");
        let identity = OwnerIdentity::current().unwrap();
        let guarded = SupervisorDecision {
            state: SupervisorState::Guarded,
            actions: vec![SupervisorAction::CloseAdmission],
            healthy_samples: 0,
        };
        let runtime = root.join("runtime");
        let ledger = ledger_root.join("reservations.json");
        let action_paths = ActionPaths {
            runtime: &runtime,
            ledger: &ledger,
            daemon_instance_id: Some("fixture-daemon"),
            issued_at_unix_ms: Some(1_000),
        };
        let runner = FakeUnitRunner {
            calls: RefCell::new(Vec::new()),
            fail: false,
        };
        let guarded_results = apply_decision_with(
            &state_path,
            &ledger_root,
            &guarded,
            &action_paths,
            &runner,
            &identity,
            1_000,
        )
        .unwrap();
        assert_eq!(guarded_results[0].status, SupervisorActionStatus::Succeeded);
        let gate: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(ledger_root.join("admission-state.json")).unwrap(),
        )
        .unwrap();
        let state: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
        assert_eq!(gate["admission_open"], false);
        assert_eq!(gate["control_state"], "GUARDED");
        assert_eq!(state["control_state"], "GUARDED");

        let healthy = SupervisorDecision {
            state: SupervisorState::Healthy,
            actions: Vec::new(),
            healthy_samples: 60,
        };
        apply_decision_with(
            &state_path,
            &ledger_root,
            &healthy,
            &action_paths,
            &runner,
            &identity,
            2_000,
        )
        .unwrap();
        let reopened: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(ledger_root.join("admission-state.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(reopened["admission_open"], true);
        assert_eq!(reopened["control_state"], "HEALTHY");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    // TestName: critical_actions_remain_executable_when_admission_transition_busy
    fn critical_actions_remain_executable_when_admission_transition_busy() {
        let root = fixture();
        let state_path = root.join("supervisor-state.json");
        let ledger_root = root.join("admission");
        fs::create_dir_all(&ledger_root).unwrap();
        let authority = File::open(&ledger_root).unwrap();
        authority.try_lock().unwrap();
        let runtime = root.join("runtime");
        let decision = SupervisorDecision {
            state: SupervisorState::Critical,
            actions: vec![
                SupervisorAction::CloseAdmission,
                SupervisorAction::ReduceVramCache,
                SupervisorAction::RequestReclaim,
            ],
            healthy_samples: 0,
        };
        let identity = OwnerIdentity::current().unwrap();
        let runner = FakeUnitRunner {
            calls: RefCell::new(Vec::new()),
            fail: false,
        };

        let error = apply_decision_with(
            &state_path,
            &ledger_root,
            &decision,
            &ActionPaths {
                runtime: &runtime,
                ledger: &ledger_root.join("reservations.json"),
                daemon_instance_id: Some("fixture-daemon"),
                issued_at_unix_ms: Some(1_000),
            },
            &runner,
            &identity,
            1_000,
        )
        .unwrap_err();

        assert!(
            error.to_string().contains("admission close failed"),
            "{error}"
        );
        assert!(runtime.join("cache-target.json").is_file());
        assert!(runtime.join("reclaim-request.json").is_file());
        let state: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
        assert_eq!(state["control_state"], "CRITICAL");
        assert_eq!(state["action_results"].as_array().unwrap().len(), 3);
        let close = state["action_results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|result| result["action"] == "close_admission")
            .unwrap();
        assert_eq!(
            close["status"], "failed",
            "CloseAdmission published a synthetic success while its canonical lock was busy"
        );
        assert!(
            close["error"]
                .as_str()
                .is_some_and(|error| error.contains("busy")),
            "CloseAdmission did not retain the actual publication failure"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    // TestName: bounded_systemctl_adapter_is_fixture_scoped_under_parallel_execution
    fn bounded_systemctl_adapter_is_fixture_scoped_under_parallel_execution() {
        let fixture = TestDir::new();
        let systemctl = fixture.program(
            "systemctl-fixture",
            "#!/bin/sh\ncase \"$1\" in\n  --version) exit 0 ;;\n  ramshared-invalid-command) exit 1 ;;\n  *) exit 2 ;;\nesac\n",
        );
        assert!(
            run_systemctl_bounded_for(&systemctl, &["--version"], Duration::from_millis(100),)
                .is_ok()
        );
        assert!(
            run_systemctl_bounded_for(
                &systemctl,
                &["ramshared-invalid-command"],
                Duration::from_millis(100),
            )
            .is_err()
        );
    }

    #[test]
    // TestName: bounded_systemctl_adapter_reaps_its_owned_timeout_fixture
    fn bounded_systemctl_adapter_reaps_its_owned_timeout_fixture() {
        let fixture = TestDir::new();
        let pid_file = fixture.path.join("owned-systemctl.pid");
        let systemctl = fixture.program(
            "blocking-systemctl-fixture",
            "#!/bin/sh\nprintf '%s' \"$$\" > \"$1\"\nwhile :; do sleep 0.05; done\n",
        );
        let started = Instant::now();
        let error = run_systemctl_bounded_for(
            &systemctl,
            &[pid_file.to_str().unwrap()],
            Duration::from_millis(100),
        )
        .unwrap_err();
        assert!(error.contains("timed out"), "{error}");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "owned timeout cleanup exceeded one second"
        );
        let pid = fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap();
        assert!(
            !std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "owned timed-out fixture process was not reaped"
        );
    }

    #[test]
    // TestName: bounded_systemctl_adapter_parallel_fixtures_stay_isolated
    fn bounded_systemctl_adapter_parallel_fixtures_stay_isolated() {
        let start = std::sync::Arc::new(std::sync::Barrier::new(2));
        std::thread::scope(|scope| {
            let success_start = std::sync::Arc::clone(&start);
            let success = scope.spawn(move || {
                let fixture = TestDir::new();
                let systemctl = fixture.program(
                    "successful-systemctl-fixture",
                    "#!/bin/sh\nsleep 0.05\n[ \"$1\" = \"--version\" ]\n",
                );
                success_start.wait();
                run_systemctl_bounded_for(&systemctl, &["--version"], Duration::from_millis(500))
            });
            let timeout_start = std::sync::Arc::clone(&start);
            let timeout = scope.spawn(move || {
                let fixture = TestDir::new();
                let systemctl = fixture.program(
                    "blocking-systemctl-fixture",
                    "#!/bin/sh\nwhile :; do sleep 0.05; done\n",
                );
                timeout_start.wait();
                run_systemctl_bounded_for(&systemctl, &[], Duration::from_millis(100))
            });
            assert!(success.join().unwrap().is_ok());
            let error = timeout.join().unwrap().unwrap_err();
            assert!(error.contains("timed out"), "{error}");
        });
    }
}
