use super::*;

/// Host grant event.  It is validated as a whole before a lease is installed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Grant {
    pub contract_version: u16,
    pub lease_id: LeaseId,
    pub generation: u64,
    pub event_id: EventId,
    pub capacity_bytes: u64,
    pub host_epoch: u64,
    pub issued_at: u64,
    pub deadline: u64,
    pub expected_state: Option<StateTag>,
}

impl Grant {
    /// Convenience constructor for the current public schema.
    pub fn host(
        lease_id: LeaseId,
        generation: u64,
        event_id: EventId,
        capacity_bytes: u64,
        host_epoch: u64,
        issued_at: u64,
        deadline: u64,
    ) -> Self {
        Self::new(
            N3_SCHEMA_VERSION,
            lease_id,
            generation,
            event_id,
            capacity_bytes,
            host_epoch,
            issued_at,
            deadline,
        )
    }

    /// Creates a host grant event without applying it to a machine.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        contract_version: u16,
        lease_id: LeaseId,
        generation: u64,
        event_id: EventId,
        capacity_bytes: u64,
        host_epoch: u64,
        issued_at: u64,
        deadline: u64,
    ) -> Self {
        Self {
            contract_version,
            lease_id,
            generation,
            event_id,
            capacity_bytes,
            host_epoch,
            issued_at,
            deadline,
            expected_state: None,
        }
    }

    pub fn with_capacity_bytes(mut self, capacity_bytes: u64) -> Self {
        self.capacity_bytes = capacity_bytes;
        self
    }

    pub fn with_expected_state(mut self, expected_state: StateTag) -> Self {
        self.expected_state = Some(expected_state);
        self
    }
}

/// Host revoke event.  Revoke blocks new I/O before any drain decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Revoke {
    pub lease_id: LeaseId,
    pub generation: u64,
    pub event_id: EventId,
    pub deadline: u64,
    pub expected_state: Option<StateTag>,
}

impl Revoke {
    /// Convenience constructor for an exact active lease identity.
    pub fn host(lease_id: LeaseId, generation: u64, event_id: EventId, deadline: u64) -> Self {
        Self::new(lease_id, generation, event_id, deadline)
    }

    pub fn new(lease_id: LeaseId, generation: u64, event_id: EventId, deadline: u64) -> Self {
        Self {
            lease_id,
            generation,
            event_id,
            deadline,
            expected_state: None,
        }
    }

    pub fn with_expected_state(mut self, expected_state: StateTag) -> Self {
        self.expected_state = Some(expected_state);
        self
    }
}

/// Host completion after it has accepted a guest `DRAIN_ACK`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevokeCompletion {
    pub lease_id: LeaseId,
    pub generation: u64,
    pub event_id: EventId,
}

impl RevokeCompletion {
    pub fn new(lease_id: LeaseId, generation: u64, event_id: EventId) -> Self {
        Self {
            lease_id,
            generation,
            event_id,
        }
    }
}

/// Guest acknowledgement for a host grant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GrantAck {
    pub contract_version: u16,
    pub lease_id: LeaseId,
    pub generation: u64,
    pub event_id: EventId,
}

/// Guest acknowledgement after all drain and privacy conditions are complete.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DrainAck {
    pub lease_id: LeaseId,
    pub generation: u64,
    pub event_id: EventId,
    pub in_flight: u32,
    pub callbacks_pending: u32,
    pub scrubbed: bool,
}

/// Guest failure acknowledgement.  It never asserts that the lease drained.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailAck {
    pub lease_id: Option<LeaseId>,
    pub generation: Option<u64>,
    pub event_id: Option<EventId>,
    pub reason: FailureReason,
}

/// State tag used by optional event prior-state expectations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateTag {
    Absent,
    Negotiating,
    Granted,
    Quiescing,
    Drained,
    Revoked,
    Failed,
}

/// Exact host-led lease lifecycle.  No preflight variant is represented here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LeaseState {
    /// No active or pending lease exists.
    Absent,
    /// A complete host grant is awaiting host acceptance of `GRANT_ACK`.
    Negotiating(u64),
    /// Host has accepted the guest acknowledgement for this generation.
    Granted(u64),
    /// Revoke has blocked new I/O and awaits drain/scrub.
    Quiescing(u64),
    /// Guest drain proof was sent; host completion is still required.
    Drained(u64),
    /// Host completed revoke; cleanup returns to `Absent`.
    Revoked,
    /// Fail-closed terminal state until cleanup or restart.
    Failed(FailureReason),
}

/// Descriptive aliases for integrations that call the model N3 state.
pub type N3State = LeaseState;
pub type N3StateMachine = LeaseMachine;

impl LeaseState {
    pub fn tag(self) -> StateTag {
        match self {
            Self::Absent => StateTag::Absent,
            Self::Negotiating(_) => StateTag::Negotiating,
            Self::Granted(_) => StateTag::Granted,
            Self::Quiescing(_) => StateTag::Quiescing,
            Self::Drained(_) => StateTag::Drained,
            Self::Revoked => StateTag::Revoked,
            Self::Failed(_) => StateTag::Failed,
        }
    }

    pub fn generation(self) -> Option<u64> {
        match self {
            Self::Negotiating(generation)
            | Self::Granted(generation)
            | Self::Quiescing(generation)
            | Self::Drained(generation) => Some(generation),
            Self::Absent | Self::Revoked | Self::Failed(_) => None,
        }
    }

    pub fn is_granted(self) -> bool {
        matches!(self, Self::Granted(_))
    }
}

/// Host/guest lifecycle events that invalidate a lease without a safe revoke.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleEvent {
    Reset,
    ChannelLoss,
    LeaseExpired,
    WslRestart,
    Suspend,
    DriverUpgrade,
    GuestCrash,
}

/// Explicit guest claims rejected at the N3 boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuestClaim {
    PfnRange { start: u64, count: u64 },
    NumaNode(u32),
    DevicePrivate,
    PhysicalResidency,
}

/// Product or upstream choices intentionally owned by other SPECs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProductTransport {
    Nbd,
    Ublk,
    WindowsDriver,
    UpstreamContribution,
}

/// Pure scrub result.  The model records completion; it never touches memory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScrubResult {
    Pending,
    Succeeded,
    Failed,
}

/// Host-originated event dispatcher for callers that want one event surface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostEvent {
    Grant(Grant),
    GrantAckAccepted(GrantAck),
    Revoke(Revoke),
    RevokeCompleted(RevokeCompletion),
    Lifecycle(LifecycleEvent),
}

pub type ProtocolEvent = HostEvent;

/// Decision emitted by the pure protocol model.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProtocolDecision {
    Noop,
    GrantAck(GrantAck),
    Accepted(LeaseState),
    BeginDrain,
    DrainAck(DrainAck),
    Revoked,
    FailAck(FailAck),
    Blocked(FailureReason),
    Refused(FailureReason),
}

/// Alias used by callers that call all output a guest decision.
pub type GuestDecision = ProtocolDecision;

#[derive(Clone, Debug, Eq, PartialEq)]
enum EventFingerprint {
    Grant(Grant),
    Revoke(Revoke),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SeenEvent {
    event_id: EventId,
    fingerprint: EventFingerprint,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ActiveLease {
    lease_id: LeaseId,
    generation: u64,
    grant_event_id: EventId,
    capacity_bytes: u64,
    revoke: Option<Revoke>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScrubState {
    Pending,
    Succeeded,
    Failed,
}

impl From<ScrubResult> for ScrubState {
    fn from(result: ScrubResult) -> Self {
        match result {
            ScrubResult::Pending => Self::Pending,
            ScrubResult::Succeeded => Self::Succeeded,
            ScrubResult::Failed => Self::Failed,
        }
    }
}

/// Pure N3 model: bounded observation preflight plus host-led lease protocol.
#[derive(Clone, Debug)]
pub struct LeaseMachine {
    preflight: PreflightModel,
    lease_state: LeaseState,
    pending_grant: Option<Grant>,
    active_lease: Option<ActiveLease>,
    seen_events: Vec<SeenEvent>,
    accepted_grant_acks: Vec<GrantAck>,
    completed_revokes: Vec<RevokeCompletion>,
    generation_history: Vec<(LeaseId, u64)>,
    restored_restart_epoch: Option<u64>,
    in_flight: u32,
    callbacks_pending: u32,
    unknown_in_flight: bool,
    scrub_state: ScrubState,
    sent_drain_ack: bool,
}

impl Default for LeaseMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl LeaseMachine {
    /// Starts with no host observation and no lease authorization.
    pub fn new() -> Self {
        Self {
            preflight: PreflightModel::new(),
            lease_state: LeaseState::Absent,
            pending_grant: None,
            active_lease: None,
            seen_events: Vec::new(),
            accepted_grant_acks: Vec::new(),
            completed_revokes: Vec::new(),
            generation_history: Vec::new(),
            restored_restart_epoch: None,
            in_flight: 0,
            callbacks_pending: 0,
            unknown_in_flight: false,
            scrub_state: ScrubState::Pending,
            sent_drain_ack: false,
        }
    }

    /// Applies a preflight observation; this cannot change the lease state.
    pub fn observe(&mut self, observation: HostObservation, now: u64) -> PreflightDecision {
        self.preflight.observe(observation, now)
    }

    pub fn preflight_state(&self) -> PreflightState {
        self.preflight.state()
    }

    pub fn lease_state(&self) -> LeaseState {
        self.lease_state
    }

    pub fn state(&self) -> LeaseState {
        self.lease_state
    }

    pub fn has_host_grant(&self) -> bool {
        self.active_lease.is_some() || self.pending_grant.is_some()
    }

    pub fn observation_count(&self) -> u64 {
        self.preflight.observation_count()
    }

    pub fn refusal_count(&self) -> u64 {
        self.preflight.refusal_count()
    }

    pub fn sent_drain_ack(&self) -> bool {
        self.sent_drain_ack
    }

    pub fn active_lease_id(&self) -> Option<&LeaseId> {
        self.active_lease.as_ref().map(|lease| &lease.lease_id)
    }

    pub fn lease_capacity_bytes(&self) -> Option<u64> {
        self.active_lease.as_ref().map(|lease| lease.capacity_bytes)
    }

    pub fn in_flight(&self) -> u32 {
        self.in_flight
    }

    pub fn callbacks_pending(&self) -> u32 {
        self.callbacks_pending
    }

    pub fn scrubbed(&self) -> bool {
        self.scrub_state == ScrubState::Succeeded
    }

    /// Returns the host epoch supplied by a successfully restored restart record.
    pub fn restored_restart_epoch(&self) -> Option<u64> {
        self.restored_restart_epoch
    }

    /// Decodes a caller-supplied bounded restart record, then atomically seeds
    /// a fresh model's generation history. This pure method performs no I/O.
    pub fn restore_restart_bytes(&mut self, bytes: &[u8]) -> Result<(), FailureReason> {
        let record = match RestartRecord::from_bytes(bytes) {
            Ok(record) => record,
            Err(reason) => return Err(self.fail_restart_restore(reason)),
        };
        self.restore_restart_record(record)
    }

    /// Atomically seeds a fresh model from an already-decoded host record.
    ///
    /// A caller must restore before observations or protocol events. Any invalid
    /// input or non-fresh state fails closed without replacing existing history.
    pub fn restore_restart_record(&mut self, record: RestartRecord) -> Result<(), FailureReason> {
        if let Err(reason) = record.validate() {
            return Err(self.fail_restart_restore(reason));
        }
        if self.lease_state != LeaseState::Absent
            || self.pending_grant.is_some()
            || self.active_lease.is_some()
            || !self.seen_events.is_empty()
            || !self.accepted_grant_acks.is_empty()
            || !self.completed_revokes.is_empty()
            || !self.generation_history.is_empty()
            || self.restored_restart_epoch.is_some()
            || self.preflight.observation_count() != 0
        {
            return Err(self.fail_restart_restore(FailureReason::StateTransition(
                StateTransitionError::IllegalTransition {
                    expected: Some(StateTag::Absent),
                    actual: self.lease_state.tag(),
                },
            )));
        }

        let host_epoch = record.host_epoch();
        let generation_history = record
            .into_checkpoints()
            .into_iter()
            .map(|checkpoint| (checkpoint.lease_id, checkpoint.generation))
            .collect();
        self.generation_history = generation_history;
        self.restored_restart_epoch = Some(host_epoch);
        Ok(())
    }

    pub fn request_demotion(&mut self) -> PreflightDecision {
        self.preflight.request_demotion()
    }

    pub fn refuse_guest_claim(&mut self, _claim: GuestClaim) -> PreflightDecision {
        self.preflight.refuse_guest_claim()
    }

    pub fn refuse_product_transport(&mut self, _transport: ProductTransport) -> PreflightDecision {
        self.preflight.refuse_product_scope()
    }

    /// Dispatches one host event through the same bounded transition methods.
    pub fn apply_host_event(&mut self, event: HostEvent, now: u64) -> ProtocolDecision {
        match event {
            HostEvent::Grant(grant) => self.receive_grant(grant, now),
            HostEvent::GrantAckAccepted(ack) => self.accept_grant_ack(ack),
            HostEvent::Revoke(revoke) => self.receive_revoke(revoke),
            HostEvent::RevokeCompleted(completion) => self.confirm_revoke(completion),
            HostEvent::Lifecycle(event) => self.fail_closed(event),
        }
    }

    /// Receives a host `GRANT`.  Validation enters `NEGOTIATING`; only a later
    /// host acceptance of the returned `GRANT_ACK` enters `GRANTED`.
    pub fn receive_grant(&mut self, grant: Grant, now: u64) -> ProtocolDecision {
        match self.register_event(EventFingerprint::Grant(grant.clone())) {
            EventRegistration::Duplicate => return ProtocolDecision::Noop,
            EventRegistration::Conflict => {
                return self.fail_for_event(
                    Some(grant.lease_id),
                    Some(grant.generation),
                    Some(grant.event_id),
                    FailureReason::ConflictingDuplicate,
                );
            }
            EventRegistration::Overflow => {
                return self.fail_for_event(
                    Some(grant.lease_id),
                    Some(grant.generation),
                    Some(grant.event_id),
                    FailureReason::MalformedRecord,
                );
            }
            EventRegistration::New => {}
        }

        if self.lease_state != LeaseState::Absent {
            return self.fail_for_event(
                Some(grant.lease_id),
                Some(grant.generation),
                Some(grant.event_id),
                FailureReason::StateTransition(StateTransitionError::IllegalTransition {
                    expected: Some(StateTag::Absent),
                    actual: self.lease_state.tag(),
                }),
            );
        }
        if grant
            .expected_state
            .is_some_and(|state| state != StateTag::Absent)
        {
            return self.fail_for_event(
                Some(grant.lease_id),
                Some(grant.generation),
                Some(grant.event_id),
                FailureReason::StateTransition(StateTransitionError::IllegalTransition {
                    expected: Some(StateTag::Absent),
                    actual: self.lease_state.tag(),
                }),
            );
        }
        if grant.contract_version != N3_SCHEMA_VERSION
            || grant.lease_id.as_bytes().is_empty()
            || grant.event_id.as_bytes().is_empty()
            || grant.generation == 0
            || grant.host_epoch == 0
            || grant.deadline <= grant.issued_at
            || now < grant.issued_at
            || now >= grant.deadline
        {
            return self.fail_for_event(
                Some(grant.lease_id),
                Some(grant.generation),
                Some(grant.event_id),
                FailureReason::MalformedRecord,
            );
        }
        if self
            .restored_restart_epoch
            .is_some_and(|epoch| grant.host_epoch < epoch)
        {
            return self.fail_for_event(
                Some(grant.lease_id),
                Some(grant.generation),
                Some(grant.event_id),
                FailureReason::StateTransition(StateTransitionError::StaleGeneration {
                    provided: grant.host_epoch,
                    expected: self.restored_restart_epoch.unwrap_or(0),
                }),
            );
        }
        let Some(observation) = self.preflight.latest_observation() else {
            return self.fail_for_event(
                Some(grant.lease_id),
                Some(grant.generation),
                Some(grant.event_id),
                FailureReason::NoFreshHostObservation,
            );
        };
        if observation.validate(now).is_err()
            || observation.host_epoch != grant.host_epoch
            || observation.budget_bytes < grant.capacity_bytes
            || !matches!(
                self.preflight.state(),
                PreflightState::Observing
                    | PreflightState::Constrained
                    | PreflightState::DemotionRequested
            )
        {
            return self.fail_for_event(
                Some(grant.lease_id),
                Some(grant.generation),
                Some(grant.event_id),
                FailureReason::NoFreshHostObservation,
            );
        }
        if !valid_capacity(grant.capacity_bytes) {
            return self.fail_for_event(
                Some(grant.lease_id),
                Some(grant.generation),
                Some(grant.event_id),
                FailureReason::InvalidCapacity,
            );
        }
        match self.validate_generation(&grant.lease_id, grant.generation) {
            Ok(()) => {}
            Err(reason) => {
                return self.fail_for_event(
                    Some(grant.lease_id),
                    Some(grant.generation),
                    Some(grant.event_id),
                    reason,
                );
            }
        }
        self.remember_generation(grant.lease_id.clone(), grant.generation);
        let ack = GrantAck {
            contract_version: grant.contract_version,
            lease_id: grant.lease_id.clone(),
            generation: grant.generation,
            event_id: grant.event_id.clone(),
        };
        self.lease_state = LeaseState::Negotiating(grant.generation);
        self.pending_grant = Some(grant);
        ProtocolDecision::GrantAck(ack)
    }

    /// Host acceptance of a previously emitted `GRANT_ACK`.
    pub fn accept_grant_ack(&mut self, ack: GrantAck) -> ProtocolDecision {
        if self.accepted_grant_acks.iter().any(|seen| seen == &ack) {
            return ProtocolDecision::Noop;
        }
        let Some(grant) = self.pending_grant.clone() else {
            return self.fail_for_event(
                Some(ack.lease_id),
                Some(ack.generation),
                Some(ack.event_id),
                FailureReason::StateTransition(StateTransitionError::IllegalTransition {
                    expected: Some(StateTag::Negotiating),
                    actual: self.lease_state.tag(),
                }),
            );
        };
        if self.lease_state != LeaseState::Negotiating(grant.generation)
            || ack.contract_version != grant.contract_version
            || ack.lease_id != grant.lease_id
            || ack.generation != grant.generation
            || ack.event_id != grant.event_id
        {
            return self.fail_for_event(
                Some(ack.lease_id),
                Some(ack.generation),
                Some(ack.event_id),
                FailureReason::LeaseIdentityMismatch,
            );
        }
        if self.accepted_grant_acks.len() >= MAX_PROTOCOL_EVENT_HISTORY {
            return self.fail_for_active(FailureReason::MalformedRecord);
        }
        self.accepted_grant_acks.push(ack);
        self.active_lease = Some(ActiveLease {
            lease_id: grant.lease_id,
            generation: grant.generation,
            grant_event_id: grant.event_id,
            capacity_bytes: grant.capacity_bytes,
            revoke: None,
        });
        self.pending_grant = None;
        self.lease_state = LeaseState::Granted(grant.generation);
        ProtocolDecision::Accepted(self.lease_state)
    }

    /// Alias for host-facing terminology.
    pub fn host_accepts_grant_ack(&mut self, ack: GrantAck) -> ProtocolDecision {
        self.accept_grant_ack(ack)
    }

    /// Receives a matching host `REVOKE`, blocks new I/O, and begins drain.
    pub fn receive_revoke(&mut self, revoke: Revoke) -> ProtocolDecision {
        match self.register_event(EventFingerprint::Revoke(revoke.clone())) {
            EventRegistration::Duplicate => return ProtocolDecision::Noop,
            EventRegistration::Conflict => {
                return self.fail_for_event(
                    Some(revoke.lease_id),
                    Some(revoke.generation),
                    Some(revoke.event_id),
                    FailureReason::ConflictingDuplicate,
                );
            }
            EventRegistration::Overflow => {
                return self.fail_for_event(
                    Some(revoke.lease_id),
                    Some(revoke.generation),
                    Some(revoke.event_id),
                    FailureReason::MalformedRecord,
                );
            }
            EventRegistration::New => {}
        }
        let Some(active) = self.active_lease.as_mut() else {
            return self.fail_for_event(
                Some(revoke.lease_id),
                Some(revoke.generation),
                Some(revoke.event_id),
                FailureReason::StateTransition(StateTransitionError::IllegalTransition {
                    expected: Some(StateTag::Granted),
                    actual: self.lease_state.tag(),
                }),
            );
        };
        if self.lease_state != LeaseState::Granted(active.generation)
            || revoke.lease_id != active.lease_id
            || revoke.generation != active.generation
            || revoke.deadline == 0
        {
            return self.fail_for_event(
                Some(revoke.lease_id),
                Some(revoke.generation),
                Some(revoke.event_id),
                FailureReason::LeaseIdentityMismatch,
            );
        }
        if revoke
            .expected_state
            .is_some_and(|state| state != StateTag::Granted)
        {
            return self.fail_for_event(
                Some(revoke.lease_id),
                Some(revoke.generation),
                Some(revoke.event_id),
                FailureReason::StateTransition(StateTransitionError::IllegalTransition {
                    expected: Some(StateTag::Granted),
                    actual: self.lease_state.tag(),
                }),
            );
        }
        active.revoke = Some(revoke);
        self.lease_state = LeaseState::Quiescing(active.generation);
        self.scrub_state = ScrubState::Pending;
        self.sent_drain_ack = false;
        ProtocolDecision::BeginDrain
    }

    /// Begins one bounded I/O operation while the exact host grant is active.
    pub fn begin_io(&mut self) -> Result<(), FailureReason> {
        if self.lease_state != LeaseState::Granted(self.active_generation().unwrap_or(0)) {
            return Err(FailureReason::IoNotGranted);
        }
        if self.in_flight >= MAX_IN_FLIGHT {
            self.unknown_in_flight = true;
            return Err(FailureReason::UnknownInFlight);
        }
        self.in_flight += 1;
        Ok(())
    }

    /// Sets a deterministic in-flight count for a test or adapter boundary.
    pub fn set_in_flight(&mut self, count: u32) -> Result<(), FailureReason> {
        if count > MAX_IN_FLIGHT {
            self.unknown_in_flight = true;
            return Err(FailureReason::UnknownInFlight);
        }
        self.in_flight = count;
        Ok(())
    }

    pub fn complete_io(&mut self) -> Result<(), FailureReason> {
        if self.in_flight == 0 {
            self.unknown_in_flight = true;
            return Err(FailureReason::UnknownInFlight);
        }
        self.in_flight -= 1;
        Ok(())
    }

    pub fn set_callbacks_pending(&mut self, count: u32) -> Result<(), FailureReason> {
        if count > MAX_IN_FLIGHT {
            self.unknown_in_flight = true;
            return Err(FailureReason::UnknownInFlight);
        }
        self.callbacks_pending = count;
        Ok(())
    }

    pub fn mark_unknown_in_flight(&mut self) {
        self.unknown_in_flight = true;
    }

    /// Records the result of the guest-visible scrub.  No bytes are touched by
    /// this pure module; `Succeeded` is evidence supplied by its owner.
    pub fn scrub_guest_data(&mut self, result: ScrubResult) -> ProtocolDecision {
        self.scrub_state = result.into();
        if result == ScrubResult::Failed {
            return self.fail_for_active(FailureReason::ScrubFailed);
        }
        ProtocolDecision::Noop
    }

    /// Attempts to finish a revoke drain before its host deadline.
    pub fn drain(&mut self, now: u64) -> ProtocolDecision {
        let Some(active) = self.active_lease.as_ref() else {
            return self.fail_for_event(
                None,
                None,
                None,
                FailureReason::StateTransition(StateTransitionError::IllegalTransition {
                    expected: Some(StateTag::Granted),
                    actual: self.lease_state.tag(),
                }),
            );
        };
        let Some(revoke) = active.revoke.as_ref() else {
            return self.fail_for_active(FailureReason::StateTransition(
                StateTransitionError::IllegalTransition {
                    expected: Some(StateTag::Quiescing),
                    actual: self.lease_state.tag(),
                },
            ));
        };
        if self.lease_state != LeaseState::Quiescing(active.generation) {
            return self.fail_for_active(FailureReason::StateTransition(
                StateTransitionError::IllegalTransition {
                    expected: Some(StateTag::Quiescing),
                    actual: self.lease_state.tag(),
                },
            ));
        }
        if now >= revoke.deadline {
            return self.fail_for_active(FailureReason::DrainTimeout);
        }
        if self.unknown_in_flight {
            return self.fail_for_active(FailureReason::UnknownInFlight);
        }
        if self.in_flight != 0 {
            return self.fail_for_active(FailureReason::InFlightNotDrained);
        }
        if self.callbacks_pending != 0 {
            return self.fail_for_active(FailureReason::CallbackNotDrained);
        }
        match self.scrub_state {
            ScrubState::Pending => return ProtocolDecision::Blocked(FailureReason::ScrubPending),
            ScrubState::Failed => return self.fail_for_active(FailureReason::ScrubFailed),
            ScrubState::Succeeded => {}
        }
        let ack = DrainAck {
            lease_id: active.lease_id.clone(),
            generation: active.generation,
            event_id: revoke.event_id.clone(),
            in_flight: self.in_flight,
            callbacks_pending: self.callbacks_pending,
            scrubbed: true,
        };
        self.sent_drain_ack = true;
        self.lease_state = LeaseState::Drained(active.generation);
        ProtocolDecision::DrainAck(ack)
    }

    /// Host completion is required after `DRAIN_ACK`; absence or contradiction
    /// fails closed rather than assuming physical host zeroing.
    pub fn confirm_revoke(&mut self, completion: RevokeCompletion) -> ProtocolDecision {
        if self
            .completed_revokes
            .iter()
            .any(|seen| seen == &completion)
        {
            return ProtocolDecision::Noop;
        }
        let Some(active) = self.active_lease.as_ref() else {
            return self.fail_for_event(
                Some(completion.lease_id),
                Some(completion.generation),
                Some(completion.event_id),
                FailureReason::MissingRevokeCompletion,
            );
        };
        let Some(revoke) = active.revoke.as_ref() else {
            return self.fail_for_active(FailureReason::MissingRevokeCompletion);
        };
        if self.lease_state != LeaseState::Drained(active.generation)
            || completion.lease_id != active.lease_id
            || completion.generation != active.generation
            || completion.event_id != revoke.event_id
        {
            return self.fail_for_event(
                Some(completion.lease_id),
                Some(completion.generation),
                Some(completion.event_id),
                FailureReason::MissingRevokeCompletion,
            );
        }
        if self.completed_revokes.len() >= MAX_PROTOCOL_EVENT_HISTORY {
            return self.fail_for_active(FailureReason::MalformedRecord);
        }
        self.completed_revokes.push(completion);
        self.lease_state = LeaseState::Revoked;
        ProtocolDecision::Revoked
    }

    /// Completes local cleanup after `REVOKED` or `FAILED`.
    pub fn complete_cleanup(&mut self) {
        if self.active_lease.is_some() && self.scrub_state != ScrubState::Succeeded {
            return;
        }
        if matches!(
            self.lease_state,
            LeaseState::Revoked | LeaseState::Failed(_)
        ) {
            self.pending_grant = None;
            self.active_lease = None;
            self.in_flight = 0;
            self.callbacks_pending = 0;
            self.unknown_in_flight = false;
            self.scrub_state = ScrubState::Pending;
            self.sent_drain_ack = false;
            self.lease_state = LeaseState::Absent;
        }
    }

    /// A crash/restart always discards the active lease but retains generation
    /// history so the old host identity cannot be replayed.
    pub fn restart(&mut self) {
        self.pending_grant = None;
        self.active_lease = None;
        self.seen_events.clear();
        self.accepted_grant_acks.clear();
        self.completed_revokes.clear();
        self.in_flight = 0;
        self.callbacks_pending = 0;
        self.unknown_in_flight = false;
        self.scrub_state = ScrubState::Pending;
        self.sent_drain_ack = false;
        self.lease_state = LeaseState::Absent;
        self.preflight = PreflightModel::new();
    }

    /// Invalidates the current lease for reset, channel, liveness, or crash.
    pub fn fail_closed(&mut self, event: LifecycleEvent) -> ProtocolDecision {
        let reason = match event {
            LifecycleEvent::Reset => FailureReason::Reset,
            LifecycleEvent::ChannelLoss => FailureReason::ChannelLoss,
            LifecycleEvent::LeaseExpired => FailureReason::LeaseExpired,
            LifecycleEvent::WslRestart => FailureReason::WslRestart,
            LifecycleEvent::Suspend => FailureReason::Suspend,
            LifecycleEvent::DriverUpgrade => FailureReason::DriverUpgrade,
            LifecycleEvent::GuestCrash => FailureReason::GuestCrash,
        };
        if self.active_lease.is_none() && self.pending_grant.is_none() {
            self.lease_state = LeaseState::Failed(reason);
            return ProtocolDecision::FailAck(FailAck {
                lease_id: None,
                generation: None,
                event_id: None,
                reason,
            });
        }
        self.fail_for_active(reason)
    }

    fn fail_for_active(&mut self, reason: FailureReason) -> ProtocolDecision {
        let (lease_id, generation, event_id) = if let Some(active) = &self.active_lease {
            (
                Some(active.lease_id.clone()),
                Some(active.generation),
                Some(
                    active
                        .revoke
                        .as_ref()
                        .map_or(&active.grant_event_id, |revoke| &revoke.event_id)
                        .clone(),
                ),
            )
        } else if let Some(grant) = &self.pending_grant {
            (
                Some(grant.lease_id.clone()),
                Some(grant.generation),
                Some(grant.event_id.clone()),
            )
        } else {
            (None, None, None)
        };
        self.lease_state = LeaseState::Failed(reason);
        ProtocolDecision::FailAck(FailAck {
            lease_id,
            generation,
            event_id,
            reason,
        })
    }

    fn fail_for_event(
        &mut self,
        lease_id: Option<LeaseId>,
        generation: Option<u64>,
        event_id: Option<EventId>,
        reason: FailureReason,
    ) -> ProtocolDecision {
        self.pending_grant = None;
        self.lease_state = LeaseState::Failed(reason);
        ProtocolDecision::FailAck(FailAck {
            lease_id,
            generation,
            event_id,
            reason,
        })
    }

    fn fail_restart_restore(&mut self, reason: FailureReason) -> FailureReason {
        self.pending_grant = None;
        self.active_lease = None;
        self.lease_state = LeaseState::Failed(reason);
        reason
    }

    fn active_generation(&self) -> Option<u64> {
        self.active_lease.as_ref().map(|lease| lease.generation)
    }

    fn validate_generation(
        &self,
        lease_id: &LeaseId,
        generation: u64,
    ) -> Result<(), FailureReason> {
        if let Some((_, previous)) = self
            .generation_history
            .iter()
            .find(|(known_lease, _)| known_lease == lease_id)
        {
            if generation <= *previous {
                return Err(FailureReason::StateTransition(
                    StateTransitionError::StaleGeneration {
                        provided: generation,
                        expected: *previous,
                    },
                ));
            }
            if generation != previous.saturating_add(1) {
                return Err(FailureReason::GenerationGap);
            }
            return Ok(());
        }
        if self.generation_history.len() >= MAX_GENERATION_HISTORY {
            return Err(FailureReason::MalformedRecord);
        }
        Ok(())
    }

    fn remember_generation(&mut self, lease_id: LeaseId, generation: u64) {
        if let Some((_, previous)) = self
            .generation_history
            .iter_mut()
            .find(|(known_lease, _)| known_lease == &lease_id)
        {
            *previous = generation;
            return;
        }
        if self.generation_history.len() < MAX_GENERATION_HISTORY {
            self.generation_history.push((lease_id, generation));
        }
    }

    fn register_event(&mut self, fingerprint: EventFingerprint) -> EventRegistration {
        let event_id = match &fingerprint {
            EventFingerprint::Grant(grant) => grant.event_id.clone(),
            EventFingerprint::Revoke(revoke) => revoke.event_id.clone(),
        };
        if let Some(seen) = self
            .seen_events
            .iter()
            .find(|seen| seen.event_id == event_id)
        {
            if seen.fingerprint == fingerprint {
                return EventRegistration::Duplicate;
            }
            return EventRegistration::Conflict;
        }
        if self.seen_events.len() >= MAX_PROTOCOL_EVENT_HISTORY {
            return EventRegistration::Overflow;
        }
        self.seen_events.push(SeenEvent {
            event_id,
            fingerprint,
        });
        EventRegistration::New
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EventRegistration {
    New,
    Duplicate,
    Conflict,
    Overflow,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn test_initial_state() {
        let machine = LeaseMachine::new();
        assert_eq!(machine.lease_state(), LeaseState::Absent);
        assert_eq!(machine.preflight_state(), PreflightState::HostUnavailable);
    }

    #[test]
    fn test_request_demotion() {
        let mut model = PreflightModel::new();
        // Requesting demotion when not constrained should be an invalid transition
        let decision = model.request_demotion();
        assert_eq!(
            decision.action,
            PreflightAction::Unavailable(FailureReason::StateTransition(
                StateTransitionError::IllegalPreflight {
                    expected: Some(PreflightState::Constrained),
                    actual: model.state
                }
            ))
        );

        // Explicitly set state to other non-constrained values
        model.state = PreflightState::Observing;
        let decision2 = model.request_demotion();
        assert_eq!(
            decision2.action,
            PreflightAction::Unavailable(FailureReason::StateTransition(
                StateTransitionError::IllegalPreflight {
                    expected: Some(PreflightState::Constrained),
                    actual: PreflightState::Observing
                }
            ))
        );

        // Let's create a constrained state by observing an empty budget
        let event_id =
            EventId::new(b"event-1").unwrap_or_else(|_| panic!("failed to create event_id"));
        let adapter_id = AdapterId::new(b"adapter-1").unwrap();

        let observation = HostObservation::new(
            N3_SCHEMA_VERSION,
            1,
            adapter_id,
            Authority::Host,
            0, // Budget 0 -> Constrained
            0,
            0,
            100,
            MAX_OBSERVATION_AGE,
            event_id,
            Vec::new(),
        );

        let decision = model.observe(observation, 100);
        assert_eq!(decision.state, PreflightState::Constrained);

        // Now request demotion should succeed
        let decision = model.request_demotion();
        assert_eq!(decision.state, PreflightState::DemotionRequested);
        assert_eq!(decision.action, PreflightAction::DemotionRequested);
    }

    #[test]
    fn test_restart_record_serialization() {
        let lease_id = LeaseId::new(b"lease-abc").unwrap();
        let checkpoint = GenerationCheckpoint {
            lease_id: lease_id.clone(),
            generation: 42,
        };

        let record = RestartRecord::host(10, vec![checkpoint.clone()]).unwrap();
        let bytes = record.to_bytes();

        let decoded = RestartRecord::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.host_epoch(), 10);

        let checkpoints = decoded.checkpoints();
        assert_eq!(checkpoints.len(), 1);
        assert_eq!(checkpoints[0].lease_id, lease_id);
        assert_eq!(checkpoints[0].generation, 42);
    }

    #[test]
    fn test_invalid_revoke_transition() {
        let mut machine = LeaseMachine::new();
        let lease_id =
            LeaseId::new(b"lease-1").unwrap_or_else(|_| panic!("failed to create lease_id"));
        let event_id =
            EventId::new(b"event-1").unwrap_or_else(|_| panic!("failed to create event_id"));

        let revoke = Revoke::host(lease_id, 1, event_id, 200);

        // Applying Revoke to an Absent state should result in an invalid transition error
        let decision = machine.receive_revoke(revoke);
        match decision {
            ProtocolDecision::FailAck(fail_ack) => {
                assert_eq!(
                    fail_ack.reason,
                    FailureReason::StateTransition(StateTransitionError::IllegalTransition {
                        expected: Some(StateTag::Granted),
                        actual: StateTag::Absent
                    })
                );
            }
            _ => panic!("Expected FailAck"),
        }

        // The machine's state should now be Failed(InvalidTransition)
        assert_eq!(
            machine.lease_state(),
            LeaseState::Failed(FailureReason::StateTransition(
                StateTransitionError::IllegalTransition {
                    expected: Some(StateTag::Granted),
                    actual: StateTag::Absent
                }
            ))
        );
    }
}

#[cfg(test)]
mod additional_tests {
    use super::*;

    #[test]
    fn test_guard_clauses_generation_validation() {
        let mut machine = LeaseMachine::new();
        let lease_id =
            LeaseId::new(b"lease-1").unwrap_or_else(|_| panic!("failed to create lease_id"));
        machine.remember_generation(lease_id.clone(), 10);

        assert_eq!(
            machine.validate_generation(&lease_id, 10),
            Err(FailureReason::StateTransition(
                StateTransitionError::StaleGeneration {
                    provided: 10,
                    expected: 10
                }
            ))
        );
        assert_eq!(
            machine.validate_generation(&lease_id, 9),
            Err(FailureReason::StateTransition(
                StateTransitionError::StaleGeneration {
                    provided: 9,
                    expected: 10
                }
            ))
        );
        assert_eq!(
            machine.validate_generation(&lease_id, 12),
            Err(FailureReason::GenerationGap)
        );
        assert_eq!(machine.validate_generation(&lease_id, 11), Ok(()));
    }

    #[test]
    fn test_opaque_id_validation() -> Result<(), FailureReason> {
        assert_eq!(LeaseId::new(b""), Err(FailureReason::MalformedRecord));
        let long_id = vec![b'a'; 65];
        assert_eq!(LeaseId::new(&long_id), Err(FailureReason::MalformedRecord));
        let valid = LeaseId::new(b"valid-lease-id")?;
        assert_eq!(valid.as_bytes(), b"valid-lease-id");
        Ok(())
    }

    #[test]
    fn test_restart_record_deserialization_errors() -> Result<(), FailureReason> {
        // Less than 17 bytes (header size)
        assert_eq!(
            RestartRecord::from_bytes(&[0u8; 13]),
            Err(FailureReason::MalformedRecord)
        );

        let valid_bytes = RestartRecord::host(1, Vec::new())?.to_bytes();

        // Unknown schema (bytes 4..6 is schema_version)
        let mut bad_schema = valid_bytes.clone();
        bad_schema[5] = 0x99;
        assert_eq!(
            RestartRecord::from_bytes(&bad_schema),
            Err(FailureReason::UnknownSchema)
        );

        // Guest authority marker (byte 6 is authority)
        let mut guest_auth = valid_bytes.clone();
        guest_auth[6] = GUEST_AUTHORITY_MARKER;
        assert_eq!(
            RestartRecord::from_bytes(&guest_auth),
            Err(FailureReason::HostAuthorityRequired)
        );
        Ok(())
    }

    #[test]
    fn test_grant_and_revoke_constructors() -> Result<(), FailureReason> {
        let lease_id = LeaseId::new(b"lease-1")?;
        let event_id = EventId::new(b"event-1")?;

        let grant = Grant::host(lease_id.clone(), 1, event_id.clone(), 4096, 1, 100, 200);
        assert_eq!(grant.lease_id, lease_id);
        assert_eq!(grant.capacity_bytes, 4096);

        let revoke = Revoke::host(lease_id.clone(), 1, event_id.clone(), 200);
        assert_eq!(revoke.lease_id, lease_id);
        assert_eq!(revoke.deadline, 200);
        Ok(())
    }

    #[test]
    fn test_state_transition_error_display() {
        use alloc::string::ToString;
        extern crate alloc;

        let err1 = StateTransitionError::IllegalTransition {
            expected: Some(StateTag::Granted),
            actual: StateTag::Absent,
        };
        assert_eq!(
            err1.to_string(),
            "illegal state transition: expected Granted, actual Absent"
        );

        let err2 = StateTransitionError::IllegalTransition {
            expected: None,
            actual: StateTag::Absent,
        };
        assert_eq!(err2.to_string(), "illegal state transition: actual Absent");

        let err3 = StateTransitionError::IllegalPreflight {
            expected: Some(PreflightState::Constrained),
            actual: PreflightState::HostUnavailable,
        };
        assert_eq!(
            err3.to_string(),
            "illegal preflight transition: expected Constrained, actual HostUnavailable"
        );

        let err4 = StateTransitionError::IllegalPreflight {
            expected: None,
            actual: PreflightState::HostUnavailable,
        };
        assert_eq!(
            err4.to_string(),
            "illegal preflight transition: actual HostUnavailable"
        );

        let err5 = StateTransitionError::StaleGeneration {
            provided: 5,
            expected: 10,
        };
        assert_eq!(
            err5.to_string(),
            "stale generation: provided 5, expected > 10"
        );
    }

    #[test]
    fn test_host_observation_validate() -> Result<(), FailureReason> {
        let valid_obs = HostObservation::host(1, 4096, 0, 4096, 100, 10, EventId::new(b"evt-1")?);

        // Happy path
        assert_eq!(valid_obs.validate(100), Ok(()));
        assert_eq!(valid_obs.validate(105), Ok(()));

        // Unknown schema
        assert_eq!(
            valid_obs.clone().with_schema_version(99).validate(100),
            Err(FailureReason::UnknownSchema)
        );

        // Zero epoch
        let mut zero_epoch = valid_obs.clone();
        zero_epoch.host_epoch = 0;
        assert_eq!(
            zero_epoch.validate(100),
            Err(FailureReason::MalformedRecord)
        );

        // Empty adapter ID
        let mut empty_adapter = valid_obs.clone();
        empty_adapter.adapter_id = OpaqueId(Vec::new());
        assert_eq!(
            empty_adapter.validate(100),
            Err(FailureReason::MalformedRecord)
        );

        // Empty event ID
        let mut empty_event = valid_obs.clone();
        empty_event.event_id = OpaqueId(Vec::new());
        assert_eq!(
            empty_event.validate(100),
            Err(FailureReason::MalformedRecord)
        );

        // Non-host authority
        let mut guest_auth = valid_obs.clone();
        guest_auth.authority = Authority::Guest;
        assert_eq!(
            guest_auth.validate(100),
            Err(FailureReason::HostAuthorityRequired)
        );

        // Invalid max_age (0 or > MAX_OBSERVATION_AGE)
        let mut zero_max_age = valid_obs.clone();
        zero_max_age.max_age = 0;
        assert_eq!(
            zero_max_age.validate(100),
            Err(FailureReason::MalformedRecord)
        );

        let mut excess_max_age = valid_obs.clone();
        excess_max_age.max_age = MAX_OBSERVATION_AGE + 1;
        assert_eq!(
            excess_max_age.validate(100),
            Err(FailureReason::MalformedRecord)
        );

        // Clock errors
        assert_eq!(
            valid_obs.validate(99),
            Err(FailureReason::InvalidObservationClock)
        );
        assert_eq!(
            valid_obs.validate(111),
            Err(FailureReason::StaleObservation)
        );

        // Observation events validation
        let mut too_many_events = valid_obs.clone();
        too_many_events.events = (0..=MAX_OBSERVATION_EVENTS)
            .map(|i| {
                let id_bytes = [b'e', i as u8];
                ObservationEvent::new(
                    EventId::new(id_bytes).unwrap_or_else(|_| panic!("failed to create event_id")),
                    ObservationEventKind::Healthy,
                )
            })
            .collect();
        assert_eq!(
            too_many_events.validate(100),
            Err(FailureReason::MalformedRecord)
        );

        let mut unknown_event_kind = valid_obs.clone();
        unknown_event_kind.events = vec![ObservationEvent::new(
            EventId::new(b"evt-unk")?,
            ObservationEventKind::Unknown,
        )];
        assert_eq!(
            unknown_event_kind.validate(100),
            Err(FailureReason::MalformedRecord)
        );

        let mut empty_event_id_event = valid_obs.clone();
        empty_event_id_event.events = vec![ObservationEvent::new(
            OpaqueId(Vec::new()),
            ObservationEventKind::Healthy,
        )];
        assert_eq!(
            empty_event_id_event.validate(100),
            Err(FailureReason::MalformedRecord)
        );

        // Budget counters validation
        let mut unaligned_budget = valid_obs.clone();
        unaligned_budget.budget_bytes = 100;
        assert_eq!(
            unaligned_budget.validate(100),
            Err(FailureReason::ImpossibleBudget)
        );

        let mut excess_budget = valid_obs.clone();
        excess_budget.budget_bytes = MAX_CAPACITY_BYTES + CAPACITY_ALIGNMENT_BYTES;
        assert_eq!(
            excess_budget.validate(100),
            Err(FailureReason::ImpossibleBudget)
        );

        let mut resident_over_budget = valid_obs.clone();
        resident_over_budget.resident_bytes = 8192;
        resident_over_budget.budget_bytes = 4096;
        assert_eq!(
            resident_over_budget.validate(100),
            Err(FailureReason::ImpossibleBudget)
        );

        let mut available_over_budget = valid_obs.clone();
        available_over_budget.available_bytes = 8192;
        available_over_budget.budget_bytes = 4096;
        assert_eq!(
            available_over_budget.validate(100),
            Err(FailureReason::ImpossibleBudget)
        );

        Ok(())
    }
}
