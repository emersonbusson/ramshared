//! Pure host-authoritative N3 observation and lease state model.
//!
//! This module is deliberately independent of Windows, WDDM, CUDA, kernel
//! memory management, and the RamShared transport.  It models the contract
//! boundary described by `microsoft-native-vram-memory-tier/SPEC.md`; it does
//! not establish physical residency or guest ownership.

use core::fmt;

/// The only schema revision understood by this pure model.
pub const N3_SCHEMA_VERSION: u16 = 1;
/// Maximum size of any opaque host-issued identity.
pub const MAX_OPAQUE_ID_BYTES: usize = 64;
/// Maximum logical capacity accepted by the model (1 PiB).
pub const MAX_CAPACITY_BYTES: u64 = 1 << 50;
/// Memory values use page-sized logical units in this model.
pub const CAPACITY_ALIGNMENT_BYTES: u64 = 4096;
/// Maximum declared freshness window in deterministic model ticks.
pub const MAX_OBSERVATION_AGE: u64 = 3600;
/// Maximum number of observation events in one bounded record.
pub const MAX_OBSERVATION_EVENTS: usize = 16;
/// Maximum in-flight operation count tracked by the model.
pub const MAX_IN_FLIGHT: u32 = 1_000_000;
/// Maximum retained event identities before the model refuses further input.
pub const MAX_PROTOCOL_EVENT_HISTORY: usize = 256;
/// Maximum retained lease identities for generation monotonicity.
pub const MAX_GENERATION_HISTORY: usize = 256;
/// Bounded restart-record wire header: magic, schema, authority, epoch, count.
pub const RESTART_RECORD_HEADER_BYTES: usize = 17;
/// Maximum serialized host restart-record input accepted by the pure model.
pub const MAX_RESTART_RECORD_BYTES: usize = RESTART_RECORD_HEADER_BYTES
    + MAX_GENERATION_HISTORY * (1 + MAX_OPAQUE_ID_BYTES + core::mem::size_of::<u64>());

const RESTART_RECORD_MAGIC: &[u8; 4] = b"RSN3";
const HOST_AUTHORITY_MARKER: u8 = 1;
const GUEST_AUTHORITY_MARKER: u8 = 2;

/// Fail-closed reasons shared by preflight and protocol decisions.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateTransitionError {
    IllegalTransition {
        expected: Option<StateTag>,
        actual: StateTag,
    },
    IllegalPreflight {
        expected: Option<PreflightState>,
        actual: PreflightState,
    },
    StaleGeneration {
        provided: u64,
        expected: u64,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureReason {
    /// A schema revision is not understood by this model.
    UnknownSchema,
    /// A bounded identity, epoch, timestamp, or event record is malformed.
    MalformedRecord,
    /// The observation is not explicitly host authoritative.
    HostAuthorityRequired,
    /// An observation arrived after its declared freshness window.
    StaleObservation,
    /// The observation clock is missing or runs backwards.
    InvalidObservationClock,
    /// The observation epoch regressed or changed its payload on replay.
    EpochRegression,
    /// Budget counters cannot describe a valid host observation.
    ImpossibleBudget,
    /// A host grant was received without a fresh matching host observation.
    NoFreshHostObservation,

    /// A generation skipped the next host-monotonic value.
    GenerationGap,
    /// An event ID was reused with a changed payload or identity.
    ConflictingDuplicate,
    /// An event does not match the active opaque lease identity.
    LeaseIdentityMismatch,
    /// A grant has zero, unaligned, over-budget, or otherwise invalid capacity.
    InvalidCapacity,
    StateTransition(StateTransitionError),
    /// An operation was attempted while the lease was not granted.
    IoNotGranted,
    /// In-flight operations remain when a drain was requested.
    InFlightNotDrained,
    /// A callback has not completed when a drain was requested.
    CallbackNotDrained,
    /// The in-flight counter is not trustworthy.
    UnknownInFlight,
    /// A drain deadline elapsed before safety conditions held.
    DrainTimeout,
    /// Guest-visible scrubbing explicitly failed.
    ScrubFailed,
    /// Guest-visible scrubbing has not completed yet.
    ScrubPending,
    /// Host revoke completion was missing or contradicted.
    MissingRevokeCompletion,
    /// A host reset/TDR invalidated the lease.
    Reset,
    /// The host channel disappeared.
    ChannelLoss,
    /// The host liveness deadline expired.
    LeaseExpired,
    /// WSL restarted and cannot resume a prior lease.
    WslRestart,
    /// Suspend/resume requires a fresh host grant.
    Suspend,
    /// A driver replacement invalidated the old generation.
    DriverUpgrade,
    /// The guest crashed or missed its liveness obligation.
    GuestCrash,
    /// A guest PFN/NUMA/device-private claim is not a host contract.
    GuestResidencyClaim,
    /// N3 is not allowed to select a product transport or upstream path.
    ProductScope,
}

/// Compatibility alias for callers that describe all failures as refusals.
pub type RefusalCode = FailureReason;

/// Host ownership marker in an observation record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Authority {
    /// The record was issued by the host-owned contract.
    Host,
    /// A guest-originated value is never sufficient for N3 authority.
    Guest,
    /// Unknown authority fails closed.
    Unknown,
}

/// Bounded opaque identity.  The guest compares bytes but never interprets
/// them as CUDA ordinals, PFNs, adapter indexes, or host pointers.
#[derive(Clone, Eq, Hash, PartialEq)]
pub struct OpaqueId(Vec<u8>);

impl OpaqueId {
    /// Creates an opaque identity after applying the contract size bound.
    pub fn new<B: AsRef<[u8]>>(bytes: B) -> Result<Self, FailureReason> {
        let bytes = bytes.as_ref();
        if bytes.is_empty() || bytes.len() > MAX_OPAQUE_ID_BYTES {
            return Err(FailureReason::MalformedRecord);
        }
        Ok(Self(bytes.to_vec()))
    }

    /// Returns the identity bytes for exact equality checks.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for OpaqueId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpaqueId")
            .field("length", &self.0.len())
            .finish()
    }
}

/// Lease identity is opaque and host-issued.
pub type LeaseId = OpaqueId;
/// Event identity is opaque and host-issued.
pub type EventId = OpaqueId;
/// Adapter identity is opaque and host-issued.
pub type AdapterId = OpaqueId;

/// One host-issued generation checkpoint retained across a process restart.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenerationCheckpoint {
    pub lease_id: LeaseId,
    pub generation: u64,
}

/// Bounded, canonical restart input supplied by the host authority.
///
/// This type only parses and serializes caller-owned bytes. It neither reads
/// nor writes durable storage, and it makes no claim that an external caller
/// authenticated the bytes before supplying them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestartRecord {
    schema_version: u16,
    authority: Authority,
    host_epoch: u64,
    checkpoints: Vec<GenerationCheckpoint>,
}

impl RestartRecord {
    /// Builds a canonical host-authoritative record from bounded checkpoints.
    pub fn host(
        host_epoch: u64,
        mut checkpoints: Vec<GenerationCheckpoint>,
    ) -> Result<Self, FailureReason> {
        checkpoints.sort_by(|left, right| left.lease_id.as_bytes().cmp(right.lease_id.as_bytes()));
        let record = Self {
            schema_version: N3_SCHEMA_VERSION,
            authority: Authority::Host,
            host_epoch,
            checkpoints,
        };
        record.validate()?;
        Ok(record)
    }

    /// Serializes the validated canonical record into bounded caller-owned bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(
            RESTART_RECORD_HEADER_BYTES
                + self
                    .checkpoints
                    .iter()
                    .map(|checkpoint| 1 + checkpoint.lease_id.as_bytes().len() + 8)
                    .sum::<usize>(),
        );
        bytes.extend_from_slice(RESTART_RECORD_MAGIC);
        bytes.extend_from_slice(&self.schema_version.to_be_bytes());
        bytes.push(authority_marker(self.authority));
        bytes.extend_from_slice(&self.host_epoch.to_be_bytes());
        bytes.extend_from_slice(&(self.checkpoints.len() as u16).to_be_bytes());
        for checkpoint in &self.checkpoints {
            bytes.push(checkpoint.lease_id.as_bytes().len() as u8);
            bytes.extend_from_slice(checkpoint.lease_id.as_bytes());
            bytes.extend_from_slice(&checkpoint.generation.to_be_bytes());
        }
        bytes
    }

    /// Parses a complete canonical host restart record without changing model state.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FailureReason> {
        if !(RESTART_RECORD_HEADER_BYTES..=MAX_RESTART_RECORD_BYTES).contains(&bytes.len())
            || bytes.get(..4) != Some(RESTART_RECORD_MAGIC.as_slice())
        {
            return Err(FailureReason::MalformedRecord);
        }
        let mut cursor = 4;
        let schema_version = read_u16(bytes, &mut cursor)?;
        let authority = read_authority(bytes, &mut cursor)?;
        let host_epoch = read_u64(bytes, &mut cursor)?;
        let checkpoint_count = usize::from(read_u16(bytes, &mut cursor)?);
        if checkpoint_count > MAX_GENERATION_HISTORY {
            return Err(FailureReason::MalformedRecord);
        }

        let mut checkpoints = Vec::with_capacity(checkpoint_count);
        for _ in 0..checkpoint_count {
            let identity_length = usize::from(read_u8(bytes, &mut cursor)?);
            if identity_length == 0 || identity_length > MAX_OPAQUE_ID_BYTES {
                return Err(FailureReason::MalformedRecord);
            }
            let identity = read_exact(bytes, &mut cursor, identity_length)?;
            let lease_id = LeaseId::new(identity)?;
            let generation = read_u64(bytes, &mut cursor)?;
            checkpoints.push(GenerationCheckpoint {
                lease_id,
                generation,
            });
        }
        if cursor != bytes.len() {
            return Err(FailureReason::MalformedRecord);
        }

        let record = Self {
            schema_version,
            authority,
            host_epoch,
            checkpoints,
        };
        record.validate()?;
        Ok(record)
    }

    /// Returns the non-zero host epoch carried by this validated record.
    pub fn host_epoch(&self) -> u64 {
        self.host_epoch
    }

    /// Returns canonical lease-generation checkpoints without granting a lease.
    pub fn checkpoints(&self) -> &[GenerationCheckpoint] {
        &self.checkpoints
    }

    fn validate(&self) -> Result<(), FailureReason> {
        if self.schema_version != N3_SCHEMA_VERSION {
            return Err(FailureReason::UnknownSchema);
        }
        if self.authority != Authority::Host {
            return Err(FailureReason::HostAuthorityRequired);
        }
        if self.host_epoch == 0 || self.checkpoints.len() > MAX_GENERATION_HISTORY {
            return Err(FailureReason::MalformedRecord);
        }
        for checkpoint in &self.checkpoints {
            if checkpoint.lease_id.as_bytes().is_empty() || checkpoint.generation == 0 {
                return Err(FailureReason::MalformedRecord);
            }
        }
        if self
            .checkpoints
            .windows(2)
            .any(|pair| pair[0].lease_id.as_bytes() >= pair[1].lease_id.as_bytes())
        {
            return Err(FailureReason::MalformedRecord);
        }
        if self.to_bytes().len() > MAX_RESTART_RECORD_BYTES {
            return Err(FailureReason::MalformedRecord);
        }
        Ok(())
    }
}

fn authority_marker(authority: Authority) -> u8 {
    match authority {
        Authority::Host => HOST_AUTHORITY_MARKER,
        Authority::Guest => GUEST_AUTHORITY_MARKER,
        Authority::Unknown => 0,
    }
}

fn read_exact<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    length: usize,
) -> Result<&'a [u8], FailureReason> {
    let end = cursor
        .checked_add(length)
        .filter(|end| *end <= bytes.len())
        .ok_or(FailureReason::MalformedRecord)?;
    let output = &bytes[*cursor..end];
    *cursor = end;
    Ok(output)
}

fn read_u8(bytes: &[u8], cursor: &mut usize) -> Result<u8, FailureReason> {
    read_exact(bytes, cursor, 1).map(|value| value[0])
}

fn read_u16(bytes: &[u8], cursor: &mut usize) -> Result<u16, FailureReason> {
    let value = read_exact(bytes, cursor, 2)?;
    Ok(u16::from_be_bytes([value[0], value[1]]))
}

fn read_u64(bytes: &[u8], cursor: &mut usize) -> Result<u64, FailureReason> {
    let value = read_exact(bytes, cursor, 8)?;
    Ok(u64::from_be_bytes([
        value[0], value[1], value[2], value[3], value[4], value[5], value[6], value[7],
    ]))
}

fn read_authority(bytes: &[u8], cursor: &mut usize) -> Result<Authority, FailureReason> {
    match read_u8(bytes, cursor)? {
        HOST_AUTHORITY_MARKER => Ok(Authority::Host),
        GUEST_AUTHORITY_MARKER => Ok(Authority::Guest),
        _ => Ok(Authority::Unknown),
    }
}

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

fn valid_capacity(value: u64) -> bool {
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
    pub state: PreflightState,
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
    state: PreflightState,
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
        if self.state == PreflightState::Constrained {
            self.state = PreflightState::DemotionRequested;
            return PreflightDecision {
                state: self.state,
                action: PreflightAction::DemotionRequested,
                retry_allowed: true,
            };
        }
        PreflightDecision {
            state: self.state,
            action: PreflightAction::Unavailable(FailureReason::StateTransition(StateTransitionError::IllegalPreflight { expected: Some(PreflightState::Constrained), actual: self.state })),
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
            return Err(self.fail_restart_restore(FailureReason::StateTransition(StateTransitionError::IllegalTransition { expected: Some(StateTag::Absent), actual: self.lease_state.tag() })));
        }

        let generation_history = record
            .checkpoints()
            .iter()
            .map(|checkpoint| (checkpoint.lease_id.clone(), checkpoint.generation))
            .collect();
        self.generation_history = generation_history;
        self.restored_restart_epoch = Some(record.host_epoch());
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
                FailureReason::StateTransition(StateTransitionError::IllegalTransition { expected: Some(StateTag::Absent), actual: self.lease_state.tag() }),
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
                FailureReason::StateTransition(StateTransitionError::IllegalTransition { expected: Some(StateTag::Absent), actual: self.lease_state.tag() }),
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
                FailureReason::StateTransition(StateTransitionError::StaleGeneration { provided: grant.host_epoch, expected: self.restored_restart_epoch.unwrap_or(0) }),
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
                FailureReason::StateTransition(StateTransitionError::IllegalTransition { expected: Some(StateTag::Negotiating), actual: self.lease_state.tag() }),
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
                FailureReason::StateTransition(StateTransitionError::IllegalTransition { expected: Some(StateTag::Granted), actual: self.lease_state.tag() }),
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
                FailureReason::StateTransition(StateTransitionError::IllegalTransition { expected: Some(StateTag::Granted), actual: self.lease_state.tag() }),
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
            return self.fail_for_event(None, None, None, FailureReason::StateTransition(StateTransitionError::IllegalTransition { expected: Some(StateTag::Granted), actual: self.lease_state.tag() }));
        };
        let Some(revoke) = active.revoke.as_ref() else {
            return self.fail_for_active(FailureReason::StateTransition(StateTransitionError::IllegalTransition { expected: Some(StateTag::Quiescing), actual: self.lease_state.tag() }));
        };
        if self.lease_state != LeaseState::Quiescing(active.generation) {
            return self.fail_for_active(FailureReason::StateTransition(StateTransitionError::IllegalTransition { expected: Some(StateTag::Quiescing), actual: self.lease_state.tag() }));
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
                active
                    .revoke
                    .as_ref()
                    .map(|revoke| revoke.event_id.clone())
                    .or_else(|| Some(active.grant_event_id.clone())),
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
                return Err(FailureReason::StateTransition(StateTransitionError::StaleGeneration { provided: generation, expected: *previous }));
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
            PreflightAction::Unavailable(FailureReason::StateTransition(StateTransitionError::IllegalPreflight { expected: Some(PreflightState::Constrained), actual: model.state }))
        );

        // Let's create a constrained state by observing an empty budget
        let event_id = EventId::new(b"event-1").unwrap_or_else(|_| panic!("failed to create event_id"));
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
        let lease_id = LeaseId::new(b"lease-1").unwrap_or_else(|_| panic!("failed to create lease_id"));
        let event_id = EventId::new(b"event-1").unwrap_or_else(|_| panic!("failed to create event_id"));

        let revoke = Revoke::host(lease_id, 1, event_id, 200);

        // Applying Revoke to an Absent state should result in an invalid transition error
        let decision = machine.receive_revoke(revoke);
        match decision {
            ProtocolDecision::FailAck(fail_ack) => {
                assert_eq!(fail_ack.reason, FailureReason::StateTransition(StateTransitionError::IllegalTransition { expected: Some(StateTag::Granted), actual: StateTag::Absent }));
            }
            _ => panic!("Expected FailAck"),
        }

        // The machine's state should now be Failed(InvalidTransition)
        assert_eq!(
            machine.lease_state(),
            LeaseState::Failed(FailureReason::StateTransition(StateTransitionError::IllegalTransition { expected: Some(StateTag::Granted), actual: StateTag::Absent }))
        );
    }
}

#[cfg(test)]
mod additional_tests {
    use super::*;

    #[test]
    fn test_guard_clauses_generation_validation() {
        let mut machine = LeaseMachine::new();
        let lease_id = LeaseId::new(b"lease-1").unwrap_or_else(|_| panic!("failed to create lease_id"));
        machine.remember_generation(lease_id.clone(), 10);

        assert_eq!(
            machine.validate_generation(&lease_id, 10),
            Err(FailureReason::StateTransition(StateTransitionError::StaleGeneration { provided: 10, expected: 10 }))
        );
        assert_eq!(
            machine.validate_generation(&lease_id, 9),
            Err(FailureReason::StateTransition(StateTransitionError::StaleGeneration { provided: 9, expected: 10 }))
        );
        assert_eq!(
            machine.validate_generation(&lease_id, 12),
            Err(FailureReason::GenerationGap)
        );
        assert_eq!(machine.validate_generation(&lease_id, 11), Ok(()));
    }
}
