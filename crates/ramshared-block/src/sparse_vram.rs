//! Sparse VRAM block backend — capacity without full pre-alloc.
//!
//! SPEC: `docs/specs/no-milestone/cascade-vram-ondemand/SPEC.md` ITEM-1/2.
//! Allocates CUDA/provider chunks on first **write**; Empty ranges read as zeros.
//! Free Live chunks only when the NBD swap device is empty (`used_kb == 0`) or on drop/down.

use std::time::{Duration, Instant};

use ramshared_vram::{VramError, VramMemory, VramProvider};

use crate::{BlockBackend, IoError};

/// Default chunk size (MiB) — SPEC `RAMSHARED_VRAM_CHUNK_MIB` default 128.
pub const DEFAULT_CHUNK_MIB: u64 = 128;

/// Host-authoritative admission check invoked before a physical VRAM commit.
pub trait CommitBudgetGate {
    fn allow_commit(&self, committed: u64, next_chunk: u64) -> Result<(), String>;
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
    reserve_floor_bytes: u64,
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
        Self::new_with_limits_and_gate(
            provider,
            capacity,
            chunk_bytes,
            block_size,
            reserve_floor_bytes_from_env(),
            None,
            None,
        )
    }

    /// Same as [`new`] with explicit safety limits (tests + daemon).
    pub fn new_with_limits(
        provider: &'p P,
        capacity: u64,
        chunk_bytes: u64,
        block_size: u32,
        reserve_floor_bytes: u64,
        commit_cap_bytes: Option<u64>,
    ) -> Result<Self, IoError> {
        Self::new_with_limits_and_gate(
            provider,
            capacity,
            chunk_bytes,
            block_size,
            reserve_floor_bytes,
            commit_cap_bytes,
            None,
        )
    }

    pub fn new_with_limits_and_gate(
        provider: &'p P,
        capacity: u64,
        chunk_bytes: u64,
        block_size: u32,
        reserve_floor_bytes: u64,
        commit_cap_bytes: Option<u64>,
        budget_gate: Option<&'p dyn CommitBudgetGate>,
    ) -> Result<Self, IoError> {
        if capacity == 0 {
            return Err(IoError("sparse: capacity 0".into()));
        }
        if chunk_bytes == 0 || !chunk_bytes.is_multiple_of(u64::from(block_size)) {
            return Err(IoError(format!(
                "sparse: chunk_bytes={chunk_bytes} must be >0 and multiple of block_size={block_size}"
            )));
        }
        let n = capacity.div_ceil(chunk_bytes);
        if n > 1_000_000 {
            return Err(IoError(format!("sparse: too many chunks ({n})")));
        }
        // Cap commit to capacity; optional env can lower further.
        let commit_cap = commit_cap_bytes
            .unwrap_or_else(commit_cap_bytes_from_env)
            .min(capacity)
            .max(chunk_bytes);
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
            capacity,
            chunk_bytes,
            block_size,
            reserve_floor_bytes,
            commit_cap_bytes: commit_cap,
            budget_gate,
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

    pub fn reserve_floor_bytes(&self) -> u64 {
        self.reserve_floor_bytes
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
        if self.chunks[idx].mem.is_some() {
            return Ok(());
        }
        // Commit cap: do not fill past safe physical budget (capacity may be 6G on 6G GPU).
        let next_commit = self.committed_bytes().saturating_add(self.chunk_bytes);
        if let Some(gate) = self.budget_gate
            && let Err(message) = gate.allow_commit(self.committed_bytes(), self.chunk_bytes)
        {
            self.budget_refuses = self.budget_refuses.saturating_add(1);
            eprintln!("sparse host budget constrained: {message}");
        }
        if next_commit > self.commit_cap_bytes {
            self.floor_refuses = self.floor_refuses.saturating_add(1);
            return Err(IoError(format!(
                "sparse commit_cap: committed would be {} MiB > cap {} MiB (capacity {} MiB) — \
                 refuse chunk; kernel may use lower swap tier",
                next_commit >> 20,
                self.commit_cap_bytes >> 20,
                self.capacity >> 20
            )));
        }
        // Free-floor: never take the last reserve of GPU (desktop/game headroom).
        match self.provider.mem_info() {
            Ok((free, _total)) => {
                let need = self.reserve_floor_bytes.saturating_add(self.chunk_bytes);
                if free < need {
                    self.floor_refuses = self.floor_refuses.saturating_add(1);
                    return Err(IoError(format!(
                        "sparse free-floor: free {} MiB < reserve+chunk {} MiB — refuse alloc \
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
        match self.provider.alloc(len) {
            Ok(mut m) => {
                m.zero().map_err(|e| IoError(e.to_string()))?;
                self.chunks[idx].mem = Some(m);
                Ok(())
            }
            Err(e) => {
                self.alloc_fails = self.alloc_fails.saturating_add(1);
                Err(IoError(format!("sparse alloc chunk {idx}: {e}")))
            }
        }
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

impl<'p, P: VramProvider + 'p> BlockBackend for SparseVramBackend<'p, P> {
    fn size_bytes(&self) -> u64 {
        self.capacity
    }

    fn block_size(&self) -> u32 {
        self.block_size
    }

    fn read_at(&self, off: u64, buf: &mut [u8]) -> Result<(), IoError> {
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
            match &self.chunks[idx].mem {
                None => buf[done..done + n].fill(0),
                Some(m) => m
                    .read_at(rel as u64, &mut buf[done..done + n])
                    .map_err(|e: VramError| IoError(e.to_string()))?,
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
            {
                let m = self.chunks[idx]
                    .mem
                    .as_mut()
                    .ok_or_else(|| IoError("sparse: mem missing after ensure".into()))?;
                m.write_at(rel as u64, &data[done..done + n])
                    .map_err(|e: VramError| IoError(e.to_string()))?;
            }
            self.chunks[idx].written = true;
            self.chunks[idx].last_write = Some(now);
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

/// True when full Day-1 prealloc is forced.
pub fn prealloc_enabled() -> bool {
    matches!(
        std::env::var("RAMSHARED_VRAM_PREALLOC")
            .map(|s| s.to_ascii_lowercase())
            .as_deref(),
        Ok("1") | Ok("true") | Ok("yes") | Ok("on")
    )
}

/// Idle free hysteresis seconds.
pub fn idle_free_secs_from_env() -> u64 {
    std::env::var("RAMSHARED_VRAM_IDLE_FREE_SEC")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(30)
        .clamp(1, 3600)
}

/// GPU free floor before another chunk alloc (MiB → bytes). Default 512.
pub fn reserve_floor_bytes_from_env() -> u64 {
    let mib = std::env::var("RAMSHARED_MIN_VRAM_FREE_MIB")
        .or_else(|_| std::env::var("MIN_VRAM_HEADROOM_MIB"))
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(512)
        .clamp(128, 4096);
    mib.saturating_mul(1024 * 1024)
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
    fn read_empty_is_zeros_without_alloc() {
        let p = FakeProvider::new();
        let be = SparseVramBackend::new(&p, 1024 * 1024, 256 * 1024, 4096).unwrap();
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
            0,           // no free-floor (fake has lots of free)
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
    fn host_budget_gate_blocks_before_cuda_allocation() {
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
            0,
            None,
            Some(&Deny),
        )
        .unwrap();
        be.write_at(0, &[1u8; 4096]).unwrap();
        assert_eq!(p.allocs.get(), 1);
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
        assert!(be.reserve_floor_bytes() > 0);
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
            512 * 1024, // reserve 512KiB
            None,
        )
        .unwrap();
        let err = be.write_at(0, &[1u8; 4096]).unwrap_err();
        assert!(err.0.contains("free-floor") || err.0.contains("floor"), "{err:?}");
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
        let mut be = SparseVramBackend::new_with_limits(&p, 1024 * 1024, 256 * 1024, 4096, 0, None)
            .unwrap();
        let err = be.write_at(0, &[1u8; 4096]).unwrap_err();
        assert!(err.0.contains("mem_info") || err.0.contains("no gpu"), "{err:?}");
        assert_eq!(be.alloc_fails, 1);
    }

    #[test]
    fn env_helpers_have_sane_defaults() {
        // Do not clobber user env permanently — only assert defaults when unset
        if std::env::var("RAMSHARED_VRAM_CHUNK_MIB").is_err() {
            let b = chunk_bytes_from_env();
            assert!(b >= 16 * 1024 * 1024);
        }
        if std::env::var("RAMSHARED_VRAM_PREALLOC").is_err() {
            assert!(!prealloc_enabled());
        }
        if std::env::var("RAMSHARED_VRAM_IDLE_FREE_SEC").is_err() {
            assert_eq!(idle_free_secs_from_env(), 30);
        }
        if std::env::var("RAMSHARED_MIN_VRAM_FREE_MIB").is_err()
            && std::env::var("MIN_VRAM_HEADROOM_MIB").is_err()
        {
            assert_eq!(reserve_floor_bytes_from_env(), 512 * 1024 * 1024);
        }
        if std::env::var("RAMSHARED_VRAM_COMMIT_CAP_MIB").is_err() {
            assert!(commit_cap_bytes_from_env() > 1 << 30);
        }
    }
}
