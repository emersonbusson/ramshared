//! Sparse VRAM block backend — capacity without full pre-alloc.
//!
//! SPEC: `docs/specs/no-milestone/cascade-vram-ondemand/SPEC.md` ITEM-1/2.
//! Allocates CUDA/provider chunks on first **write**; Empty ranges read as zeros.
//! Free Live chunks only when the NBD swap device is empty (`used_kb == 0`) or on drop/down.

use std::time::{Duration, Instant};

use ramshared_vram::{ReserveFloorEnv, ReserveFloorPolicy, VramError, VramMemory, VramProvider};

use crate::gpu_cache_worker::RUNTIME_FREE_BUFFER_BYTES;
use crate::{BlockBackend, IoError};

/// Default chunk size (MiB) — SPEC `RAMSHARED_VRAM_CHUNK_MIB` default 128.
pub const DEFAULT_CHUNK_MIB: u64 = 128;

/// Host-authoritative admission check invoked before a physical VRAM commit.
pub trait CommitBudgetGate {
    fn allow_commit(&self, committed: u64, next_chunk: u64) -> Result<(), String>;
}

/// Named sparse allocation and safety policy.
pub struct SparseVramConfig<'p> {
    pub capacity: u64,
    pub chunk_bytes: u64,
    pub block_size: u32,
    /// The one configured-reserve authority (DT-1). Resolved once at startup
    /// and passed by value to every admission surface.
    pub reserve_policy: ReserveFloorPolicy,
    pub commit_cap_bytes: Option<u64>,
    pub budget_gate: Option<&'p dyn CommitBudgetGate>,
}

/// One sparse slot.
struct Chunk<'p, P: VramProvider + 'p> {
    mem: Option<P::Mem<'p>>,
    written: bool,
    last_write: Option<Instant>,
}

/// Block device: advertised `capacity`, physical commit in `chunk_bytes` units.
pub struct SparseVramBackend<'p, P: VramProvider + 'p> {
    provider: &'p P,
    capacity: u64,
    chunk_bytes: u64,
    block_size: u32,
    /// Never allocate if `mem_info.free < reserve_floor + chunk` (keep GPU headroom).
    /// The one configured-reserve authority (DT-1).
    reserve_policy: ReserveFloorPolicy,
    /// Hard cap on sum of Live chunks (≤ capacity). Protects 6 GiB cards from full fill.
    commit_cap_bytes: u64,
    budget_gate: Option<&'p dyn CommitBudgetGate>,
    chunks: Vec<Chunk<'p, P>>,
    /// Telemetry counters.
    pub alloc_fails: u64,
    pub reclaim_frees: u64,
    pub floor_refuses: u64,
    pub budget_refuses: u64,
}

impl<'p, P: VramProvider + 'p> SparseVramBackend<'p, P> {
    /// Build empty sparse map (no provider alloc except later canary outside).
    pub fn new(
        provider: &'p P,
        capacity: u64,
        chunk_bytes: u64,
        block_size: u32,
    ) -> Result<Self, IoError> {
        Self::new_with_config(
            provider,
            SparseVramConfig {
                capacity,
                chunk_bytes,
                block_size,
                reserve_policy: sealed_reserve_policy(),
                commit_cap_bytes: None,
                budget_gate: None,
            },
        )
    }

    /// Same as [`new`] with explicit safety limits (tests + daemon).
    pub fn new_with_limits(
        provider: &'p P,
        capacity: u64,
        chunk_bytes: u64,
        block_size: u32,
        reserve_policy: ReserveFloorPolicy,
        commit_cap_bytes: Option<u64>,
    ) -> Result<Self, IoError> {
        Self::new_with_config(
            provider,
            SparseVramConfig {
                capacity,
                chunk_bytes,
                block_size,
                reserve_policy,
                commit_cap_bytes,
                budget_gate: None,
            },
        )
    }

    /// Convenience constructor for callers that supply every safety boundary.
    pub fn new_with_limits_and_gate(
        provider: &'p P,
        capacity: u64,
        chunk_bytes: u64,
        block_size: u32,
        reserve_policy: ReserveFloorPolicy,
        commit_cap_bytes: Option<u64>,
        budget_gate: Option<&'p dyn CommitBudgetGate>,
    ) -> Result<Self, IoError> {
        Self::new_with_config(
            provider,
            SparseVramConfig {
                capacity,
                chunk_bytes,
                block_size,
                reserve_policy,
                commit_cap_bytes,
                budget_gate,
            },
        )
    }

    pub fn new_with_config(provider: &'p P, config: SparseVramConfig<'p>) -> Result<Self, IoError> {
        if config.capacity == 0 {
            return Err(IoError("sparse: capacity 0".into()));
        }
        if config.chunk_bytes == 0
            || !config
                .chunk_bytes
                .is_multiple_of(u64::from(config.block_size))
        {
            return Err(IoError(format!(
                "sparse: chunk_bytes={} must be >0 and multiple of block_size={}",
                config.chunk_bytes, config.block_size
            )));
        }
        let n = config.capacity.div_ceil(config.chunk_bytes);
        if n > 1_000_000 {
            return Err(IoError(format!("sparse: too many chunks ({n})")));
        }
        // Cap commit to capacity; optional env can lower further.
        let commit_cap = config
            .commit_cap_bytes
            .unwrap_or_else(commit_cap_bytes_from_env)
            .min(config.capacity)
            .max(config.chunk_bytes);
        let mut chunks = Vec::with_capacity(n as usize);
        for _ in 0..n {
            chunks.push(Chunk {
                mem: None,
                written: false,
                last_write: None,
            });
        }
        Ok(Self {
            provider,
            capacity: config.capacity,
            chunk_bytes: config.chunk_bytes,
            block_size: config.block_size,
            reserve_policy: config.reserve_policy,
            commit_cap_bytes: commit_cap,
            budget_gate: config.budget_gate,
            chunks,
            alloc_fails: 0,
            reclaim_frees: 0,
            floor_refuses: 0,
            budget_refuses: 0,
        })
    }

    pub fn commit_cap_bytes(&self) -> u64 {
        self.commit_cap_bytes
    }

    pub fn reserve_policy(&self) -> ReserveFloorPolicy {
        self.reserve_policy
    }

    pub fn capacity_bytes(&self) -> u64 {
        self.capacity
    }

    pub fn chunk_bytes(&self) -> u64 {
        self.chunk_bytes
    }

    pub fn chunks_total(&self) -> usize {
        self.chunks.len()
    }

    pub fn chunks_live(&self) -> usize {
        self.chunks.iter().filter(|c| c.mem.is_some()).count()
    }

    pub fn committed_bytes(&self) -> u64 {
        self.chunks_live() as u64 * self.chunk_bytes
    }

    /// Free all Live chunks (caller must ensure nbd used_kb == 0 or shutdown).
    pub fn free_all_live(&mut self) -> u64 {
        let mut freed = 0u64;
        for c in &mut self.chunks {
            if c.mem.take().is_some() {
                freed = freed.saturating_add(self.chunk_bytes);
                c.written = false;
                c.last_write = None;
                self.reclaim_frees = self.reclaim_frees.saturating_add(1);
            }
        }
        freed
    }

    /// MVP reclaim: only when `nbd_used_kb == 0` and (free below floor or idle).
    ///
    /// `free_floor_bytes` must be the shared three-term floor from
    /// `enforced_free_floor_from_configured` — the same value admission uses
    /// (DT-5). Callers must not pass the configured reserve alone.
    ///
    /// Returns bytes freed. Never frees when `nbd_used_kb > 0` (corruption class).
    pub fn try_reclaim(
        &mut self,
        nbd_used_kb: u64,
        free_vram_bytes: Option<u64>,
        free_floor_bytes: u64,
        idle: Duration,
    ) -> Result<u64, IoError> {
        if nbd_used_kb > 0 {
            return Ok(0);
        }
        let below_floor = free_vram_bytes.is_some_and(|f| f < free_floor_bytes);
        let now = Instant::now();
        let idle_ok = self.chunks.iter().any(|c| c.mem.is_some())
            && self.chunks.iter().filter(|c| c.mem.is_some()).all(|c| {
                c.last_write
                    .map(|t| now.duration_since(t) >= idle)
                    .unwrap_or(true)
            });
        if below_floor || idle_ok {
            return Ok(self.free_all_live());
        }
        Ok(0)
    }

    fn ensure_live(&mut self, idx: usize) -> Result<(), IoError> {
        let Some(chunk) = self.chunks.get(idx) else {
            return Err(IoError(format!(
                "sparse page table oob idx={idx} len={}",
                self.chunks.len()
            )));
        };
        if chunk.mem.is_some() {
            return Ok(());
        }
        // Commit cap: do not fill past safe physical budget (capacity may be 6G on 6G GPU).
        let next_commit = self.committed_bytes().saturating_add(self.chunk_bytes);
        if let Some(gate) = self.budget_gate
            && let Err(message) = gate.allow_commit(self.committed_bytes(), self.chunk_bytes)
        {
            self.budget_refuses = self.budget_refuses.saturating_add(1);
            return Err(IoError(format!(
                "sparse host budget constrained before allocation: {message}"
            )));
        }
        if next_commit > self.commit_cap_bytes {
            self.floor_refuses = self.floor_refuses.saturating_add(1);
            return Err(IoError(format!(
                "sparse commit_cap: committed would be {} MiB > cap {} MiB (capacity {} MiB); \
                 refusing the write because swap fallback is not guaranteed",
                next_commit >> 20,
                self.commit_cap_bytes >> 20,
                self.capacity >> 20
            )));
        }
        // Free-floor: never take the last reserve of GPU (desktop/game headroom).
        //
        // DT-5 of `gpu-reserve-floor-authority`: the threshold is the shared
        // three-term floor `required_free_bytes(configured, RUNTIME_FREE_BUFFER_BYTES)`,
        // not the configured reserve alone. Before this, the sparse tier dropped
        // both the 20% capacity share and the 640 MiB runtime buffer — the most
        // permissive production surface.
        match self.provider.mem_info() {
            Ok((free, total)) => {
                let capacity = ReserveFloorPolicy::helper_capacity(Some(total), total);
                let floor = self
                    .reserve_policy
                    .enforced_free_floor_bytes(capacity, RUNTIME_FREE_BUFFER_BYTES);
                let need = floor.saturating_add(self.chunk_bytes);
                if free < need {
                    self.floor_refuses = self.floor_refuses.saturating_add(1);
                    return Err(IoError(format!(
                        "sparse free-floor: free {} MiB < shared-floor+chunk {} MiB — refuse alloc \
                         (protect GPU)",
                        free >> 20,
                        need >> 20
                    )));
                }
            }
            Err(e) => {
                self.alloc_fails = self.alloc_fails.saturating_add(1);
                return Err(IoError(format!("sparse mem_info: {e}")));
            }
        }
        let len = self.chunk_bytes as usize;
        // Last chunk may be partial capacity — still alloc full chunk_bytes (simpler MVP).
        let mut m = match self.provider.alloc(len) {
            Ok(m) => m,
            Err(e) => {
                self.alloc_fails = self.alloc_fails.saturating_add(1);
                return Err(IoError(format!("sparse alloc chunk {idx}: {e}")));
            }
        };
        m.zero().map_err(|e| IoError(e.to_string()))?;
        let Some(chunk) = self.chunks.get_mut(idx) else {
            return Err(IoError(format!(
                "sparse page table oob idx={idx} len={}",
                self.chunks.len()
            )));
        };
        chunk.mem = Some(m);
        Ok(())
    }

    fn chunk_index(&self, off: u64) -> Result<usize, IoError> {
        if off >= self.capacity {
            return Err(IoError(format!(
                "sparse oob off={off} capacity={}",
                self.capacity
            )));
        }
        Ok((off / self.chunk_bytes) as usize)
    }
}

fn physical_range_fits(physical_len: usize, relative: usize, transfer_len: usize) -> bool {
    relative
        .checked_add(transfer_len)
        .is_some_and(|end| end <= physical_len)
}

impl<'p, P: VramProvider + 'p> BlockBackend for SparseVramBackend<'p, P> {
    fn size_bytes(&self) -> u64 {
        self.capacity
    }

    fn block_size(&self) -> u32 {
        self.block_size
    }

    fn read_at(&mut self, off: u64, buf: &mut [u8]) -> Result<(), IoError> {
        if buf.is_empty() {
            return Ok(());
        }
        let end = off
            .checked_add(buf.len() as u64)
            .filter(|&e| e <= self.capacity)
            .ok_or_else(|| {
                IoError(format!(
                    "sparse read oob off={off} len={} cap={}",
                    buf.len(),
                    self.capacity
                ))
            })?;
        let _ = end;
        let mut done = 0usize;
        while done < buf.len() {
            let abs = off + done as u64;
            let idx = self.chunk_index(abs)?;
            let chunk_base = idx as u64 * self.chunk_bytes;
            let rel = (abs - chunk_base) as usize;
            let room = (self.chunk_bytes as usize).saturating_sub(rel);
            let n = (buf.len() - done).min(room);
            let Some(chunk) = self.chunks.get(idx) else {
                return Err(IoError(format!(
                    "sparse page table oob idx={idx} len={}",
                    self.chunks.len()
                )));
            };
            if let Some(m) = &chunk.mem {
                if !physical_range_fits(m.len(), rel, n) {
                    return Err(IoError(format!(
                        "sparse physical read oob rel={rel} len={n} phys_len={}",
                        m.len()
                    )));
                }
                m.read_at(rel as u64, &mut buf[done..done + n])
                    .map_err(|e: VramError| IoError(e.to_string()))?;
            } else {
                buf[done..done + n].fill(0);
            }
            done += n;
        }
        Ok(())
    }

    fn write_at(&mut self, off: u64, data: &[u8]) -> Result<(), IoError> {
        if data.is_empty() {
            return Ok(());
        }
        let end = off
            .checked_add(data.len() as u64)
            .filter(|&e| e <= self.capacity)
            .ok_or_else(|| {
                IoError(format!(
                    "sparse write oob off={off} len={} cap={}",
                    data.len(),
                    self.capacity
                ))
            })?;
        let _ = end;
        let mut done = 0usize;
        let now = Instant::now();
        while done < data.len() {
            let abs = off + done as u64;
            let idx = self.chunk_index(abs)?;
            self.ensure_live(idx)?;
            let chunk_base = idx as u64 * self.chunk_bytes;
            let rel = (abs - chunk_base) as usize;
            let room = (self.chunk_bytes as usize).saturating_sub(rel);
            let n = (data.len() - done).min(room);
            let Some(chunk) = self.chunks.get_mut(idx) else {
                return Err(IoError(format!(
                    "sparse page table oob idx={idx} len={}",
                    self.chunks.len()
                )));
            };
            let m = chunk
                .mem
                .as_mut()
                .ok_or_else(|| IoError("sparse: mem missing after ensure".into()))?;
            if !physical_range_fits(m.len(), rel, n) {
                return Err(IoError(format!(
                    "sparse physical write oob rel={rel} len={n} phys_len={}",
                    m.len()
                )));
            }
            m.write_at(rel as u64, &data[done..done + n])
                .map_err(|e: VramError| IoError(e.to_string()))?;

            chunk.written = true;
            chunk.last_write = Some(now);
            done += n;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), IoError> {
        Ok(())
    }
}

/// Parse chunk MiB from env (SPEC bounds 16..512).
pub fn chunk_bytes_from_env() -> u64 {
    let mib = std::env::var("RAMSHARED_VRAM_CHUNK_MIB")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_CHUNK_MIB)
        .clamp(16, 512);
    mib.saturating_mul(1024 * 1024)
}

/// Idle free hysteresis seconds.
pub fn idle_free_secs_from_env() -> u64 {
    std::env::var("RAMSHARED_VRAM_IDLE_FREE_SEC")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(30)
        .clamp(1, 3600)
}

/// Sealed manifest literals for the configured reserve (DT-3).
///
/// Re-exported from `ramshared_vram::reserve_policy` — the single authority.
/// A path that has no manifest yet still enforces the sealed values instead of
/// an arbitrary default; a second literal here would be a drift hazard the
/// seal cannot detect.
pub use ramshared_vram::{SEALED_RESERVE_MIN_MIB, SEALED_RESERVE_PERCENT};

/// Resolves the configured reserve once: sealed manifest values, raised only
/// by an environment override that is at least as conservative (DT-8).
///
/// Replaces the old `unwrap_or(512).clamp(128, 4096)` reader, which silently
/// accepted a 128 MiB floor on a shared host and never applied the sealed
/// percentage share. An override below the sealed authority is a hard error:
/// the caller fails closed rather than under-reserving.
/// The sealed authority with no environment override applied.
///
/// Infallible: the sealed literals are compile-time constants and are unit
/// tested by `sealed_reserve_policy_rejects_override_below_sealed`.
pub fn sealed_reserve_policy() -> ReserveFloorPolicy {
    ReserveFloorPolicy::from_manifest(SEALED_RESERVE_MIN_MIB, SEALED_RESERVE_PERCENT)
        // Unreachable: SEALED_RESERVE_MIN_MIB is non-zero and
        // SEALED_RESERVE_PERCENT is at the safety floor. Fail closed on the
        // strongest known-good policy rather than under-reserve.
        .unwrap_or(ReserveFloorPolicy {
            min_floor_bytes: u64::MAX,
            sealed_percent: SEALED_RESERVE_PERCENT,
        })
}

/// Sealed authority raised only by an environment override (DT-8).
///
/// Errors when the override is below the seal or the two documented names
/// disagree. Callers must fail closed — never clamp, never pick one.
pub fn sealed_reserve_policy_from_env()
-> Result<ReserveFloorPolicy, ramshared_vram::ReserveFloorError> {
    let base = sealed_reserve_policy();
    let env = ReserveFloorEnv::from_process_env();
    ReserveFloorPolicy::resolve_with_env(&base, &env)
}

/// Raise-only override input in MiB, or `None` when neither name is set.
///
/// No default and no clamp (DT-8): an operator may be more conservative than
/// the seal, never less, and the resolver is what enforces that.
pub fn reserve_floor_override_mib() -> Option<u64> {
    std::env::var("RAMSHARED_MIN_VRAM_FREE_MIB")
        .or_else(|_| std::env::var("MIN_VRAM_HEADROOM_MIB"))
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
}

/// Optional hard commit cap (MiB). Unset → no extra cap beyond capacity (still free-floor).
pub fn commit_cap_bytes_from_env() -> u64 {
    if let Ok(s) = std::env::var("RAMSHARED_VRAM_COMMIT_CAP_MIB")
        && let Ok(mib) = s.trim().parse::<u64>()
    {
        return mib.clamp(256, 64 * 1024).saturating_mul(1024 * 1024);
    }
    // Default: huge (effectively capacity.min later)
    u64::MAX / 4
}

/// Safe commit budget: min(capacity, total_vram − reserve) when total known.
pub fn safe_commit_cap(capacity: u64, total_vram: u64, reserve: u64) -> u64 {
    let by_total = total_vram.saturating_sub(reserve);
    capacity.min(by_total).max(1)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use ramshared_vram::enforced_free_floor_from_configured;

    /// A policy with an explicit configured minimum and the sealed percent.
    ///
    /// `min_floor_bytes` is the resolved minimum term (DT-1), not a raw
    /// "reserve floor". `0` is the DT-9 degenerate case that never raises an
    /// allocation because the 20% share still binds.
    fn policy(min_floor_bytes: u64) -> ReserveFloorPolicy {
        ReserveFloorPolicy {
            min_floor_bytes,
            sealed_percent: SEALED_RESERVE_PERCENT,
        }
    }
    use std::cell::Cell;

    struct FakeMem(Vec<u8>);

    impl VramMemory for FakeMem {
        fn len(&self) -> usize {
            self.0.len()
        }
        fn zero(&mut self) -> Result<(), VramError> {
            self.0.fill(0);
            Ok(())
        }
        fn read_at(&self, off: u64, dst: &mut [u8]) -> Result<(), VramError> {
            let off = off as usize;
            let end = off
                .checked_add(dst.len())
                .filter(|&e| e <= self.0.len())
                .ok_or(VramError::OutOfRange {
                    off: off as u64,
                    len: dst.len() as u64,
                    size: self.0.len() as u64,
                })?;
            dst.copy_from_slice(&self.0[off..end]);
            Ok(())
        }
        fn write_at(&mut self, off: u64, src: &[u8]) -> Result<(), VramError> {
            let off = off as usize;
            let end = off
                .checked_add(src.len())
                .filter(|&e| e <= self.0.len())
                .ok_or(VramError::OutOfRange {
                    off: off as u64,
                    len: src.len() as u64,
                    size: self.0.len() as u64,
                })?;
            self.0[off..end].copy_from_slice(src);
            Ok(())
        }
    }

    struct FakeProvider {
        allocs: Cell<usize>,
        fail_next: Cell<bool>,
    }

    impl FakeProvider {
        fn new() -> Self {
            Self {
                allocs: Cell::new(0),
                fail_next: Cell::new(false),
            }
        }
    }

    /// Provider whose reported free/total are settable, so the shared free
    /// floor can be exercised at its boundary.
    struct VarProvider {
        free: Cell<u64>,
        total: u64,
        allocs: Cell<usize>,
    }

    impl VarProvider {
        fn new(free: u64, total: u64) -> Self {
            Self {
                free: Cell::new(free),
                total,
                allocs: Cell::new(0),
            }
        }
    }

    impl VramProvider for VarProvider {
        type Mem<'a>
            = FakeMem
        where
            Self: 'a;

        fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError> {
            self.allocs.set(self.allocs.get() + 1);
            Ok(FakeMem(vec![0u8; bytes]))
        }

        fn mem_info(&self) -> Result<(u64, u64), VramError> {
            Ok((self.free.get(), self.total))
        }
    }

    impl VramProvider for FakeProvider {
        type Mem<'a>
            = FakeMem
        where
            Self: 'a;

        fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError> {
            if self.fail_next.get() {
                self.fail_next.set(false);
                return Err(VramError::Provider("injected fail".into()));
            }
            self.allocs.set(self.allocs.get() + 1);
            Ok(FakeMem(vec![0u8; bytes]))
        }

        fn mem_info(&self) -> Result<(u64, u64), VramError> {
            Ok((8 << 30, 8 << 30))
        }
    }

    #[test]
    fn zero_block_size_is_rejected_without_panic() {
        let provider = FakeProvider::new();
        assert!(SparseVramBackend::new(&provider, 4096, 4096, 0).is_err());
    }

    #[test]
    fn physical_bounds_refuse_provider_io() {
        let provider = FakeProvider::new();
        let mut backend = SparseVramBackend::new(&provider, 1024 * 1024, 256 * 1024, 4096).unwrap();
        backend.ensure_live(0).unwrap();
        backend.chunks[0].mem.as_mut().unwrap().0.truncate(4096);

        let write_error = backend.write_at(0, &[1u8; 8192]).unwrap_err();
        assert!(write_error.0.contains("sparse physical write oob"));

        let mut read_buffer = [0u8; 8192];
        let read_error = backend.read_at(0, &mut read_buffer).unwrap_err();
        assert!(read_error.0.contains("sparse physical read oob"));
    }

    #[test]
    fn page_table_bounds_guard_enforces_limit() {
        let p = FakeProvider::new();
        let mut be = SparseVramBackend::new(&p, 1024 * 1024, 256 * 1024, 4096).unwrap();
        // Artificially truncate chunks array to simulate a broken page table
        be.chunks.pop();
        let off = 3 * 256 * 1024;
        let err_read = be.read_at(off, &mut [0u8; 4096]).unwrap_err();
        assert!(err_read.0.contains("sparse page table oob idx="));

        let err_write = be.write_at(off, &[0u8; 4096]).unwrap_err();
        assert!(err_write.0.contains("sparse page table oob idx="));
    }

    #[test]
    fn read_empty_is_zeros_without_alloc() {
        let p = FakeProvider::new();
        let mut be = SparseVramBackend::new(&p, 1024 * 1024, 256 * 1024, 4096).unwrap();
        let mut buf = [0xAAu8; 8192];
        be.read_at(0, &mut buf).unwrap();
        assert_eq!(buf, [0u8; 8192]);
        assert_eq!(p.allocs.get(), 0);
        assert_eq!(be.chunks_live(), 0);
    }

    #[test]
    fn write_then_read_roundtrip_one_chunk() {
        let p = FakeProvider::new();
        let mut be = SparseVramBackend::new(&p, 1024 * 1024, 256 * 1024, 4096).unwrap();
        let payload = vec![0x5Au8; 4096];
        be.write_at(4096, &payload).unwrap();
        assert_eq!(p.allocs.get(), 1);
        assert_eq!(be.chunks_live(), 1);
        let mut buf = vec![0u8; 4096];
        be.read_at(4096, &mut buf).unwrap();
        assert_eq!(buf, payload);
    }

    #[test]
    fn cross_chunk_write_two_allocs() {
        let p = FakeProvider::new();
        let chunk = 256 * 1024u64;
        let mut be = SparseVramBackend::new(&p, 2 * chunk, chunk, 4096).unwrap();
        // 8 KiB straddling the boundary
        let mut payload = vec![0x11u8; 8192];
        payload[0] = 0xAA;
        payload[8191] = 0xBB;
        let off = chunk - 4096;
        be.write_at(off, &payload).unwrap();
        assert_eq!(p.allocs.get(), 2);
        let mut buf = vec![0u8; 8192];
        be.read_at(off, &mut buf).unwrap();
        assert_eq!(buf, payload);
    }

    #[test]
    fn reclaim_blocked_when_used_kb_nonzero() {
        let p = FakeProvider::new();
        let mut be = SparseVramBackend::new(&p, 1024 * 1024, 256 * 1024, 4096).unwrap();
        be.write_at(0, &[1u8; 4096]).unwrap();
        assert_eq!(be.chunks_live(), 1);
        let freed = be
            .try_reclaim(100, Some(0), 1 << 30, Duration::from_secs(0))
            .unwrap();
        assert_eq!(freed, 0);
        assert_eq!(be.chunks_live(), 1);
    }

    #[test]
    fn reclaim_frees_when_used_zero_and_below_floor() {
        let p = FakeProvider::new();
        let mut be = SparseVramBackend::new(&p, 1024 * 1024, 256 * 1024, 4096).unwrap();
        be.write_at(0, &[1u8; 4096]).unwrap();
        let freed = be
            .try_reclaim(0, Some(0), 1 << 30, Duration::from_secs(9999))
            .unwrap();
        assert!(freed > 0);
        assert_eq!(be.chunks_live(), 0);
        // reads still zeros
        let mut buf = [0xFFu8; 4096];
        be.read_at(0, &mut buf).unwrap();
        assert_eq!(buf, [0u8; 4096]);
    }

    #[test]
    fn alloc_fail_returns_io_error() {
        let p = FakeProvider::new();
        p.fail_next.set(true);
        let mut be = SparseVramBackend::new(&p, 1024 * 1024, 256 * 1024, 4096).unwrap();
        let err = be.write_at(0, &[1u8; 4096]).unwrap_err();
        assert!(err.0.contains("alloc") || err.0.contains("fail"));
        assert_eq!(be.alloc_fails, 1);
    }

    #[test]
    fn free_floor_refuses_when_headroom_tight() {
        // FakeProvider reports 8GiB free always — use commit_cap instead for hard stop.
        let p = FakeProvider::new();
        let chunk = 256 * 1024u64;
        let mut be = SparseVramBackend::new_with_limits(
            &p,
            2 * chunk,
            chunk,
            4096,
            policy(0),   // no configured floor (fake has lots of free)
            Some(chunk), // only one chunk allowed
        )
        .unwrap();
        be.write_at(0, &[1u8; 4096]).unwrap();
        let err = be.write_at(chunk, &[2u8; 4096]).unwrap_err();
        assert!(err.0.contains("commit_cap"), "{err:?}");
        assert_eq!(be.chunks_live(), 1);
        assert!(be.floor_refuses >= 1);
    }

    #[test]
    fn host_budget_denial_prevents_cuda_allocation() {
        struct Deny;
        impl CommitBudgetGate for Deny {
            fn allow_commit(&self, _committed: u64, _next_chunk: u64) -> Result<(), String> {
                Err("WDDM constrained".into())
            }
        }
        let p = FakeProvider::new();
        let mut be = SparseVramBackend::new_with_limits_and_gate(
            &p,
            1024 * 1024,
            256 * 1024,
            4096,
            policy(0),
            None,
            Some(&Deny),
        )
        .unwrap();
        let error = be.write_at(0, &[1u8; 4096]).unwrap_err();
        assert!(error.0.contains("WDDM constrained"), "{error:?}");
        assert_eq!(p.allocs.get(), 0);
        assert_eq!(be.budget_refuses, 1);
    }

    #[test]
    fn safe_commit_cap_leaves_reserve() {
        let cap = safe_commit_cap(6 << 30, 6 << 30, 512 << 20);
        assert_eq!(cap, (6 << 30) - (512 << 20));
        let cap2 = safe_commit_cap(4 << 30, 6 << 30, 512 << 20);
        assert_eq!(cap2, 4 << 30);
    }

    #[test]
    fn rejects_zero_capacity_and_bad_chunk() {
        let p = FakeProvider::new();
        assert!(SparseVramBackend::new(&p, 0, 256 * 1024, 4096).is_err());
        assert!(SparseVramBackend::new(&p, 1024 * 1024, 0, 4096).is_err());
        assert!(SparseVramBackend::new(&p, 1024 * 1024, 1000, 4096).is_err());
    }

    #[test]
    fn ensure_live_out_of_bounds_returns_io_error() {
        let p = FakeProvider::new();
        let mut be = SparseVramBackend::new(&p, 1024 * 1024, 256 * 1024, 4096).unwrap();
        let err = be.ensure_live(9999).expect_err("should return IoError");
        assert!(
            err.0.contains("sparse page table oob idx=9999")
                || err.0.contains("exceeds physical map len")
        );
    }

    #[test]
    fn empty_read_write_and_flush_and_accessors() {
        let p = FakeProvider::new();
        let mut be = SparseVramBackend::new(&p, 1024 * 1024, 256 * 1024, 4096).unwrap();
        be.read_at(0, &mut []).unwrap();
        be.write_at(0, &[]).unwrap();
        be.flush().unwrap();
        assert_eq!(be.size_bytes(), 1024 * 1024);
        assert_eq!(be.block_size(), 4096);
        assert_eq!(be.capacity_bytes(), 1024 * 1024);
        assert_eq!(be.chunk_bytes(), 256 * 1024);
        assert!(be.chunks_total() >= 1);
        assert_eq!(be.committed_bytes(), 0);
        assert!(be.commit_cap_bytes() > 0);
        assert!(be.reserve_policy().min_floor_bytes > 0);
    }

    #[test]
    fn free_all_live_and_oob_read() {
        let p = FakeProvider::new();
        let mut be = SparseVramBackend::new(&p, 1024 * 1024, 256 * 1024, 4096).unwrap();
        be.write_at(0, &[9u8; 4096]).unwrap();
        assert_eq!(be.chunks_live(), 1);
        let freed = be.free_all_live();
        assert!(freed > 0);
        assert_eq!(be.chunks_live(), 0);
        let mut buf = [0u8; 16];
        assert!(be.read_at(2 * 1024 * 1024, &mut buf).is_err());
        assert!(be.write_at(2 * 1024 * 1024, &[1u8; 16]).is_err());
    }

    #[test]
    fn free_floor_refuses_when_provider_free_is_low() {
        struct TightProvider;
        impl VramProvider for TightProvider {
            type Mem<'a>
                = FakeMem
            where
                Self: 'a;
            fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError> {
                Ok(FakeMem(vec![0u8; bytes]))
            }
            fn mem_info(&self) -> Result<(u64, u64), VramError> {
                Ok((64 * 1024, 8 << 30)) // free tiny
            }
        }
        let p = TightProvider;
        let mut be = SparseVramBackend::new_with_limits(
            &p,
            1024 * 1024,
            256 * 1024,
            4096,
            policy(512 * 1024), // reserve 512KiB
            None,
        )
        .unwrap();
        let err = be.write_at(0, &[1u8; 4096]).unwrap_err();
        assert!(
            err.0.contains("free-floor") || err.0.contains("floor"),
            "{err:?}"
        );
    }

    #[test]
    fn mem_info_error_surfaces() {
        struct BadInfo;
        impl VramProvider for BadInfo {
            type Mem<'a>
                = FakeMem
            where
                Self: 'a;
            fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError> {
                Ok(FakeMem(vec![0u8; bytes]))
            }
            fn mem_info(&self) -> Result<(u64, u64), VramError> {
                Err(VramError::Provider("no gpu".into()))
            }
        }
        let p = BadInfo;
        let mut be =
            SparseVramBackend::new_with_limits(&p, 1024 * 1024, 256 * 1024, 4096, policy(0), None)
                .unwrap();
        let err = be.write_at(0, &[1u8; 4096]).unwrap_err();
        assert!(
            err.0.contains("mem_info") || err.0.contains("no gpu"),
            "{err:?}"
        );
        assert_eq!(be.alloc_fails, 1);
    }

    #[test]
    fn env_helpers_have_sane_defaults() {
        // Do not clobber user env permanently — only assert defaults when unset
        if std::env::var("RAMSHARED_VRAM_CHUNK_MIB").is_err() {
            let b = chunk_bytes_from_env();
            assert!(b >= 16 * 1024 * 1024);
        }
        if std::env::var("RAMSHARED_VRAM_IDLE_FREE_SEC").is_err() {
            assert_eq!(idle_free_secs_from_env(), 30);
        }
        if std::env::var("RAMSHARED_MIN_VRAM_FREE_MIB").is_err()
            && std::env::var("MIN_VRAM_HEADROOM_MIB").is_err()
        {
            // DT-3: the sealed manifest authority, not an arbitrary default.
            // The old reader returned 512 MiB and clamped overrides to
            // [128, 4096], silently accepting a 128 MiB floor on a shared host.
            let policy = sealed_reserve_policy();
            assert_eq!(policy.min_floor_bytes, SEALED_RESERVE_MIN_MIB * 1024 * 1024);
            assert_eq!(policy.sealed_percent, SEALED_RESERVE_PERCENT);
        }
        if std::env::var("RAMSHARED_VRAM_COMMIT_CAP_MIB").is_err() {
            assert!(commit_cap_bytes_from_env() > 1 << 30);
        }
    }
    #[test]
    fn sealed_reserve_policy_rejects_override_below_sealed() {
        // DT-8: raise-only. A 128 MiB override — the value the old clamp
        // silently accepted — must be refused, not clamped up.
        let base =
            ReserveFloorPolicy::from_manifest(SEALED_RESERVE_MIN_MIB, SEALED_RESERVE_PERCENT)
                .expect("sealed literals are valid");
        let too_low = ReserveFloorEnv {
            env_mib: Some(128),
            alias_mib: None,
        };
        assert!(ReserveFloorPolicy::resolve_with_env(&base, &too_low).is_err());
        // A more conservative override is accepted and raises only the minimum.
        let raised = ReserveFloorEnv {
            env_mib: Some(SEALED_RESERVE_MIN_MIB + 1024),
            alias_mib: None,
        };
        let resolved =
            ReserveFloorPolicy::resolve_with_env(&base, &raised).expect("raise is legal");
        assert_eq!(
            resolved.min_floor_bytes,
            (SEALED_RESERVE_MIN_MIB + 1024) * 1024 * 1024
        );
        // The sealed percentage share is never dropped through the seam.
        assert_eq!(resolved.sealed_percent, SEALED_RESERVE_PERCENT);
    }

    #[test]
    fn sparse_admission_uses_shared_reserve_floor() {
        // Kahneman #13 — refusal plus legitimate pass.
        //
        // DT-5: the sparse tier must refuse below the **shared** three-term
        // floor, not below the configured reserve alone. Before this rewire,
        // `reserve_floor_bytes = 0` admitted any allocation and the tier
        // dropped both the 20% capacity share and the 640 MiB runtime buffer.
        let chunk = 4 * 1024 * 1024u64;
        let total = 5 * 1024 * 1024 * 1024u64;
        // Shared floor = max(0, ceil(5 GiB / 5)) + 640 MiB = 1 GiB + 640 MiB.
        let expected_floor =
            enforced_free_floor_from_configured(0, total, RUNTIME_FREE_BUFFER_BYTES);
        assert_eq!(expected_floor, (1 << 30) + RUNTIME_FREE_BUFFER_BYTES);

        // Refusal: free is above the *configured* floor (0) but below the
        // shared floor. The old surface would have admitted this.
        let just_below = expected_floor - 1;
        let p = VarProvider::new(just_below, total);
        let mut be =
            SparseVramBackend::new_with_limits(&p, 2 * chunk, chunk, 4096, policy(0), None)
                .unwrap();
        let err = be.write_at(0, &[1u8; 4096]).unwrap_err();
        assert!(
            err.0.contains("shared-floor"),
            "must refuse under the shared floor: {err:?}"
        );
        assert_eq!(be.floor_refuses, 1);
        assert_eq!(be.chunks_live(), 0);

        // Legitimate pass: the same request above the shared floor still hits.
        let p = VarProvider::new(expected_floor + chunk, total);
        let mut be =
            SparseVramBackend::new_with_limits(&p, 2 * chunk, chunk, 4096, policy(0), None)
                .unwrap();
        be.write_at(0, &[1u8; 4096]).unwrap();
        assert_eq!(be.chunks_live(), 1);
        assert_eq!(be.floor_refuses, 0);
    }

    #[test]
    fn sparse_probe_floor_matches_shared_reserve() {
        // DT-5: the demotion probe floor is the same value admission uses.
        let total = 6 * 1024 * 1024 * 1024u64;
        let configured = 2048 * 1024 * 1024u64;
        let capacity = ramshared_vram::ReserveFloorPolicy::helper_capacity(Some(total), total);
        let floor =
            enforced_free_floor_from_configured(configured, capacity, RUNTIME_FREE_BUFFER_BYTES);
        // Refusal below the shared floor (Kahneman #13).
        let p = VarProvider::new(total, total);
        let mut be = SparseVramBackend::new_with_limits(
            &p,
            total,
            4 * 1024 * 1024,
            4096,
            policy(configured),
            None,
        )
        .unwrap();
        be.write_at(0, &[1u8; 4096]).unwrap();
        let freed_tight = be
            .try_reclaim(0, Some(floor - 1), floor, Duration::from_secs(9999))
            .unwrap();
        assert!(freed_tight > 0, "probe must reclaim below the shared floor");
        // A free reading at or above the shared floor must not reclaim on the
        // floor condition alone (only on idle, which is held off here).
        be.write_at(0, &[1u8; 4096]).unwrap();
        let freed_healthy = be
            .try_reclaim(0, Some(floor), floor, Duration::from_secs(9999))
            .unwrap();
        assert_eq!(
            freed_healthy, 0,
            "no floor-triggered reclaim at or above the shared floor"
        );
    }
}
