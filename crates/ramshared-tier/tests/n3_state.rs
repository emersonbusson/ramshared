use ramshared_tier::n3_state::{
    StateTransitionError, StateTag,
    AdapterId, Authority, DrainAck, EventId, FailAck, FailureReason, GenerationCheckpoint, Grant,
    GrantAck, GuestClaim, HostObservation, LeaseId, LeaseMachine, LeaseState, LifecycleEvent,
    ObservationEvent, ObservationEventKind, OpaqueId, PreflightAction, PreflightState,
    ProductTransport, ProtocolDecision, RestartRecord, Revoke, RevokeCompletion, ScrubResult,
};

fn opaque(bytes: &[u8]) -> OpaqueId {
    match OpaqueId::new(bytes) {
        Ok(value) => value,
        Err(error) => panic!("valid opaque test id: {error:?}"),
    }
}

fn observation(epoch: u64, event: &[u8], signal: ObservationEventKind) -> HostObservation {
    HostObservation::new(
        1,
        epoch,
        AdapterId::from(opaque(b"host-adapter")),
        Authority::Host,
        1024 * 1024 * 1024,
        256 * 1024 * 1024,
        768 * 1024 * 1024,
        100,
        10,
        EventId::from(opaque(event)),
        vec![ObservationEvent::new(
            EventId::from(opaque(b"observation-signal")),
            signal,
        )],
    )
}

fn prepared_machine() -> LeaseMachine {
    let mut machine = LeaseMachine::new();
    let decision = machine.observe(
        observation(1, b"observation-1", ObservationEventKind::Healthy),
        100,
    );
    assert_eq!(decision.state, PreflightState::Observing);
    machine
}

fn grant(generation: u64, event: &[u8]) -> Grant {
    Grant::new(
        1,
        LeaseId::from(opaque(b"lease-1")),
        generation,
        EventId::from(opaque(event)),
        1024 * 1024,
        1,
        100,
        200,
    )
}

fn negotiate(machine: &mut LeaseMachine, grant: Grant) -> GrantAck {
    match machine.receive_grant(grant, 100) {
        ProtocolDecision::GrantAck(ack) => ack,
        decision => panic!("expected GRANT_ACK, got {decision:?}"),
    }
}

fn granted_machine() -> LeaseMachine {
    let mut machine = prepared_machine();
    let ack = negotiate(&mut machine, grant(1, b"grant-1"));
    let decision = machine.accept_grant_ack(ack);
    assert!(matches!(
        decision,
        ProtocolDecision::Accepted(LeaseState::Granted(1))
    ));
    assert_eq!(machine.lease_state(), LeaseState::Granted(1));
    machine
}

#[test]
fn valid_host_observation_enters_observing() {
    let mut machine = LeaseMachine::new();
    let decision = machine.observe(
        observation(1, b"observation-1", ObservationEventKind::Healthy),
        100,
    );
    assert_eq!(decision.state, PreflightState::Observing);
    assert!(matches!(decision.action, PreflightAction::Observed));
}

#[test]
fn unknown_schema_is_refused() {
    let mut machine = LeaseMachine::new();
    let observation =
        observation(1, b"observation-1", ObservationEventKind::Healthy).with_schema_version(99);
    let decision = machine.observe(observation, 100);
    assert_eq!(decision.state, PreflightState::Refused);
    assert!(matches!(decision.action, PreflightAction::Refused(_)));
}

#[test]
fn stale_observation_is_unavailable() {
    let mut machine = LeaseMachine::new();
    let observation =
        observation(1, b"observation-1", ObservationEventKind::Healthy).with_observed_at(1);
    let decision = machine.observe(observation, 100);
    assert_eq!(decision.state, PreflightState::HostUnavailable);
    assert!(matches!(decision.action, PreflightAction::Unavailable(_)));
}

#[test]
fn epoch_regression_is_refused() {
    let mut machine = LeaseMachine::new();
    let first = machine.observe(
        observation(2, b"observation-2", ObservationEventKind::Healthy),
        100,
    );
    assert_eq!(first.state, PreflightState::Observing);
    let second = machine.observe(
        observation(1, b"observation-1", ObservationEventKind::Healthy),
        100,
    );
    assert_eq!(second.state, PreflightState::Refused);
    assert!(matches!(second.action, PreflightAction::Refused(_)));
}

#[test]
fn impossible_budget_counters_are_refused() {
    let mut machine = LeaseMachine::new();
    let observation = observation(1, b"observation-1", ObservationEventKind::Healthy)
        .with_resident_bytes(2 * 1024 * 1024 * 1024);
    let decision = machine.observe(observation, 100);
    assert_eq!(decision.state, PreflightState::Refused);
    assert!(matches!(decision.action, PreflightAction::Refused(_)));
}

#[test]
fn preflight_never_claims_grant() {
    let machine = prepared_machine();
    assert!(!machine.lease_state().is_granted());
    assert!(!machine.has_host_grant());
}

#[test]
fn host_pressure_requests_demote() {
    let mut machine = LeaseMachine::new();
    let decision = machine.observe(
        observation(1, b"observation-1", ObservationEventKind::Pressure),
        100,
    );
    assert_eq!(decision.state, PreflightState::Constrained);
    let decision = machine.request_demotion();
    assert_eq!(decision.state, PreflightState::DemotionRequested);
    assert!(matches!(
        decision.action,
        PreflightAction::DemotionRequested
    ));
    assert!(!machine.lease_state().is_granted());
}

#[test]
fn reset_revoke_and_offline_are_safe() {
    let mut machine = granted_machine();
    let decision = machine.fail_closed(LifecycleEvent::Reset);
    assert!(matches!(decision, ProtocolDecision::FailAck(_)));
    assert!(matches!(
        machine.lease_state(),
        LeaseState::Failed(FailureReason::Reset)
    ));
    machine.restart();
    assert_eq!(machine.lease_state(), LeaseState::Absent);
    assert!(!machine.has_host_grant());
}

#[test]
fn guest_pfn_or_numa_claim_is_refused() {
    let mut machine = LeaseMachine::new();
    let decision = machine.refuse_guest_claim(GuestClaim::PfnRange { start: 1, count: 1 });
    assert_eq!(decision.state, PreflightState::Refused);
    assert!(matches!(decision.action, PreflightAction::Refused(_)));
    let decision = machine.refuse_guest_claim(GuestClaim::NumaNode(3));
    assert!(matches!(decision.action, PreflightAction::Refused(_)));
}

#[test]
fn deterministic_contract_failure_is_not_retried() {
    let mut machine = LeaseMachine::new();
    let observation =
        observation(1, b"observation-1", ObservationEventKind::Healthy).with_schema_version(99);
    let decision = machine.observe(observation, 100);
    assert!(!decision.retry_allowed);
    assert_eq!(machine.refusal_count(), 1);
}

#[test]
fn replayed_observation_is_idempotent() {
    let mut machine = LeaseMachine::new();
    let observation = observation(1, b"observation-1", ObservationEventKind::Healthy);
    let first = machine.observe(observation.clone(), 100);
    let second = machine.observe(observation, 100);
    assert_eq!(first.state, PreflightState::Observing);
    assert_eq!(second.state, PreflightState::Observing);
    assert!(matches!(second.action, PreflightAction::Idempotent));
    assert_eq!(machine.observation_count(), 1);
}

#[test]
// TestName: N3_RUST_GRANT_REVOKE_STATE_MACHINE
fn n3_rust_grant_revoke_state_machine() {
    let mut machine = granted_machine();
    let revoke = Revoke::new(
        LeaseId::from(opaque(b"lease-1")),
        1,
        EventId::from(opaque(b"revoke-1")),
        200,
    );
    assert!(matches!(
        machine.receive_revoke(revoke.clone()),
        ProtocolDecision::BeginDrain
    ));
    assert_eq!(machine.lease_state(), LeaseState::Quiescing(1));
    assert!(matches!(
        machine.scrub_guest_data(ScrubResult::Succeeded),
        ProtocolDecision::Noop
    ));
    let decision = machine.drain(150);
    assert!(matches!(
        decision,
        ProtocolDecision::DrainAck(DrainAck { .. })
    ));
    assert_eq!(machine.lease_state(), LeaseState::Drained(1));
    let completion = RevokeCompletion::new(
        LeaseId::from(opaque(b"lease-1")),
        1,
        EventId::from(opaque(b"revoke-1")),
    );
    assert!(matches!(
        machine.confirm_revoke(completion),
        ProtocolDecision::Revoked
    ));
    assert_eq!(machine.lease_state(), LeaseState::Revoked);
    machine.complete_cleanup();
    assert_eq!(machine.lease_state(), LeaseState::Absent);
    assert!(!machine.has_host_grant());
}

#[test]
// TestName: N3_RUST_STALE_GENERATION_REFUSAL
fn n3_rust_stale_generation_refusal() {
    let mut machine = granted_machine();
    let decision = machine.fail_closed(LifecycleEvent::Reset);
    assert!(matches!(decision, ProtocolDecision::FailAck(_)));
    machine.restart();
    let decision = machine.receive_grant(grant(1, b"grant-old"), 100);
    assert!(matches!(decision, ProtocolDecision::FailAck(_)));
    assert!(matches!(machine.lease_state(), LeaseState::Failed(_)));
    machine.restart();
    let decision = machine.receive_grant(grant(3, b"grant-gap"), 100);
    assert!(matches!(decision, ProtocolDecision::FailAck(_)));
    assert!(matches!(machine.lease_state(), LeaseState::Failed(_)));
}

#[test]
// TestName: N3_RUST_DUPLICATE_EVENT_IDEMPOTENCE
fn n3_rust_duplicate_event_idempotence() {
    let mut machine = prepared_machine();
    let first_grant = grant(1, b"grant-1");
    let ack = negotiate(&mut machine, first_grant.clone());
    assert!(matches!(
        machine.receive_grant(first_grant, 100),
        ProtocolDecision::Noop
    ));
    assert!(matches!(
        machine.accept_grant_ack(ack.clone()),
        ProtocolDecision::Accepted(LeaseState::Granted(1))
    ));
    assert!(matches!(
        machine.accept_grant_ack(ack),
        ProtocolDecision::Noop
    ));
    let conflicting = grant(1, b"grant-1").with_capacity_bytes(2 * 1024 * 1024);
    let decision = machine.receive_grant(conflicting, 100);
    assert!(matches!(decision, ProtocolDecision::FailAck(_)));
    assert!(matches!(
        machine.lease_state(),
        LeaseState::Failed(FailureReason::ConflictingDuplicate)
    ));
}

#[test]
// TestName: N3_RUST_REVOKE_WITH_INFLIGHT_REFUSAL
fn n3_rust_revoke_with_inflight_refusal() {
    let mut machine = granted_machine();
    assert!(machine.begin_io().is_ok());
    let revoke = Revoke::new(
        LeaseId::from(opaque(b"lease-1")),
        1,
        EventId::from(opaque(b"revoke-1")),
        200,
    );
    assert!(matches!(
        machine.receive_revoke(revoke),
        ProtocolDecision::BeginDrain
    ));
    let decision = machine.scrub_guest_data(ScrubResult::Succeeded);
    assert!(matches!(decision, ProtocolDecision::Noop));
    let decision = machine.drain(150);
    assert!(matches!(decision, ProtocolDecision::FailAck(_)));
    assert!(matches!(
        machine.lease_state(),
        LeaseState::Failed(FailureReason::InFlightNotDrained)
    ));
    assert!(!machine.sent_drain_ack());
}

#[test]
// TestName: N3_RUST_GUEST_CRASH_FAILSAFE
fn n3_rust_guest_crash_failsafe() {
    let mut machine = granted_machine();
    let decision = machine.fail_closed(LifecycleEvent::GuestCrash);
    assert!(matches!(decision, ProtocolDecision::FailAck(_)));
    assert!(matches!(
        machine.lease_state(),
        LeaseState::Failed(FailureReason::GuestCrash)
    ));
    machine.restart();
    assert_eq!(machine.lease_state(), LeaseState::Absent);
    let decision = machine.receive_grant(grant(1, b"grant-reused"), 100);
    assert!(matches!(decision, ProtocolDecision::FailAck(_)));
}

#[test]
// TestName: N3_RUST_DURABLE_RESTART_GENERATION_REFUSAL
fn n3_rust_durable_restart_generation_refusal() {
    let record = RestartRecord::host(
        1,
        vec![GenerationCheckpoint {
            lease_id: LeaseId::from(opaque(b"lease-1")),
            generation: 1,
        }],
    )
    .unwrap_or_else(|error| panic!("bounded host restart record: {error:?}"));
    let bytes = record.to_bytes();

    let mut old_generation = LeaseMachine::new();
    old_generation
        .restore_restart_bytes(&bytes)
        .unwrap_or_else(|error| panic!("fresh model restores host record: {error:?}"));
    assert_eq!(old_generation.restored_restart_epoch(), Some(1));
    assert_eq!(
        old_generation
            .observe(
                observation(1, b"restart-old-observation", ObservationEventKind::Healthy),
                100,
            )
            .state,
        PreflightState::Observing
    );
    assert!(matches!(
        old_generation.receive_grant(grant(1, b"restart-old-generation"), 100),
        ProtocolDecision::FailAck(FailAck {
            reason: FailureReason::StateTransition(StateTransitionError::StaleGeneration { provided: 1, expected: 1 }),
            ..
        })
    ));

    let mut newer_generation = LeaseMachine::new();
    newer_generation
        .restore_restart_bytes(&bytes)
        .unwrap_or_else(|error| panic!("second fresh model restores host record: {error:?}"));
    assert_eq!(
        newer_generation
            .observe(
                observation(1, b"restart-new-observation", ObservationEventKind::Healthy),
                100,
            )
            .state,
        PreflightState::Observing
    );
    assert!(matches!(
        newer_generation.receive_grant(grant(2, b"restart-new-generation"), 100),
        ProtocolDecision::GrantAck(_)
    ));
}

#[test]
fn restart_record_rejects_non_host_or_truncated_input_without_partial_history() {
    let record = RestartRecord::host(
        1,
        vec![GenerationCheckpoint {
            lease_id: LeaseId::from(opaque(b"lease-1")),
            generation: 1,
        }],
    )
    .unwrap_or_else(|error| panic!("bounded host restart record: {error:?}"));
    let mut non_host = record.to_bytes();
    non_host[6] = 2;

    let mut machine = LeaseMachine::new();
    assert_eq!(
        machine.restore_restart_bytes(&non_host),
        Err(FailureReason::HostAuthorityRequired)
    );
    assert_eq!(
        machine.lease_state(),
        LeaseState::Failed(FailureReason::HostAuthorityRequired)
    );
    machine.restart();

    let bytes = record.to_bytes();
    let truncated = &bytes[..bytes.len() - 1];
    assert_eq!(
        machine.restore_restart_bytes(truncated),
        Err(FailureReason::MalformedRecord)
    );
    assert_eq!(
        machine.lease_state(),
        LeaseState::Failed(FailureReason::MalformedRecord)
    );
    machine.restart();
    assert!(matches!(
        machine.receive_grant(grant(1, b"no-partial-history"), 100),
        ProtocolDecision::FailAck(FailAck {
            reason: FailureReason::NoFreshHostObservation,
            ..
        })
    ));
}

#[test]
fn n3_product_transport_scope_refusal() {
    let mut machine = LeaseMachine::new();
    let decision = machine.refuse_product_transport(ProductTransport::Nbd);
    assert_eq!(decision.state, PreflightState::Refused);
    assert!(matches!(decision.action, PreflightAction::Refused(_)));
}

#[test]
fn host_contract_owner_is_explicit() {
    let observation = observation(1, b"observation-1", ObservationEventKind::Healthy);
    assert_eq!(observation.authority, Authority::Host);
    assert!(observation.is_host_authoritative());
    assert!(!observation.uses_guest_residency_claim());
}

#[test]
fn bounded_observation_and_identity_refusals_are_fail_closed() {
    assert!(OpaqueId::new([]).is_err());
    assert!(OpaqueId::new(vec![0; 65]).is_err());
    let mut machine = LeaseMachine::new();
    let mut guest = observation(1, b"guest-authority", ObservationEventKind::Healthy);
    guest.authority = Authority::Guest;
    assert!(matches!(
        machine.observe(guest, 100).action,
        PreflightAction::Refused(FailureReason::HostAuthorityRequired)
    ));

    let mut malformed = HostObservation::host(
        2,
        1024 * 1024,
        0,
        0,
        100,
        0,
        EventId::from(opaque(b"bad-age")),
    );
    assert_eq!(malformed.validate(100), Err(FailureReason::MalformedRecord));
    malformed.max_age = 3601;
    assert_eq!(malformed.validate(100), Err(FailureReason::MalformedRecord));
    malformed.max_age = 10;
    malformed.observed_at = 101;
    assert_eq!(
        malformed.validate(100),
        Err(FailureReason::InvalidObservationClock)
    );

    let mut unknown_event = HostObservation::host(
        3,
        1024 * 1024,
        0,
        0,
        100,
        10,
        EventId::from(opaque(b"unknown-event")),
    );
    unknown_event.events = vec![ObservationEvent::new(
        EventId::from(opaque(b"unknown-signal")),
        ObservationEventKind::Unknown,
    )];
    assert_eq!(
        unknown_event.validate(100),
        Err(FailureReason::MalformedRecord)
    );
}

#[test]
fn observation_replay_conflicts_and_offline_are_safe() {
    let mut machine = LeaseMachine::new();
    let first = observation(1, b"observation-1", ObservationEventKind::Healthy);
    assert_eq!(
        machine.observe(first.clone(), 100).state,
        PreflightState::Observing
    );
    let mut changed_event = first.clone();
    changed_event.host_epoch = 2;
    assert!(matches!(
        machine.observe(changed_event, 100).action,
        PreflightAction::Refused(FailureReason::ConflictingDuplicate)
    ));

    let mut adapter_change = observation(3, b"observation-3", ObservationEventKind::Healthy);
    adapter_change.adapter_id = opaque(b"other-adapter");
    assert!(matches!(
        machine.observe(adapter_change, 100).action,
        PreflightAction::Refused(FailureReason::HostAuthorityRequired)
    ));

    let mut offline = LeaseMachine::new();
    let decision = offline.observe(
        observation(1, b"offline", ObservationEventKind::Offline),
        100,
    );
    assert_eq!(decision.state, PreflightState::HostUnavailable);
    assert!(matches!(decision.action, PreflightAction::Unavailable(_)));
    let grant = grant(1, b"offline-grant");
    assert!(matches!(
        offline.receive_grant(grant, 100),
        ProtocolDecision::FailAck(_)
    ));

    let mut fresh = LeaseMachine::new();
    assert!(matches!(
        fresh.request_demotion().action,
        PreflightAction::Unavailable(FailureReason::StateTransition(StateTransitionError::IllegalPreflight { expected: Some(PreflightState::Constrained), actual: PreflightState::HostUnavailable }))
    ));
    assert!(matches!(
        fresh
            .observe(observation(1, b"clock", ObservationEventKind::Healthy), 111)
            .action,
        PreflightAction::Unavailable(FailureReason::StaleObservation)
    ));
    assert!(matches!(
        fresh
            .observe(
                observation(2, b"clock-2", ObservationEventKind::Healthy),
                99
            )
            .action,
        PreflightAction::Unavailable(FailureReason::InvalidObservationClock)
    ));
}

#[test]
fn invalid_capacity_and_grant_identity_never_install() {
    let mut machine = prepared_machine();
    let mut invalid = grant(1, b"invalid-capacity").with_capacity_bytes(3);
    assert!(matches!(
        machine.receive_grant(invalid.clone(), 100),
        ProtocolDecision::FailAck(_)
    ));
    assert!(matches!(machine.lease_state(), LeaseState::Failed(_)));
    machine.restart();
    invalid.capacity_bytes = 2 * 1024 * 1024;
    assert!(matches!(
        machine.receive_grant(invalid, 100),
        ProtocolDecision::FailAck(FailAck { .. })
    ));

    let mut machine = prepared_machine();
    let mut bad_version = grant(1, b"bad-version");
    bad_version.contract_version = 9;
    assert!(matches!(
        machine.receive_grant(bad_version, 100),
        ProtocolDecision::FailAck(_)
    ));
    let mut machine = prepared_machine();
    let mut too_early = grant(1, b"too-early");
    too_early.issued_at = 101;
    assert!(matches!(
        machine.receive_grant(too_early, 100),
        ProtocolDecision::FailAck(_)
    ));
    let mut machine = prepared_machine();
    let mut expired = grant(1, b"expired");
    expired.deadline = 100;
    assert!(matches!(
        machine.receive_grant(expired, 100),
        ProtocolDecision::FailAck(_)
    ));
}

#[test]
fn protocol_event_dispatch_and_state_helpers_are_exercised() {
    use ramshared_tier::n3_state::{
    HostEvent, N3State, PreflightModel};

    let preflight = PreflightModel::default();
    assert_eq!(preflight.state(), PreflightState::HostUnavailable);
    assert!(preflight.latest_observation().is_none());
    assert_eq!(preflight.observation_count(), 0);
    assert_eq!(preflight.refusal_count(), 0);

    let mut machine = LeaseMachine::default();
    assert_eq!(machine.state(), LeaseState::Absent);
    assert_eq!(machine.preflight_state(), PreflightState::HostUnavailable);
    assert_eq!(LeaseState::Absent.tag(), StateTag::Absent);
    assert_eq!(LeaseState::Negotiating(7).generation(), Some(7));
    assert_eq!(LeaseState::Granted(7).generation(), Some(7));
    assert_eq!(LeaseState::Quiescing(7).generation(), Some(7));
    assert_eq!(LeaseState::Drained(7).generation(), Some(7));
    assert_eq!(LeaseState::Revoked.generation(), None);
    assert!(!LeaseState::Failed(FailureReason::Reset).is_granted());
    let _state_alias: N3State = LeaseState::Absent;

    let obs = observation(1, b"dispatch-observation", ObservationEventKind::Healthy);
    assert!(matches!(
        machine.observe(obs, 100).action,
        PreflightAction::Observed
    ));
    let g = Grant::host(
        LeaseId::from(opaque(b"lease-dispatch")),
        1,
        EventId::from(opaque(b"dispatch-grant")),
        1024 * 1024,
        1,
        100,
        200,
    )
    .with_expected_state(StateTag::Absent);
    let ack = match machine.apply_host_event(HostEvent::Grant(g), 100) {
        ProtocolDecision::GrantAck(ack) => ack,
        decision => panic!("expected grant ack, got {decision:?}"),
    };
    assert!(matches!(
        machine.apply_host_event(HostEvent::GrantAckAccepted(ack), 100),
        ProtocolDecision::Accepted(LeaseState::Granted(1))
    ));
    let dispatch_lease = opaque(b"lease-dispatch");
    assert_eq!(machine.active_lease_id(), Some(&dispatch_lease));
    assert_eq!(machine.lease_capacity_bytes(), Some(1024 * 1024));
    assert_eq!(machine.in_flight(), 0);
    assert_eq!(machine.callbacks_pending(), 0);
    assert!(!machine.scrubbed());
    assert!(machine.begin_io().is_ok());
    assert_eq!(machine.in_flight(), 1);
    assert!(machine.complete_io().is_ok());
    assert!(machine.complete_io().is_err());
}

#[test]
fn drain_refuses_pending_callbacks_timeout_and_scrub_failure() {
    let mut callbacks = granted_machine();
    assert!(callbacks.set_callbacks_pending(1).is_ok());
    let revoke = Revoke::host(
        LeaseId::from(opaque(b"lease-1")),
        1,
        EventId::from(opaque(b"revoke-callback")),
        200,
    )
    .with_expected_state(ramshared_tier::n3_state::StateTag::Granted);
    assert!(matches!(
        callbacks.receive_revoke(revoke),
        ProtocolDecision::BeginDrain
    ));
    assert!(matches!(callbacks.drain(150), ProtocolDecision::FailAck(_)));

    let mut pending = granted_machine();
    let revoke = Revoke::host(
        LeaseId::from(opaque(b"lease-1")),
        1,
        EventId::from(opaque(b"revoke-pending")),
        200,
    );
    assert!(matches!(
        pending.receive_revoke(revoke),
        ProtocolDecision::BeginDrain
    ));
    assert!(matches!(
        pending.drain(150),
        ProtocolDecision::Blocked(FailureReason::ScrubPending)
    ));
    assert!(matches!(
        pending.scrub_guest_data(ScrubResult::Failed),
        ProtocolDecision::FailAck(_)
    ));

    let mut timeout = granted_machine();
    let revoke = Revoke::host(
        LeaseId::from(opaque(b"lease-1")),
        1,
        EventId::from(opaque(b"revoke-timeout")),
        200,
    );
    assert!(matches!(
        timeout.receive_revoke(revoke),
        ProtocolDecision::BeginDrain
    ));
    assert!(matches!(
        timeout.scrub_guest_data(ScrubResult::Pending),
        ProtocolDecision::Noop
    ));
    assert!(matches!(timeout.drain(200), ProtocolDecision::FailAck(_)));
}

#[test]
fn revoke_identity_completion_and_cleanup_fail_closed() {
    let mut machine = granted_machine();
    let bad = Revoke::host(
        LeaseId::from(opaque(b"wrong-lease")),
        1,
        EventId::from(opaque(b"bad-revoke")),
        200,
    );
    assert!(matches!(
        machine.receive_revoke(bad),
        ProtocolDecision::FailAck(_)
    ));

    let mut machine = granted_machine();
    let revoke = Revoke::host(
        LeaseId::from(opaque(b"lease-1")),
        1,
        EventId::from(opaque(b"revoke-mismatch")),
        200,
    );
    assert!(matches!(
        machine.receive_revoke(revoke),
        ProtocolDecision::BeginDrain
    ));
    assert!(matches!(
        machine.scrub_guest_data(ScrubResult::Succeeded),
        ProtocolDecision::Noop
    ));
    assert!(matches!(machine.drain(150), ProtocolDecision::DrainAck(_)));
    let bad_completion = RevokeCompletion::new(
        LeaseId::from(opaque(b"lease-1")),
        2,
        EventId::from(opaque(b"revoke-mismatch")),
    );
    assert!(matches!(
        machine.confirm_revoke(bad_completion),
        ProtocolDecision::FailAck(_)
    ));
    assert!(machine.has_host_grant());
    machine.complete_cleanup();
    assert!(!machine.has_host_grant());
    machine.restart();
    assert_eq!(machine.lease_state(), LeaseState::Absent);
}
