//! Canary-based detection of WDDM eviction (SPEC §9). **Pure decision**: fed
//! with samples (latency / integrity / free), decides DEMOTE based on triggers from
//! §9.3. Real CUDA sampling and tier `swapoff` live in the daemon loop —
//! here lies only the logic (testable without GPU/root), similar to `ramshared-tier`.

/// Trigger parameters (§9.3). Calibration (DT-31): WDDM eviction spikes ~330× baseline
/// (Phase 0), BUT serve latency under heavy LOAD reaches ~17× (measured on e2e cross-host
/// civm). `8×` gave false positives and dropped the swap under the very load it was supposed to support.
/// `64×` has margins on both sides (>>17× load, <<330× eviction); the **content probe
/// §9.4** is the AUTHORITATIVE eviction detector (latency is just a fast, coarse hint).
#[derive(Clone, Copy, Debug)]
pub struct ResidencyConfig {
    /// (a) latency > `latency_mult` × baseline.
    pub latency_mult: u64,
    /// ...for `consecutive` consecutive samples.
    pub consecutive: u32,
    /// (c) `cuMemGetInfo` free below this floor → host reclaiming VRAM.
    pub free_floor_bytes: u64,
}

impl Default for ResidencyConfig {
    fn default() -> Self {
        Self {
            latency_mult: 64, // DT-31: 8× gave false positives under load (~17×); 64× < eviction (330×)
            consecutive: 3,
            // DT-3: "GPU critically full" floor. Conservative and tunable; with the
            // hysteresis of `ResidencySampler` (DT-9) the risk of false positives drops.
            free_floor_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DemoteReason {
    Latency,
    Corruption,
    FreeFloor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Verdict {
    Ok,
    Demote(DemoteReason),
}

/// Canary state: baseline (median right after `VramAllocated`) + streak of
/// consecutive samples above the latency threshold.
pub struct Canary {
    cfg: ResidencyConfig,
    baseline_us: u64,
    over_count: u32,
}

impl Canary {
    pub fn new(cfg: ResidencyConfig, baseline_us: u64) -> Self {
        Self {
            cfg,
            baseline_us,
            over_count: 0,
        }
    }

    /// Feeds a sample. `content_ok=false` = corrupted canary (b);
    /// `free_bytes` = free `cuMemGetInfo` (c); `latency_us` = round-trip of
    /// the canary (a). SPEC §9.3.
    pub fn sample(&mut self, latency_us: u64, content_ok: bool, free_bytes: u64) -> Verdict {
        if !content_ok {
            return Verdict::Demote(DemoteReason::Corruption);
        }
        if free_bytes < self.cfg.free_floor_bytes {
            return Verdict::Demote(DemoteReason::FreeFloor);
        }
        let threshold = self.baseline_us.saturating_mul(self.cfg.latency_mult);
        if latency_us > threshold {
            self.over_count += 1;
            if self.over_count >= self.cfg.consecutive {
                return Verdict::Demote(DemoteReason::Latency);
            }
        } else {
            self.over_count = 0; // a good sample resets the streak (anti-false-positive)
        }
        Verdict::Ok
    }

    pub fn over_count(&self) -> u32 {
        self.over_count
    }
}

/// Sampler of the dedicated probe (§9.4) with hysteresis. Different from [`Canary`]
/// (per-request latency), this receives content + free and decides:
/// - confirmed corruption (`content = Some(false)`) ⇒ **immediate** DEMOTE (rare,
///   unequivocal; DT-9);
/// - free below floor **OR** degraded sample (probe/`mem_info` error) ⇒
///   increments `bad_streak`; only demotes on `bad_streak >= consecutive` (DT-9/DT-11);
/// - good sample resets the streak.
///
/// SPEC: `docs/specs/no-milestone/wsl2-cascade-swap/SPEC.md` DT-9/DT-10/DT-11.
pub struct ResidencySampler {
    cfg: ResidencyConfig,
    bad_streak: u32,
}

impl ResidencySampler {
    pub fn new(cfg: ResidencyConfig) -> Self {
        Self { cfg, bad_streak: 0 }
    }

    /// Feeds a probe sample in cadence.
    /// - `content`: `Some(true)` = ok, `Some(false)` = corruption (immediate),
    ///   `None` = probe error (degraded, DT-11).
    /// - `free`: `Some(bytes)` or `None` (mem_info error, degraded, DT-11).
    pub fn sample(&mut self, content: Option<bool>, free: Option<u64>) -> Verdict {
        // Corruption is the only immediate trigger: rare and unambiguous.
        if content == Some(false) {
            return Verdict::Demote(DemoteReason::Corruption);
        }
        // Weak/transient signal: low free, probe error, or mem_info error.
        let degraded = content.is_none()
            || free.is_none()
            || free.is_some_and(|f| f < self.cfg.free_floor_bytes);
        if degraded {
            self.bad_streak += 1;
            if self.bad_streak >= self.cfg.consecutive {
                return Verdict::Demote(DemoteReason::FreeFloor);
            }
        } else {
            self.bad_streak = 0; // good sample resets the streak (anti-false-positive)
        }
        Verdict::Ok
    }

    pub fn bad_streak(&self) -> u32 {
        self.bad_streak
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canary() -> Canary {
        Canary::new(ResidencyConfig::default(), 4000) // baseline 4 ms → threshold 256 ms (64×, DT-31)
    }

    #[test]
    fn latency_demote_needs_consecutive() {
        let mut c = canary();
        // the spike measured in Phase 0 (1.18 s) is far above the threshold
        assert_eq!(c.sample(1_183_094, true, u64::MAX), Verdict::Ok); // 1
        assert_eq!(c.sample(1_183_094, true, u64::MAX), Verdict::Ok); // 2
        assert_eq!(
            c.sample(1_183_094, true, u64::MAX),
            Verdict::Demote(DemoteReason::Latency)
        ); // 3 consecutive
    }

    #[test]
    fn good_sample_resets_streak() {
        let mut c = canary();
        c.sample(500_000, true, u64::MAX); // over the 256 ms threshold (1)
        c.sample(500_000, true, u64::MAX); // over (2)
        assert_eq!(c.sample(3000, true, u64::MAX), Verdict::Ok); // good → resets
        assert_eq!(c.over_count(), 0);
        assert_eq!(c.sample(500_000, true, u64::MAX), Verdict::Ok); // restarts from 1
    }

    #[test]
    fn load_spike_below_threshold_stays_ok() {
        // Regression DT-31: LOAD spike ~17× baseline (not eviction) must NOT demote.
        // With 8× this triggered and dropped the swap under load (e2e civm bug); with 64×, it remains Ok.
        let mut c = canary(); // baseline 4 ms → limiar 256 ms
        for _ in 0..10 {
            assert_eq!(c.sample(4000 * 17, true, u64::MAX), Verdict::Ok); // 68 ms = 17× < 256 ms
        }
    }

    #[test]
    fn corruption_demotes_immediately() {
        let mut c = canary();
        assert_eq!(
            c.sample(1000, false, u64::MAX),
            Verdict::Demote(DemoteReason::Corruption)
        );
    }

    #[test]
    fn free_floor_demotes() {
        let cfg = ResidencyConfig {
            free_floor_bytes: 1 << 30,
            ..Default::default()
        };
        let mut c = Canary::new(cfg, 4000);
        assert_eq!(
            c.sample(1000, true, 256 * 1024 * 1024),
            Verdict::Demote(DemoteReason::FreeFloor)
        );
    }

    #[test]
    fn normal_latency_stays_ok() {
        let mut c = canary();
        for _ in 0..100 {
            assert_eq!(c.sample(3500, true, u64::MAX), Verdict::Ok);
        }
    }
}

#[cfg(test)]
mod sampler_tests {
    use super::*;

    fn sampler() -> ResidencySampler {
        // default: consecutive=3, free_floor_bytes=64 MiB (DT-3).
        ResidencySampler::new(ResidencyConfig::default())
    }

    // Kahneman ITEM-5 (#13 validity illusion): corruption returns wrong data
    // despite "data-safe" → guard that demotes immediately, without streak.
    #[test]
    fn corruption_is_immediate() {
        let mut s = sampler();
        assert_eq!(
            s.sample(Some(false), Some(u64::MAX)),
            Verdict::Demote(DemoteReason::Corruption)
        );
        assert_eq!(s.bad_streak(), 0); // corruption does not pass through streak
    }

    // Kahneman ITEM-6 (#5 worst-case): 1 low free reading is noise; only
    // `consecutive` low readings configure GPU-wide pressure (DT-10).
    #[test]
    fn free_floor_needs_consecutive() {
        let mut s = sampler();
        let low = Some(8 * 1024 * 1024); // below the 64 MiB floor
        assert_eq!(s.sample(Some(true), low), Verdict::Ok); // 1
        assert_eq!(s.sample(Some(true), low), Verdict::Ok); // 2
        assert_eq!(
            s.sample(Some(true), low),
            Verdict::Demote(DemoteReason::FreeFloor)
        ); // 3 consecutive
    }

    // Kahneman ITEM-6 (#5 worst-case): an isolated CUDA/`mem_info` error is not
    // loss of residency (DT-11) — counts towards the streak, does not demote alone.
    #[test]
    fn transient_error_needs_consecutive() {
        let mut s = sampler();
        assert_eq!(s.sample(None, Some(u64::MAX)), Verdict::Ok); // 1 (probe error)
        assert_eq!(s.sample(Some(true), None), Verdict::Ok); // 2 (mem_info error)
        assert_eq!(
            s.sample(None, None),
            Verdict::Demote(DemoteReason::FreeFloor)
        ); // 3 degraded
    }

    #[test]
    fn good_sample_resets_streak() {
        let mut s = sampler();
        let low = Some(8 * 1024 * 1024);
        s.sample(Some(true), low); // degraded (1)
        s.sample(Some(true), low); // degraded (2)
        assert_eq!(s.bad_streak(), 2);
        assert_eq!(s.sample(Some(true), Some(u64::MAX)), Verdict::Ok); // good → resets
        assert_eq!(s.bad_streak(), 0);
        // restarts from 1: 2 degraded are not enough to demote
        assert_eq!(s.sample(Some(true), low), Verdict::Ok); // 1
        assert_eq!(s.sample(Some(true), low), Verdict::Ok); // 2
        assert_eq!(s.bad_streak(), 2);
    }
}
