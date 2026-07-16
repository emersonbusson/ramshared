//! Provision helpers and Gate A/B teardown (SPEC DT-8 / DT-9 / ITEM-5).
//!
//! Pure sequencing with injectable backends so Linux unit tests cover pagefile gates.
//! Product phase ownership lives in [`crate::runtime`]; this module keeps the
//! co-residency path and authoritative two-gate stop frontier.

use crate::broker_tenant::{BrokerTenant, BrokerTenantError, LeaseState};
use crate::config::WinDriveConfig;
use crate::runtime::{RuntimeError, RuntimeErrorClass};

/// Read-only identity observed for a mounted volume before teardown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedVolumeIdentity {
    pub letter: char,
    pub vendor: String,
    pub product: String,
    pub serial: String,
    pub size_bytes: u64,
}

/// Parse the exact product prefix emitted by Windows storage surfaces.
///
/// `Get-Disk` may append the standard `SCSI Disk Device` class suffix to the
/// INQUIRY vendor/product pair. No other suffix is accepted.
pub fn parse_product_friendly_name(name: &str) -> Result<(String, String), String> {
    let fields: Vec<&str> = name.split_whitespace().collect();
    let prefix_matches = fields.len() >= 2
        && fields[0].eq_ignore_ascii_case("RAMSHARE")
        && fields[1].eq_ignore_ascii_case("VRAMDISK");
    let suffix_matches = fields.len() == 2
        || (fields.len() == 5
            && fields[2].eq_ignore_ascii_case("SCSI")
            && fields[3].eq_ignore_ascii_case("Disk")
            && fields[4].eq_ignore_ascii_case("Device"));
    if !prefix_matches || !suffix_matches {
        return Err("unexpected product friendly name".into());
    }
    Ok(("RAMSHARE".into(), "VRAMDISK".into()))
}

/// Exact product identity required to select a teardown volume.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeardownTarget {
    letter: char,
    serial: String,
    size_bytes: u64,
}

impl TeardownTarget {
    pub fn new(letter: char, serial: impl Into<String>, size_bytes: u64) -> Result<Self, String> {
        let letter = letter.to_ascii_uppercase();
        let serial = serial.into();
        if !('D'..='Z').contains(&letter) {
            return Err("teardown letter must be D..=Z".into());
        }
        if serial.len() != 16 || !serial.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("teardown serial must be 16 hexadecimal characters".into());
        }
        if size_bytes == 0 {
            return Err("teardown size must be non-zero".into());
        }
        Ok(Self {
            letter,
            serial,
            size_bytes,
        })
    }

    pub fn verify_unique(&self, observed: &[ObservedVolumeIdentity]) -> Result<char, String> {
        let mut matches = observed.iter().filter(|identity| {
            identity.letter.to_ascii_uppercase() == self.letter
                && identity.vendor.trim() == "RAMSHARE"
                && identity.product.trim() == "VRAMDISK"
                && identity.serial.eq_ignore_ascii_case(&self.serial)
                && identity.size_bytes == self.size_bytes
        });
        let Some(first) = matches.next() else {
            return Err("product volume identity missing or mismatched".into());
        };
        if matches.next().is_some() {
            return Err("product volume identity is ambiguous".into());
        }
        Ok(first.letter.to_ascii_uppercase())
    }
}

/// Teardown sub-phases for structured logging (SPEC observability).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TeardownPhase {
    Identity,
    GateA,
    Drain,
    VolumeLock,
    GateB,
    FlushDismount,
    Unregister,
    Destroy,
    Unlock,
    Wipe,
    Release,
    ResumeOnline,
}

/// High-level service state (no live handles on Linux).
#[derive(Clone, Debug, Default)]
pub struct ServiceState {
    pub lease: Option<LeaseState>,
    pub disk_created: bool,
    pub registered_queue: bool,
    /// Online after successful provision (storage-only product never sets pagefile hot).
    pub online: bool,
}

/// Result of a provision attempt.
#[derive(Debug, PartialEq)]
pub enum ProvisionError {
    Broker(BrokerTenantError),
    Config(String),
    Disk(String),
    PagefileSafety(String),
    Runtime(RuntimeError),
}

impl std::fmt::Display for ProvisionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProvisionError::Broker(e) => write!(f, "broker: {e}"),
            ProvisionError::Config(s) => write!(f, "config: {s}"),
            ProvisionError::Disk(s) => write!(f, "disk: {s}"),
            ProvisionError::PagefileSafety(s) => write!(f, "pagefile safety: {s}"),
            ProvisionError::Runtime(e) => write!(f, "runtime: {e}"),
        }
    }
}

impl std::error::Error for ProvisionError {}

/// Injectable free-VRAM probe (DT-20 / co-residency).
pub trait FreeVram {
    fn free_bytes(&self) -> u64;
}

/// Injectable disk control (IOCTL CREATE/DESTROY/REGISTER).
pub trait DiskControl {
    fn create_disk(&mut self, size_bytes: u64, block_size: u32) -> Result<(), String>;
    fn destroy_disk(&mut self) -> Result<(), String>;
    fn register_queue(&mut self) -> Result<(), String>;
    fn unregister_queue(&mut self) -> Result<(), String>;
}

/// Wipe VRAM after destroy.
pub trait WipeVram {
    fn zero(&mut self) -> Result<(), String>;
}

/// Authoritative OS pagefile / volume queries (DT-8).
pub trait PagefileGates {
    /// Read-only exact identity check. Any mismatch or ambiguity is unsafe.
    fn verify_volume_identity(&self, letter: char) -> Result<(), String>;
    /// Gate A/B: list active pagefile identities. Err = query unsafe (fail-closed).
    fn active_pagefiles(&self) -> Result<Vec<String>, String>;
    /// Exclusive volume lock. Err = lock failure.
    fn lock_volume(&mut self, letter: char) -> Result<(), String>;
    fn unlock_volume(&mut self) -> Result<(), String>;
    fn flush_and_dismount(&mut self) -> Result<(), String>;
    fn volume_locked(&self) -> bool;
}

/// Apply co-residency gate then CREATE + REGISTER (unit-testable path).
pub fn provision_after_lease(
    cfg: &WinDriveConfig,
    state: &mut ServiceState,
    lease: LeaseState,
    free: &dyn FreeVram,
    disk: &mut dyn DiskControl,
    tenant: &mut BrokerTenant,
) -> Result<(), ProvisionError> {
    state.lease = Some(lease.clone());
    if let Err(e) = tenant.coresidence_gate(free.free_bytes(), cfg.size_bytes) {
        tenant.clear_lease();
        state.lease = None;
        return Err(ProvisionError::Broker(e));
    }
    disk.create_disk(cfg.size_bytes, cfg.block_size)
        .map_err(ProvisionError::Disk)?;
    state.disk_created = true;
    disk.register_queue().map_err(ProvisionError::Disk)?;
    state.registered_queue = true;
    state.online = true;
    Ok(())
}

/// Exact identity → Gate A → drain → lock → Gate B → flush/dismount → unregister → destroy → unlock → wipe.
///
/// On Gate A failure / Gate B failure / lock failure: resume Online, no destructive effects (code 7).
pub fn teardown_storage_only(
    cfg: &WinDriveConfig,
    state: &mut ServiceState,
    disk: &mut dyn DiskControl,
    wipe: &mut dyn WipeVram,
    gates: &mut dyn PagefileGates,
    phases: &mut Vec<TeardownPhase>,
) -> Result<(), ProvisionError> {
    if !state.online && !state.disk_created && !state.registered_queue {
        return Ok(());
    }

    phases.push(TeardownPhase::Identity);
    gates
        .verify_volume_identity(cfg.volume_letter)
        .map_err(|e| ProvisionError::PagefileSafety(format!("volume_identity: {e}")))?;

    // Gate A — before any runtime mutation past Online stop intent.
    phases.push(TeardownPhase::GateA);
    let pagefiles = gates
        .active_pagefiles()
        .map_err(|e| ProvisionError::PagefileSafety(format!("gate_a_query: {e}")))?;
    if !pagefiles.is_empty() {
        phases.push(TeardownPhase::ResumeOnline);
        return Err(ProvisionError::PagefileSafety(format!(
            "gate_a_active: {}",
            pagefiles.join(",")
        )));
    }

    phases.push(TeardownPhase::Drain);
    // Drain is caller/runtime responsibility for live I/O; marker only here.

    phases.push(TeardownPhase::VolumeLock);
    if let Err(e) = gates.lock_volume(cfg.volume_letter) {
        phases.push(TeardownPhase::ResumeOnline);
        return Err(ProvisionError::PagefileSafety(format!("volume_lock: {e}")));
    }

    phases.push(TeardownPhase::GateB);
    match gates.active_pagefiles() {
        Ok(pf) if pf.is_empty() => {}
        Ok(pf) => {
            let _ = gates.unlock_volume();
            phases.push(TeardownPhase::ResumeOnline);
            return Err(ProvisionError::PagefileSafety(format!(
                "gate_b_active: {}",
                pf.join(",")
            )));
        }
        Err(e) => {
            let _ = gates.unlock_volume();
            phases.push(TeardownPhase::ResumeOnline);
            return Err(ProvisionError::PagefileSafety(format!("gate_b_query: {e}")));
        }
    }

    phases.push(TeardownPhase::FlushDismount);
    if let Err(e) = gates.flush_and_dismount() {
        let _ = gates.unlock_volume();
        phases.push(TeardownPhase::ResumeOnline);
        return Err(ProvisionError::PagefileSafety(format!(
            "flush_dismount: {e}"
        )));
    }

    // Destructive frontier — only while volume remains locked.
    if state.registered_queue {
        phases.push(TeardownPhase::Unregister);
        disk.unregister_queue().map_err(ProvisionError::Disk)?;
        state.registered_queue = false;
    }
    if state.disk_created {
        phases.push(TeardownPhase::Destroy);
        disk.destroy_disk().map_err(ProvisionError::Disk)?;
        state.disk_created = false;
    }

    phases.push(TeardownPhase::Unlock);
    gates
        .unlock_volume()
        .map_err(|e| ProvisionError::Disk(format!("unlock: {e}")))?;

    phases.push(TeardownPhase::Wipe);
    wipe.zero().map_err(ProvisionError::Disk)?;

    phases.push(TeardownPhase::Release);
    // LeaseRelease is caller's responsibility on the broker stream after this returns.
    state.lease = None;
    state.online = false;
    Ok(())
}

/// Map pagefile safety refusal to runtime code 7.
pub fn pagefile_refusal_to_runtime(err: &ProvisionError) -> Option<RuntimeError> {
    match err {
        ProvisionError::PagefileSafety(s) => Some(RuntimeError::new(
            RuntimeErrorClass::PagefileSafety,
            7,
            s.clone(),
        )),
        _ => None,
    }
}

/// Legacy name retained for call sites that still use `teardown` without volume letter.
///
/// Prefer [`teardown_storage_only`]. This wrapper uses a refuse-all gate if no gates provided.
#[deprecated(note = "use teardown_storage_only with PagefileGates")]
pub fn teardown(
    state: &mut ServiceState,
    disk: &mut dyn DiskControl,
    wipe: &mut dyn WipeVram,
    phases: &mut Vec<TeardownPhase>,
    pagefile_clear: bool,
) -> Result<(), ProvisionError> {
    let mut gates = SimpleGates {
        clear: pagefile_clear,
        locked: false,
    };
    let cfg = WinDriveConfig {
        size_bytes: 64 * 1024 * 1024,
        block_size: 4096,
        cuda_device: 0,
        reserve_bytes: 512 * 1024 * 1024,
        queue_depth: 4,
        max_io_bytes: 1024 * 1024,
        evidence_path: std::path::PathBuf::from(r"C:\ProgramData\RamShared\evidence"),
        volume_letter: 'D',
        broker: "127.0.0.1:7700".into(),
        tenant: "wd".into(),
        heartbeat_secs: 5,
    };
    teardown_storage_only(&cfg, state, disk, wipe, &mut gates, phases)
}

struct SimpleGates {
    clear: bool,
    locked: bool,
}

impl PagefileGates for SimpleGates {
    fn verify_volume_identity(&self, _: char) -> Result<(), String> {
        Ok(())
    }

    fn active_pagefiles(&self) -> Result<Vec<String>, String> {
        if self.clear {
            Ok(vec![])
        } else {
            Ok(vec!["pagefile.sys".into()])
        }
    }
    fn lock_volume(&mut self, _: char) -> Result<(), String> {
        self.locked = true;
        Ok(())
    }
    fn unlock_volume(&mut self) -> Result<(), String> {
        self.locked = false;
        Ok(())
    }
    fn flush_and_dismount(&mut self) -> Result<(), String> {
        if !self.locked {
            return Err("not locked".into());
        }
        Ok(())
    }
    fn volume_locked(&self) -> bool {
        self.locked
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::broker_tenant::BrokerTenant;
    use std::time::Duration;

    struct FixedFree(u64);
    impl FreeVram for FixedFree {
        fn free_bytes(&self) -> u64 {
            self.0
        }
    }

    #[derive(Default)]
    struct MemDisk {
        created: bool,
        registered: bool,
        destroy_calls: u32,
        unreg_calls: u32,
    }
    impl DiskControl for MemDisk {
        fn create_disk(&mut self, _: u64, _: u32) -> Result<(), String> {
            self.created = true;
            Ok(())
        }
        fn destroy_disk(&mut self) -> Result<(), String> {
            self.destroy_calls += 1;
            self.created = false;
            Ok(())
        }
        fn register_queue(&mut self) -> Result<(), String> {
            self.registered = true;
            Ok(())
        }
        fn unregister_queue(&mut self) -> Result<(), String> {
            self.unreg_calls += 1;
            self.registered = false;
            Ok(())
        }
    }

    struct NopWipe;
    impl WipeVram for NopWipe {
        fn zero(&mut self) -> Result<(), String> {
            Ok(())
        }
    }

    struct CountingGates {
        a: Result<Vec<String>, String>,
        b: Result<Vec<String>, String>,
        n: std::cell::Cell<u32>,
        lock_fail: bool,
        locked: bool,
    }
    impl PagefileGates for CountingGates {
        fn verify_volume_identity(&self, _: char) -> Result<(), String> {
            Ok(())
        }

        fn active_pagefiles(&self) -> Result<Vec<String>, String> {
            let i = self.n.get();
            self.n.set(i + 1);
            if i == 0 {
                self.a.clone()
            } else {
                self.b.clone()
            }
        }
        fn lock_volume(&mut self, _: char) -> Result<(), String> {
            if self.lock_fail {
                return Err("lock denied".into());
            }
            self.locked = true;
            Ok(())
        }
        fn unlock_volume(&mut self) -> Result<(), String> {
            self.locked = false;
            Ok(())
        }
        fn flush_and_dismount(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn volume_locked(&self) -> bool {
            self.locked
        }
    }

    struct IdentityFailGates;

    impl PagefileGates for IdentityFailGates {
        fn verify_volume_identity(&self, _: char) -> Result<(), String> {
            Err("serial mismatch".into())
        }

        fn active_pagefiles(&self) -> Result<Vec<String>, String> {
            panic!("Gate A must not run after identity refusal")
        }

        fn lock_volume(&mut self, _: char) -> Result<(), String> {
            panic!("volume lock must not run after identity refusal")
        }

        fn unlock_volume(&mut self) -> Result<(), String> {
            Ok(())
        }

        fn flush_and_dismount(&mut self) -> Result<(), String> {
            panic!("dismount must not run after identity refusal")
        }

        fn volume_locked(&self) -> bool {
            false
        }
    }

    fn cfg() -> WinDriveConfig {
        WinDriveConfig {
            size_bytes: 64 * 1024 * 1024,
            block_size: 4096,
            cuda_device: 0,
            reserve_bytes: 512 * 1024 * 1024,
            queue_depth: 4,
            max_io_bytes: 1024 * 1024,
            evidence_path: std::path::PathBuf::from(r"C:\ProgramData\RamShared\evidence"),
            volume_letter: 'D',
            broker: "127.0.0.1:7700".into(),
            tenant: "wd".into(),
            heartbeat_secs: 5,
        }
    }

    fn online_state() -> ServiceState {
        ServiceState {
            lease: Some(LeaseState {
                lease: 1,
                bytes: 64 * 1024 * 1024,
            }),
            disk_created: true,
            registered_queue: true,
            online: true,
        }
    }

    #[test]
    fn coresidence_fail_closed_no_disk() {
        let c = cfg();
        let mut state = ServiceState::default();
        let mut disk = MemDisk::default();
        let mut tenant = BrokerTenant::new("wd", Duration::from_secs(5));
        tenant.force_lease_for_test(1, c.size_bytes);
        let e = provision_after_lease(
            &c,
            &mut state,
            LeaseState {
                lease: 1,
                bytes: c.size_bytes,
            },
            &FixedFree(100),
            &mut disk,
            &mut tenant,
        )
        .unwrap_err();
        assert!(matches!(
            e,
            ProvisionError::Broker(BrokerTenantError::CoresidenceFailClosed { .. })
        ));
        assert!(!disk.created);
        assert!(state.lease.is_none());
    }

    #[test]
    fn provision_ok_when_free_enough() {
        let c = cfg();
        let mut state = ServiceState::default();
        let mut disk = MemDisk::default();
        let mut tenant = BrokerTenant::new("wd", Duration::from_secs(5));
        provision_after_lease(
            &c,
            &mut state,
            LeaseState {
                lease: 2,
                bytes: c.size_bytes,
            },
            &FixedFree(2 << 30),
            &mut disk,
            &mut tenant,
        )
        .unwrap();
        assert!(disk.created && disk.registered);
        assert!(state.disk_created && state.online);
    }

    #[test]
    fn pagefile_active_refuses_before_mutation() {
        let c = cfg();
        let mut state = online_state();
        let mut disk = MemDisk {
            created: true,
            registered: true,
            ..Default::default()
        };
        let mut wipe = NopWipe;
        let mut gates = CountingGates {
            a: Ok(vec![r"D:\pagefile.sys".into()]),
            b: Ok(vec![]),
            n: std::cell::Cell::new(0),
            lock_fail: false,
            locked: false,
        };
        let mut phases = Vec::new();
        let e = teardown_storage_only(
            &c,
            &mut state,
            &mut disk,
            &mut wipe,
            &mut gates,
            &mut phases,
        )
        .unwrap_err();
        assert!(matches!(e, ProvisionError::PagefileSafety(s) if s.contains("gate_a")));
        assert_eq!(disk.destroy_calls, 0);
        assert_eq!(disk.unreg_calls, 0);
        assert!(state.online);
        assert!(state.disk_created);
        assert!(phases.contains(&TeardownPhase::ResumeOnline));
    }

    #[test]
    fn identity_mismatch_refuses_before_gate_a_or_mutation() {
        let c = cfg();
        let mut state = online_state();
        let mut disk = MemDisk {
            created: true,
            registered: true,
            ..Default::default()
        };
        let mut wipe = NopWipe;
        let mut gates = IdentityFailGates;
        let mut phases = Vec::new();
        let error = teardown_storage_only(
            &c,
            &mut state,
            &mut disk,
            &mut wipe,
            &mut gates,
            &mut phases,
        )
        .unwrap_err();
        assert!(
            matches!(error, ProvisionError::PagefileSafety(message) if message.contains("volume_identity"))
        );
        assert_eq!(phases, [TeardownPhase::Identity]);
        assert_eq!(disk.destroy_calls, 0);
        assert_eq!(disk.unreg_calls, 0);
        assert!(state.online);
    }

    #[test]
    fn pagefile_query_error_refuses_before_mutation() {
        let c = cfg();
        let mut state = online_state();
        let mut disk = MemDisk {
            created: true,
            registered: true,
            ..Default::default()
        };
        let mut wipe = NopWipe;
        let mut gates = CountingGates {
            a: Err("WMI timeout".into()),
            b: Ok(vec![]),
            n: std::cell::Cell::new(0),
            lock_fail: false,
            locked: false,
        };
        let mut phases = Vec::new();
        let e = teardown_storage_only(
            &c,
            &mut state,
            &mut disk,
            &mut wipe,
            &mut gates,
            &mut phases,
        )
        .unwrap_err();
        assert!(matches!(e, ProvisionError::PagefileSafety(s) if s.contains("gate_a_query")));
        assert_eq!(disk.destroy_calls, 0);
        assert!(state.online);
    }

    #[test]
    fn gate_b_failure_resumes_online_before_destroy() {
        let c = cfg();
        let mut state = online_state();
        let mut disk = MemDisk {
            created: true,
            registered: true,
            ..Default::default()
        };
        let mut wipe = NopWipe;
        let mut gates = CountingGates {
            a: Ok(vec![]),
            b: Ok(vec![r"D:\pagefile.sys".into()]),
            n: std::cell::Cell::new(0),
            lock_fail: false,
            locked: false,
        };
        let mut phases = Vec::new();
        let e = teardown_storage_only(
            &c,
            &mut state,
            &mut disk,
            &mut wipe,
            &mut gates,
            &mut phases,
        )
        .unwrap_err();
        assert!(matches!(e, ProvisionError::PagefileSafety(s) if s.contains("gate_b")));
        assert_eq!(disk.destroy_calls, 0);
        assert_eq!(disk.unreg_calls, 0);
        assert!(state.online);
        assert!(!gates.locked);
        assert!(phases.contains(&TeardownPhase::ResumeOnline));
    }

    #[test]
    fn pagefile_absent_tears_down_cleanly() {
        let c = cfg();
        let mut state = online_state();
        let mut disk = MemDisk {
            created: true,
            registered: true,
            ..Default::default()
        };
        let mut wipe = NopWipe;
        let mut gates = CountingGates {
            a: Ok(vec![]),
            b: Ok(vec![]),
            n: std::cell::Cell::new(0),
            lock_fail: false,
            locked: false,
        };
        let mut phases = Vec::new();
        teardown_storage_only(
            &c,
            &mut state,
            &mut disk,
            &mut wipe,
            &mut gates,
            &mut phases,
        )
        .unwrap();
        assert!(!state.disk_created);
        assert!(!state.online);
        assert_eq!(disk.destroy_calls, 1);
        assert_eq!(disk.unreg_calls, 1);
        assert!(!gates.locked);
    }

    #[test]
    fn stop_refusal_preserves_online_state() {
        let c = cfg();
        let mut state = online_state();
        let mut disk = MemDisk {
            created: true,
            registered: true,
            ..Default::default()
        };
        let mut wipe = NopWipe;
        let mut gates = CountingGates {
            a: Ok(vec![]),
            b: Ok(vec![]),
            n: std::cell::Cell::new(0),
            lock_fail: true,
            locked: false,
        };
        let mut phases = Vec::new();
        let e = teardown_storage_only(
            &c,
            &mut state,
            &mut disk,
            &mut wipe,
            &mut gates,
            &mut phases,
        )
        .unwrap_err();
        assert!(matches!(e, ProvisionError::PagefileSafety(ref s) if s.contains("volume_lock")));
        assert!(state.online);
        assert!(state.disk_created);
        assert_eq!(disk.destroy_calls, 0);
        let rt = pagefile_refusal_to_runtime(&e).unwrap();
        assert_eq!(rt.code, 7);
    }

    #[test]
    fn clean_teardown_order_is_drain_lock_recheck_flush_dismount_unregister_destroy_unlock_wipe_release()
     {
        let c = cfg();
        let mut state = online_state();
        let mut disk = MemDisk {
            created: true,
            registered: true,
            ..Default::default()
        };
        let mut wipe = NopWipe;
        let mut gates = CountingGates {
            a: Ok(vec![]),
            b: Ok(vec![]),
            n: std::cell::Cell::new(0),
            lock_fail: false,
            locked: false,
        };
        let mut phases = Vec::new();
        teardown_storage_only(
            &c,
            &mut state,
            &mut disk,
            &mut wipe,
            &mut gates,
            &mut phases,
        )
        .unwrap();
        assert_eq!(
            phases,
            [
                TeardownPhase::Identity,
                TeardownPhase::GateA,
                TeardownPhase::Drain,
                TeardownPhase::VolumeLock,
                TeardownPhase::GateB,
                TeardownPhase::FlushDismount,
                TeardownPhase::Unregister,
                TeardownPhase::Destroy,
                TeardownPhase::Unlock,
                TeardownPhase::Wipe,
                TeardownPhase::Release,
            ]
        );
    }

    #[test]
    fn teardown_target_requires_exact_unique_product_identity() {
        let expected = TeardownTarget::new('S', "A1B2C3D4E5F60708", 64 * 1024 * 1024).unwrap();
        let observed = ObservedVolumeIdentity {
            letter: 'S',
            vendor: "RAMSHARE".into(),
            product: "VRAMDISK".into(),
            serial: "A1B2C3D4E5F60708".into(),
            size_bytes: 64 * 1024 * 1024,
        };
        assert_eq!(expected.verify_unique(&[observed]).unwrap(), 'S');
    }

    #[test]
    fn product_friendly_name_accepts_only_exact_identity_and_standard_suffix() {
        assert_eq!(
            parse_product_friendly_name("RAMSHARE VRAMDISK SCSI Disk Device").unwrap(),
            ("RAMSHARE".into(), "VRAMDISK".into())
        );
        assert_eq!(
            parse_product_friendly_name("RAMSHARE VRAMDISK").unwrap(),
            ("RAMSHARE".into(), "VRAMDISK".into())
        );
        assert!(parse_product_friendly_name("RAMSHARE OTHER SCSI Disk Device").is_err());
        assert!(parse_product_friendly_name("RAMSHARE VRAMDISK USB Device").is_err());
    }

    #[test]
    fn teardown_target_fails_closed_on_missing_mismatch_or_ambiguity() {
        let expected = TeardownTarget::new('S', "A1B2C3D4E5F60708", 64 * 1024 * 1024).unwrap();
        assert!(expected.verify_unique(&[]).is_err());

        let wrong = ObservedVolumeIdentity {
            letter: 'S',
            vendor: "KINGSTON".into(),
            product: "SSD".into(),
            serial: "OTHER".into(),
            size_bytes: 64 * 1024 * 1024,
        };
        assert!(expected.verify_unique(&[wrong]).is_err());

        let matching = ObservedVolumeIdentity {
            letter: 'S',
            vendor: "RAMSHARE".into(),
            product: "VRAMDISK".into(),
            serial: "A1B2C3D4E5F60708".into(),
            size_bytes: 64 * 1024 * 1024,
        };
        assert!(
            expected
                .verify_unique(&[matching.clone(), matching])
                .is_err()
        );
    }
}
