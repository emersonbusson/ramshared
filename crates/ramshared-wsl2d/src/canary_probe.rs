//! Canary residency probe (SPEC §14.3, DT-11, P1-1).
//! Periodically writes a known pattern to a dedicated VRAM region and verifies it.

use ramshared_integrity::{Pattern, fill_block, verify_block};
use ramshared_vram::{VramError, VramMemory};

/// Size of the dedicated canary region: 4 KiB (one page).
pub const CANARY_BYTES: usize = 4096;

/// Default cadence: every 64 requests.
pub const DEFAULT_CADENCE: u64 = 64;
pub const CANARY_EVERY: u64 = DEFAULT_CADENCE;

/// Cadence counter for periodic canary probes.
#[derive(Debug, Default)]
pub struct Cadence {
    count: u64,
    interval: u64,
}

impl Cadence {
    pub fn new(interval: u64) -> Self {
        Self {
            count: 0,
            interval: interval.max(1),
        }
    }

    /// Increments the request count and returns `true` if a probe should be performed.
    pub fn tick(&mut self) -> bool {
        self.count += 1;
        if self.count >= self.interval {
            self.count = 0;
            true
        } else {
            false
        }
    }

    pub fn reset(&mut self) {
        self.count = 0;
    }
}

/// A dedicated canary probe instance that owns a small VRAM region (`CANARY_BYTES`).
pub struct CanaryProbe<M: VramMemory> {
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
        Ok(verify_block(&self.rbuf, self.seq, Pattern::Random).is_ok())
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
            assert!(!cad.tick(), "must not fire before the 64th tick");
        }
        assert!(cad.tick(), "must fire on the 64th tick");
    }

    #[test]
    fn cadence_resets() {
        let mut cad = Cadence::new(4);
        for _ in 0..3 {
            assert!(!cad.tick());
        }
        assert!(cad.tick()); // 4th → fires and resets
        assert!(!cad.tick()); // starts again
    }
}
