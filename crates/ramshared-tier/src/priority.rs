//! Fixed priority layout for the swap cascade. SPEC §1, §6.2 (step 4), §11.
//!
//! The configuration `zram > VRAM > VHDX` forces VRAM to behave as a cold tier (avoiding
//! hot swap regressions). Phase 0 findings (§9.5) showed that VRAM is latency-unsafe under
//! memory pressure. zram (compressed RAM) absorbs the hot working set, while VRAM only
//! absorbs cold overflows.

use core::fmt;

/// Priority of the zram tier (HOT, compressed RAM). Higher = used first by kernel.
pub const ZRAM_PRIO: i32 = 200;

/// Priority of the VRAM tier (COLD, `nbd-vram`). Must always satisfy `< ZRAM_PRIO` and `> VHDX`.
pub const VRAM_PRIO: i32 = 100;

/// Effective priority metrics of the three active swap tiers.
///
/// `vhdx` is the **observed** priority of the default WSL2 swap VHDX
/// (typically `-2`). RamShared only validates it, leaving its configuration unchanged.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TierPriorities {
    pub zram: i32,
    pub vram: i32,
    pub vhdx: i32,
}

impl Default for TierPriorities {
    fn default() -> Self {
        Self {
            zram: ZRAM_PRIO,
            vram: VRAM_PRIO,
            vhdx: -2,
        }
    }
}

/// Violations of the strict cascade priority hierarchy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrderError {
    /// zram priority must be strictly greater than VRAM priority.
    ZramNotAboveVram,
    /// VRAM priority must be strictly greater than VHDX priority.
    VramNotAboveVhdx,
}

impl fmt::Display for OrderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OrderError::ZramNotAboveVram => {
                f.write_str("invalid swap cascade: zram priority must be greater than VRAM")
            }
            OrderError::VramNotAboveVhdx => {
                f.write_str("invalid swap cascade: VRAM priority must be greater than VHDX")
            }
        }
    }
}

impl core::error::Error for OrderError {}

/// Validates the strict priority hierarchy `zram > VRAM > VHDX` required by the architecture (§6.2).
///
/// Rejects configurations violating this order, preventing v2 anti-patterns
/// (VRAM configured as max-priority hot swap) which Phase 0 proved to be latency-unsafe.
pub fn validate_order(p: TierPriorities) -> Result<(), OrderError> {
    if p.zram <= p.vram {
        return Err(OrderError::ZramNotAboveVram);
    }
    if p.vram <= p.vhdx {
        return Err(OrderError::VramNotAboveVhdx);
    }
    Ok(())
}

/// Errors related to purging aged memory regions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PurgeAgeError {
    /// Attempted to purge data older than system uptime, which is physically impossible.
    AgeExceedsUptime,
}

impl fmt::Display for PurgeAgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PurgeAgeError::AgeExceedsUptime => {
                f.write_str("invalid purge age: cannot exceed system uptime")
            }
        }
    }
}

impl core::error::Error for PurgeAgeError {}

/// Validates that a requested purge age is physically possible given the system uptime.
///
/// Enforces physical bounds: one cannot purge data that claims to be older than the system
/// has been alive.
pub fn validate_purge_age(
    purge_age_seconds: u64,
    uptime_seconds: u64,
) -> Result<(), PurgeAgeError> {
    if purge_age_seconds > uptime_seconds {
        return Err(PurgeAgeError::AgeExceedsUptime);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PriorityError {
    InvalidWeight(i32),
    ThresholdOutOfRange { val: u64, min: u64, max: u64 },
}

impl fmt::Display for PriorityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidWeight(w) => write!(f, "invalid weight: {w}"),
            Self::ThresholdOutOfRange { val, min, max } => {
                write!(f, "threshold {val} out of range ({min}..={max})")
            }
        }
    }
}

impl core::error::Error for PriorityError {}

pub fn validate_weight(weight: i32) -> Result<(), PriorityError> {
    if weight < 0 {
        return Err(PriorityError::InvalidWeight(weight));
    }
    Ok(())
}

pub fn validate_threshold(val: u64, min: u64, max: u64) -> Result<(), PriorityError> {
    if val < min || val > max {
        return Err(PriorityError::ThresholdOutOfRange { val, min, max });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_weight_rejects_negative() {
        assert_eq!(validate_weight(-1), Err(PriorityError::InvalidWeight(-1)));
    }

    #[test]
    fn validate_weight_accepts_valid() {
        assert!(validate_weight(0).is_ok());
        assert!(validate_weight(100).is_ok());
    }

    #[test]
    fn validate_threshold_rejects_out_of_bounds() {
        assert_eq!(
            validate_threshold(5, 10, 50),
            Err(PriorityError::ThresholdOutOfRange {
                val: 5,
                min: 10,
                max: 50
            })
        );
        assert_eq!(
            validate_threshold(100, 10, 50),
            Err(PriorityError::ThresholdOutOfRange {
                val: 100,
                min: 10,
                max: 50
            })
        );
    }

    #[test]
    fn validate_threshold_accepts_valid() {
        assert!(validate_threshold(10, 10, 50).is_ok());
        assert!(validate_threshold(50, 10, 50).is_ok());
        assert!(validate_threshold(25, 10, 50).is_ok());
    }

    #[test]
    fn default_priorities_follow_spec_order() {
        let p = TierPriorities::default();
        assert_eq!(p.zram, ZRAM_PRIO);
        assert_eq!(p.vram, VRAM_PRIO);
        assert!(validate_order(p).is_ok());
    }

    #[test]
    fn validate_order_rejects_inverted_zram_vram() {
        let p = TierPriorities {
            zram: 50,
            vram: 100,
            vhdx: -2,
        };
        assert_eq!(validate_order(p), Err(OrderError::ZramNotAboveVram));
    }

    #[test]
    fn validate_order_rejects_equal_zram_vram() {
        let p = TierPriorities {
            zram: 100,
            vram: 100,
            vhdx: -2,
        };
        assert_eq!(validate_order(p), Err(OrderError::ZramNotAboveVram));
    }

    #[test]
    fn validate_order_rejects_vram_below_vhdx() {
        let p = TierPriorities {
            zram: 200,
            vram: -5,
            vhdx: -2,
        };
        assert_eq!(validate_order(p), Err(OrderError::VramNotAboveVhdx));
    }

    #[test]
    fn validate_order_rejects_equal_vram_vhdx() {
        let p = TierPriorities {
            zram: 200,
            vram: -2,
            vhdx: -2,
        };
        assert_eq!(validate_order(p), Err(OrderError::VramNotAboveVhdx));
    }

    #[test]
    fn validate_purge_age_enforces_uptime() {
        assert!(validate_purge_age(100, 200).is_ok());
        assert!(validate_purge_age(200, 200).is_ok());
        assert_eq!(
            validate_purge_age(201, 200),
            Err(PurgeAgeError::AgeExceedsUptime)
        );
    }
}

use std::collections::BinaryHeap;
use std::cmp::Ordering;

/// A request in the priority queue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Request {
    pub id: u64,
    pub base_priority: i32,
    pub tick_inserted: u64,
    effective_key: i64,
}

impl Ord for Request {
    fn cmp(&self, other: &Self) -> Ordering {
        self.effective_key
            .cmp(&other.effective_key)
            .then_with(|| other.tick_inserted.cmp(&self.tick_inserted))
    }
}

impl PartialOrd for Request {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A priority queue that implements an aging mechanism in O(log N) time to prevent starvation.
///
/// It uses a static virtual priority key `P_i - alpha * t_i` to preserve the relative ordering
/// of elements over time, avoiding the need to re-evaluate all elements on each pop.
#[derive(Clone, Debug, Default)]
pub struct AgedPriorityQueue {
    queue: BinaryHeap<Request>,
    aging_factor: u32,
    current_tick: u64,
}

impl AgedPriorityQueue {
    /// Creates a new `AgedPriorityQueue` with the given aging factor.
    pub fn new(aging_factor: u32) -> Self {
        Self {
            queue: BinaryHeap::new(),
            aging_factor,
            current_tick: 0,
        }
    }

    /// Pushes a new request onto the queue with a base priority.
    pub fn push(&mut self, id: u64, base_priority: i32) {
        let key = (base_priority as i64)
            .saturating_sub((self.aging_factor as i64).saturating_mul(self.current_tick as i64));

        self.queue.push(Request {
            id,
            base_priority,
            tick_inserted: self.current_tick,
            effective_key: key,
        });

        // Advance the tick on each push to simulate time passing for new arrivals.
        self.current_tick = self.current_tick.saturating_add(1);
    }

    /// Pops the request with the highest effective priority.
    pub fn pop(&mut self) -> Option<Request> {
        let req = self.queue.pop();
        if req.is_some() {
            // Also advance tick on pop to allow aging when only pops are happening
            self.current_tick = self.current_tick.saturating_add(1);
        }
        req
    }

    /// Returns the number of requests in the queue.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Returns true if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

#[cfg(test)]
mod queue_tests {
    use super::*;

    #[test]
    fn test_basic_priority() {
        let mut pq = AgedPriorityQueue::new(0); // No aging
        pq.push(1, 10);
        pq.push(2, 20);
        pq.push(3, 5);

        assert_eq!(pq.pop().unwrap().id, 2); // Prio 20
        assert_eq!(pq.pop().unwrap().id, 1); // Prio 10
        assert_eq!(pq.pop().unwrap().id, 3); // Prio 5
    }

    #[test]
    fn test_aging_prevents_starvation() {
        let mut pq = AgedPriorityQueue::new(10);

        // Push a low priority item
        pq.push(1, 5); // t=0, key = 5

        // Push a high priority item
        pq.push(2, 20); // t=1, key = 20 - 10 = 10

        // First pop should be the high priority item (id 2)
        assert_eq!(pq.pop().unwrap().id, 2); // t becomes 3

        // Now, id 1 should have aged.
        // Let's push another item with priority 10.
        pq.push(3, 10); // t=3, key = 10 - 30 = -20

        // Next pop should be id 1 because it aged.
        // id 1 key is 5. id 3 key is -20.
        assert_eq!(pq.pop().unwrap().id, 1);

        // Finally, id 3
        assert_eq!(pq.pop().unwrap().id, 3);
    }
}
