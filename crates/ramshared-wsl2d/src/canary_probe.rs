//! Dedicated residency canary (§9.4): canary-region separated from swap +
//! cadence content probe. **Pure I/O logic over VRAM**: the decision
//! to DEMOTE (streak/hysteresis) lives in [`crate::residency::ResidencySampler`];
//! DEMOTE timing by latency is tracked per-request in the daemon.
//!
//! SPEC: `docs/008-vram-residency-canary/SPECv3.md` (DT-1, DT-2, DT-4, DT-12).
//! `Cadence` is testable without GPU; the real round-trip of [`CanaryProbe`] requires
//! VRAM (covered by `--ignored` test in the daemon composition).

use ramshared_integrity::{Pattern, fill_block, verify_block};
use ramshared_vram::{VramError, VramMemory};

/// Size of the probe round-trip: 1 page (aligned to `BLOCK_SIZE`). DT-1.
pub const CANARY_BYTES: usize = 4096;
/// Cadence of content/free probe: 1 every `CANARY_EVERY` requests. DT-2.
pub const CANARY_EVERY: u32 = 64;

/// Pure cadence: fires every `every` ticks and restarts from zero.
pub struct Cadence {
    every: u32,
    counter: u32,
}

impl Cadence {
    pub fn new(every: u32) -> Self {
        Self { every, counter: 0 }
    }

    /// Counts a tick; returns `true` (and resets) when it completes `every`.
    pub fn tick(&mut self) -> bool {
        self.counter += 1;
        if self.counter >= self.every {
            self.counter = 0;
            true
        } else {
            false
        }
    }
}

/// Canary-region probe. Has the dedicated VRAM region (`M: VramMemory`)
/// (separated from swap, **not addressable** by NBD). Reuses the reproducible
/// sentinels from `ramshared-integrity` (DT-4).
pub struct CanaryProbe<M> {
    region: M,
    wbuf: Vec<u8>,
    rbuf: Vec<u8>,
    seq: u64,
}

impl<M: VramMemory> CanaryProbe<M> {
    pub fn new(region: M) -> Self {
        Self {
            region,
            wbuf: vec![0u8; CANARY_BYTES],
            rbuf: vec![0u8; CANARY_BYTES],
            seq: 0,
        }
    }

    /// A content cycle: `fill(seq)` → `write_at(0)` → `read_at(0)` →
    /// `verify(seq)`. The per-cycle `seq` also catches stale reads. Returns
    /// `content_ok`; VRAM error is propagated (treated as degraded sample
    /// by the sampler, DT-11). Probe latency is **not** exported (latency-based
    /// detection is done per-request in the daemon).
    pub fn check_content(&mut self) -> Result<bool, VramError> {
        self.seq += 1;
        fill_block(&mut self.wbuf, self.seq, Pattern::Random);
        self.region.write_at(0, &self.wbuf)?;
        self.region.read_at(0, &mut self.rbuf)?;
        Ok(verify_block(&self.rbuf, self.seq, Pattern::Random))
    }

    /// Zeroes the canary-region (teardown §11, DT-12). The region is encapsulated
    /// here, so the daemon delegates zeroing via this method.
    pub fn zero(&mut self) -> Result<(), VramError> {
        self.region.zero()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cadence_fires_every_n() {
        let mut cad = Cadence::new(64);
        for _ in 0..63 {
            assert!(!cad.tick(), "não deve disparar antes do 64º");
        }
        assert!(cad.tick(), "deve disparar no 64º tick");
    }

    #[test]
    fn cadence_resets() {
        let mut cad = Cadence::new(4);
        for _ in 0..3 {
            assert!(!cad.tick());
        }
        assert!(cad.tick()); // 4th → fires and resets
        // restarts from zero: 3 more false before the next fire
        for _ in 0..3 {
            assert!(!cad.tick());
        }
        assert!(cad.tick());
    }
}
