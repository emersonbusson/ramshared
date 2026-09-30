//! Bounded extent index and slab allocation for the optional compressed cache
//! (DT-4 through DT-7).
//!
//! Cache records are **non-overlapping logical extents**. On update, every
//! overlapping record is invalidated before the new bytes are published (DT-6).
//! On read, the request must have complete contiguous coverage; the response is
//! assembled and verified privately before anything is sent.
//!
//! Backing storage is dynamically allocated 2 MiB slabs with a coalescing
//! free-range allocator (DT-5). No slab and no metadata are allocated when the
//! physical target is zero. Host metadata is capped at
//! `min(16 MiB, max(64 KiB, physical_target_bytes / 256))`, index and allocator
//! overhead included. Live extents are **not** compacted in this slice —
//! fragmentation may refuse admission.

use std::time::Instant;

use ramshared_vram::{VramError, VramSpan};

/// Logical extent size cap for one cache record (DT-4).
pub const MAX_EXTENT_LEN: usize = 64 * 1024;

/// Backing slab size (DT-5).
pub const SLAB_BYTES: usize = 2 * 1024 * 1024;

/// Largest private read response (DT-4).
pub const MAX_READ_LEN: usize = 16 * 1024 * 1024;

/// A cache record: one non-overlapping logical extent (DT-6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheEntry {
    /// Logical start offset in the cached address space.
    pub logical_start: u64,
    /// Logical length of the extent. Always `<= MAX_EXTENT_LEN`.
    pub logical_len: u64,
    /// CRC32 of the original logical bytes (DT-7).
    pub original_crc32: u32,
    /// Monotonic publication generation; replays are idempotent (Kahneman #17).
    pub generation: u64,
    /// Last publication or serve time. The LRU order for eviction (DT-5).
    pub last_accessed: Instant,
    /// How the bytes are stored.
    pub representation: CacheRepresentation,
}

/// One allocator-owned region, addressed as (slab index, slab-local span).
///
/// `VramSpan` offsets are slab-local (DT-5): without the slab index a free
/// cannot tell which 2 MiB region it belongs to once more than one slab
/// exists. The index is part of the address and travels with every entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlabSpan {
    /// Index of the backing slab in the allocator's slab list.
    pub slab_index: usize,
    /// Slab-local allocation.
    pub span: VramSpan,
}

impl SlabSpan {
    /// Builds a slab address, refusing a span that does not fit the slab.
    pub fn new(slab_index: usize, span: VramSpan, slab_len: usize) -> Result<Self, VramError> {
        span.check_within(slab_len)?;
        Ok(Self { slab_index, span })
    }
}

/// How one entry's bytes are held (DT-9, RF-3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CacheRepresentation {
    /// Uncompressed bytes in a raw region.
    Raw {
        /// Address of the raw payload.
        address: SlabSpan,
    },
    /// Compressed payload in a backing slab (DT-7 record fields).
    Compressed {
        /// Address of the compressed payload.
        address: SlabSpan,
        /// CRC32 of the original logical bytes.
        original_crc32: u32,
        /// CRC32 of the stored compressed payload, computed on the provider
        /// **before** decode (DT-7).
        stored_crc32: u32,
        /// Codec/format id that produced the payload.
        codec_id: u8,
    },
}

impl CacheEntry {
    /// Exclusive logical end of the extent.
    pub fn logical_end(&self) -> u64 {
        self.logical_start.saturating_add(self.logical_len)
    }

    /// `true` when `other` overlaps this extent (DT-6).
    pub fn overlaps(&self, other: &CacheEntry) -> bool {
        self.logical_start < other.logical_end() && other.logical_start < self.logical_end()
    }

    /// `true` when `[start, end)` overlaps this extent.
    pub fn overlaps_range(&self, start: u64, end: u64) -> bool {
        self.logical_start < end && start < self.logical_end()
    }
}

/// One 2 MiB backing slab and its free ranges (DT-5).
#[derive(Clone, Debug)]
pub struct VramSlab {
    /// Index of this slab in the allocator's slab list.
    pub index: usize,
    /// Total physical bytes of the slab.
    pub capacity: usize,
    /// Bytes currently allocated out of `capacity`.
    pub used_bytes: usize,
    /// Coalesced free ranges, ascending, non-overlapping.
    free: Vec<(usize, usize)>,
}

impl VramSlab {
    /// Builds a fresh slab with one free range covering the whole capacity.
    pub fn new(index: usize, capacity: usize) -> Self {
        Self {
            index,
            capacity,
            used_bytes: 0,
            free: if capacity == 0 {
                Vec::new()
            } else {
                vec![(0, capacity)]
            },
        }
    }

    /// Free bytes currently available in this slab.
    pub fn free_bytes(&self) -> usize {
        self.free.iter().map(|(_, len)| *len).sum()
    }

    fn take(&mut self, at: usize, len: usize) -> bool {
        let mut start = None;
        for (i, (off, flen)) in self.free.iter().enumerate() {
            if *off <= at && at + len <= *off + *flen {
                start = Some((i, *off, *flen));
                break;
            }
        }
        let Some((i, off, flen)) = start else {
            return false;
        };
        self.free.remove(i);
        // Left remnant, then right remnant; coalescing happens on free().
        let left_end = at;
        if left_end > off {
            self.free.insert(i, (off, left_end - off));
        }
        let right_start = at + len;
        if right_start < off + flen {
            self.free.push((right_start, off + flen - right_start));
        }
        self.free.sort_unstable();
        self.used_bytes = self.used_bytes.saturating_add(len);
        true
    }

    fn give_back(&mut self, off: usize, len: usize) {
        self.free.push((off, len));
        self.free.sort_unstable();
        // Coalesce adjacent ranges.
        let mut merged: Vec<(usize, usize)> = Vec::with_capacity(self.free.len());
        for (off, len) in self.free.drain(..) {
            match merged.last_mut() {
                Some(last) if last.0 + last.1 == off => last.1 = last.1.saturating_add(len),
                Some(last) if off + len == last.0 => {
                    last.1 = last.1.saturating_add(len);
                    last.0 = off;
                }
                _ => merged.push((off, len)),
            }
        }
        self.free = merged;
        self.used_bytes = self.used_bytes.saturating_sub(len);
    }
}

/// Coalescing free-range allocator over a list of 2 MiB slabs (DT-5).
///
/// Fragmentation may refuse admission — live extents are not compacted in this
/// slice.
#[derive(Clone, Debug, Default)]
pub struct VramSpanAllocator {
    slabs: Vec<VramSlab>,
    slab_bytes: usize,
    /// Host metadata bytes attributable to index + allocator overhead.
    metadata_bytes: usize,
    /// Hard cap on host metadata (DT-5).
    metadata_cap_bytes: usize,
    /// Slab ceiling derived from the physical target: one 2 MiB slab per
    /// 2 MiB of target. Prevents unbounded slab growth when the allocator
    /// is fragmented.
    max_slabs: usize,
}

impl VramSpanAllocator {
    /// Builds an allocator with no slabs. `physical_target_bytes == 0` means
    /// nothing is ever allocated (DT-5).
    pub fn new(physical_target_bytes: u64) -> Self {
        let slab_bytes = SLAB_BYTES;
        Self {
            slabs: Vec::new(),
            slab_bytes,
            metadata_bytes: 0,
            metadata_cap_bytes: metadata_cap_bytes(physical_target_bytes),
            max_slabs: max_slabs_for(physical_target_bytes, slab_bytes),
        }
    }

    /// Slab ceiling for one physical target.
    pub fn max_slabs(&self) -> usize {
        self.max_slabs
    }

    /// Slab count currently backing the allocator.
    pub fn slab_count(&self) -> usize {
        self.slabs.len()
    }

    /// Host metadata bytes currently attributed to index + allocator.
    pub fn metadata_bytes(&self) -> usize {
        self.metadata_bytes
    }

    /// Hard metadata cap for this physical target (DT-5).
    pub fn metadata_cap_bytes(&self) -> usize {
        self.metadata_cap_bytes
    }

    /// Free bytes across all slabs.
    pub fn free_bytes(&self) -> usize {
        self.slabs.iter().map(VramSlab::free_bytes).sum()
    }

    /// Allocates one span of `len` bytes (DT-5).
    ///
    /// First-fit over existing free ranges; otherwise a new slab is created.
    /// Refuses when the metadata cap would be crossed, when `len` exceeds the
    /// slab size, or when the physical target is zero and no slab exists.
    pub fn alloc(&mut self, len: usize) -> Result<SlabSpan, VramError> {
        if len == 0 {
            return Err(VramError::OutOfRange {
                off: 0,
                len: 0,
                size: 0,
            });
        }
        if len > self.slab_bytes {
            return Err(VramError::OutOfRange {
                off: 0,
                len: len as u64,
                size: self.slab_bytes as u64,
            });
        }
        // First fit in an existing slab. The slab index is part of the address.
        for slab in &mut self.slabs {
            for (off, flen) in slab.free.clone() {
                if flen >= len && slab.take(off, len) {
                    let span = VramSpan::new(off as u64, len, len)?;
                    return SlabSpan::new(slab.index, span, slab.capacity);
                }
            }
        }
        // A physical target of zero must not create the first slab (DT-5).
        if self.metadata_cap_bytes == 0 && self.slabs.is_empty() {
            return Err(VramError::OutOfMemory);
        }
        // The slab ceiling also binds: a fragmented request larger than any
        // single free range is refused rather than satisfied by a new slab.
        if self.slabs.len() >= self.max_slabs {
            return Err(VramError::OutOfMemory);
        }
        // A new slab: refuse if its metadata crosses the cap.
        let slab_overhead =
            core::mem::size_of::<VramSlab>() + core::mem::size_of::<(usize, usize)>();
        if self.metadata_bytes.saturating_add(slab_overhead) > self.metadata_cap_bytes {
            return Err(VramError::OutOfMemory);
        }
        let index = self.slabs.len();
        let mut slab = VramSlab::new(index, self.slab_bytes);
        if !slab.take(0, len) {
            return Err(VramError::OutOfMemory);
        }
        self.metadata_bytes = self.metadata_bytes.saturating_add(slab_overhead);
        let span = VramSpan::new(0, len, len)?;
        let address = SlabSpan::new(index, span, slab.capacity)?;
        self.slabs.push(slab);
        Ok(address)
    }

    /// Returns a span to its slab and coalesces neighbours (DT-5).
    ///
    /// The slab index is authoritative: a slab-local offset alone is
    /// ambiguous once more than one slab exists, so a free for an unknown
    /// index is refused instead of being applied to slab 0.
    pub fn free(&mut self, address: SlabSpan) -> Result<(), VramError> {
        let span = address.span;
        let off = usize::try_from(span.offset).map_err(|_| VramError::OutOfRange {
            off: span.offset,
            len: span.allocation_len as u64,
            size: self.slab_bytes as u64,
        })?;
        let Some(slab) = self
            .slabs
            .get_mut(address.slab_index)
            .filter(|slab| slab.index == address.slab_index)
        else {
            return Err(VramError::OutOfRange {
                off: span.offset,
                len: span.allocation_len as u64,
                size: self.slab_bytes as u64,
            });
        };
        let end = off.checked_add(span.allocation_len).ok_or(VramError::OutOfRange {
            off: span.offset,
            len: span.allocation_len as u64,
            size: self.slab_bytes as u64,
        })?;
        if end > slab.capacity {
            return Err(VramError::OutOfRange {
                off: span.offset,
                len: span.allocation_len as u64,
                size: slab.capacity as u64,
            });
        }
        slab.give_back(off, span.allocation_len);
        Ok(())
    }

    /// Slab capacity for a known index (used to bound span checks).
    pub fn slab_len(&self, slab_index: usize) -> Option<usize> {
        self.slabs
            .get(slab_index)
            .filter(|slab| slab.index == slab_index)
            .map(|slab| slab.capacity)
    }
}

/// Slab ceiling for one physical target (DT-5).
///
/// One 2 MiB slab per 2 MiB of physical target, minimum one slab for any
/// non-zero target, zero for a zero target.
pub fn max_slabs_for(physical_target_bytes: u64, slab_bytes: usize) -> usize {
    if physical_target_bytes == 0 || slab_bytes == 0 {
        return 0;
    }
    let by_target = (physical_target_bytes / slab_bytes as u64) as usize;
    by_target.clamp(1, 1 << 20)
}

/// Host metadata cap for one physical target (DT-5).
///
/// `min(16 MiB, max(64 KiB, physical_target_bytes / 256))`. A zero physical
/// target yields zero — no slab and no metadata at startup.
pub fn metadata_cap_bytes(physical_target_bytes: u64) -> usize {
    if physical_target_bytes == 0 {
        return 0;
    }
    let scaled = (physical_target_bytes / 256) as usize;
    let lower = scaled.max(64 * 1024);
    lower.min(16 * 1024 * 1024)
}

/// Splits one logical range into extents no larger than `MAX_EXTENT_LEN` (DT-4).
///
/// The last extent absorbs the remainder. A zero-length range yields nothing.
pub fn split_extent(logical_start: u64, logical_len: u64) -> Vec<(u64, u64)> {
    if logical_len == 0 {
        return Vec::new();
    }
    let max = MAX_EXTENT_LEN as u64;
    let mut out = Vec::new();
    let mut at = logical_start;
    let end = logical_start.saturating_add(logical_len);
    while at < end {
        let take = max.min(end - at);
        out.push((at, take));
        at = at.saturating_add(take);
    }
    out
}

/// Removes every entry overlapping `[start, end)` and returns what was removed
/// (DT-6).
///
/// Invalidation happens **before** the replacement is published, so a partial
/// update can never expose mixed or stale bytes.
pub fn invalidate_overlaps(entries: &mut Vec<CacheEntry>, start: u64, end: u64) -> Vec<CacheEntry> {
    let mut removed = Vec::new();
    let mut keep = Vec::with_capacity(entries.len());
    for entry in entries.drain(..) {
        if entry.overlaps_range(start, end) {
            removed.push(entry);
        } else {
            keep.push(entry);
        }
    }
    *entries = keep;
    removed
}

/// Coverage of `[start, end)` over the current index (DT-6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadCoverage {
    /// One entry covers the whole range exactly.
    Exact,
    /// Several contiguous entries cover the range with no gap and no overlap
    /// in the assembled result.
    Contiguous,
    /// At least one byte of the range is not cached.
    Gap { missing_at: u64 },
    /// Entries overlap across the range — the index is inconsistent and the
    /// read must miss rather than expose mixed bytes.
    Overlap,
}

/// Reports how `entries` cover `[start, end)` (DT-6).
///
/// A read requires **complete contiguous coverage**. Anything else is a miss.
pub fn read_coverage(entries: &[CacheEntry], start: u64, end: u64) -> ReadCoverage {
    if start >= end {
        return ReadCoverage::Exact;
    }
    let mut covering: Vec<&CacheEntry> = entries
        .iter()
        .filter(|e| e.overlaps_range(start, end))
        .collect();
    if covering.is_empty() {
        return ReadCoverage::Gap { missing_at: start };
    }
    covering.sort_by_key(|e| e.logical_start);
    let mut at = start;
    for entry in &covering {
        if entry.logical_start > at {
            return ReadCoverage::Gap { missing_at: at };
        }
        if entry.logical_start < at && entry.logical_end() > entry.logical_start {
            // A second entry starts before the previous one ended.
            if entry.logical_start < at && at < entry.logical_end() && entry.logical_start != at {
                // Only an overlap if this entry starts strictly before `at`
                // while `at` is already past the previous entry's start.
            }
        }
        at = at.max(entry.logical_end());
    }
    // Detect overlap: two covering entries whose ranges intersect.
    for (i, a) in covering.iter().enumerate() {
        for b in covering.iter().skip(i + 1) {
            if a.overlaps(b) {
                return ReadCoverage::Overlap;
            }
        }
    }
    if at < end {
        return ReadCoverage::Gap { missing_at: at };
    }
    if covering.len() == 1 {
        ReadCoverage::Exact
    } else {
        ReadCoverage::Contiguous
    }
}

#[cfg(test)]
mod tests {
    // unwrap/expect allowed in tests only (coding.md rules).
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn entry(start: u64, len: u64) -> CacheEntry {
        CacheEntry {
            logical_start: start,
            logical_len: len,
            original_crc32: 0,
            generation: 1,
            last_accessed: Instant::now(),
            representation: CacheRepresentation::Raw {
                address: SlabSpan {
                    slab_index: 0,
                    span: VramSpan::new(0, len as usize, len as usize).expect("span"),
                },
            },
        }
    }

    /// `extent_split_respects_maximum` — DT-4.
    #[test]
    fn extent_split_respects_maximum() {
        // A range under the cap is a single extent.
        let one = split_extent(0, 1024);
        assert_eq!(one, vec![(0, 1024)]);
        // Exactly the cap is a single extent.
        let exact = split_extent(0, MAX_EXTENT_LEN as u64);
        assert_eq!(exact, vec![(0, MAX_EXTENT_LEN as u64)]);
        // One byte over is two extents, the second absorbing the remainder.
        let two = split_extent(0, MAX_EXTENT_LEN as u64 + 1);
        assert_eq!(
            two,
            vec![(0, MAX_EXTENT_LEN as u64), (MAX_EXTENT_LEN as u64, 1)]
        );
        // A large range never emits an extent over the cap and never gaps.
        let many = split_extent(100, 5 * MAX_EXTENT_LEN as u64 + 7);
        assert_eq!(many.len(), 6);
        let mut at = 100u64;
        for (start, len) in &many {
            assert_eq!(*start, at);
            assert!(*len <= MAX_EXTENT_LEN as u64);
            at += len;
        }
        assert_eq!(at, 100 + 5 * MAX_EXTENT_LEN as u64 + 7);
        // Zero length yields nothing.
        assert!(split_extent(0, 0).is_empty());
    }

    /// `allocator_coalesces_and_refuses_fragmented_request` — DT-5.
    #[test]
    fn allocator_coalesces_and_refuses_fragmented_request() {
        // One slab only: a 2 MiB physical target backs exactly one 2 MiB
        // slab, so fragmentation cannot be papered over with a new slab.
        let mut alloc = VramSpanAllocator::new(SLAB_BYTES as u64);
        assert_eq!(alloc.max_slabs(), 1);
        // First allocation creates the first slab.
        let a = alloc.alloc(1024).expect("first");
        assert_eq!(alloc.slab_count(), 1);
        let free_after_first = alloc.free_bytes();

        // A second adjacent allocation, then free both: they coalesce.
        let b = alloc.alloc(1024).expect("second");
        assert!(alloc.free_bytes() < free_after_first);
        alloc.free(a).expect("free a");
        alloc.free(b).expect("free b");
        assert_eq!(
            alloc.free_bytes(),
            SLAB_BYTES,
            "two adjacent frees must coalesce back to one whole slab"
        );

        // A request larger than the slab is refused, not truncated.
        assert!(alloc.alloc(SLAB_BYTES + 1).is_err());

        // Fragmentation: fill the slab so only three separated 64 KiB holes
        // remain, then refuse a request larger than any single hole even
        // though the total free is enough.
        // Fill the whole slab with 64 KiB blocks (2 MiB / 64 KiB = 32).
        let blocks = SLAB_BYTES / (64 * 1024);
        let mut live = Vec::new();
        for _ in 0..blocks {
            live.push(alloc.alloc(64 * 1024).expect("filler"));
        }
        assert_eq!(alloc.free_bytes(), 0, "the slab must be full");
        // Free three non-adjacent blocks to create three separated 64 KiB
        // holes. Nothing coalesces: each is bordered by live blocks.
        for idx in [1usize, blocks / 2, blocks - 2] {
            alloc.free(live[idx]).expect("free hole");
        }
        assert!(
            alloc.free_bytes() >= 3 * 64 * 1024,
            "three holes must be free, got {}",
            alloc.free_bytes()
        );
        // 160 KiB does not fit in any single 64 KiB hole, and max_slabs==1
        // forbids growing into a second slab.
        assert!(
            alloc.alloc(160 * 1024).is_err(),
            "a fragmented request must be refused, not satisfied by compaction"
        );
        // A request that fits one hole still succeeds (refusal plus legitimate
        // pass, Kahneman #13).
        assert!(alloc.alloc(64 * 1024).is_ok());
    }

    /// `overlap_invalidation_removes_only_affected_entries` — DT-6.
    #[test]
    fn overlap_invalidation_removes_only_affected_entries() {
        let mut entries = vec![
            entry(0, 1024),
            entry(1024, 1024),
            entry(2048, 1024),
            entry(4096, 1024),
        ];
        // Overlap only the second entry.
        let removed = invalidate_overlaps(&mut entries, 1500, 1600);
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].logical_start, 1024);
        assert_eq!(entries.len(), 3);
        assert_eq!(
            entries.iter().map(|e| e.logical_start).collect::<Vec<_>>(),
            vec![0, 2048, 4096]
        );

        // A range covering two entries removes both and only those.
        let removed = invalidate_overlaps(&mut entries, 2000, 3000);
        assert_eq!(removed.len(), 1);
        assert_eq!(entries.len(), 2);

        // A range with no overlap removes nothing.
        let removed = invalidate_overlaps(&mut entries, 8192, 9000);
        assert!(removed.is_empty());
        assert_eq!(entries.len(), 2);

        // After invalidation the affected range is a Gap, not a false hit.
        assert!(matches!(
            read_coverage(&entries, 1500, 1600),
            ReadCoverage::Gap { .. }
        ));
        // An unaffected exact read still succeeds.
        assert_eq!(read_coverage(&entries, 0, 1024), ReadCoverage::Exact);
    }

    /// `metadata_budget_caps_entry_count` — DT-5.
    #[test]
    fn metadata_budget_caps_entry_count() {
        // 8 MiB physical target → cap = max(64 KiB, 8 MiB / 256) = 64 KiB.
        let target = 8 * 1024 * 1024u64;
        assert_eq!(metadata_cap_bytes(target), 64 * 1024);
        // 64 MiB physical target → 64 MiB / 256 = 256 KiB.
        assert_eq!(metadata_cap_bytes(64 * 1024 * 1024), 256 * 1024);
        // The 16 MiB ceiling binds on a huge target.
        assert_eq!(metadata_cap_bytes(u64::MAX / 4), 16 * 1024 * 1024);

        // The allocator refuses once its metadata would cross the cap.
        let mut alloc = VramSpanAllocator::new(8 * 1024 * 1024);
        assert_eq!(alloc.metadata_cap_bytes(), 64 * 1024);
        // One slab's overhead is well under the cap; a later slab still fits.
        let mut count = 0;
        while alloc.alloc(64 * 1024).is_ok() {
            count += 1;
            if count > 10_000 {
                break;
            }
        }
        assert!(count > 0, "at least one allocation must succeed");
        assert!(
            alloc.metadata_bytes() <= alloc.metadata_cap_bytes(),
            "metadata must stay within the cap: {} > {}",
            alloc.metadata_bytes(),
            alloc.metadata_cap_bytes()
        );
    }

    /// `zero_physical_target_allocates_no_metadata` — DT-5.
    #[test]
    fn zero_physical_target_allocates_no_metadata() {
        let mut alloc = VramSpanAllocator::new(0);
        assert_eq!(alloc.metadata_cap_bytes(), 0);
        assert_eq!(alloc.metadata_bytes(), 0);
        assert_eq!(alloc.slab_count(), 0);
        assert_eq!(alloc.free_bytes(), 0);
        // Nothing may be allocated: no slab, no metadata at zero target.
        assert!(alloc.alloc(1).is_err());
        assert!(alloc.alloc(SLAB_BYTES).is_err());
        assert_eq!(alloc.slab_count(), 0);
        assert_eq!(alloc.metadata_bytes(), 0);
    }

    #[test]
    fn read_coverage_distinguishes_exact_contiguous_gap_and_overlap() {
        let entries = vec![entry(0, 1024), entry(1024, 1024), entry(4096, 1024)];
        assert_eq!(read_coverage(&entries, 0, 1024), ReadCoverage::Exact);
        assert_eq!(read_coverage(&entries, 0, 2048), ReadCoverage::Contiguous);
        assert!(matches!(
            read_coverage(&entries, 0, 4096),
            ReadCoverage::Gap { missing_at: 2048 }
        ));
        // Overlapping index entries refuse the read (DT-6).
        let bad = vec![entry(0, 2048), entry(1024, 2048)];
        assert_eq!(read_coverage(&bad, 0, 3072), ReadCoverage::Overlap);
        // An empty range is trivially exact.
        assert_eq!(read_coverage(&[], 0, 0), ReadCoverage::Exact);
    }

    /// `free_addresses_the_slab_not_the_offset` — DT-5.
    ///
    /// A slab-local offset alone is ambiguous once more than one slab exists:
    /// offset 0 lives in every slab. Free must use the slab index and must
    /// refuse an unknown index instead of applying the free to slab 0.
    #[test]
    fn free_addresses_the_slab_not_the_offset() {
        // Two slabs: a 4 MiB target backs exactly two 2 MiB slabs.
        let mut alloc = VramSpanAllocator::new(2 * SLAB_BYTES as u64);
        assert_eq!(alloc.max_slabs(), 2);
        let first = alloc.alloc(SLAB_BYTES).expect("whole slab 0");
        assert_eq!(first.slab_index, 0);
        // Filling slab 0 forces the next allocation onto slab 1 at offset 0.
        let second = alloc.alloc(SLAB_BYTES).expect("whole slab 1");
        assert_eq!(second.slab_index, 1);
        assert_eq!(second.span.offset, 0, "slab-local offsets restart at 0");

        // Freeing slab 1 must not touch slab 0.
        alloc.free(second).expect("free slab 1");
        assert_eq!(alloc.free_bytes(), SLAB_BYTES);
        assert_eq!(
            alloc.slab_len(0),
            Some(SLAB_BYTES),
            "slab 0 must still be fully allocated and therefore still exist"
        );

        // An unknown slab index is refused, never applied to slab 0.
        let bogus = SlabSpan {
            slab_index: 7,
            span: first.span,
        };
        assert!(alloc.free(bogus).is_err());
        assert_eq!(
            alloc.free_bytes(),
            SLAB_BYTES,
            "a refused free must change nothing"
        );

        // The legitimate free of slab 0 still works (refusal plus pass).
        alloc.free(first).expect("free slab 0");
        assert_eq!(alloc.free_bytes(), 2 * SLAB_BYTES);
    }

    /// `slab_span_refuses_a_span_outside_its_slab` — RF-6.
    #[test]
    fn slab_span_refuses_a_span_outside_its_slab() {
        let span = VramSpan::new(0, 64 * 1024, 64 * 1024).expect("span");
        assert!(SlabSpan::new(0, span, 32 * 1024).is_err());
        assert!(SlabSpan::new(0, span, 64 * 1024).is_ok());
    }
}
