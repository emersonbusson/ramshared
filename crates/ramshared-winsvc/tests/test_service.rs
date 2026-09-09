use std::time::Duration;
use std::path::PathBuf;
use ramshared_winsvc::service::{ServiceState, ProvisionError, provision_after_lease, teardown_storage_only, TeardownPhase, FreeVram, DiskControl, WipeVram, PagefileGates};
use ramshared_winsvc::config::{WinDriveConfig, BrokerPipeV1};
use ramshared_winsvc::broker_tenant::{BrokerTenant, LeaseState};

// Objective: Test valid transitions (Stopped->Starting->Running->Stopping->Stopped) and reject invalid transitions.

struct MockFreeVram { bytes: u64 }
impl FreeVram for MockFreeVram { fn free_bytes(&self) -> u64 { self.bytes } }

struct MockDiskControl { created: bool, registered: bool }
impl DiskControl for MockDiskControl {
    fn create_disk(&mut self, _: u64, _: u32) -> Result<(), String> { self.created = true; Ok(()) }
    fn destroy_disk(&mut self) -> Result<(), String> { self.created = false; Ok(()) }
    fn register_queue(&mut self) -> Result<(), String> { self.registered = true; Ok(()) }
    fn unregister_queue(&mut self) -> Result<(), String> { self.registered = false; Ok(()) }
}

struct MockWipeVram;
impl WipeVram for MockWipeVram { fn zero(&mut self) -> Result<(), String> { Ok(()) } }

struct MockPagefileGates { active: bool }
impl PagefileGates for MockPagefileGates {
    fn verify_volume_identity(&self, _: char) -> Result<(), String> { Ok(()) }
    fn active_pagefiles(&self) -> Result<Vec<String>, String> {
        if self.active { Ok(vec!["D:\\pagefile.sys".to_string()]) } else { Ok(vec![]) }
    }
    fn lock_volume(&mut self, _: char) -> Result<(), String> { Ok(()) }
    fn unlock_volume(&mut self) -> Result<(), String> { Ok(()) }
    fn flush_and_dismount(&mut self) -> Result<(), String> { Ok(()) }
    fn volume_locked(&self) -> bool { false }
}

#[test]
fn test_service_transition_valid_lifecycle() {
    let mut state = ServiceState::default();
    assert!(!state.online);

    // Stop -> Starting -> Running (provision)
    let cfg = WinDriveConfig {
        tenant: "default".into(),
        size_bytes: 1024,
        block_size: 512,
        max_io_bytes: 4096,
        queue_depth: 32,
        broker_ready_timeout_secs: 5,
        evidence_path: PathBuf::from("C:\\evidence"),
        volume_letter: 'R',
        cuda_device: 0,
        reserve_bytes: 1024,
        volume_mount_path: None,
        broker_pipe: BrokerPipeV1::NamedPipeV1,
        heartbeat_secs: 5,
    };

    let mut disk = MockDiskControl { created: false, registered: false };
    let free = MockFreeVram { bytes: 2048 };
    let mut tenant = BrokerTenant::new("default", Duration::from_secs(5));

    // Attempt provision
    let lease = LeaseState { lease: 1, bytes: 1024 };
    provision_after_lease(&cfg, &mut state, lease, &free, &mut disk, &mut tenant).unwrap();

    assert!(state.online);
    assert!(state.disk_created);
    assert!(state.registered_queue);

    // Running -> Stopping -> Stopped
    let mut wipe = MockWipeVram;
    let mut gates = MockPagefileGates { active: false };
    let mut phases = vec![];

    teardown_storage_only(&cfg, &mut state, &mut disk, &mut wipe, &mut gates, &mut phases).unwrap();

    assert!(!state.online);
    assert!(!state.disk_created);
    assert!(!state.registered_queue);
}

#[test]
fn test_service_transition_invalid_gate_a_failure_preserves_running_state() {
    let mut state = ServiceState::default();
    let cfg = WinDriveConfig {
        tenant: "default".into(),
        size_bytes: 1024,
        block_size: 512,
        max_io_bytes: 4096,
        queue_depth: 32,
        broker_ready_timeout_secs: 5,
        evidence_path: PathBuf::from("C:\\evidence"),
        volume_letter: 'R',
        cuda_device: 0,
        reserve_bytes: 1024,
        volume_mount_path: None,
        broker_pipe: BrokerPipeV1::NamedPipeV1,
        heartbeat_secs: 5,
    };

    let mut disk = MockDiskControl { created: false, registered: false };
    let free = MockFreeVram { bytes: 2048 };
    let mut tenant = BrokerTenant::new("default", Duration::from_secs(5));
    let lease = LeaseState { lease: 1, bytes: 1024 };

    provision_after_lease(&cfg, &mut state, lease, &free, &mut disk, &mut tenant).unwrap();

    // Attempt teardown with active pagefile (Gate A failure)
    let mut wipe = MockWipeVram;
    let mut gates = MockPagefileGates { active: true };
    let mut phases = vec![];

    let err = teardown_storage_only(&cfg, &mut state, &mut disk, &mut wipe, &mut gates, &mut phases).unwrap_err();
    assert!(matches!(err, ProvisionError::PagefileSafety(_)));

    // State remains online
    assert!(state.online);
    assert!(state.disk_created);
    assert!(state.registered_queue);
}
