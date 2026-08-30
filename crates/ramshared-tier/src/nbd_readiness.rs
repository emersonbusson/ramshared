//! Pure WSL2 NBD product readiness policy.
//!
//! This module does not inspect the host or perform lifecycle actions. Callers
//! provide a fresh observation and receive one stable state and reason. The
//! outer lifecycle remains responsible for swapoff-first teardown.
//!
//! SPEC: docs/specs/no-milestone/wsl2-nbd-product-readiness/SPEC.md

/// One mebibyte in bytes.
pub const MIB_BYTES: u64 = 1024 * 1024;
/// The minimum lower-tier margin required for NBD demotion.
pub const MIN_LOWER_TIER_MARGIN_BYTES: u64 = 512 * MIB_BYTES;

/// Product-level state, distinct from the daemon's internal lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProductState {
    /// The managed NBD swap and daemon are both absent after a safe teardown.
    ProductOff,
    /// All gates pass for the NBD product, including active identity when present.
    Ready,
    /// A deterministic gate failed; no retry or mutation is authorized.
    Blocked,
}

impl ProductState {
    /// Stable public spelling for status output.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProductOff => "PRODUCT_OFF",
            Self::Ready => "READY",
            Self::Blocked => "BLOCKED",
        }
    }
}

/// Product transport values. There is deliberately no ublk product value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProductTransport {
    /// The only supported WSL2 product transport.
    Nbd,
    /// No managed NBD transport is currently active.
    None,
}

impl ProductTransport {
    /// Stable public spelling for status output.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Nbd => "nbd",
            Self::None => "none",
        }
    }
}

/// Result of one observed external gate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Gate {
    /// The observed gate passed.
    Pass,
    /// The observed gate failed.
    Fail,
    /// The gate could not be measured safely.
    Unknown,
    /// The gate has no subject in this observation, such as binary identity
    /// while no daemon is running.
    NotApplicable,
}

/// Explicit authorization status for a requested mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Approval {
    /// The operation is read-only and does not need approval.
    NotRequired,
    /// Current scoped approval was supplied by the caller.
    Present,
    /// A mutation was requested without current scoped approval.
    Missing,
}

/// Requested operation. This policy only plans or refuses; it never mutates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    /// Inspect current state without host mutation.
    ReadOnly,
    /// Install a sealed product release.
    Install,
    /// Start the NBD cascade.
    Activate,
    /// Stop the NBD cascade after swapoff succeeds.
    Deactivate,
    /// Retire the exact legacy ublk product service wiring.
    RetireLegacyUblk,
}

impl Operation {
    const fn is_mutating(self) -> bool {
        !matches!(self, Self::ReadOnly)
    }
}

/// Identity of the measured lower-tier sink.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LowerTierSink {
    /// One explicit lower-tier identity was observed.
    Known(String),
    /// More than one candidate could own the claimed capacity.
    Ambiguous,
}

/// Capacity observation supplied by the owning filesystem/swap layer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CapacitySample {
    /// No lower-tier sink or fresh capacity reading is available.
    Unknown,
    /// The lower-tier measurement is older than the caller's freshness bound.
    Stale,
    /// A current free absorbable-capacity measurement.
    Observed {
        /// Exactly one sink identity.
        sink: LowerTierSink,
        /// Capacity available to absorb NBD pages now, not nominal size.
        free_absorbable_bytes: u64,
        /// Filesystem or swap allocation alignment used by this measurement.
        alignment_bytes: u64,
    },
}

/// Stable refusal classifications. These are deterministic and never retried
/// by this policy layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefusalCode {
    /// The product transport is missing or is not NBD for an active NBD tier.
    TransportMustBeNbd,
    /// A legacy ublk product unit or device is still active.
    LegacyUblkProductActive,
    /// The NBD swap and daemon presence disagree.
    NbdLifecycleIncomplete,
    /// The sealed-release gate failed.
    ReleaseGateFailed,
    /// The sealed-release gate could not be measured.
    ReleaseGateUnknown,
    /// Relay `--check` failed.
    RelayGateFailed,
    /// Relay `--check` could not be measured.
    RelayGateUnknown,
    /// A live daemon does not match the selected sealed binary.
    BinaryMatchFailed,
    /// A live daemon needs a binary identity result but none is available.
    BinaryMatchRequired,
    /// Binary identity data is inconsistent or unreadable.
    BinaryMatchUnknown,
    /// The configured NBD logical capacity is zero.
    InvalidVramSize,
    /// Capacity arithmetic cannot represent the required lower-tier size.
    CapacityOverflow,
    /// No exact lower-tier identity was supplied.
    LowerTierSinkUnknown,
    /// The lower-tier owner is ambiguous.
    LowerTierSinkAmbiguous,
    /// The capacity sample is stale.
    LowerTierMeasurementStale,
    /// The lower-tier measurement does not respect its allocation alignment.
    LowerTierAlignmentInvalid,
    /// The free lower-tier capacity is below the required demotion sink.
    LowerTierShortfall,
    /// A requested mutation lacks a current scoped approval.
    ApprovalMissing,
    /// A reboot, WSL shutdown, or equivalent restart action was requested.
    RebootRequested,
}

impl RefusalCode {
    /// Stable public spelling for status output.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TransportMustBeNbd => "TRANSPORT_MUST_BE_NBD",
            Self::LegacyUblkProductActive => "LEGACY_UBLK_PRODUCT_ACTIVE",
            Self::NbdLifecycleIncomplete => "NBD_LIFECYCLE_INCOMPLETE",
            Self::ReleaseGateFailed => "RELEASE_GATE_FAILED",
            Self::ReleaseGateUnknown => "RELEASE_GATE_UNKNOWN",
            Self::RelayGateFailed => "RELAY_GATE_FAILED",
            Self::RelayGateUnknown => "RELAY_GATE_UNKNOWN",
            Self::BinaryMatchFailed => "BINARY_MATCH_FAILED",
            Self::BinaryMatchRequired => "BINARY_MATCH_REQUIRED",
            Self::BinaryMatchUnknown => "BINARY_MATCH_UNKNOWN",
            Self::InvalidVramSize => "INVALID_VRAM_SIZE",
            Self::CapacityOverflow => "CAPACITY_OVERFLOW",
            Self::LowerTierSinkUnknown => "LOWER_TIER_SINK_UNKNOWN",
            Self::LowerTierSinkAmbiguous => "LOWER_TIER_SINK_AMBIGUOUS",
            Self::LowerTierMeasurementStale => "LOWER_TIER_MEASUREMENT_STALE",
            Self::LowerTierAlignmentInvalid => "LOWER_TIER_ALIGNMENT_INVALID",
            Self::LowerTierShortfall => "LOWER_TIER_SHORTFALL",
            Self::ApprovalMissing => "APPROVAL_MISSING",
            Self::RebootRequested => "REBOOT_REQUESTED",
        }
    }
}

/// Why the product state was selected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadinessReason {
    /// No state evaluation was requested.
    NotEvaluated,
    /// The product is intentionally quiescent and all observed gates passed.
    ProductOff,
    /// Every required gate passed.
    AllGatesPass,
    /// A deterministic refusal selected `BLOCKED`.
    Refusal(RefusalCode),
}

impl ReadinessReason {
    /// Stable public spelling for status output.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotEvaluated => "not_evaluated",
            Self::ProductOff => "product_off",
            Self::AllGatesPass => "all_gates_pass",
            Self::Refusal(code) => code.as_str(),
        }
    }
}

/// Fresh observations consumed by [`evaluate_product`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProductInput {
    /// Reported product transport.
    pub transport: ProductTransport,
    /// A managed NBD device is still listed in the active swap set.
    pub nbd_swap_active: bool,
    /// The exact managed daemon identity is alive.
    pub daemon_running: bool,
    /// An active legacy ublk service or product device was observed.
    pub legacy_ublk_product_active: bool,
    /// Sealed version ownership, mode, manifest, and selector result.
    pub release_gate: Gate,
    /// Read-only Relay `--check` result.
    pub relay_gate: Gate,
    /// Selected-release BINARY_MATCH result for an active daemon.
    pub binary_match: Gate,
    /// Fresh lower-tier free-capacity observation.
    pub capacity: CapacitySample,
    /// Configured logical NBD tier size.
    pub vram_bytes: u64,
    /// Requested action classification.
    pub operation: Operation,
    /// Current scoped approval status.
    pub approval: Approval,
    /// Any forbidden reboot, shutdown, or termination request.
    pub reboot_requested: bool,
}

/// A deterministic evaluation result. `mutation_permitted` only states that
/// a separately implemented outer lifecycle has the required approval and
/// gates; this module itself performs no mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProductDecision {
    /// Product status.
    pub state: ProductState,
    /// Stable successful or refusal reason.
    pub reason: ReadinessReason,
    /// Deterministic gate failures are never retried by this policy.
    pub retry_allowed: bool,
    /// Every evaluation is exactly one bounded attempt.
    pub attempts: u8,
    /// An approved mutating operation may advance to its outer lifecycle.
    pub mutation_permitted: bool,
}

impl ProductDecision {
    fn blocked(reason: RefusalCode) -> Self {
        Self {
            state: ProductState::Blocked,
            reason: ReadinessReason::Refusal(reason),
            retry_allowed: false,
            attempts: 1,
            mutation_permitted: false,
        }
    }

    fn accepted(state: ProductState, reason: ReadinessReason, operation: Operation) -> Self {
        Self {
            state,
            reason,
            retry_allowed: false,
            attempts: 1,
            mutation_permitted: operation.is_mutating(),
        }
    }
}

/// Computes the exact required free lower-tier capacity:
/// `V + max(ceil(0.10 * V), 512 MiB)`.
pub fn minimum_lower_tier_bytes(vram_bytes: u64) -> Result<u64, RefusalCode> {
    if vram_bytes == 0 {
        return Err(RefusalCode::InvalidVramSize);
    }

    let ten_percent = vram_bytes
        .checked_div(10)
        .and_then(|whole| whole.checked_add(u64::from(!vram_bytes.is_multiple_of(10))))
        .ok_or(RefusalCode::CapacityOverflow)?;
    let margin = ten_percent.max(MIN_LOWER_TIER_MARGIN_BYTES);
    vram_bytes
        .checked_add(margin)
        .ok_or(RefusalCode::CapacityOverflow)
}

/// Validates a fresh, exact lower-tier sink against the NBD demotion formula.
/// The returned number is the required free lower-tier capacity in bytes.
pub fn validate_lower_tier_capacity(
    vram_bytes: u64,
    sample: &CapacitySample,
) -> Result<u64, RefusalCode> {
    let required = minimum_lower_tier_bytes(vram_bytes)?;
    match sample {
        CapacitySample::Unknown => Err(RefusalCode::LowerTierSinkUnknown),
        CapacitySample::Stale => Err(RefusalCode::LowerTierMeasurementStale),
        CapacitySample::Observed {
            sink,
            free_absorbable_bytes,
            alignment_bytes,
        } => {
            match sink {
                LowerTierSink::Known(identity) if !identity.trim().is_empty() => {}
                LowerTierSink::Known(_) => return Err(RefusalCode::LowerTierSinkUnknown),
                LowerTierSink::Ambiguous => return Err(RefusalCode::LowerTierSinkAmbiguous),
            }
            if *alignment_bytes == 0 || free_absorbable_bytes % alignment_bytes != 0 {
                return Err(RefusalCode::LowerTierAlignmentInvalid);
            }
            if *free_absorbable_bytes < required {
                return Err(RefusalCode::LowerTierShortfall);
            }
            Ok(required)
        }
    }
}

/// Evaluates the NBD product state from one fresh observation.
///
/// The order is fail-closed: forbidden host requests and missing approval
/// return before any positive classification; lifecycle disagreement, legacy
/// ublk ownership, and every required read-only gate then block readiness.
pub fn evaluate_product(input: &ProductInput) -> ProductDecision {
    if input.reboot_requested {
        return ProductDecision::blocked(RefusalCode::RebootRequested);
    }
    if input.operation.is_mutating() && input.approval != Approval::Present {
        return ProductDecision::blocked(RefusalCode::ApprovalMissing);
    }
    if input.legacy_ublk_product_active {
        return ProductDecision::blocked(RefusalCode::LegacyUblkProductActive);
    }
    if input.nbd_swap_active != input.daemon_running {
        return ProductDecision::blocked(RefusalCode::NbdLifecycleIncomplete);
    }
    if input.nbd_swap_active && input.transport != ProductTransport::Nbd {
        return ProductDecision::blocked(RefusalCode::TransportMustBeNbd);
    }
    if let Some(reason) = release_refusal(input.release_gate) {
        return ProductDecision::blocked(reason);
    }
    if let Err(reason) = validate_lower_tier_capacity(input.vram_bytes, &input.capacity) {
        return ProductDecision::blocked(reason);
    }
    if let Some(reason) = relay_refusal(input.relay_gate) {
        return ProductDecision::blocked(reason);
    }
    if let Some(reason) = binary_refusal(input.binary_match, input.daemon_running) {
        return ProductDecision::blocked(reason);
    }

    if !input.nbd_swap_active {
        return ProductDecision::accepted(
            ProductState::ProductOff,
            ReadinessReason::ProductOff,
            input.operation,
        );
    }

    ProductDecision::accepted(
        ProductState::Ready,
        ReadinessReason::AllGatesPass,
        input.operation,
    )
}

fn release_refusal(gate: Gate) -> Option<RefusalCode> {
    match gate {
        Gate::Pass => None,
        Gate::Fail => Some(RefusalCode::ReleaseGateFailed),
        Gate::Unknown | Gate::NotApplicable => Some(RefusalCode::ReleaseGateUnknown),
    }
}

fn relay_refusal(gate: Gate) -> Option<RefusalCode> {
    match gate {
        Gate::Pass => None,
        Gate::Fail => Some(RefusalCode::RelayGateFailed),
        Gate::Unknown | Gate::NotApplicable => Some(RefusalCode::RelayGateUnknown),
    }
}

fn binary_refusal(gate: Gate, daemon_running: bool) -> Option<RefusalCode> {
    match (gate, daemon_running) {
        (Gate::Pass, _) => None,
        (Gate::NotApplicable, false) => None,
        (Gate::Fail, _) => Some(RefusalCode::BinaryMatchFailed),
        (Gate::Unknown, _) => Some(RefusalCode::BinaryMatchUnknown),
        (Gate::NotApplicable, true) => Some(RefusalCode::BinaryMatchRequired),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * MIB_BYTES;

    fn ready_input() -> ProductInput {
        ProductInput {
            transport: ProductTransport::Nbd,
            nbd_swap_active: true,
            daemon_running: true,
            legacy_ublk_product_active: false,
            release_gate: Gate::Pass,
            relay_gate: Gate::Pass,
            binary_match: Gate::Pass,
            capacity: CapacitySample::Observed {
                sink: LowerTierSink::Known("lower-sink".into()),
                free_absorbable_bytes: 2 * GIB,
                alignment_bytes: MIB_BYTES,
            },
            vram_bytes: GIB,
            operation: Operation::ReadOnly,
            approval: Approval::NotRequired,
            reboot_requested: false,
        }
    }

    #[test]
    fn nbd_only_transport_is_the_only_ready_value() {
        assert_eq!(evaluate_product(&ready_input()).state, ProductState::Ready);

        let mut no_transport = ready_input();
        no_transport.transport = ProductTransport::None;
        assert_eq!(
            evaluate_product(&no_transport).reason,
            ReadinessReason::Refusal(RefusalCode::TransportMustBeNbd)
        );

        let mut legacy_ublk = ready_input();
        legacy_ublk.legacy_ublk_product_active = true;
        assert_eq!(
            evaluate_product(&legacy_ublk).reason,
            ReadinessReason::Refusal(RefusalCode::LegacyUblkProductActive)
        );
    }

    #[test]
    fn lower_tier_formula_uses_ten_percent_or_512_mib() {
        assert_eq!(
            minimum_lower_tier_bytes(GIB),
            Ok(GIB + MIN_LOWER_TIER_MARGIN_BYTES)
        );
        let ten_gib = 10 * GIB;
        assert_eq!(minimum_lower_tier_bytes(ten_gib), Ok(11 * GIB));
        assert_eq!(
            minimum_lower_tier_bytes(0),
            Err(RefusalCode::InvalidVramSize)
        );
    }

    #[test]
    fn capacity_shortfall_refuses_before_mutation() {
        let required = match minimum_lower_tier_bytes(GIB) {
            Ok(value) => value,
            Err(error) => panic!("valid one GiB capacity requirement: {error:?}"),
        };
        let mut input = ready_input();
        input.capacity = CapacitySample::Observed {
            sink: LowerTierSink::Known("lower-sink".into()),
            free_absorbable_bytes: required - MIB_BYTES,
            alignment_bytes: MIB_BYTES,
        };
        let decision = evaluate_product(&input);
        assert_eq!(decision.state, ProductState::Blocked);
        assert_eq!(
            decision.reason,
            ReadinessReason::Refusal(RefusalCode::LowerTierShortfall)
        );
        assert!(!decision.mutation_permitted);

        input.capacity = CapacitySample::Stale;
        assert_eq!(
            evaluate_product(&input).reason,
            ReadinessReason::Refusal(RefusalCode::LowerTierMeasurementStale)
        );
        input.capacity = CapacitySample::Unknown;
        assert_eq!(
            evaluate_product(&input).reason,
            ReadinessReason::Refusal(RefusalCode::LowerTierSinkUnknown)
        );
        input.vram_bytes = u64::MAX;
        assert_eq!(
            evaluate_product(&input).reason,
            ReadinessReason::Refusal(RefusalCode::CapacityOverflow)
        );
    }

    #[test]
    fn product_off_is_not_ready_alias() {
        let ready = evaluate_product(&ready_input());
        let mut off_input = ready_input();
        off_input.transport = ProductTransport::None;
        off_input.nbd_swap_active = false;
        off_input.daemon_running = false;
        off_input.binary_match = Gate::NotApplicable;
        let off = evaluate_product(&off_input);
        assert_eq!(off.state, ProductState::ProductOff);
        assert_eq!(off.reason, ReadinessReason::ProductOff);
        assert_ne!(off.state, ready.state);
    }

    #[test]
    fn deterministic_gate_failure_is_not_retried() {
        let mut input = ready_input();
        input.release_gate = Gate::Fail;
        let first = evaluate_product(&input);
        let second = evaluate_product(&input);
        assert_eq!(
            first.reason,
            ReadinessReason::Refusal(RefusalCode::ReleaseGateFailed)
        );
        assert!(!first.retry_allowed);
        assert_eq!(first.attempts, 1);
        assert_eq!(first, second);
    }

    #[test]
    fn activation_and_deactivation_are_idempotent() {
        let mut activation = ready_input();
        activation.operation = Operation::Activate;
        activation.approval = Approval::Present;
        let first_activation = evaluate_product(&activation);
        assert_eq!(first_activation, evaluate_product(&activation));
        assert!(first_activation.mutation_permitted);

        let mut deactivation = ready_input();
        deactivation.transport = ProductTransport::None;
        deactivation.nbd_swap_active = false;
        deactivation.daemon_running = false;
        deactivation.binary_match = Gate::NotApplicable;
        deactivation.operation = Operation::Deactivate;
        deactivation.approval = Approval::Present;
        let first_deactivation = evaluate_product(&deactivation);
        assert_eq!(first_deactivation, evaluate_product(&deactivation));
        assert_eq!(first_deactivation.state, ProductState::ProductOff);
        assert!(first_deactivation.mutation_permitted);
    }

    #[test]
    fn invalid_gate_shapes_fail_closed() {
        let mut input = ready_input();
        input.operation = Operation::Install;
        input.approval = Approval::Missing;
        assert_eq!(
            evaluate_product(&input).reason,
            ReadinessReason::Refusal(RefusalCode::ApprovalMissing)
        );

        input.operation = Operation::ReadOnly;
        input.approval = Approval::NotRequired;
        input.reboot_requested = true;
        assert_eq!(
            evaluate_product(&input).reason,
            ReadinessReason::Refusal(RefusalCode::RebootRequested)
        );

        input.reboot_requested = false;
        input.capacity = CapacitySample::Observed {
            sink: LowerTierSink::Ambiguous,
            free_absorbable_bytes: 2 * GIB,
            alignment_bytes: MIB_BYTES,
        };
        assert_eq!(
            evaluate_product(&input).reason,
            ReadinessReason::Refusal(RefusalCode::LowerTierSinkAmbiguous)
        );

        input.capacity = CapacitySample::Observed {
            sink: LowerTierSink::Known("lower-sink".into()),
            free_absorbable_bytes: 2 * GIB + 1,
            alignment_bytes: MIB_BYTES,
        };
        assert_eq!(
            evaluate_product(&input).reason,
            ReadinessReason::Refusal(RefusalCode::LowerTierAlignmentInvalid)
        );
    }

    #[test]
    fn binary_and_lifecycle_disagreement_fail_closed() {
        let mut input = ready_input();
        input.binary_match = Gate::NotApplicable;
        assert_eq!(
            evaluate_product(&input).reason,
            ReadinessReason::Refusal(RefusalCode::BinaryMatchRequired)
        );

        input.binary_match = Gate::Pass;
        input.daemon_running = false;
        assert_eq!(
            evaluate_product(&input).reason,
            ReadinessReason::Refusal(RefusalCode::NbdLifecycleIncomplete)
        );
    }
}

/// Semantic error for NBD probe connection failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NbdReadinessError {
    /// Connection was refused (ECONNREFUSED).
    ConnectionRefused,
    /// Connection timed out (ETIMEDOUT).
    Timeout,
    /// Any other IO error kind.
    Other(std::io::ErrorKind),
}

impl core::fmt::Display for NbdReadinessError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ConnectionRefused => f.write_str("NBD connection refused"),
            Self::Timeout => f.write_str("NBD connection timed out"),
            Self::Other(kind) => write!(f, "NBD IO error: {:?}", kind),
        }
    }
}

impl core::error::Error for NbdReadinessError {}

impl From<std::io::ErrorKind> for NbdReadinessError {
    fn from(kind: std::io::ErrorKind) -> Self {
        match kind {
            std::io::ErrorKind::ConnectionRefused => Self::ConnectionRefused,
            std::io::ErrorKind::TimedOut => Self::Timeout,
            _ => Self::Other(kind),
        }
    }
}

#[cfg(test)]
mod nbd_readiness_error_tests {
    use super::*;

    #[test]
    fn nbd_readiness_error_mappings() {
        assert_eq!(
            NbdReadinessError::from(std::io::ErrorKind::ConnectionRefused),
            NbdReadinessError::ConnectionRefused
        );
        assert_eq!(
            NbdReadinessError::from(std::io::ErrorKind::TimedOut),
            NbdReadinessError::Timeout
        );
        assert_eq!(
            NbdReadinessError::from(std::io::ErrorKind::InvalidData),
            NbdReadinessError::Other(std::io::ErrorKind::InvalidData)
        );
    }
}
