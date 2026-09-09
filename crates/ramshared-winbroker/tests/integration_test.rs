use ramshared_broker::model::TransportKind;
use ramshared_broker::protocol::{Msg, PROTO_VERSION};
use ramshared_winbroker::{BrokerEffect, BrokerSessionCore};

#[test]
fn test_broker_initialization_and_ipc_flow() {
    let mut core = BrokerSessionCore::new(4096, "test_tenant", "inst_01");

    // Init state
    let status = core.status();
    assert_eq!(status.broker_instance_id, "inst_01");
    assert!(!status.registered);
    assert!(status.active_lease.is_none());

    // Register
    let effects = core.on_authenticated_msg(1, Msg::Register {
        proto: PROTO_VERSION,
        tenant: "test_tenant".into(),
        transport: TransportKind::WinDrive,
    });
    assert!(effects.contains(&BrokerEffect::Audit("registered_ready".into())));
    assert!(effects.contains(&BrokerEffect::Reply(Msg::Registered { tenant_id: 1 })));

    let status2 = core.status();
    assert!(status2.registered);

    // Lease request
    let effects2 = core.on_authenticated_msg(1, Msg::LeaseRequest { bytes: 4096 });
    assert!(effects2.contains(&BrokerEffect::Audit("lease_granted".into())));
    assert!(effects2.contains(&BrokerEffect::Reply(Msg::LeaseGranted { lease: 1, bytes: 4096 })));

    let status3 = core.status();
    assert_eq!(status3.active_lease.unwrap().bytes, 4096);

    // Heartbeat
    let effects3 = core.on_authenticated_msg(1, Msg::Psi {
        sample: Default::default(),
        swaps: Vec::new(),
        mem: None,
    });
    assert!(effects3.is_empty());

    // Shutdown/disconnect
    let effects4 = core.on_disconnect(1);
    assert!(effects4.contains(&BrokerEffect::Audit("session_disconnected".into())));
    assert!(effects4.contains(&BrokerEffect::LeaseReleased(1)));

    let status4 = core.status();
    assert!(!status4.registered);
    assert!(status4.active_lease.is_none());
}
