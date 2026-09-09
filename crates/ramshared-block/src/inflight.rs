//! Map of inflight blocks (SPEC §8.1): ensures that a request to a range with
//! an inflight operation on the **same** range is serialized behind it — avoiding torn
//! reads or reordered write-after-write. Pure logic; the daemon queries before
//! queueing the CUDA copy.

/// Set of ranges `[offset, offset+len)` currently inflight.
pub const MAX_INFLIGHT: usize = 256;

#[derive(Default)]
pub struct Inflight {
    ranges: Vec<(u64, u64)>,
}

impl Inflight {
    pub fn new() -> Self {
        Self { ranges: Vec::new() }
    }

    /// `true` if `[off, off+len)` overlaps some inflight range.
    pub fn conflicts(&self, off: u64, len: u64) -> bool {
        let end = off.saturating_add(len);
        self.ranges.iter().any(|&(s, e)| off < e && s < end)
    }

    /// Marks the range as inflight. Returns `false` if it already conflicts (caller should
    /// serialize behind the existing operation).
    pub fn try_insert(&mut self, off: u64, len: u64) -> bool {
        if self.ranges.len() >= MAX_INFLIGHT {
            return false;
        }
        if self.conflicts(off, len) {
            return false;
        }
        self.ranges.push((off, off.saturating_add(len)));
        true
    }

    /// Removes the range upon completing the operation.
    pub fn remove(&mut self, off: u64, len: u64) {
        let end = off.saturating_add(len);
        if let Some(i) = self.ranges.iter().position(|&r| r == (off, end)) {
            self.ranges.swap_remove(i);
        }
    }

    /// Drains all inflight ranges, clearing the queue upon shutdown.
    pub fn drain(&mut self) {
        self.ranges.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
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
    fn test_inflight_max_capacity_rejects_new_inserts() {
        let mut f = Inflight::new();
        for i in 0..MAX_INFLIGHT {
            assert!(f.try_insert(i as u64 * 4096, 4096));
        }
        // Queue is full, should reject
        assert!(!f.try_insert(MAX_INFLIGHT as u64 * 4096, 4096));
    }

    #[test]
    fn test_inflight_shutdown_drain_clears_all_ranges() {
        let mut f = Inflight::new();
        f.try_insert(0, 4096);
        f.try_insert(4096, 4096);
        assert!(!f.is_empty());
        f.drain();
        assert!(f.is_empty());
        assert!(f.try_insert(0, 4096)); // verify usable after drain
    }

    #[test]
    fn test_inflight_zero_length_allows_insert() {
        let mut f = Inflight::new();
        assert!(f.try_insert(0, 0)); // zero length inserts [0, 0)
        assert!(f.try_insert(0, 4096)); // overlaps? [0, 0) vs [0, 4096)
        // [0,0) ends at 0. off < 0 && s < 4096 -> 0 < 0 is false. So no conflict.
        assert!(!f.is_empty());
    }

    #[test]
    fn test_inflight_overflow_length_saturates_at_max() {
        let mut f = Inflight::new();
        // MAX bounds
        assert!(f.try_insert(u64::MAX - 100, 200));
        // ends at u64::MAX due to saturating_add
        assert!(f.conflicts(u64::MAX - 50, 10));
    }

    #[test]
    fn test_inflight_misaligned_range_detects_conflict() {
        let mut f = Inflight::new();
        assert!(f.try_insert(1, 3));
        assert!(f.conflicts(2, 1));
        assert!(!f.conflicts(4, 1));
        assert!(!f.conflicts(0, 1));
    }
}
