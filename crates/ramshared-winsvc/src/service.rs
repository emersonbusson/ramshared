//! Provision / teardown orchestration (SPEC ITEM-3/6/7 — RF-3/RF-5/RF-6/RF-7).
//!
//! Pure sequencing with injectable backends so Linux unit tests cover DT-9 / DT-20.

use crate::broker_tenant::{BrokerTenant, BrokerTenantError, LeaseState};
use crate::config::WinDriveConfig;
use crate::ntpagefile::{self, OsBuild, PagefileError};
use crate::smoke::{SmokeInputs, SmokeResult, post_boot_smoke};

/// Provision phase for structured logging (SPEC observability).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TeardownPhase {
    PagefileOff,
    Drain,
    Destroy,
    Wipe,
    Release,
}

/// High-level service state (no live handles on Linux).
#[derive(Clone, Debug, Default)]
pub struct ServiceState {
    pub lease: Option<LeaseState>,
    pub disk_created: bool,
    pub pagefile_active: bool,
    pub registered_queue: bool,
}

/// Result of a provision attempt.
#[derive(Debug, PartialEq)]
pub enum ProvisionError {
    Broker(BrokerTenantError),
    Pagefile(PagefileError),
    Config(String),
    Disk(String),
}

impl std::fmt::Display for ProvisionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProvisionError::Broker(e) => write!(f, "broker: {e}"),
            ProvisionError::Pagefile(e) => write!(f, "pagefile: {e}"),
            ProvisionError::Config(s) => write!(f, "config: {s}"),
            ProvisionError::Disk(s) => write!(f, "disk: {s}"),
        }
    }
}

impl std::error::Error for ProvisionError {}

/// Injectable free-VRAM probe (DT-20). Production: `cuMemGetInfo`.
pub trait FreeVram {
    fn free_bytes(&self) -> u64;
}

/// Injectable disk control (IOCTL CREATE/DESTROY). Production: control device.
pub trait DiskControl {
    fn create_disk(&mut self, size_bytes: u64, block_size: u32) -> Result<(), String>;
    fn destroy_disk(&mut self) -> Result<(), String>;
    fn register_queue(&mut self) -> Result<(), String>;
    fn unregister_queue(&mut self) -> Result<(), String>;
}

/// Wipe VRAM after destroy (DT-9).
pub trait WipeVram {
    fn zero(&mut self) -> Result<(), String>;
}

/// Apply co-residency gate then mark disk created (unit-testable DT-20 path).
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
        // Fail-closed: release lease, no CREATE_DISK.
        tenant.clear_lease();
        state.lease = None;
        return Err(ProvisionError::Broker(e));
    }
    disk.create_disk(cfg.size_bytes, cfg.block_size)
        .map_err(ProvisionError::Disk)?;
    state.disk_created = true;
    disk.register_queue().map_err(ProvisionError::Disk)?;
    state.registered_queue = true;
    Ok(())
}

/// Ordered teardown (DT-9). Never destroy disk while pagefile active.
pub fn teardown(
    state: &mut ServiceState,
    disk: &mut dyn DiskControl,
    wipe: &mut dyn WipeVram,
    phases: &mut Vec<TeardownPhase>,
) -> Result<(), ProvisionError> {
    if state.pagefile_active {
        phases.push(TeardownPhase::PagefileOff);
        // Caller must have called ntpagefile::remove_secondary (or rebooted).
        state.pagefile_active = false;
    }
    phases.push(TeardownPhase::Drain);
    if state.registered_queue {
        disk.unregister_queue().map_err(ProvisionError::Disk)?;
        state.registered_queue = false;
    }
    if state.disk_created {
        if state.pagefile_active {
            return Err(ProvisionError::Disk(
                "refusing destroy with pagefile active (DT-9 / B1 vector)".into(),
            ));
        }
        phases.push(TeardownPhase::Destroy);
        disk.destroy_disk().map_err(ProvisionError::Disk)?;
        state.disk_created = false;
    }
    phases.push(TeardownPhase::Wipe);
    wipe.zero().map_err(ProvisionError::Disk)?;
    phases.push(TeardownPhase::Release);
    // LeaseRelease is caller's responsibility on the broker stream after this returns.
    state.lease = None;
    Ok(())
}

/// Activate secondary pagefile if build allow-listed (DT-8 / DT-24).
pub fn try_enable_pagefile(
    state: &mut ServiceState,
    volume: &std::path::Path,
    cfg: &WinDriveConfig,
    build: OsBuild,
) -> Result<(), PagefileError> {
    ntpagefile::create_secondary(volume, cfg.pagefile_min, cfg.pagefile_max, Some(build))?;
    state.pagefile_active = true;
    Ok(())
}

/// Post-boot smoke wrapper.
pub fn run_smoke(inputs: &SmokeInputs) -> SmokeResult {
    post_boot_smoke(inputs)
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
    }
    impl DiskControl for MemDisk {
        fn create_disk(&mut self, _: u64, _: u32) -> Result<(), String> {
            self.created = true;
            Ok(())
        }
        fn destroy_disk(&mut self) -> Result<(), String> {
            self.created = false;
            Ok(())
        }
        fn register_queue(&mut self) -> Result<(), String> {
            self.registered = true;
            Ok(())
        }
        fn unregister_queue(&mut self) -> Result<(), String> {
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

    fn cfg() -> WinDriveConfig {
        WinDriveConfig {
            size_bytes: 1 << 30,
            block_size: 4096,
            pagefile_min: 1 << 28,
            pagefile_max: 1 << 30,
            priority: 1,
            broker: "127.0.0.1:7700".into(),
            tenant: "wd".into(),
            heartbeat_secs: 5,
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
        assert!(state.disk_created);
    }

    #[test]
    fn teardown_order_pagefile_first() {
        let mut state = ServiceState {
            lease: Some(LeaseState { lease: 1, bytes: 1 }),
            disk_created: true,
            pagefile_active: true,
            registered_queue: true,
        };
        let mut disk = MemDisk {
            created: true,
            registered: true,
        };
        let mut wipe = NopWipe;
        let mut phases = Vec::new();
        teardown(&mut state, &mut disk, &mut wipe, &mut phases).unwrap();
        assert_eq!(
            phases,
            [
                TeardownPhase::PagefileOff,
                TeardownPhase::Drain,
                TeardownPhase::Destroy,
                TeardownPhase::Wipe,
                TeardownPhase::Release,
            ]
        );
        assert!(!state.disk_created);
        assert!(!state.pagefile_active);
    }
}
