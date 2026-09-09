//! Map of inflight blocks (SPEC §8.1): ensures that a request to a range with
//! an inflight operation on the **same** range is serialized behind it — avoiding torn
//! reads or reordered write-after-write. Pure logic; the daemon queries before
//! queueing the CUDA copy.

/// Set of ranges `[offset, offset+len)` currently inflight.
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

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inflight_initialization_is_empty() {
        let f = Inflight::new();
        assert!(f.is_empty());
        assert!(!f.conflicts(0, 4096));
    }

    #[test]
    fn test_inflight_overlapping_ranges_conflicts_true() {
        let mut f = Inflight::new();
        assert!(f.try_insert(4096, 4096));
        assert!(f.conflicts(4096, 4096)); // same range
        assert!(f.conflicts(6000, 4096)); // partial overlap
        assert!(!f.conflicts(8192, 4096)); // adjacent, no overlap
        assert!(!f.conflicts(0, 4096)); // before, no overlap
    }

    #[test]
    fn test_inflight_insert_conflict_returns_false() {
        let mut f = Inflight::new();
        assert!(f.try_insert(0, 4096));
        assert!(!f.try_insert(0, 4096)); // same block inflight → serialize
    }

    #[test]
    fn test_inflight_remove_existing_allows_reinsert() {
        let mut f = Inflight::new();
        assert!(f.try_insert(0, 4096));
        f.remove(0, 4096);
        assert!(f.try_insert(0, 4096)); // released
        assert!(!f.is_empty());
    }

    #[test]
    fn test_inflight_distinct_blocks_concurrent_insertion_succeeds() {
        let mut f = Inflight::new();
        assert!(f.try_insert(0, 4096));
        assert!(f.try_insert(4096, 4096));
        assert!(f.try_insert(8192, 4096));
    }

    #[test]
    fn test_inflight_zero_length_insertion_succeeds() {
        let mut f = Inflight::new();
        assert!(f.try_insert(1000, 0));
        assert!(!f.is_empty());
        f.remove(1000, 0);
        assert!(f.is_empty());
    }

    #[test]
    fn test_inflight_max_values_saturating_add_handles_overflow() {
        let mut f = Inflight::new();
        // Insert a range at the very end of u64
        assert!(f.try_insert(u64::MAX - 100, 200)); // Will saturate to u64::MAX
        assert!(f.conflicts(u64::MAX - 50, 10));
        f.remove(u64::MAX - 100, 200);
        assert!(f.is_empty());
    }

    #[test]
    fn test_inflight_remove_missing_is_noop() {
        let mut f = Inflight::new();
        assert!(f.try_insert(0, 4096));
        f.remove(4096, 4096); // removing non-existent
        assert!(!f.is_empty()); // should still have the original
        f.remove(0, 4096);
        assert!(f.is_empty());
    }
}
