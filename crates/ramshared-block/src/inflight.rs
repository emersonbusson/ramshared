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
    fn test_inflight_insert_valid_range_increments_counter() {
        let mut f = Inflight::new();
        assert!(f.is_empty());
        assert!(f.try_insert(100, 100));
        assert!(!f.is_empty());
    }

    #[test]
    fn test_inflight_remove_existing_decrements_counter() {
        let mut f = Inflight::new();
        assert!(f.try_insert(100, 100));
        f.remove(100, 100);
        assert!(f.is_empty());
    }

    #[test]
    fn test_inflight_remove_missing_maintains_zero_crossing_guard() {
        let mut f = Inflight::new();
        // Remove from empty
        f.remove(500, 10);
        assert!(f.is_empty());

        // Remove missing from non-empty
        assert!(f.try_insert(100, 100));
        f.remove(200, 100);
        assert!(!f.is_empty());
    }

    #[test]
    fn test_inflight_insert_zero_length_allows_duplicates() {
        let mut f = Inflight::new();
        assert!(f.try_insert(100, 0));
        // A zero length range shouldn't conflict with itself because it represents no bytes
        assert!(f.try_insert(100, 0));
    }

    #[test]
    fn test_inflight_insert_max_length_saturates_without_overflow() {
        let mut f = Inflight::new();
        let max = u64::MAX;

        // saturating_add handles overflow
        assert!(f.try_insert(max - 10, 20)); // range is (MAX-10, MAX)

        // should conflict with something in the overflowed area
        assert!(f.conflicts(max - 5, 10));
        assert!(!f.try_insert(max - 5, 10));
    }

    #[test]
    fn test_inflight_insert_concurrent_access_safety_succeeds() {
        use std::sync::{Arc, Mutex};
        use std::thread;

        let inflight = Arc::new(Mutex::new(Inflight::new()));
        let mut handles = vec![];

        for i in 0..10 {
            let inflight_clone = Arc::clone(&inflight);
            handles.push(thread::spawn(move || {
                let off = i * 1000;
                let len = 500;
                let mut locked = inflight_clone.lock().unwrap();
                assert!(locked.try_insert(off, len));
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert!(!inflight.lock().unwrap().is_empty());

        let mut handles = vec![];
        for i in 0..10 {
            let inflight_clone = Arc::clone(&inflight);
            handles.push(thread::spawn(move || {
                let off = i * 1000;
                let len = 500;
                let mut locked = inflight_clone.lock().unwrap();
                locked.remove(off, len);
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert!(inflight.lock().unwrap().is_empty());
    }
}
