//! Conservative composition of allocator-reported and WDDM video-memory budgets.

use std::time::{Duration, Instant};

use ramshared_block::{GpuWorkerConfig, RUNTIME_FREE_BUFFER_BYTES};
use ramshared_dxg::{AdapterLuid, BudgetSnapshot, DxgError, GpuBudgetProvider};
use ramshared_vram::{
    GpuAdapterIdentity, GpuBudgetSnapshot, GpuBudgetSource, VramError, VramProvider,
};

pub const WDDM_BUDGET_MAX_AGE: Duration = Duration::from_secs(5);
pub const BROKER_DISPLAY_RESERVE_BYTES: u64 = 1536 * 1024 * 1024;
pub const BROKER_RUNTIME_HEADROOM_BYTES: u64 = 768 * 1024 * 1024;
const BROKER_SLICE_ALIGNMENT_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GpuBackendKind {
    Cuda,
    Vulkan,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GpuAdapterCandidate {
    pub backend: GpuBackendKind,
    pub ordinal: u32,
    pub identity: GpuAdapterIdentity,
    pub safe_target_bytes: u64,
}

/// Keeps the worker's advertised target at or below the candidate target used for ranking.
pub fn worker_config_for_candidate(
    mut config: GpuWorkerConfig,
    candidate_safe_target_bytes: u64,
) -> GpuWorkerConfig {
    config.target_bytes = config.target_bytes.min(candidate_safe_target_bytes);
    config
}

/// Calculates the same conservative capacity that the worker can actually commit.
pub fn safe_cache_target(
    budget: &GpuBudgetSnapshot,
    requested_bytes: u64,
    reserve_floor_bytes: u64,
    now: Instant,
) -> Option<u64> {
    safe_cache_target_with_runtime(
        budget,
        requested_bytes,
        reserve_floor_bytes,
        RUNTIME_FREE_BUFFER_BYTES,
        now,
    )
}

fn safe_cache_target_with_runtime(
    budget: &GpuBudgetSnapshot,
    requested_bytes: u64,
    reserve_floor_bytes: u64,
    runtime_headroom_bytes: u64,
    now: Instant,
) -> Option<u64> {
    if budget.source != GpuBudgetSource::DriverReported
        || budget.used_bytes > budget.budget_bytes
        || budget
            .total_bytes
            .is_some_and(|total| budget.budget_bytes > total)
        || budget.adapter.is_none()
        || now.checked_duration_since(budget.sampled_at)? > WDDM_BUDGET_MAX_AGE
    {
        return None;
    }
    let target =
        budget.safe_target_bytes(requested_bytes, reserve_floor_bytes, runtime_headroom_bytes);
    (target > 0).then_some(target)
}

/// Bounds the legacy direct broker's per-slice allocation against fresh live headroom,
/// preserving its canary, display reserve, and runtime buffer.
pub fn safe_broker_slice_bytes(
    budget: &GpuBudgetSnapshot,
    requested_slice_bytes: u64,
    slices: u16,
    canary_bytes: u64,
    now: Instant,
) -> Option<u64> {
    if slices == 0 || requested_slice_bytes == 0 {
        return None;
    }
    let requested_total = requested_slice_bytes.checked_mul(u64::from(slices))?;
    let requested_with_canary = requested_total.checked_add(canary_bytes)?;
    let safe_total = safe_cache_target_with_runtime(
        budget,
        requested_with_canary,
        BROKER_DISPLAY_RESERVE_BYTES,
        BROKER_RUNTIME_HEADROOM_BYTES,
        now,
    )?;
    let data_capacity = safe_total.checked_sub(canary_bytes)?;
    let safe_slice = data_capacity / u64::from(slices);
    if safe_slice == 0 {
        return None;
    }
    if safe_slice >= requested_slice_bytes {
        return Some(requested_slice_bytes);
    }
    let aligned = safe_slice / BROKER_SLICE_ALIGNMENT_BYTES * BROKER_SLICE_ALIGNMENT_BYTES;
    Some(if aligned > 0 { aligned } else { safe_slice })
}

/// Chooses the adapter with the largest safe cache target. Ties prefer CUDA, then the
/// lowest ordinal, with the normalized adapter key as the final stable tie-break.
pub fn select_gpu_candidate(candidates: &[GpuAdapterCandidate]) -> Option<&GpuAdapterCandidate> {
    candidates.iter().max_by(|left, right| {
        left.safe_target_bytes
            .cmp(&right.safe_target_bytes)
            .then_with(|| backend_tie_rank(left.backend).cmp(&backend_tie_rank(right.backend)))
            .then_with(|| right.ordinal.cmp(&left.ordinal))
            .then_with(|| right.identity.key.cmp(&left.identity.key))
    })
}

const fn backend_tie_rank(backend: GpuBackendKind) -> u8 {
    match backend {
        GpuBackendKind::Cuda => 1,
        GpuBackendKind::Vulkan => 0,
    }
}

/// Returns the lower same-adapter allocator and WDDM headroom snapshot.
pub fn constrained_budget(
    allocator: GpuBudgetSnapshot,
    wddm: BudgetSnapshot,
    now: Instant,
) -> Result<GpuBudgetSnapshot, VramError> {
    let provider_error =
        |reason: &str| VramError::Provider(format!("WDDM budget guard rejected sample: {reason}"));
    if allocator.source != GpuBudgetSource::DriverReported
        || allocator.used_bytes > allocator.budget_bytes
        || !allocator.can_admit_at(0, now, WDDM_BUDGET_MAX_AGE)
        || now.checked_duration_since(allocator.sampled_at).is_none()
        || now
            .checked_duration_since(allocator.sampled_at)
            .is_some_and(|age| age > WDDM_BUDGET_MAX_AGE)
    {
        return Err(provider_error("allocator_budget_untrusted_or_stale"));
    }
    let wddm_age = now
        .checked_duration_since(wddm.sampled_at)
        .ok_or_else(|| provider_error("wddm_sample_from_future"))?;
    if wddm_age > WDDM_BUDGET_MAX_AGE {
        return Err(provider_error("wddm_sample_stale"));
    }

    let allocator_adapter = allocator
        .adapter
        .as_ref()
        .ok_or_else(|| provider_error("allocator_adapter_missing"))?;
    let wddm_adapter = wddm.to_vram_budget();
    let wddm_identity = wddm_adapter
        .adapter
        .as_ref()
        .ok_or_else(|| provider_error("wddm_adapter_missing"))?;
    if !allocator_adapter.matches_physical_adapter(wddm_identity) {
        return Err(provider_error("adapter_mismatch"));
    }

    let wddm_available = wddm
        .budget
        .saturating_sub(wddm.current_usage)
        .min(wddm.available_for_reservation);
    let available_bytes = allocator.available_bytes().min(wddm_available);
    let budget_bytes = allocator
        .used_bytes
        .checked_add(available_bytes)
        .ok_or_else(|| provider_error("combined_budget_overflow"))?;
    Ok(GpuBudgetSnapshot {
        adapter: allocator.adapter,
        total_bytes: allocator.total_bytes,
        budget_bytes,
        used_bytes: allocator.used_bytes,
        source: GpuBudgetSource::DriverReported,
        sampled_at: allocator.sampled_at.min(wddm.sampled_at),
    })
}

/// Uses WDDM only when a provider for the exact adapter is available.
pub struct OptionalWddmBudgetProvider<P, B> {
    pub allocator: P,
    pub wddm: Option<B>,
}

impl<P, B> VramProvider for OptionalWddmBudgetProvider<P, B>
where
    P: VramProvider,
    B: GpuBudgetProvider,
{
    type Mem<'p>
        = P::Mem<'p>
    where
        Self: 'p;

    fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError> {
        self.allocator.alloc(bytes)
    }

    fn mem_info(&self) -> Result<(u64, u64), VramError> {
        let budget = self.budget_snapshot()?;
        let total = budget.total_bytes.unwrap_or(budget.budget_bytes);
        Ok((budget.available_bytes().min(total), total))
    }

    fn budget_snapshot(&self) -> Result<GpuBudgetSnapshot, VramError> {
        match &self.wddm {
            Some(wddm) => {
                let allocator = self.allocator.budget_snapshot()?;
                let wddm = wddm
                    .snapshot()
                    .map_err(|error| VramError::Provider(error.to_string()))?;
                constrained_budget(allocator, wddm, Instant::now())
            }
            None => self.allocator.budget_snapshot(),
        }
    }
}

/// Checks each direct broker allocation against fresh headroom after all reserves.
pub struct BudgetAdmissionProvider<P> {
    pub inner: P,
    pub reserve_floor_bytes: u64,
    pub runtime_headroom_bytes: u64,
}

impl<P> BudgetAdmissionProvider<P> {
    pub fn new(inner: P, reserve_floor_bytes: u64, runtime_headroom_bytes: u64) -> Self {
        Self {
            inner,
            reserve_floor_bytes,
            runtime_headroom_bytes,
        }
    }
}

impl<P: VramProvider> VramProvider for BudgetAdmissionProvider<P> {
    type Mem<'p>
        = P::Mem<'p>
    where
        Self: 'p;

    fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError> {
        let budget = self.inner.budget_snapshot()?;
        let bytes = u64::try_from(bytes).map_err(|_| VramError::OutOfMemory)?;
        let required = budget
            .required_free_bytes(self.reserve_floor_bytes, self.runtime_headroom_bytes)
            .checked_add(bytes)
            .ok_or(VramError::OutOfMemory)?;
        if !budget.can_admit(required) {
            return Err(VramError::OutOfMemory);
        }
        self.inner
            .alloc(usize::try_from(bytes).map_err(|_| VramError::OutOfMemory)?)
    }

    fn mem_info(&self) -> Result<(u64, u64), VramError> {
        self.inner.mem_info()
    }

    fn budget_snapshot(&self) -> Result<GpuBudgetSnapshot, VramError> {
        self.inner.budget_snapshot()
    }
}

/// Opens a WDDM provider only for the selected allocator's exact Windows LUID.
/// Unavailable DXG support permits allocator-only startup; malformed identity and
/// operational DXG errors remain errors.
pub fn open_matching_wddm_provider<P, B, F>(allocator: &P, open: F) -> Result<Option<B>, String>
where
    P: VramProvider,
    B: GpuBudgetProvider,
    F: FnOnce(AdapterLuid) -> Result<B, DxgError>,
{
    let Ok(snapshot) = allocator.budget_snapshot() else {
        return Ok(None);
    };
    if !snapshot.can_admit(0) {
        return Ok(None);
    }
    let Some(identity) = snapshot.adapter.as_ref() else {
        return Ok(None);
    };
    let Some(luid) = identity.luid.as_deref() else {
        return Ok(None);
    };
    let luid = AdapterLuid::parse_normalized(luid).map_err(|error| error.to_string())?;
    match open(luid) {
        Ok(provider) => {
            let observed = provider.snapshot().map_err(|error| error.to_string())?;
            if observed.adapter != luid {
                return Err(
                    "DXG returned an adapter different from the selected allocator LUID".into(),
                );
            }
            Ok(Some(provider))
        }
        Err(error) if error.permits_startup_fallback() => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

/// Forwards allocation to the active provider while constraining every budget read.
pub struct WddmBudgetGuard<P, B> {
    pub allocator: P,
    pub wddm: B,
}

impl<P, B> VramProvider for WddmBudgetGuard<P, B>
where
    P: VramProvider,
    B: GpuBudgetProvider,
{
    type Mem<'p>
        = P::Mem<'p>
    where
        Self: 'p;

    fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError> {
        self.allocator.alloc(bytes)
    }

    fn mem_info(&self) -> Result<(u64, u64), VramError> {
        self.allocator.mem_info()
    }

    fn budget_snapshot(&self) -> Result<GpuBudgetSnapshot, VramError> {
        let allocator = self.allocator.budget_snapshot()?;
        let wddm = self
            .wddm
            .snapshot()
            .map_err(|error| VramError::Provider(error.to_string()))?;
        constrained_budget(allocator, wddm, Instant::now())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    use ramshared_block::GpuCacheWorker;
    use ramshared_vram::{GpuAdapterIdentity, VramMemory};

    fn allocator_budget(sampled_at: Instant) -> GpuBudgetSnapshot {
        GpuBudgetSnapshot {
            adapter: Some(GpuAdapterIdentity {
                backend: "cuda".into(),
                key: "gpu-uuid-1".into(),
                luid: Some("aabbccdd:00001122".into()),
            }),
            total_bytes: Some(8 * 1024 * 1024 * 1024),
            budget_bytes: 6 * 1024 * 1024 * 1024,
            used_bytes: 1024 * 1024 * 1024,
            source: GpuBudgetSource::DriverReported,
            sampled_at,
        }
    }

    #[test]
    fn candidate_target_applies_reserve_freshness_and_request_cap() {
        const GIB: u64 = 1024 * 1024 * 1024;
        let now = Instant::now();
        let budget = allocator_budget(now);
        assert_eq!(safe_cache_target(&budget, 500, 100, now), Some(500));
        assert_eq!(
            safe_cache_target(&budget, 10 * GIB, 100, now),
            Some(5 * GIB - (6 * GIB).div_ceil(5) - RUNTIME_FREE_BUFFER_BYTES)
        );
        assert_eq!(safe_cache_target(&budget, 10 * GIB, 6 * GIB, now), None);
        assert_eq!(
            safe_cache_target(
                &allocator_budget(now - WDDM_BUDGET_MAX_AGE - Duration::from_millis(1)),
                500,
                100,
                now
            ),
            None
        );
        assert_eq!(
            safe_cache_target(
                &allocator_budget(now + Duration::from_secs(1)),
                500,
                100,
                now
            ),
            None
        );
    }

    #[test]
    fn direct_broker_slice_preserves_live_reserve_canary_and_alignment() {
        const GIB: u64 = 1024 * 1024 * 1024;
        let now = Instant::now();
        let canary = 16 * 1024 * 1024;
        let expected_unaligned =
            5 * GIB - BROKER_DISPLAY_RESERVE_BYTES - BROKER_RUNTIME_HEADROOM_BYTES - canary;
        let expected_aligned =
            expected_unaligned / BROKER_SLICE_ALIGNMENT_BYTES * BROKER_SLICE_ALIGNMENT_BYTES;

        assert_eq!(
            safe_broker_slice_bytes(&allocator_budget(now), 4 * GIB, 1, canary, now),
            Some(expected_aligned)
        );
        assert_eq!(
            safe_broker_slice_bytes(&allocator_budget(now), u64::MAX, 2, canary, now),
            None,
            "slice multiplication overflow must refuse allocation"
        );
        assert_eq!(
            safe_broker_slice_bytes(&allocator_budget(now), 1, 0, canary, now),
            None
        );
    }

    #[test]
    fn candidate_selection_prefers_largest_safe_target_then_stable_ties() {
        let identity = |key: &str| GpuAdapterIdentity {
            backend: "test".into(),
            key: key.into(),
            luid: None,
        };
        let candidates = [
            GpuAdapterCandidate {
                backend: GpuBackendKind::Cuda,
                ordinal: 1,
                identity: identity("cuda-1"),
                safe_target_bytes: 2_000,
            },
            GpuAdapterCandidate {
                backend: GpuBackendKind::Vulkan,
                ordinal: 0,
                identity: identity("vulkan-0"),
                safe_target_bytes: 4_000,
            },
            GpuAdapterCandidate {
                backend: GpuBackendKind::Cuda,
                ordinal: 0,
                identity: identity("cuda-0"),
                safe_target_bytes: 4_000,
            },
        ];
        let selected = select_gpu_candidate(&candidates).expect("candidate exists");
        assert_eq!(selected.backend, GpuBackendKind::Cuda);
        assert_eq!(selected.ordinal, 0);
    }

    #[test]
    fn selected_candidate_caps_worker_target_without_expanding_user_request() {
        let requested = GpuWorkerConfig {
            target_bytes: 4_000,
            chunk_bytes: 512,
            reserve_floor_bytes: 1_000,
        };

        assert_eq!(
            worker_config_for_candidate(requested, 1_000).target_bytes,
            1_000
        );
        assert_eq!(
            worker_config_for_candidate(requested, 8_000).target_bytes,
            4_000
        );
    }

    fn wddm_budget(high: u32, low: u32, sampled_at: Instant) -> BudgetSnapshot {
        BudgetSnapshot {
            adapter: AdapterLuid { high, low },
            budget: 900,
            current_usage: 300,
            current_reservation: 100,
            available_for_reservation: 450,
            sampled_at,
        }
    }

    #[test]
    fn same_adapter_budget_uses_lower_allocator_and_wddm_headroom() {
        let now = Instant::now();
        let combined = constrained_budget(
            allocator_budget(now),
            wddm_budget(0xaabb_ccdd, 0x1122, now),
            now,
        )
        .expect("same adapter budgets must combine");

        assert_eq!(combined.budget_bytes, 1024 * 1024 * 1024 + 450);
        assert_eq!(combined.used_bytes, 1024 * 1024 * 1024);
        assert_eq!(combined.available_bytes(), 450);
        assert!(combined.can_admit_at(450, now, WDDM_BUDGET_MAX_AGE));
        assert!(!combined.can_admit_at(451, now, WDDM_BUDGET_MAX_AGE));
    }

    #[test]
    fn mismatched_stale_future_and_malformed_budgets_are_rejected() {
        let now = Instant::now();
        assert!(
            constrained_budget(
                allocator_budget(now),
                wddm_budget(0xaabb_ccdd, 0x3344, now),
                now
            )
            .is_err()
        );

        let stale = wddm_budget(
            0xaabb_ccdd,
            0x1122,
            now - WDDM_BUDGET_MAX_AGE - Duration::from_millis(1),
        );
        assert!(constrained_budget(allocator_budget(now), stale, now).is_err());
        assert!(
            constrained_budget(
                allocator_budget(now + Duration::from_millis(1)),
                wddm_budget(0xaabb_ccdd, 0x1122, now),
                now
            )
            .is_err()
        );

        let mut malformed = allocator_budget(now);
        malformed.used_bytes = malformed.budget_bytes + 1;
        assert!(constrained_budget(malformed, wddm_budget(0xaabb_ccdd, 0x1122, now), now).is_err());
    }

    struct Memory(Vec<u8>);

    impl VramMemory for Memory {
        fn len(&self) -> usize {
            self.0.len()
        }

        fn zero(&mut self) -> Result<(), VramError> {
            self.0.fill(0);
            Ok(())
        }

        fn read_at(&self, off: u64, dst: &mut [u8]) -> Result<(), VramError> {
            let size = u64::try_from(self.0.len()).unwrap_or(u64::MAX);
            let len = u64::try_from(dst.len()).unwrap_or(u64::MAX);
            let start =
                usize::try_from(off).map_err(|_| VramError::OutOfRange { off, len, size })?;
            let end =
                start
                    .checked_add(dst.len())
                    .ok_or(VramError::OutOfRange { off, len, size })?;
            dst.copy_from_slice(self.0.get(start..end).ok_or(VramError::OutOfRange {
                off,
                len,
                size,
            })?);
            Ok(())
        }

        fn write_at(&mut self, off: u64, src: &[u8]) -> Result<(), VramError> {
            let size = u64::try_from(self.0.len()).unwrap_or(u64::MAX);
            let len = u64::try_from(src.len()).unwrap_or(u64::MAX);
            let start =
                usize::try_from(off).map_err(|_| VramError::OutOfRange { off, len, size })?;
            let end =
                start
                    .checked_add(src.len())
                    .ok_or(VramError::OutOfRange { off, len, size })?;
            self.0
                .get_mut(start..end)
                .ok_or(VramError::OutOfRange { off, len, size })?
                .copy_from_slice(src);
            Ok(())
        }
    }

    struct Allocator {
        budget: GpuBudgetSnapshot,
        allocations: Rc<Cell<usize>>,
        fail_budget: bool,
    }

    impl VramProvider for Allocator {
        type Mem<'p> = Memory;

        fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError> {
            self.allocations.set(self.allocations.get() + 1);
            Ok(Memory(vec![0; bytes]))
        }

        fn mem_info(&self) -> Result<(u64, u64), VramError> {
            Ok((
                self.budget.available_bytes(),
                self.budget.total_bytes.unwrap_or(0),
            ))
        }

        fn budget_snapshot(&self) -> Result<GpuBudgetSnapshot, VramError> {
            if self.fail_budget {
                Err(VramError::Provider("injected allocator failure".into()))
            } else {
                Ok(self.budget.clone())
            }
        }
    }

    struct FailingWddm;

    impl GpuBudgetProvider for FailingWddm {
        fn snapshot(&self) -> Result<BudgetSnapshot, DxgError> {
            Err(DxgError::Io("injected query failure".into()))
        }
    }

    #[test]
    fn established_wddm_query_failure_blocks_allocations() {
        let allocations = Rc::new(Cell::new(0));
        let guard = WddmBudgetGuard {
            allocator: Allocator {
                budget: allocator_budget(Instant::now()),
                allocations: allocations.clone(),
                fail_budget: false,
            },
            wddm: FailingWddm,
        };
        let mut worker = GpuCacheWorker::new(
            &guard,
            GpuWorkerConfig {
                target_bytes: 1024,
                chunk_bytes: 64,
                reserve_floor_bytes: 0,
            },
        );
        worker.handle_update(0, &[0x5a; 64]);
        assert_eq!(worker.target_bytes(), 0);
        assert_eq!(worker.active_chunks_count(), 0);
        assert_eq!(allocations.get(), 0);
    }

    struct FakeWddm {
        snapshot: BudgetSnapshot,
    }

    impl GpuBudgetProvider for FakeWddm {
        fn snapshot(&self) -> Result<BudgetSnapshot, DxgError> {
            Ok(self.snapshot)
        }
    }

    #[test]
    fn provider_open_uses_exact_luid_and_rejects_other_adapter() {
        let allocator = Allocator {
            budget: allocator_budget(Instant::now()),
            allocations: Rc::new(Cell::new(0)),
            fail_budget: false,
        };
        let selected = AdapterLuid {
            high: 0xaabb_ccdd,
            low: 0x1122,
        };
        let provider = open_matching_wddm_provider(&allocator, move |luid| {
            assert_eq!(luid, selected);
            Ok(FakeWddm {
                snapshot: wddm_budget(luid.high, luid.low, Instant::now()),
            })
        })
        .expect("matching provider should open");
        assert!(provider.is_some());

        let mismatch = open_matching_wddm_provider(&allocator, |_| {
            Ok(FakeWddm {
                snapshot: wddm_budget(0x1234, 0x5678, Instant::now()),
            })
        });
        assert!(mismatch.is_err());
    }

    #[test]
    fn missing_luid_and_unavailable_dxg_allow_allocator_only_startup() {
        let mut budget = allocator_budget(Instant::now());
        budget.adapter.as_mut().expect("adapter exists").luid = None;
        let allocator = Allocator {
            budget,
            allocations: Rc::new(Cell::new(0)),
            fail_budget: false,
        };
        let result: Result<Option<FakeWddm>, _> = open_matching_wddm_provider(&allocator, |_| {
            panic!("provider must not open without an allocator LUID")
        });
        assert!(result.expect("missing LUID is optional").is_none());

        let allocator = Allocator {
            budget: allocator_budget(Instant::now()),
            allocations: Rc::new(Cell::new(0)),
            fail_budget: false,
        };
        let result: Result<Option<FakeWddm>, _> = open_matching_wddm_provider(&allocator, |_| {
            Err(DxgError::Unavailable("missing".into()))
        });
        assert!(result.expect("unavailable DXG permits fallback").is_none());
    }

    #[test]
    fn open_policy_fails_closed_for_bad_identity_and_operational_errors() {
        let mut no_adapter = allocator_budget(Instant::now());
        no_adapter.adapter = None;
        let allocator = Allocator {
            budget: no_adapter,
            allocations: Rc::new(Cell::new(0)),
            fail_budget: false,
        };
        assert!(
            open_matching_wddm_provider(&allocator, |_| -> Result<FakeWddm, DxgError> {
                panic!("must not open")
            })
            .expect("missing adapter permits allocator startup")
            .is_none()
        );

        let allocator = Allocator {
            budget: allocator_budget(Instant::now()),
            allocations: Rc::new(Cell::new(0)),
            fail_budget: true,
        };
        assert!(
            open_matching_wddm_provider(&allocator, |_| -> Result<FakeWddm, DxgError> {
                panic!("must not open")
            })
            .expect("allocator query failure keeps startup available")
            .is_none()
        );

        let mut estimated = allocator_budget(Instant::now());
        estimated.source = GpuBudgetSource::ProviderLocalEstimate;
        let allocator = Allocator {
            budget: estimated,
            allocations: Rc::new(Cell::new(0)),
            fail_budget: false,
        };
        assert!(
            open_matching_wddm_provider(&allocator, |_| -> Result<FakeWddm, DxgError> {
                panic!("must not open")
            })
            .expect("untrusted allocator data must not open DXG")
            .is_none()
        );

        let mut malformed = allocator_budget(Instant::now());
        malformed.adapter.as_mut().unwrap().luid = Some("AABBCCDD:00001122".into());
        let allocator = Allocator {
            budget: malformed,
            allocations: Rc::new(Cell::new(0)),
            fail_budget: false,
        };
        assert!(
            open_matching_wddm_provider(&allocator, |_| -> Result<FakeWddm, DxgError> {
                panic!("must not open")
            })
            .is_err()
        );

        let allocator = Allocator {
            budget: allocator_budget(Instant::now()),
            allocations: Rc::new(Cell::new(0)),
            fail_budget: false,
        };
        assert!(
            open_matching_wddm_provider(&allocator, |_| -> Result<FakeWddm, DxgError> {
                Err(DxgError::Io("injected operational error".into()))
            })
            .is_err()
        );
        assert!(open_matching_wddm_provider(&allocator, |_| Ok(FailingWddm)).is_err());
    }

    #[test]
    fn guard_forwards_allocations_and_combines_successful_snapshots() {
        let allocations = Rc::new(Cell::new(0));
        let guard = WddmBudgetGuard {
            allocator: Allocator {
                budget: allocator_budget(Instant::now()),
                allocations: allocations.clone(),
                fail_budget: false,
            },
            wddm: FakeWddm {
                snapshot: wddm_budget(0xaabb_ccdd, 0x1122, Instant::now()),
            },
        };
        let mut memory = guard.alloc(4).expect("allocation forwards to provider");
        memory.write_at(1, &[2, 3]).expect("in-range write");
        let mut contents = [0; 2];
        memory.read_at(1, &mut contents).expect("in-range read");
        assert_eq!(contents, [2, 3]);
        assert!(memory.read_at(4, &mut contents).is_err());
        assert!(memory.write_at(4, &[1]).is_err());
        memory.zero().expect("memory wipe");
        assert_eq!(allocations.get(), 1);
        assert_eq!(
            guard.mem_info().expect("provider memory info").1,
            8 * 1024 * 1024 * 1024
        );
        assert_eq!(
            guard
                .budget_snapshot()
                .expect("combined snapshot")
                .available_bytes(),
            450
        );
    }

    #[test]
    fn optional_wddm_mem_info_reports_intersected_headroom() {
        let provider = OptionalWddmBudgetProvider {
            allocator: Allocator {
                budget: allocator_budget(Instant::now()),
                allocations: Rc::new(Cell::new(0)),
                fail_budget: false,
            },
            wddm: Some(FakeWddm {
                snapshot: wddm_budget(0xaabb_ccdd, 0x1122, Instant::now()),
            }),
        };

        assert_eq!(provider.mem_info().expect("combined memory info").0, 450);
    }

    #[test]
    fn broker_allocation_admission_preserves_reserves_and_refuses_estimates() {
        let allocations = Rc::new(Cell::new(0));
        let provider = BudgetAdmissionProvider::new(
            Allocator {
                budget: allocator_budget(Instant::now()),
                allocations: allocations.clone(),
                fail_budget: false,
            },
            BROKER_DISPLAY_RESERVE_BYTES,
            BROKER_RUNTIME_HEADROOM_BYTES,
        );
        provider
            .alloc(4096)
            .expect("small allocation fits safe headroom");
        assert_eq!(allocations.get(), 1);
        assert!(provider.alloc(4 * 1024 * 1024 * 1024usize).is_err());
        assert_eq!(
            allocations.get(),
            1,
            "unsafe allocation never reached the driver"
        );

        let estimated = GpuBudgetSnapshot {
            source: GpuBudgetSource::ProviderLocalEstimate,
            ..allocator_budget(Instant::now())
        };
        let estimated_allocations = Rc::new(Cell::new(0));
        let provider = BudgetAdmissionProvider::new(
            Allocator {
                budget: estimated,
                allocations: estimated_allocations.clone(),
                fail_budget: false,
            },
            BROKER_DISPLAY_RESERVE_BYTES,
            BROKER_RUNTIME_HEADROOM_BYTES,
        );
        assert!(provider.alloc(4096).is_err());
        assert_eq!(estimated_allocations.get(), 0);
    }
}
