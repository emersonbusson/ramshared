use super::*;

/// Pressure and lifecycle signals carried by a bounded observation record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObservationEventKind {
    /// No pressure or lifecycle transition is asserted.
    Healthy,
    /// Host budget pressure constrains the possible tier.
    Pressure,
    /// Host reset/TDR invalidated the previous observation.
    Reset,
    /// Host channel was lost.
    ChannelLoss,
    /// Host is taking the adapter offline.
    Offline,
    /// Host reports migration activity without granting ownership.
    Migrate,
    /// Unknown event kinds are rejected rather than guessed.
    Unknown,
}

/// One versioned, bounded observation event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservationEvent {
    pub event_id: EventId,
    pub kind: ObservationEventKind,
}

impl ObservationEvent {
    /// Builds an observation event; identity bounds are enforced by `OpaqueId`.
    pub fn new(event_id: EventId, kind: ObservationEventKind) -> Self {
        Self { event_id, kind }
    }
}

/// Host-authoritative observation used only for deterministic preflight.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostObservation {
    pub schema_version: u16,
    pub host_epoch: u64,
    pub adapter_id: AdapterId,
    pub authority: Authority,
    pub budget_bytes: u64,
    pub resident_bytes: u64,
    pub available_bytes: u64,
    pub observed_at: u64,
    pub max_age: u64,
    pub event_id: EventId,
    pub events: Vec<ObservationEvent>,
}

impl HostObservation {
    /// Convenience constructor for the current public schema and explicit host
    /// authority.  It is still validated by `LeaseMachine::observe`.
    pub fn host(
        host_epoch: u64,
        budget_bytes: u64,
        resident_bytes: u64,
        available_bytes: u64,
        observed_at: u64,
        max_age: u64,
        event_id: EventId,
    ) -> Self {
        Self::new(
            N3_SCHEMA_VERSION,
            host_epoch,
            OpaqueId(b"host-adapter".to_vec()),
            Authority::Host,
            budget_bytes,
            resident_bytes,
            available_bytes,
            observed_at,
            max_age,
            event_id,
            Vec::new(),
        )
    }

    /// Constructs a record without applying policy.  `LeaseMachine::observe`
    /// performs the complete bounded validation atomically.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema_version: u16,
        host_epoch: u64,
        adapter_id: AdapterId,
        authority: Authority,
        budget_bytes: u64,
        resident_bytes: u64,
        available_bytes: u64,
        observed_at: u64,
        max_age: u64,
        event_id: EventId,
        events: Vec<ObservationEvent>,
    ) -> Self {
        Self {
            schema_version,
            host_epoch,
            adapter_id,
            authority,
            budget_bytes,
            resident_bytes,
            available_bytes,
            observed_at,
            max_age,
            event_id,
            events,
        }
    }

    /// Changes the schema for a negative fixture before validation.
    pub fn with_schema_version(mut self, schema_version: u16) -> Self {
        self.schema_version = schema_version;
        self
    }

    /// Changes the observation timestamp for a deterministic freshness test.
    pub fn with_observed_at(mut self, observed_at: u64) -> Self {
        self.observed_at = observed_at;
        self
    }

    /// Changes the resident counter for a bounded-counter refusal test.
    pub fn with_resident_bytes(mut self, resident_bytes: u64) -> Self {
        self.resident_bytes = resident_bytes;
        self
    }

    /// Validates the complete record against a monotonic model timestamp.
    pub fn validate(&self, now: u64) -> Result<(), FailureReason> {
        if self.schema_version != N3_SCHEMA_VERSION {
            return Err(FailureReason::UnknownSchema);
        }
        if self.host_epoch == 0
            || self.adapter_id.as_bytes().is_empty()
            || self.event_id.as_bytes().is_empty()
        {
            return Err(FailureReason::MalformedRecord);
        }
        if self.authority != Authority::Host {
            return Err(FailureReason::HostAuthorityRequired);
        }
        if self.max_age == 0 || self.max_age > MAX_OBSERVATION_AGE {
            return Err(FailureReason::MalformedRecord);
        }
        if now < self.observed_at {
            return Err(FailureReason::InvalidObservationClock);
        }
        if now - self.observed_at > self.max_age {
            return Err(FailureReason::StaleObservation);
        }
        if self.events.len() > MAX_OBSERVATION_EVENTS
            || self.events.iter().any(|event| {
                event.event_id.as_bytes().is_empty() || event.kind == ObservationEventKind::Unknown
            })
        {
            return Err(FailureReason::MalformedRecord);
        }
        if !valid_counter(self.budget_bytes)
            || !valid_counter(self.resident_bytes)
            || !valid_counter(self.available_bytes)
            || self.resident_bytes > self.budget_bytes
            || self.available_bytes > self.budget_bytes
        {
            return Err(FailureReason::ImpossibleBudget);
        }
        Ok(())
    }

    /// Whether the record carries explicit host ownership.
    pub fn is_host_authoritative(&self) -> bool {
        self.authority == Authority::Host
    }

    /// N3 observations never contain a guest PFN/NUMA residency assertion.
    pub fn uses_guest_residency_claim(&self) -> bool {
        false
    }

    fn has_pressure(&self) -> bool {
        self.events
            .iter()
            .any(|event| event.kind == ObservationEventKind::Pressure)
    }

    fn has_offline_signal(&self) -> bool {
        self.events.iter().any(|event| {
            matches!(
                event.kind,
                ObservationEventKind::Reset
                    | ObservationEventKind::ChannelLoss
                    | ObservationEventKind::Offline
            )
        })
    }
}

fn valid_counter(value: u64) -> bool {
    value <= MAX_CAPACITY_BYTES && value.is_multiple_of(CAPACITY_ALIGNMENT_BYTES)
}

pub fn valid_capacity(value: u64) -> bool {
    value != 0 && valid_counter(value)
}

/// Preflight state.  This state is deliberately separate from lease
/// authorization: none of its variants means that host memory was granted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreflightState {
    /// No fresh host authority is available.
    HostUnavailable,
    /// The product has deliberately not enabled an N3 tier.
    ProductOff,
    /// A bounded, fresh host observation was accepted.
    Observing,
    /// Host pressure constrains any possible future grant.
    Constrained,
    /// Guest emitted a demotion intent; this is not host authorization.
    DemotionRequested,
    /// Contract or ownership validation refused the record.
    Refused,
}

/// Pure preflight action emitted for an observation or refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreflightAction {
    /// A new observation was accepted.
    Observed,
    /// A host-pressure observation entered the constrained state.
    Constrained,
    /// Guest emitted an advisory demotion intent.
    DemotionRequested,
    /// Exact duplicate observation had no second effect.
    Idempotent,
    /// The record was refused and must not be blindly retried.
    Refused(FailureReason),
    /// Freshness/clock state is unavailable without authorizing anything.
    Unavailable(FailureReason),
}

/// Result of applying one preflight record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreflightDecision {
    pub  state: PreflightState,
    pub action: PreflightAction,
    /// Invalid contract records are deterministic and never auto-retried.
    pub retry_allowed: bool,
}

impl PreflightDecision {
    fn observed(state: PreflightState) -> Self {
        Self {
            state,
            action: if state == PreflightState::Constrained {
                PreflightAction::Constrained
            } else {
                PreflightAction::Observed
            },
            retry_allowed: true,
        }
    }

    fn idempotent(state: PreflightState) -> Self {
        Self {
            state,
            action: PreflightAction::Idempotent,
            retry_allowed: true,
        }
    }

    fn refused(reason: FailureReason) -> Self {
        Self {
            state: PreflightState::Refused,
            action: PreflightAction::Refused(reason),
            retry_allowed: false,
        }
    }

    fn unavailable(reason: FailureReason) -> Self {
        Self {
            state: PreflightState::HostUnavailable,
            action: PreflightAction::Unavailable(reason),
            retry_allowed: true,
        }
    }
}

/// Deterministic preflight model retained by the lease machine.
#[derive(Clone, Debug)]
pub struct PreflightModel {
    pub(crate) state: PreflightState,
    last_observation: Option<HostObservation>,
    observation_count: u64,
    refusal_count: u64,
}

impl Default for PreflightModel {
    fn default() -> Self {
        Self::new()
    }
}

impl PreflightModel {
    /// Starts without host authority and without a native lease.
    pub fn new() -> Self {
        Self {
            state: PreflightState::HostUnavailable,
            last_observation: None,
            observation_count: 0,
            refusal_count: 0,
        }
    }

    /// Applies one complete observation atomically.
    pub fn observe(&mut self, observation: HostObservation, now: u64) -> PreflightDecision {
        if let Some(previous) = &self.last_observation {
            if observation.event_id == previous.event_id && observation != *previous {
                self.refusal_count = self.refusal_count.saturating_add(1);
                self.state = PreflightState::Refused;
                return PreflightDecision::refused(FailureReason::ConflictingDuplicate);
            }
            if observation.host_epoch < previous.host_epoch {
                self.refusal_count = self.refusal_count.saturating_add(1);
                self.state = PreflightState::Refused;
                return PreflightDecision::refused(FailureReason::EpochRegression);
            }
            if observation.host_epoch == previous.host_epoch {
                if observation == *previous {
                    return PreflightDecision::idempotent(self.state);
                }
                self.refusal_count = self.refusal_count.saturating_add(1);
                self.state = PreflightState::Refused;
                return PreflightDecision::refused(FailureReason::ConflictingDuplicate);
            }
            if observation.adapter_id != previous.adapter_id {
                self.refusal_count = self.refusal_count.saturating_add(1);
                self.state = PreflightState::Refused;
                return PreflightDecision::refused(FailureReason::HostAuthorityRequired);
            }
        }

        match observation.validate(now) {
            Ok(()) => {
                self.observation_count = self.observation_count.saturating_add(1);
                self.last_observation = Some(observation.clone());
                if observation.has_offline_signal() {
                    self.state = PreflightState::HostUnavailable;
                    return PreflightDecision::unavailable(FailureReason::ChannelLoss);
                }
                self.state = if observation.has_pressure() || observation.budget_bytes == 0 {
                    PreflightState::Constrained
                } else {
                    PreflightState::Observing
                };
                PreflightDecision::observed(self.state)
            }
            Err(FailureReason::StaleObservation) => {
                self.state = PreflightState::HostUnavailable;
                PreflightDecision::unavailable(FailureReason::StaleObservation)
            }
            Err(FailureReason::InvalidObservationClock) => {
                self.state = PreflightState::HostUnavailable;
                PreflightDecision::unavailable(FailureReason::InvalidObservationClock)
            }
            Err(reason) => {
                self.refusal_count = self.refusal_count.saturating_add(1);
                self.state = PreflightState::Refused;
                PreflightDecision::refused(reason)
            }
        }
    }

    /// Emits an advisory demotion intent without creating a lease.
    pub fn request_demotion(&mut self) -> PreflightDecision {
        if self.state != PreflightState::Constrained {
            return PreflightDecision {
                state: self.state,
                action: PreflightAction::Unavailable(FailureReason::StateTransition(
                    StateTransitionError::IllegalPreflight {
                        expected: Some(PreflightState::Constrained),
                        actual: self.state,
                    },
                )),
                retry_allowed: true,
            };
        }
        self.state = PreflightState::DemotionRequested;
        PreflightDecision {
            state: self.state,
            action: PreflightAction::DemotionRequested,
            retry_allowed: true,
        }
    }

    /// Explicitly refuses a guest residency claim at the boundary.
    pub fn refuse_guest_claim(&mut self) -> PreflightDecision {
        self.refusal_count = self.refusal_count.saturating_add(1);
        self.state = PreflightState::Refused;
        PreflightDecision::refused(FailureReason::GuestResidencyClaim)
    }

    /// Explicitly refuses a product transport decision outside N3.
    pub fn refuse_product_scope(&mut self) -> PreflightDecision {
        self.refusal_count = self.refusal_count.saturating_add(1);
        self.state = PreflightState::Refused;
        PreflightDecision::refused(FailureReason::ProductScope)
    }

    pub fn state(&self) -> PreflightState {
        self.state
    }

    pub fn latest_observation(&self) -> Option<&HostObservation> {
        self.last_observation.as_ref()
    }

    pub fn observation_count(&self) -> u64 {
        self.observation_count
    }

    pub fn refusal_count(&self) -> u64 {
        self.refusal_count
    }
}
