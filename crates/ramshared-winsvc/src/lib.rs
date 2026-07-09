//! ramshared-winsvc library surface — pure logic testable on Linux (DT-16).
//!
//! SPEC: `docs/specs/no-milestone/windows-swap-driver/SPEC.md` (ITEM-3/4/6/7).
#![forbid(unsafe_code)]

pub mod broker_tenant;
pub mod config;
pub mod driver_link;
pub mod ntpagefile;
pub mod proto;
pub mod service;
pub mod smoke;

pub use broker_tenant::{BrokerTenant, BrokerTenantError, LeaseState};
pub use config::{ConfigError, WinDriveConfig};
pub use driver_link::{DriverLink, DriverLinkError, FakeDriver, QueueMap};
pub use ntpagefile::{PagefileError, supported_build};
pub use proto::{ABI_VERSION, Cqe, DiskParams, Register, RingHdr, Sqe};
pub use service::{
    FreeVram, ProvisionError, ServiceState, TeardownPhase, provision_after_lease, teardown,
};
pub use smoke::{SmokeResult, post_boot_smoke};
