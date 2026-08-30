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
pub fn validate_purge_age(purge_age_seconds: u64, uptime_seconds: u64) -> Result<(), PurgeAgeError> {
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
        assert_eq!(validate_threshold(5, 10, 50), Err(PriorityError::ThresholdOutOfRange { val: 5, min: 10, max: 50 }));
        assert_eq!(validate_threshold(100, 10, 50), Err(PriorityError::ThresholdOutOfRange { val: 100, min: 10, max: 50 }));
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
        assert_eq!(validate_purge_age(201, 200), Err(PurgeAgeError::AgeExceedsUptime));
    }
}
