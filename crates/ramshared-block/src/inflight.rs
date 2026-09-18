//! Map of inflight blocks (SPEC §8.1): ensures that a request to a range with
//! an inflight operation on the **same** range is serialized behind it — avoiding torn
//! reads or reordered write-after-write. Pure logic; the daemon queries before
//! queueing the CUDA copy.

/// Set of ranges `[offset, offset+len)` currently inflight.
use std::time::{Duration, Instant};

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
    pub fn try_insert_at(&mut self, off: u64, len: u64, started_at: Instant) -> bool {
        if self.conflicts(off, len) {
            return false;
        }
        self.ranges.push((off, off.saturating_add(len), started_at));
        true
    }

    pub fn try_insert(&mut self, off: u64, len: u64) -> bool {
        self.try_insert_at(off, len, Instant::now())
    }

    /// Removes the range upon completing the operation.
    pub fn remove(&mut self, off: u64, len: u64) {
        let end = off.saturating_add(len);
        if let Some(i) = self.ranges.iter().position(|&r| r.0 == off && r.1 == end) {
            self.ranges.swap_remove(i);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    pub fn reap_timeouts<F>(&mut self, now: Instant, timeout: Duration, mut callback: F)
    where
        F: FnMut(u64, u64),
    {
        let mut i = 0;
        while i < self.ranges.len() {
            #[allow(clippy::collapsible_if)]
            if let Some(elapsed) = now.checked_duration_since(self.ranges[i].2) {
                if elapsed >= timeout {
                    let (off, end, _) = self.ranges.swap_remove(i);
                    callback(off, end - off);
                    continue;
                }
            }
            i += 1;
        }
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
    fn reap_timeouts_calls_callback_and_removes() {
        let mut f = Inflight::new();
        let now = Instant::now();
        let old = now - Duration::from_secs(10);
        let timeout = Duration::from_secs(5);

        assert!(f.try_insert_at(0, 4096, old));
        assert!(f.try_insert(8192, 4096));

        let mut canceled = Vec::new();
        f.reap_timeouts(now, timeout, |off, len| {
            canceled.push((off, len));
        });

        assert_eq!(canceled, vec![(0, 4096)]);
        assert!(!f.conflicts(0, 4096));
        assert!(f.conflicts(8192, 4096));
    }
}
