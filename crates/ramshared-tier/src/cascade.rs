//! Invariant safety net for DEMOTE (finding A1). SPEC §6.2 (step 4), §9.2.
//!
//! DEMOTE (§9.2) runs `swapoff` only on the VRAM tier when the canary detects eviction latency.
//! Resident VRAM pages are migrated to the lower-priority active tier.
//! This migration is only **safe** if there is a lower destination below VRAM — otherwise,
//! `swapoff` cannot drain pages and may trigger out-of-memory (OOM) conditions.
//! Thus: VRAM must not be armed without a safety net tier active.

/// Tiers of the swap cascade, ordered from hottest to coldest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tier {
    /// zram — Compressed RAM, low latency (HOT).
    Zram,
    /// VRAM via `nbd-vram` — High bandwidth, volatile latency under pressure (COLD).
    Vram,
    /// WSL2 default swap VHDX — Last resort.
    Vhdx,
}

/// Safety net status for the VRAM demotion path (Invariant A1).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SafetyNet {
    /// Active VHDX swap exists at a lower priority: DEMOTE will spill into it.
    VhdxBelow,
    /// No VHDX active, but `MemAvailable >= vram_size`: DEMOTE can safely spill to RAM.
    RamHeadroom,
    /// No safety net active. Arming VRAM requires `--force-no-safety-net` (§6.2 step 4).
    None,
}

impl SafetyNet {
    /// Returns `true` if it is safe to mount VRAM swap without the `--force` override.
    pub fn is_safe(self) -> bool {
        !matches!(self, SafetyNet::None)
    }
}

/// Determines the safety net availability for the VRAM tier (A1).
///
/// Returns safe if: A lower-priority VHDX swap is active (`vhdx_present` is true),
/// **or** free system RAM headroom is large enough to absorb a total VRAM evacuation
/// (`mem_available >= vram_size`).
pub fn vram_safety_net(vhdx_present: bool, mem_available: u64, vram_size: u64) -> SafetyNet {
    if vhdx_present {
        SafetyNet::VhdxBelow
    } else if mem_available >= vram_size {
        SafetyNet::RamHeadroom
    } else {
        SafetyNet::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn vhdx_present_is_the_safety_net() {
        let net = vram_safety_net(true, 0, GIB);
        assert_eq!(net, SafetyNet::VhdxBelow);
        assert!(net.is_safe());
    }

    #[test]
    fn ram_headroom_covers_when_no_vhdx() {
        // swap=0 in .wslconfig, but 4 GiB of free RAM covers 1 GiB of VRAM capacity.
        let net = vram_safety_net(false, 4 * GIB, GIB);
        assert_eq!(net, SafetyNet::RamHeadroom);
        assert!(net.is_safe());
    }

    #[test]
    fn no_vhdx_and_no_ram_is_unsafe() {
        // swap disabled and insufficient RAM: arming VRAM would trigger OOM during DEMOTE.
        let net = vram_safety_net(false, 256 * 1024 * 1024, GIB);
        assert_eq!(net, SafetyNet::None);
        assert!(!net.is_safe());
    }

    #[test]
    fn ram_exactly_equal_to_vram_is_safe() {
        assert_eq!(vram_safety_net(false, GIB, GIB), SafetyNet::RamHeadroom);
    }
}
