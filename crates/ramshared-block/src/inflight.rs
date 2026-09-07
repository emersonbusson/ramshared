//! Map of inflight blocks (SPEC §8.1): ensures that a request to a range with
//! an inflight operation on the **same** range is serialized behind it — avoiding torn
//! reads or reordered write-after-write. Pure logic; the daemon queries before
//! queueing the CUDA copy.

use std::time::{Duration, Instant};

/// Set of ranges `[offset, offset+len)` currently inflight.
#[derive(Default)]
pub struct Inflight {
    ranges: Vec<(u64, u64, Instant)>,
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
        if let Some(i) = self.ranges.iter().position(|&r| r.0 == off && r.1 == end) {
            self.ranges.swap_remove(i);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    /// Reaps inflight requests older than `timeout` to prevent deadlocks on a stalled backend.
    /// Calls the provided `callback` with the offset and length of each reaped request.
    pub fn reap_timeouts<F>(&mut self, timeout: Duration, mut callback: F)
    where
        F: FnMut(u64, u64),
    {
        let now = Instant::now();
        let mut i = 0;
        while i < self.ranges.len() {
            if now.saturating_duration_since(self.ranges[i].2) > timeout {
                let (off, end, _) = self.ranges.swap_remove(i);
                callback(off, end - off);
            } else {
                i += 1;
            }
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
    fn reap_timeouts_removes_stalled_requests_and_invokes_callback() {
        let mut f = Inflight::new();
        assert!(f.try_insert(0, 4096));
        std::thread::sleep(Duration::from_millis(50));
        assert!(f.try_insert(8192, 4096));

        let mut reaped = Vec::new();
        // reap older than 25ms
        f.reap_timeouts(Duration::from_millis(25), |off, len| {
            reaped.push((off, len));
        });

        assert_eq!(reaped, vec![(0, 4096)]);
        assert!(!f.conflicts(0, 4096)); // older one is reaped
        assert!(f.conflicts(8192, 4096)); // recent one remains
    }
}
