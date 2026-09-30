//! `ramshared-vram` — VRAM backend abstraction (RF-G1, preparation for P3).
//!
//! Separates the VRAM **control plane** (lifecycle + allocation + wipe + free-floor) from
//! the concrete backend (CUDA and Vulkan). The **data plane** (block I/O) is
//! already abstracted by `ramshared_block::BlockBackend`; this crate handles VRAM-specific operations.
//!
//! Safe Rust only, completely driver-agnostic. Concrete CUDA and Vulkan implementations live in
//! `ramshared-cuda` and `ramshared-vulkan`.
//!
//! SPEC: docs/vram-provider/SPEC.md.
#![forbid(unsafe_code)]

use std::fmt;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// The single configured-reserve authority for every VRAM admission surface.
///
/// SPEC: `docs/specs/no-milestone/gpu-reserve-floor-authority/SPEC.md`.
pub mod codec;
pub mod reserve_policy;
pub mod worker_telemetry;
pub use codec::{
    CodecAlignments, CodecChunkResult, CodecId, CodecStatus, FakeCodec, GpuCacheCodec,
    VramOutputReservation, VramSpan, crc32,
};
pub use worker_telemetry::{
    CodecCapability, CodecState, CodecTelemetry, MAX_CODEC_REFUSAL_REASON_BYTES,
    MAX_WORKER_TELEMETRY_PAYLOAD_BYTES, TELEMETRY_MAX_AGE_MS, WORKER_TELEMETRY_SCHEMA_VERSION,
    WorkerCacheTelemetry, WorkerTelemetryEnvelope,
};
pub use reserve_policy::{
    ReserveFloorEnv, ReserveFloorError, ReserveFloorPolicy, ReserveFloorSource,
    SEALED_PERCENT_SAFETY_FLOOR, enforced_free_floor_from_configured,
};

/// VRAM operation error (mapped from the backend-specific error, e.g., `CudaError`).
#[derive(Debug)]
pub enum VramError {
    /// Backend provider failure: initialization/driver/allocation error.
    Provider(String),
    /// Attempted access out of the allocated memory range.
    OutOfRange { off: u64, len: u64, size: u64 },
    /// Allocation failed due to out-of-memory.
    OutOfMemory,
    /// Invalid alignment for VRAM operation.
    InvalidAlignment,
    /// VRAM operation failed because resource is busy.
    Busy,
}

impl fmt::Display for VramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VramError::Provider(m) => write!(f, "vram provider: {m}"),
            VramError::OutOfRange { off, len, size } => {
                write!(f, "vram out-of-range: off={off} len={len} size={size}")
            }
            VramError::OutOfMemory => write!(f, "vram out of memory"),
            VramError::InvalidAlignment => write!(f, "vram invalid alignment"),
            VramError::Busy => write!(f, "vram busy"),
        }
    }
}

impl std::error::Error for VramError {}

/// Identity of the adapter used by one concrete provider instance.
/// `key` must be stable for the physical adapter when the backend exposes such an identifier.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GpuAdapterIdentity {
    pub backend: String,
    pub key: String,
    /// Windows adapter LUID, normalized as `high:low`, when exposed by the backend.
    pub luid: Option<String>,
}

impl GpuAdapterIdentity {
    /// Whether two API providers refer to the same physical adapter.
    ///
    /// Keys are backend-specific; cross-API correlation is allowed only through a shared LUID.
    pub fn matches_physical_adapter(&self, other: &Self) -> bool {
        if self.backend == other.backend {
            self.key == other.key
        } else {
            matches!((&self.luid, &other.luid), (Some(left), Some(right)) if left == right)
        }
    }
}

/// Formats the Windows LUID byte layout used by CUDA and Vulkan as `high:low`.
/// Zero is reserved as “not available” by these provider APIs.
pub fn format_luid(bytes: [u8; 8]) -> Option<String> {
    let low = u32::from_le_bytes(bytes[..4].try_into().ok()?);
    let high = u32::from_le_bytes(bytes[4..].try_into().ok()?);
    (low != 0 || high != 0).then(|| format!("{high:08x}:{low:08x}"))
}

/// Reliability of the capacity data returned by a provider.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuBudgetSource {
    /// The driver supplies the budget; usage scope follows the backend API and is not
    /// necessarily a device-wide count of other applications' allocations.
    DriverReported,
    /// Only allocations known to this provider instance are included.
    ProviderLocalEstimate,
}

/// A memory budget tied to the adapter selected by a concrete GPU provider.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GpuBudgetSnapshot {
    pub adapter: Option<GpuAdapterIdentity>,
    /// Physical capacity when the backend reports it (WDDM budget queries may omit it).
    pub total_bytes: Option<u64>,
    pub budget_bytes: u64,
    pub used_bytes: u64,
    pub source: GpuBudgetSource,
    pub sampled_at: Instant,
}

/// Wall-clock representation of an adapter-bound GPU budget for IPC and status files.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GpuBudgetTelemetry {
    pub schema_version: u8,
    pub adapter: Option<GpuAdapterIdentity>,
    pub total_bytes: Option<u64>,
    pub budget_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub source: GpuBudgetSource,
    pub sampled_at_unix_ms: u64,
}

impl GpuBudgetTelemetry {
    pub fn from_snapshot(snapshot: &GpuBudgetSnapshot, sampled_at_unix_ms: u64) -> Self {
        Self {
            schema_version: 1,
            adapter: snapshot.adapter.clone(),
            total_bytes: snapshot.total_bytes,
            budget_bytes: snapshot.budget_bytes,
            used_bytes: snapshot.used_bytes,
            available_bytes: snapshot.available_bytes(),
            source: snapshot.source,
            sampled_at_unix_ms,
        }
    }

    /// Returns headroom only for a valid, fresh, adapter-bound driver budget.
    pub fn trusted_available_at(&self, now_unix_ms: u64, max_age_ms: u64) -> Option<u64> {
        let age_ms = now_unix_ms.checked_sub(self.sampled_at_unix_ms)?;
        let available = self.budget_bytes.checked_sub(self.used_bytes)?;
        (self.schema_version == 1
            && self.adapter.is_some()
            && self.source == GpuBudgetSource::DriverReported
            && age_ms <= max_age_ms
            && self
                .total_bytes
                .is_none_or(|total| self.budget_bytes <= total)
            && self.available_bytes == available)
            .then_some(available)
    }
}

impl GpuBudgetSnapshot {
    /// Available bytes within the current allocation budget, clamped against malformed telemetry.
    pub fn available_bytes(&self) -> u64 {
        self.budget_bytes.saturating_sub(self.used_bytes)
    }

    /// Largest allocation target that preserves the configured display reserve and the
    /// caller's independent runtime headroom, including current external use.
    /// Callers must first validate source, identity, freshness, and budget consistency.
    pub fn safe_target_bytes(
        &self,
        requested_bytes: u64,
        configured_reserve_bytes: u64,
        runtime_headroom_bytes: u64,
    ) -> u64 {
        let capacity = self
            .total_bytes
            .unwrap_or(self.budget_bytes)
            .min(self.budget_bytes);
        let reserve = configured_reserve_bytes.max(capacity.div_ceil(5));
        let within_capacity = capacity.saturating_sub(reserve);
        let within_live_headroom = self
            .available_bytes()
            .saturating_sub(reserve)
            .saturating_sub(runtime_headroom_bytes);
        requested_bytes
            .min(within_capacity)
            .min(within_live_headroom)
    }

    /// Current free bytes that must remain unavailable to new allocations.
    pub fn required_free_bytes(
        &self,
        configured_reserve_bytes: u64,
        runtime_headroom_bytes: u64,
    ) -> u64 {
        let capacity = self
            .total_bytes
            .unwrap_or(self.budget_bytes)
            .min(self.budget_bytes);
        configured_reserve_bytes
            .max(capacity.div_ceil(5))
            .saturating_add(runtime_headroom_bytes)
    }

    /// Automatic admission requires a stable adapter identity and a driver-reported budget.
    /// Backend APIs define the scope and precision of the budget and usage values.
    pub fn can_admit(&self, required_bytes: u64) -> bool {
        self.can_admit_at(required_bytes, Instant::now(), Duration::from_secs(5))
    }

    /// Checks admission against a caller-supplied clock and freshness window.
    pub fn can_admit_at(&self, required_bytes: u64, now: Instant, max_age: Duration) -> bool {
        self.adapter.is_some()
            && self.source == GpuBudgetSource::DriverReported
            && now
                .checked_duration_since(self.sampled_at)
                .is_some_and(|age| age <= max_age)
            && self
                .total_bytes
                .is_none_or(|total| self.budget_bytes <= total)
            && self.available_bytes() >= required_bytes
    }
}

/// An allocated VRAM memory region. Synchronous operations (wipe/zeroing is blocking, DT-17/§11).
///
/// **Thread Affinity:** The implementation can be thread-local (CUDA is). It must be used on the
/// same thread that allocated it. This is why the daemon handles all VRAM I/O on a single thread.
/// The trait does NOT require `Send`.
pub trait VramMemory {
    /// Size of the region in bytes.
    fn len(&self) -> usize;
    /// Returns `true` if the region has 0 bytes.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Fills the entire region with zeroes (secure wipe + synchronize). DT-17/§11.
    fn zero(&mut self) -> Result<(), VramError>;
    /// Reads `dst.len()` bytes starting at `off`.
    fn read_at(&self, off: u64, dst: &mut [u8]) -> Result<(), VramError>;
    /// Writes `src` bytes starting at `off`.
    fn write_at(&mut self, off: u64, src: &[u8]) -> Result<(), VramError>;
}

/// VRAM Provider (representing an initialized thread-affinity context): Allocates regions and reports capacity metrics.
///
/// The driver lifecycle (driver load, device selection, and context creation) is the responsibility
/// of the concrete backend constructor (e.g., `Cuda::load()` + `create_context()`), as it differs per
/// backend; the daemon receives an initialized provider and communicates solely via this trait.
pub trait VramProvider {
    /// Type of the allocated region (GAT: borrows `&self`, preserving thread affinity without `Arc`).
    type Mem<'p>: VramMemory
    where
        Self: 'p;

    /// Allocates `bytes` of VRAM. The region is released when dropped (RAII).
    fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError>;

    /// Returns free and total VRAM capacities in bytes (used by the residency canary — DT-3/9/11).
    fn mem_info(&self) -> Result<(u64, u64), VramError>;

    /// Returns adapter-bound admission telemetry. The default is deliberately marked as a local
    /// estimate because legacy providers cannot prove adapter identity or external usage.
    /// Optional GPU cache codec capability (DT-2 / RF-7).
    ///
    /// Returns `None` unless the provider's optional runtime is loaded **and**
    /// the selected adapter supports it. The worker calls this only when
    /// compression is explicitly enabled (DT-9); an absent capability means
    /// raw-only operation. Adding compression methods to raw `VramMemory` is
    /// deliberately avoided — the capability is a separate opt-in surface.
    fn cache_codec(&self) -> Option<&dyn GpuCacheCodec<Self::Mem<'_>>> {
        None
    }

    fn budget_snapshot(&self) -> Result<GpuBudgetSnapshot, VramError> {
        let (available, total) = self.mem_info()?;
        Ok(GpuBudgetSnapshot {
            adapter: None,
            total_bytes: Some(total),
            budget_bytes: total,
            used_bytes: total.saturating_sub(available),
            source: GpuBudgetSource::ProviderLocalEstimate,
            sampled_at: Instant::now(),
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn budget_admission_requires_identity_and_driver_budget() {
        let budget = GpuBudgetSnapshot {
            adapter: Some(GpuAdapterIdentity {
                backend: "vulkan".into(),
                key: "uuid:test-adapter".into(),
                luid: None,
            }),
            total_bytes: Some(8_000),
            budget_bytes: 6_000,
            used_bytes: 2_000,
            source: GpuBudgetSource::DriverReported,
            sampled_at: Instant::now(),
        };
        assert_eq!(budget.available_bytes(), 4_000);
        assert!(budget.can_admit(4_000));
        assert!(!budget.can_admit(4_001));
        assert!(budget.can_admit_at(
            4_000,
            budget.sampled_at + std::time::Duration::from_secs(1),
            std::time::Duration::from_secs(5)
        ));
        assert!(!budget.can_admit_at(
            1,
            budget.sampled_at + std::time::Duration::from_secs(6),
            std::time::Duration::from_secs(5)
        ));
        assert!(!budget.can_admit_at(
            1,
            budget.sampled_at - std::time::Duration::from_secs(1),
            std::time::Duration::from_secs(5)
        ));

        let unknown = GpuBudgetSnapshot {
            adapter: None,
            source: GpuBudgetSource::ProviderLocalEstimate,
            sampled_at: Instant::now(),
            ..budget
        };
        assert!(!unknown.can_admit(1));

        let estimated = GpuBudgetSnapshot {
            adapter: budget.adapter.clone(),
            source: GpuBudgetSource::ProviderLocalEstimate,
            ..budget
        };
        assert!(!estimated.can_admit(1));

        let budget_only = GpuBudgetSnapshot {
            total_bytes: None,
            ..budget.clone()
        };
        assert!(budget_only.can_admit(4_000));
        let malformed = GpuBudgetSnapshot {
            total_bytes: Some(3_000),
            ..budget
        };
        assert!(!malformed.can_admit(1));
    }

    #[test]
    fn budget_available_saturates_when_driver_usage_exceeds_budget() {
        let snapshot = GpuBudgetSnapshot {
            adapter: None,
            total_bytes: None,
            budget_bytes: 1,
            used_bytes: 2,
            source: GpuBudgetSource::ProviderLocalEstimate,
            sampled_at: Instant::now(),
        };
        assert_eq!(snapshot.available_bytes(), 0);
    }

    #[test]
    fn budget_target_preserves_reserve_after_existing_use() {
        let budget = GpuBudgetSnapshot {
            adapter: Some(GpuAdapterIdentity {
                backend: "cuda".into(),
                key: "gpu:test".into(),
                luid: None,
            }),
            total_bytes: Some(8_000),
            budget_bytes: 6_000,
            used_bytes: 1_000,
            source: GpuBudgetSource::DriverReported,
            sampled_at: Instant::now(),
        };

        // max(1_000, 20% of 6_000) reserve + 500 runtime headroom must remain
        // free after the existing 1_000 bytes of provider/external use.
        assert_eq!(budget.safe_target_bytes(10_000, 1_000, 500), 3_300);
        assert_eq!(budget.required_free_bytes(1_000, 500), 1_700);
    }

    #[test]
    fn adapter_identity_matches_cross_api_only_through_shared_luid() {
        let cuda = GpuAdapterIdentity {
            backend: "cuda".into(),
            key: "cuda-uuid".into(),
            luid: Some("aabbccdd:00001122".into()),
        };
        let dxg = GpuAdapterIdentity {
            backend: "dxg".into(),
            key: "aabbccdd:00001122".into(),
            luid: Some("aabbccdd:00001122".into()),
        };
        let other = GpuAdapterIdentity {
            backend: "cuda".into(),
            key: "other-uuid".into(),
            luid: Some("00000001:00000002".into()),
        };
        let missing_luid = GpuAdapterIdentity {
            backend: "dxg".into(),
            key: "cuda-uuid".into(),
            luid: None,
        };
        let same_backend = GpuAdapterIdentity {
            backend: "cuda".into(),
            key: "cuda-uuid".into(),
            luid: Some("ffffffff:ffffffff".into()),
        };
        assert!(cuda.matches_physical_adapter(&dxg));
        assert!(!cuda.matches_physical_adapter(&other));
        assert!(!cuda.matches_physical_adapter(&missing_luid));
        assert!(cuda.matches_physical_adapter(&same_backend));
    }

    #[test]
    fn luid_format_matches_windows_high_low_display_order() {
        assert_eq!(
            format_luid([0x22, 0x11, 0, 0, 0xdd, 0xcc, 0xbb, 0xaa]),
            Some("aabbccdd:00001122".into())
        );
        assert_eq!(format_luid([0; 8]), None);
    }

    #[test]
    fn published_budget_is_serializable_and_admission_expires() {
        let now_ms = 1_000_000;
        let snapshot = GpuBudgetSnapshot {
            adapter: Some(GpuAdapterIdentity {
                backend: "vulkan".into(),
                key: "uuid:abcd".into(),
                luid: Some("aabbccdd:00001122".into()),
            }),
            total_bytes: Some(8_000),
            budget_bytes: 6_000,
            used_bytes: 2_000,
            source: GpuBudgetSource::DriverReported,
            sampled_at: Instant::now(),
        };
        let telemetry = GpuBudgetTelemetry::from_snapshot(&snapshot, now_ms);
        assert_eq!(telemetry.available_bytes, 4_000);
        assert_eq!(
            telemetry.trusted_available_at(now_ms + 4_000, 5_000),
            Some(4_000)
        );
        assert_eq!(telemetry.trusted_available_at(now_ms + 5_001, 5_000), None);
        assert_eq!(telemetry.trusted_available_at(now_ms - 1, 5_000), None);

        let encoded = serde_json::to_vec(&telemetry).expect("budget telemetry serializes");
        let decoded: GpuBudgetTelemetry =
            serde_json::from_slice(&encoded).expect("budget telemetry deserializes");
        assert_eq!(decoded, telemetry);
    }

    #[test]
    fn test_vram_error_display() {
        assert_eq!(
            VramError::Provider("test".to_string()).to_string(),
            "vram provider: test"
        );
        assert_eq!(
            VramError::OutOfRange {
                off: 0,
                len: 10,
                size: 5
            }
            .to_string(),
            "vram out-of-range: off=0 len=10 size=5"
        );
        assert_eq!(VramError::OutOfMemory.to_string(), "vram out of memory");
        assert_eq!(
            VramError::InvalidAlignment.to_string(),
            "vram invalid alignment"
        );
        assert_eq!(VramError::Busy.to_string(), "vram busy");
    }
}
