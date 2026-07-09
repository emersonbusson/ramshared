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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_priorities_follow_spec_order() {
        let p = TierPriorities::default();
        assert_eq!(p.zram, ZRAM_PRIO);
        assert_eq!(p.vram, VRAM_PRIO);
        assert!(validate_order(p).is_ok());
    }

    #[test]
    fn rejects_vram_at_or_above_zram() {
        let p = TierPriorities {
            zram: 100,
            vram: 100,
            vhdx: -2,
        };
        assert_eq!(validate_order(p), Err(OrderError::ZramNotAboveVram));
    }

    #[test]
    fn rejects_vram_not_above_vhdx() {
        let p = TierPriorities {
            zram: 200,
            vram: -2,
            vhdx: -2,
        };
        assert_eq!(validate_order(p), Err(OrderError::VramNotAboveVhdx));
    }

    #[test]
    fn rejects_v2_antipattern_max_priority_vram() {
        // v2 pinned VRAM priority to 32767 (hot swap). This must fail if zram priority is lower.
        let p = TierPriorities {
            zram: ZRAM_PRIO,
            vram: 32767,
            vhdx: -2,
        };
        assert_eq!(validate_order(p), Err(OrderError::ZramNotAboveVram));
    }
}
