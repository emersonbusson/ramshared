use std::time::{Duration, Instant};

pub const RECOVERY_ACTIVATION_OBSERVATION_DEADLINE: Duration = Duration::from_secs(30);
pub const RECOVERY_ACTIVATION_POLL_TICK: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryActivationPoll {
    Idle,
    Pending,
    Succeeded,
    Failed,
}

/// Owns at most one recovery activation child receiver. A failed healthy epoch
/// remains parked until an unhealthy or nonempty observation begins a new
/// epoch, preventing retry storms against the same NBD export.
#[derive(Default)]
pub struct RecoveryActivation {
    pub result_rx: Option<std::sync::mpsc::Receiver<bool>>,
    pub failed_epoch: bool,
    pub shutdown_requested: bool,
    pub started_at: Option<Instant>,
    pub deadline_reported: bool,
}

impl RecoveryActivation {
    pub fn start(&mut self, result_rx: std::sync::mpsc::Receiver<bool>) -> Result<(), &'static str> {
        if self.result_rx.is_some() {
            return Err("recovery activation is already pending");
        }
        if self.failed_epoch {
            return Err("recovery activation is parked for this healthy epoch");
        }
        self.result_rx = Some(result_rx);
        self.started_at = Some(Instant::now());
        self.deadline_reported = false;
        Ok(())
    }

    pub fn poll(&mut self) -> RecoveryActivationPoll {
        let Some(rx) = self.result_rx.take() else {
            return RecoveryActivationPoll::Idle;
        };
        match rx.try_recv() {
            Ok(true) => {
                self.started_at = None;
                self.deadline_reported = false;
                self.failed_epoch = false;
                RecoveryActivationPoll::Succeeded
            }
            Ok(false) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.started_at = None;
                self.deadline_reported = false;
                self.failed_epoch = true;
                RecoveryActivationPoll::Failed
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                self.result_rx = Some(rx);
                RecoveryActivationPoll::Pending
            }
        }
    }

    pub fn launch_allowed(&mut self, healthy: bool, tier_empty: bool) -> bool {
        if !healthy || !tier_empty {
            self.failed_epoch = false;
            return false;
        }
        self.result_rx.is_none() && !self.failed_epoch
    }

    pub fn is_pending(&self) -> bool {
        self.result_rx.is_some()
    }

    pub fn mark_dispatch_failure(&mut self) {
        self.failed_epoch = true;
        self.started_at = None;
        self.deadline_reported = false;
    }

    pub fn request_shutdown(&mut self) {
        self.shutdown_requested = true;
    }

    pub fn backend_release_allowed(&self) -> bool {
        !self.shutdown_requested || self.result_rx.is_none()
    }

    pub fn take_observation_deadline_exceeded(&mut self) -> bool {
        let overdue = self.result_rx.is_some()
            && self.started_at.is_some_and(|started| {
                started.elapsed() >= RECOVERY_ACTIVATION_OBSERVATION_DEADLINE
            });
        if overdue && !self.deadline_reported {
            self.deadline_reported = true;
            return true;
        }
        false
    }
}

pub struct ResidencyCheckState<'a, M: ramshared_vram::VramMemory> {
    pub canary: &'a mut Option<crate::residency::Canary>,
    pub baseline: &'a mut Vec<u64>,
    pub sampler: &'a mut crate::residency::ResidencySampler,
    pub cadence: &'a mut crate::canary_probe::Cadence,
    pub probe: &'a mut crate::canary_probe::CanaryProbe<M>,
    pub free_floor_bytes: u64,
}

pub fn residency_check<M: ramshared_vram::VramMemory, F: Fn() -> Option<u64>>(
    lat_us: u64,
    state: &mut ResidencyCheckState<'_, M>,
    mem_free: F,
) -> Option<crate::residency::DemoteReason> {
    // §9: per-request latency canary. content_ok=true/free=u64::MAX ON PURPOSE — the signal
    // here is latency; content and free-floor come from the probe §9.4 below.
    let mut latency_reason = None;
    match state.canary.as_mut() {
        None => {
            state.baseline.push(lat_us);
            if state.baseline.len() >= 16 {
                state.baseline.sort_unstable();
                let med = state.baseline[state.baseline.len() / 2].max(1);
                *state.canary = Some(crate::residency::Canary::new(crate::residency::ResidencyConfig::default(), med));
                eprintln!("[ramsharedd] canario armado (baseline={med} us)");
            }
        }
        Some(c) => {
            if let crate::residency::Verdict::Demote(reason) = c.sample(lat_us, true, u64::MAX) {
                latency_reason = Some(reason);
            }
        }
    }
    // §9.4: dedicated content/free probe in cadence (corrupted content demotes immediately;
    // free-floor/transient error require streak).
    let mut probe_reason = None;
    if state.cadence.tick() {
        let content = state.probe.check_content().ok();
        let free = mem_free();
        let verdict = state.sampler.sample(content, free);
        let streak = state.sampler.bad_streak();
        let trace_probe = std::env::var("RAMSHARED_TRACE_PROBE").ok().as_deref() == Some("1");
        if should_log_probe_sample(content, free, state.free_floor_bytes, streak, trace_probe) {
            eprintln!(
                "[ramsharedd] sonda §9.4 sample: content={content:?} free={free:?} \
                 floor={} streak={streak}",
                state.free_floor_bytes
            );
        }
        if let crate::residency::Verdict::Demote(reason) = verdict {
            eprintln!(
                "[ramsharedd] sonda §9.4: content={content:?} free={free:?} streak={}",
                streak
            );
            probe_reason = Some(reason);
        }
    }
    choose_residency_reason(latency_reason, probe_reason)
}

pub fn choose_residency_reason(
    latency: Option<crate::residency::DemoteReason>,
    probe: Option<crate::residency::DemoteReason>,
) -> Option<crate::residency::DemoteReason> {
    probe.or(latency)
}

pub fn should_log_probe_sample(
    content: Option<bool>,
    free: Option<u64>,
    free_floor_bytes: u64,
    streak: u32,
    trace_probe: bool,
) -> bool {
    trace_probe
        || content != Some(true)
        || free.is_none()
        || free.is_some_and(|f| f < free_floor_bytes.saturating_mul(2))
        || streak > 0
}

pub fn sparse_residency_config(reserve_floor_bytes: u64) -> crate::residency::ResidencyConfig {
    crate::residency::ResidencyConfig {
        free_floor_bytes: reserve_floor_bytes,
        ..crate::residency::ResidencyConfig::default()
    }
}

pub fn sparse_residency_requests_swapoff(reason: crate::residency::DemoteReason) -> bool {
    !matches!(reason, crate::residency::DemoteReason::Latency)
}

pub struct NbdBudgetSnapshot {
    pub budget: u64,
    pub current_usage: u64,
    pub sampled_at: Instant,
}

pub trait NbdBudgetProvider {
    fn snapshot(&self) -> Result<NbdBudgetSnapshot, String>;
}

pub struct ProductionNbdBudgetProvider(pub ramshared_dxg::DxgBudgetProvider);

impl NbdBudgetProvider for ProductionNbdBudgetProvider {
    fn snapshot(&self) -> Result<NbdBudgetSnapshot, String> {
        let snapshot = ramshared_dxg::GpuBudgetProvider::snapshot(&self.0).map_err(|error| error.to_string())?;
        Ok(NbdBudgetSnapshot {
            budget: snapshot.budget,
            current_usage: snapshot.current_usage,
            sampled_at: snapshot.sampled_at,
        })
    }
}
