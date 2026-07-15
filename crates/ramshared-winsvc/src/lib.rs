//! ramshared-winsvc library surface — pure logic testable on Linux (DT-16).
//!
//! SPEC: `docs/specs/no-milestone/windows-storport-cuda-vram/SPEC.md`.
//! Unsafe is confined to Windows `windows_driver` / `windows_host` adapters.
#![cfg_attr(not(windows), forbid(unsafe_code))]

pub mod broker_tenant;
pub mod config;
pub mod cuda_probe;
pub mod driver_link;
pub mod evidence;
pub mod ntpagefile;
pub mod proto;
pub mod runtime;
pub mod service;
pub mod smoke;

#[cfg(windows)]
pub mod product_online;
#[cfg(windows)]
pub mod windows_driver;
#[cfg(windows)]
pub mod windows_host;

pub use broker_tenant::{BrokerTenant, BrokerTenantError, LeaseState};
pub use config::{ConfigError, WinDriveConfig};
pub use cuda_probe::{
    ProbeCudaError, ProbeCudaReport, probe_cuda_allocates_roundtrips_and_restores,
};
pub use driver_link::{
    DriverLink, DriverLinkError, FakeDriver, InMemoryQueue, QueueAccess, QueueMap,
};
pub use evidence::{
    EvidenceWriter, IoCounters, LatencySummary, RuntimeEvidence, nearest_rank_percentile,
    redacted_error, summarize_latencies,
};
pub use ntpagefile::{PagefileError, supported_build};
pub use proto::{ABI_VERSION, Cqe, DiskParams, Register, RingHdr, Sqe};
pub use runtime::{
    EffectLog, ProductCommand, RunMode, RuntimeError, RuntimeErrorClass, RuntimeOps, RuntimePhase,
    RuntimeState, RuntimeSummary, parse_product_cli, product_runtime_selected, run_runtime,
    stop_runtime,
};
pub use service::{
    DiskControl, FreeVram, PagefileGates, ProvisionError, ServiceState, TeardownPhase, WipeVram,
    pagefile_refusal_to_runtime, provision_after_lease, teardown_storage_only,
};
pub use smoke::{SmokeResult, post_boot_smoke};
