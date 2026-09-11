//! Residency heat map tracking page access frequency for intelligent eviction candidate selection.
//! Implements eviction prediction heuristics based on access frequency.

use std::collections::HashMap;
use std::time::{Instant, Duration};

/// Tracks access frequencies for VRAM pages to inform eviction decisions.
pub struct ResidencyHeatmap {
    /// Maps a page/allocation ID to its access count and last access time.
    accesses: HashMap<u64, PageStats>,
    /// How often counts should decay to avoid historical bias.
    decay_interval: Duration,
    /// Last time the map decayed.
    last_decay: Instant,
}

#[derive(Clone, Copy, Debug)]
/// Statistics for a specific VRAM page.
pub struct PageStats {
    pub frequency: u32,
    pub last_access: Instant,
}

impl ResidencyHeatmap {
    /// Creates a new heatmap with a specified decay interval.
    pub fn new(decay_interval: Duration) -> Self {
        Self {
            accesses: HashMap::new(),
            decay_interval,
            last_decay: Instant::now(),
        }
    }

    /// Records an access to a specific page.
    pub fn record_access(&mut self, page_id: u64) {
        let now = Instant::now();
        self.maybe_decay(now);

        let stats = self.accesses.entry(page_id).or_insert(PageStats {
            frequency: 0,
            last_access: now,
        });

        // Saturating add to prevent overflow attacks on long-lived instances
        stats.frequency = stats.frequency.saturating_add(1);
        stats.last_access = now;
    }

    /// Selects the best candidate for eviction based on lowest access frequency (LFU)
    /// combined with LRU (Least Recently Used) as a tie-breaker.
    pub fn predict_eviction_candidate(&mut self) -> Option<u64> {
        let now = Instant::now();
        self.maybe_decay(now);

        self.accesses
            .iter()
            .min_by(|(_, a), (_, b)| {
                // Primary heuristic: frequency
                a.frequency.cmp(&b.frequency)
                    // Secondary heuristic: oldest access time (LRU)
                    .then(a.last_access.cmp(&b.last_access))
            })
            .map(|(id, _)| *id)
    }

    /// Selects up to `n` best candidates for eviction.
    pub fn predict_eviction_candidates(&mut self, n: usize) -> Vec<u64> {
        if n == 0 {
            return Vec::new();
        }

        let now = Instant::now();
        self.maybe_decay(now);

        let mut candidates: Vec<(&u64, &PageStats)> = self.accesses.iter().collect();
        candidates.sort_unstable_by(|(_, a), (_, b)| {
            a.frequency.cmp(&b.frequency)
                .then(a.last_access.cmp(&b.last_access))
        });

        candidates.into_iter()
            .take(n)
            .map(|(id, _)| *id)
            .collect()
    }

    /// Removes a page from tracking (e.g. after eviction).
    pub fn remove(&mut self, page_id: u64) {
        self.accesses.remove(&page_id);
    }

    /// Clears the entire heatmap (e.g., when GPU is lost).
    pub fn clear(&mut self) {
        self.accesses.clear();
        self.last_decay = Instant::now();
    }

    fn maybe_decay(&mut self, now: Instant) {
        if now.duration_since(self.last_decay) >= self.decay_interval {
            // Halve the frequency of all entries to decay historical access patterns
            for stats in self.accesses.values_mut() {
                stats.frequency /= 2;
            }
            self.last_decay = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn record_and_predict_single() {
        let mut hm = ResidencyHeatmap::new(Duration::from_secs(60));
        hm.record_access(1);
        hm.record_access(2);
        hm.record_access(2);

        // 1 has lower frequency, should be evicted first
        assert_eq!(hm.predict_eviction_candidate(), Some(1));
    }

    #[test]
    fn predict_lru_tiebreaker() {
        let mut hm = ResidencyHeatmap::new(Duration::from_secs(60));

        hm.record_access(1);
        thread::sleep(Duration::from_millis(10));
        hm.record_access(2);

        // Both have frequency 1, but 1 is older, so it should be evicted
        assert_eq!(hm.predict_eviction_candidate(), Some(1));
    }

    #[test]
    fn predict_multiple() {
        let mut hm = ResidencyHeatmap::new(Duration::from_secs(60));

        hm.record_access(1); // freq: 1, old
        thread::sleep(Duration::from_millis(5));

        hm.record_access(2); // freq: 1, newer
        thread::sleep(Duration::from_millis(5));

        hm.record_access(3);
        hm.record_access(3); // freq: 2

        let candidates = hm.predict_eviction_candidates(2);
        assert_eq!(candidates.len(), 2);
        // Should be ordered by least frequent/most ancient
        assert_eq!(candidates[0], 1);
        assert_eq!(candidates[1], 2);
    }

    #[test]
    fn test_remove() {
        let mut hm = ResidencyHeatmap::new(Duration::from_secs(60));
        hm.record_access(1);
        hm.record_access(2);

        hm.remove(1);
        assert_eq!(hm.predict_eviction_candidate(), Some(2));
    }

    #[test]
    fn test_decay() {
        let mut hm = ResidencyHeatmap::new(Duration::from_millis(50));

        hm.record_access(1);
        hm.record_access(1); // freq 2
        hm.record_access(2); // freq 1

        // Let decay interval pass
        thread::sleep(Duration::from_millis(60));

        // Record on 3 triggers decay (1 freq goes 2->1, 2 goes 1->0)
        hm.record_access(3);
        hm.record_access(3); // freq 2

        // 2 has freq 0 after decay, should be first
        assert_eq!(hm.predict_eviction_candidate(), Some(2));
    }
}
