//! Pure host-authoritative N3 observation and lease state model.
//!
//! This module is deliberately independent of Windows, WDDM, CUDA, kernel
//! memory management, and the RamShared transport.  It models the contract
//! boundary described by `microsoft-native-vram-memory-tier/SPEC.md`; it does
//! not establish physical residency or guest ownership.


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

pub(crate) const RESTART_RECORD_MAGIC: &[u8; 4] = b"RSN3";
pub(crate) const HOST_AUTHORITY_MARKER: u8 = 1;
pub(crate) const GUEST_AUTHORITY_MARKER: u8 = 2;

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

impl core::fmt::Display for StateTransitionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::IllegalTransition { expected, actual } => {
                if let Some(expected) = expected {
                    write!(
                        f,
                        "illegal state transition: expected {:?}, actual {:?}",
                        expected, actual
                    )
                } else {
                    write!(f, "illegal state transition: actual {:?}", actual)
                }
            }
            Self::IllegalPreflight { expected, actual } => {
                if let Some(expected) = expected {
                    write!(
                        f,
                        "illegal preflight transition: expected {:?}, actual {:?}",
                        expected, actual
                    )
                } else {
                    write!(f, "illegal preflight transition: actual {:?}", actual)
                }
            }
            Self::StaleGeneration { provided, expected } => {
                write!(
                    f,
                    "stale generation: provided {}, expected > {}",
                    provided, expected
                )
            }
        }
    }
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


mod state_persistence;
mod state_validation;
mod state_transitions;

pub use state_persistence::*;
pub use state_validation::*;
pub use state_transitions::*;
