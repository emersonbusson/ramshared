//! Map of inflight blocks (SPEC §8.1): ensures that a request to a range with
//! an inflight operation on the **same** range is serialized behind it — avoiding torn
//! reads or reordered write-after-write. Pure logic; the daemon queries before
//! queueing the CUDA copy.

use std::time::{Duration, Instant};

/// Set of ranges `[offset, offset+len)` currently inflight.
pub struct Inflight {
    ranges: Vec<(u64, u64, Instant)>,
}

impl Default for Inflight {
    fn default() -> Self {
        Self::new()
    }
}

impl Inflight {
    pub fn new() -> Self {
        Self { ranges: Vec::new() }
    }

    /// `true` if `[off, off+len)` overlaps some inflight range.
    pub fn conflicts(&self, off: u64, len: u64) -> bool {
        let end = off.saturating_add(len);
        self.ranges.iter().any(|&(s, e, _)| off < e && s < end)
    }

    /// Marks the range as inflight. Returns `false` if it already conflicts (caller should
    /// serialize behind the existing operation).
    pub fn try_insert(&mut self, off: u64, len: u64) -> bool {
        if self.conflicts(off, len) {
            return false;
        }
        self.ranges.push((off, off.saturating_add(len), Instant::now()));
        true
    }

    /// Removes the range upon completing the operation.
    pub fn remove(&mut self, off: u64, len: u64) {
        let end = off.saturating_add(len);
        if let Some(i) = self.ranges.iter().position(|&(s, e, _)| s == off && e == end) {
            self.ranges.swap_remove(i);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    /// Reaps inflight requests that have been outstanding longer than `timeout`.
    /// Returns the number of requests reaped.
    pub fn reap_expired(&mut self, timeout: Duration) -> usize {
        let initial_len = self.ranges.len();
        self.ranges.retain(|&(_, _, inserted_at)| inserted_at.elapsed() < timeout);
        initial_len - self.ranges.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_ranges_conflict() {
        let mut f = Inflight::new();
        assert!(f.try_insert(4096, 4096));
        assert!(f.conflicts(4096, 4096)); // same range
        assert!(f.conflicts(6000, 4096)); // partial overlap
        assert!(!f.conflicts(8192, 4096)); // adjacent, no overlap
    }

    #[test]
    fn try_insert_rejects_conflict_then_allows_after_remove() {
        let mut f = Inflight::new();
        assert!(f.try_insert(0, 4096));
        assert!(!f.try_insert(0, 4096)); // same block inflight → serialize
        f.remove(0, 4096);
        assert!(f.try_insert(0, 4096)); // released
        assert!(!f.is_empty());
    }

    #[test]
    fn distinct_blocks_are_concurrent() {
        let mut f = Inflight::new();
        assert!(f.try_insert(0, 4096));
        assert!(f.try_insert(4096, 4096));
        assert!(f.try_insert(8192, 4096));
    }

    #[test]
    fn reap_expired_removes_old_requests() {
        let mut f = Inflight::new();
        assert!(f.try_insert(0, 4096));

        // Wait a tiny bit so the request has a measurable age
        std::thread::sleep(Duration::from_millis(5));

        // Reaping with a large timeout shouldn't remove it
        assert_eq!(f.reap_expired(Duration::from_secs(10)), 0);
        assert!(!f.is_empty());

        // Reaping with a very small timeout should remove it
        assert_eq!(f.reap_expired(Duration::from_millis(1)), 1);
        assert!(f.is_empty());
    }
}
