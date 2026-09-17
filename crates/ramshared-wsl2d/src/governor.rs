//! Dynamic Headroom Governor for Cooperative VRAM Tiering.
//!
//! SPEC: docs/specs/no-milestone/elastic-vram-cooperative-tier/SPEC.md §DT-1

use std::time::{Duration, Instant};

/// Physical VRAM headroom classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeadroomZone {
    /// Safe headroom (free >= high_watermark). VRAM chunk allocation allowed.
    Green,
    /// Neutral hold (low_watermark <= free < high_watermark). Allocation paused.
    Yellow,
    /// Host recall pressure (free < low_watermark). Active eviction required.
    Red,
}

/// Dynamic governor enforcing host-safe headroom with settle damping.
#[derive(Debug)]
pub struct DynamicHeadroomGovernor {
    low_watermark_bytes: u64,
    high_watermark_bytes: u64,
    settle_duration: Duration,
    current_zone: HeadroomZone,
    last_green_candidate: Option<Instant>,
}

impl DynamicHeadroomGovernor {
    /// Creates a new governor with specified watermarks and damping settle duration.
    pub fn new(
        low_watermark_bytes: u64,
        high_watermark_bytes: u64,
        settle_duration: Duration,
    ) -> Self {
        Self {
            low_watermark_bytes,
            high_watermark_bytes,
            settle_duration,
            current_zone: HeadroomZone::Green,
            last_green_candidate: None,
        }
    }

    /// Evaluates current free VRAM bytes and returns the current zone and whether a transition occurred.
    pub fn sample(&mut self, free_bytes: u64, now: Instant) -> (HeadroomZone, bool) {
        let previous = self.current_zone;

        if free_bytes < self.low_watermark_bytes {
            // Immediate fail-closed transition to Red. Never delay eviction.
            self.current_zone = HeadroomZone::Red;
            self.last_green_candidate = None;
        } else if free_bytes < self.high_watermark_bytes {
            // Drop to Yellow immediately if currently Green.
            if self.current_zone == HeadroomZone::Green {
                self.current_zone = HeadroomZone::Yellow;
            }
            self.last_green_candidate = None;
        } else {
            // In Green territory (free >= high_watermark).
            match self.current_zone {
                HeadroomZone::Green => {
                    self.last_green_candidate = None;
                }
                HeadroomZone::Yellow | HeadroomZone::Red => {
                    let candidate = match self.last_green_candidate {
                        Some(t) => t,
                        None => {
                            self.last_green_candidate = Some(now);
                            now
                        }
                    };
                    if now.saturating_duration_since(candidate) >= self.settle_duration {
                        self.current_zone = HeadroomZone::Green;
                        self.last_green_candidate = None;
                    }
                }
            }
        }

        let changed = self.current_zone != previous;
        (self.current_zone, changed)
    }

    /// True if physical VRAM headroom permits committing additional chunks.
    pub fn is_allocation_allowed(&self) -> bool {
        self.current_zone == HeadroomZone::Green
    }

    /// True if physical VRAM is below safety floor, requiring eviction to Tier 3 SSD.
    pub fn is_eviction_required(&self) -> bool {
        self.current_zone == HeadroomZone::Red
    }

    /// Returns current active zone.
    pub fn current_zone(&self) -> HeadroomZone {
        self.current_zone
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOW_WATERMARK: u64 = 768 * 1024 * 1024; // 768 MiB
    const HIGH_WATERMARK: u64 = 1200 * 1024 * 1024; // 1200 MiB
    const SETTLE: Duration = Duration::from_secs(3);

    #[test]
    fn test_governor_three_zone_hysteresis() {
        let mut gov = DynamicHeadroomGovernor::new(LOW_WATERMARK, HIGH_WATERMARK, SETTLE);
        let start = Instant::now();

        // 1. Initial sample in green zone
        let (zone, changed) = gov.sample(1500 * 1024 * 1024, start);
        assert_eq!(zone, HeadroomZone::Green);
        assert!(!changed); // Started in Green
        assert!(gov.is_allocation_allowed());
        assert!(!gov.is_eviction_required());

        // 2. Drop into yellow zone (1000 MiB)
        let (zone, changed) = gov.sample(1000 * 1024 * 1024, start + Duration::from_millis(50));
        assert_eq!(zone, HeadroomZone::Yellow);
        assert!(changed);
        assert!(!gov.is_allocation_allowed());
        assert!(!gov.is_eviction_required());

        // 3. Drop into red zone (600 MiB) - host pressure
        let (zone, changed) = gov.sample(600 * 1024 * 1024, start + Duration::from_millis(100));
        assert_eq!(zone, HeadroomZone::Red);
        assert!(changed);
        assert!(!gov.is_allocation_allowed());
        assert!(gov.is_eviction_required());
    }

    #[test]
    fn test_governor_rapid_fluctuation_damping() {
        let mut gov = DynamicHeadroomGovernor::new(LOW_WATERMARK, HIGH_WATERMARK, SETTLE);
        let start = Instant::now();

        // Plunge to Red
        let (zone, _) = gov.sample(500 * 1024 * 1024, start);
        assert_eq!(zone, HeadroomZone::Red);

        // Quick burst to 1300 MiB (above High Watermark), but for only 1 second (< SETTLE)
        let (zone, changed) = gov.sample(1300 * 1024 * 1024, start + Duration::from_secs(1));
        // Must stay in Red/Yellow and NOT immediately revert to Green
        assert_eq!(zone, HeadroomZone::Red);
        assert!(!changed);
        assert!(!gov.is_allocation_allowed());

        // Drops back to 1000 MiB (Yellow) at 2 seconds
        let (zone, _) = gov.sample(1000 * 1024 * 1024, start + Duration::from_secs(2));
        assert_eq!(zone, HeadroomZone::Red); // Still haven't settled

        // Jumps to 1400 MiB and stays for 3.1 seconds
        let t_green_start = start + Duration::from_secs(3);
        let _ = gov.sample(1400 * 1024 * 1024, t_green_start);

        let (zone, changed) = gov.sample(
            1400 * 1024 * 1024,
            t_green_start + Duration::from_millis(3100),
        );
        assert_eq!(zone, HeadroomZone::Green);
        assert!(changed);
        assert!(gov.is_allocation_allowed());
    }
}
