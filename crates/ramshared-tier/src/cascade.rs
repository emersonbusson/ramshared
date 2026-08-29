//! Invariant safety net for DEMOTE (finding A1). SPEC §6.2 (step 4), §9.2.
//!
//! DEMOTE (§9.2) runs `swapoff` only on the VRAM tier when the canary detects eviction latency.
//! Resident VRAM pages are migrated to the lower-priority active tier.
//! This migration is only **safe** if there is a lower destination below VRAM — otherwise,
//! `swapoff` cannot drain pages and may trigger out-of-memory (OOM) conditions.
//! Thus: VRAM must not be armed without a safety net tier active.

use crate::nbd_readiness::ProductTransport;

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

impl Tier {
    /// Returns the product transport owned by this tier, if it has one.
    ///
    /// The VRAM tier is deliberately NBD-only for the WSL2 product path;
    /// ublk capability is never a product dependency.
    pub const fn product_transport(self) -> Option<ProductTransport> {
        match self {
            Self::Vram => Some(ProductTransport::Nbd),
            Self::Zram | Self::Vhdx => None,
        }
    }
}

use std::sync::atomic::{AtomicU8, Ordering};

/// State of a swap tier during fast-path memory transitions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum TierState {
    /// Tier is offline and not participating in the cascade.
    Offline = 0,
    /// Tier is armed and ready but not currently active.
    Armed = 1,
    /// Tier is active and accepting memory pages.
    Active = 2,
    /// Tier is demoting resident pages to a lower tier.
    Demoting = 3,
}

impl TierState {
    fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Offline,
            1 => Self::Armed,
            2 => Self::Active,
            3 => Self::Demoting,
            _ => unreachable!("invalid TierState value"),
        }
    }
}

/// Lock-free state tracker for fast-path tier transitions.
pub struct AtomicTierState {
    state: AtomicU8,
}

impl AtomicTierState {
    /// Creates a new `AtomicTierState` initialized to the given state.
    pub const fn new(initial: TierState) -> Self {
        Self {
            state: AtomicU8::new(initial as u8),
        }
    }

    /// Loads the current state.
    pub fn load(&self, order: Ordering) -> TierState {
        TierState::from_u8(self.state.load(order))
    }

    /// Stores a new state.
    pub fn store(&self, state: TierState, order: Ordering) {
        self.state.store(state as u8, order);
    }

    /// Atomically compares the current state with `current` and, if they match,
    /// replaces it with `new`.
    pub fn compare_exchange(
        &self,
        current: TierState,
        new: TierState,
        success: Ordering,
        failure: Ordering,
    ) -> Result<TierState, TierState> {
        self.state
            .compare_exchange(current as u8, new as u8, success, failure)
            .map(|_| current)
            .map_err(TierState::from_u8)
    }
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

    #[test]
    fn ram_just_short_of_vram_is_unsafe() {
        assert_eq!(vram_safety_net(false, GIB - 1, GIB), SafetyNet::None);
    }

    #[test]
    fn vhdx_present_has_precedence_over_ram_headroom() {
        assert_eq!(vram_safety_net(true, 4 * GIB, GIB), SafetyNet::VhdxBelow);
    }

    #[test]
    fn ublk_service_is_not_a_product_dependency() {
        assert_eq!(Tier::Vram.product_transport(), Some(ProductTransport::Nbd));
        assert_eq!(Tier::Zram.product_transport(), None);
        assert_eq!(Tier::Vhdx.product_transport(), None);
    }

    #[test]
    fn atomic_tier_state_transitions() {
        use std::sync::atomic::Ordering;

        let atomic_state = AtomicTierState::new(TierState::Offline);
        assert_eq!(atomic_state.load(Ordering::SeqCst), TierState::Offline);

        // Transition: Offline -> Armed
        assert_eq!(
            atomic_state.compare_exchange(
                TierState::Offline,
                TierState::Armed,
                Ordering::SeqCst,
                Ordering::SeqCst
            ),
            Ok(TierState::Offline)
        );
        assert_eq!(atomic_state.load(Ordering::SeqCst), TierState::Armed);

        // Failed transition: expect Armed, try Armed -> Active, but pass Offline
        assert_eq!(
            atomic_state.compare_exchange(
                TierState::Offline,
                TierState::Active,
                Ordering::SeqCst,
                Ordering::SeqCst
            ),
            Err(TierState::Armed)
        );
        assert_eq!(atomic_state.load(Ordering::SeqCst), TierState::Armed);

        // Store directly
        atomic_state.store(TierState::Demoting, Ordering::SeqCst);
        assert_eq!(atomic_state.load(Ordering::SeqCst), TierState::Demoting);
    }
}
