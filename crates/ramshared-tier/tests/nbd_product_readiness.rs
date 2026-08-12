//! Contract tests for the WSL2 NBD-only product readiness policy.
//!
//! SPEC: docs/specs/no-milestone/wsl2-nbd-product-readiness/SPEC.md

use ramshared_tier::nbd_readiness::{
    Approval, CapacitySample, Gate, LowerTierSink, Operation, ProductInput, ProductState,
    ProductTransport, ReadinessReason, RefusalCode, evaluate_product, minimum_lower_tier_bytes,
};

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;

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
            alignment_bytes: MIB,
        },
        vram_bytes: GIB,
        operation: Operation::ReadOnly,
        approval: Approval::NotRequired,
        reboot_requested: false,
    }
}

#[test]
fn nbd_only_transport_is_the_only_ready_value() {
    let decision = evaluate_product(&ready_input());
    assert_eq!(decision.state, ProductState::Ready);

    let mut no_transport = ready_input();
    no_transport.transport = ProductTransport::None;
    let decision = evaluate_product(&no_transport);
    assert_eq!(decision.state, ProductState::Blocked);
    assert_eq!(
        decision.reason,
        ReadinessReason::Refusal(RefusalCode::TransportMustBeNbd)
    );

    let mut legacy_ublk = ready_input();
    legacy_ublk.legacy_ublk_product_active = true;
    let decision = evaluate_product(&legacy_ublk);
    assert_eq!(decision.state, ProductState::Blocked);
    assert_eq!(
        decision.reason,
        ReadinessReason::Refusal(RefusalCode::LegacyUblkProductActive)
    );
}

#[test]
fn lower_tier_formula_uses_ten_percent_or_512_mib() {
    assert_eq!(minimum_lower_tier_bytes(GIB), Ok(GIB + 512 * MIB));

    let ten_gib = 10 * GIB;
    assert_eq!(minimum_lower_tier_bytes(ten_gib), Ok(ten_gib + GIB),);

    assert_eq!(
        minimum_lower_tier_bytes(0),
        Err(RefusalCode::InvalidVramSize)
    );
}

#[test]
fn capacity_shortfall_refuses_before_mutation() {
    let required = match minimum_lower_tier_bytes(GIB) {
        Ok(value) => value,
        Err(error) => panic!("one GiB capacity requirement must be valid: {error:?}"),
    };

    let mut shortfall = ready_input();
    shortfall.capacity = CapacitySample::Observed {
        sink: LowerTierSink::Known("lower-sink".into()),
        free_absorbable_bytes: required - MIB,
        alignment_bytes: MIB,
    };
    let decision = evaluate_product(&shortfall);
    assert_eq!(decision.state, ProductState::Blocked);
    assert_eq!(
        decision.reason,
        ReadinessReason::Refusal(RefusalCode::LowerTierShortfall)
    );
    assert!(!decision.mutation_permitted);

    let mut stale = ready_input();
    stale.capacity = CapacitySample::Stale;
    assert_eq!(
        evaluate_product(&stale).reason,
        ReadinessReason::Refusal(RefusalCode::LowerTierMeasurementStale)
    );

    let mut unknown = ready_input();
    unknown.capacity = CapacitySample::Unknown;
    assert_eq!(
        evaluate_product(&unknown).reason,
        ReadinessReason::Refusal(RefusalCode::LowerTierSinkUnknown)
    );

    let mut overflow = ready_input();
    overflow.vram_bytes = u64::MAX;
    assert_eq!(
        evaluate_product(&overflow).reason,
        ReadinessReason::Refusal(RefusalCode::CapacityOverflow)
    );
}

#[test]
fn product_off_is_not_ready_alias() {
    let ready = evaluate_product(&ready_input());
    assert_eq!(ready.state, ProductState::Ready);

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
    let mut release_failure = ready_input();
    release_failure.release_gate = Gate::Fail;
    let first = evaluate_product(&release_failure);
    let second = evaluate_product(&release_failure);

    assert_eq!(first.state, ProductState::Blocked);
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
    let second_activation = evaluate_product(&activation);
    assert_eq!(first_activation, second_activation);
    assert_eq!(first_activation.state, ProductState::Ready);
    assert!(first_activation.mutation_permitted);

    let mut deactivation = ready_input();
    deactivation.transport = ProductTransport::None;
    deactivation.nbd_swap_active = false;
    deactivation.daemon_running = false;
    deactivation.binary_match = Gate::NotApplicable;
    deactivation.operation = Operation::Deactivate;
    deactivation.approval = Approval::Present;
    let first_deactivation = evaluate_product(&deactivation);
    let second_deactivation = evaluate_product(&deactivation);
    assert_eq!(first_deactivation, second_deactivation);
    assert_eq!(first_deactivation.state, ProductState::ProductOff);
    assert!(first_deactivation.mutation_permitted);
}

#[test]
fn unsafe_or_unapproved_inputs_fail_closed() {
    let mut approval_missing = ready_input();
    approval_missing.operation = Operation::Install;
    approval_missing.approval = Approval::Missing;
    assert_eq!(
        evaluate_product(&approval_missing).reason,
        ReadinessReason::Refusal(RefusalCode::ApprovalMissing)
    );

    let mut reboot = ready_input();
    reboot.reboot_requested = true;
    assert_eq!(
        evaluate_product(&reboot).reason,
        ReadinessReason::Refusal(RefusalCode::RebootRequested)
    );

    let mut alignment = ready_input();
    alignment.capacity = CapacitySample::Observed {
        sink: LowerTierSink::Ambiguous,
        free_absorbable_bytes: 2 * GIB,
        alignment_bytes: MIB,
    };
    assert_eq!(
        evaluate_product(&alignment).reason,
        ReadinessReason::Refusal(RefusalCode::LowerTierSinkAmbiguous)
    );
}
