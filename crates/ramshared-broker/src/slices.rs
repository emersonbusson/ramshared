//! Static partition of VRAM into K slices + dynamic slice→tenant map (RF-L1, RF-B3).
//!
//! State machine with illegal transitions rejected (mirrors `state.rs` of wsl2d). The
//! sequence of movement with hygiene is `Active → drain → Draining → (SwapOffDone+ZeroDone) →
//! release → Free` (DT-17); lease uses `Free → lease → Leased → unlease → Free` (DT-19).

use crate::model::{Slice, SliceId, SliceState, TenantId};

/// Map of VRAM slices (sole owner of truth about the state; no locks — ITEM-8 is single-threaded).
pub struct SliceMap {
    slices: Vec<Slice>,
}

/// Slice transition/lookup error.
#[derive(Debug, PartialEq, Eq)]
pub enum SliceError {
    UnknownSlice,
    BadState { have: SliceState },
}

impl SliceMap {
    /// K slices of `slice_bytes`, offsets `i * slice_bytes`, all `Free`.
    pub fn new(k: u16, slice_bytes: u64) -> Self {
        let slices = (0..k)
            .map(|i| Slice {
                id: i,
                offset: u64::from(i) * slice_bytes,
                len: slice_bytes,
                tenant: None,
                state: SliceState::Free,
            })
            .collect();
        Self { slices }
    }

    /// Sum of sizes (total exportable capacity).
    pub fn total_bytes(&self) -> u64 {
        self.slices.iter().map(|s| s.len).sum()
    }

    pub fn get(&self, id: SliceId) -> Option<&Slice> {
        self.slices.iter().find(|s| s.id == id)
    }

    pub fn slices(&self) -> &[Slice] {
        &self.slices
    }

    fn get_mut(&mut self, id: SliceId) -> Result<&mut Slice, SliceError> {
        self.slices
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or(SliceError::UnknownSlice)
    }

    /// `Free → Active(tenant)`. Err if non-`Free` (atomicity invariant; `Leased` rejects).
    pub fn assign(&mut self, id: SliceId, tenant: TenantId) -> Result<(), SliceError> {
        let s = self.get_mut(id)?;
        if s.state != SliceState::Free {
            return Err(SliceError::BadState { have: s.state });
        }
        s.state = SliceState::Active;
        s.tenant = Some(tenant);
        Ok(())
    }

    /// `Active → Draining`. Err if non-`Active`.
    pub fn drain(&mut self, id: SliceId) -> Result<(), SliceError> {
        let s = self.get_mut(id)?;
        if s.state != SliceState::Active {
            return Err(SliceError::BadState { have: s.state });
        }
        s.state = SliceState::Draining;
        Ok(())
    }

    /// `Draining → Free` (only after `SwapOffDone{ok}` **and** `ZeroDone{ok}`, DT-17). Cleans the tenant.
    pub fn release(&mut self, id: SliceId) -> Result<(), SliceError> {
        let s = self.get_mut(id)?;
        if s.state != SliceState::Draining {
            return Err(SliceError::BadState { have: s.state });
        }
        s.state = SliceState::Free;
        s.tenant = None;
        Ok(())
    }

    /// `Free → Leased` (reservation for lease, DT-19). Err if non-`Free`.
    pub fn lease(&mut self, id: SliceId) -> Result<(), SliceError> {
        let s = self.get_mut(id)?;
        if s.state != SliceState::Free {
            return Err(SliceError::BadState { have: s.state });
        }
        s.state = SliceState::Leased;
        Ok(())
    }

    /// `Leased → Free` (lease release). Err if non-`Leased`.
    pub fn unlease(&mut self, id: SliceId) -> Result<(), SliceError> {
        let s = self.get_mut(id)?;
        if s.state != SliceState::Leased {
            return Err(SliceError::BadState { have: s.state });
        }
        s.state = SliceState::Free;
        Ok(())
    }

    /// NBD export names per slice: `("s0", len), ("s1", len), ...` (DT-3/DT-21).
    pub fn exports(&self) -> Vec<(String, u64)> {
        self.slices
            .iter()
            .map(|s| (format!("s{}", s.id), s.len))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn new_creates_k_free_disjoint_slices() {
        let m = SliceMap::new(3, 64);
        assert_eq!(m.slices().len(), 3);
        assert_eq!(m.total_bytes(), 192);
        for (i, s) in m.slices().iter().enumerate() {
            assert_eq!(s.id as usize, i);
            assert_eq!(s.offset, i as u64 * 64); // disjoint offsets, no gap
            assert_eq!(s.len, 64);
            assert_eq!(s.state, SliceState::Free);
            assert_eq!(s.tenant, None);
        }
    }

    #[test]
    fn exports_are_named_s0_s1() {
        let m = SliceMap::new(2, 64);
        assert_eq!(
            m.exports(),
            vec![("s0".to_string(), 64), ("s1".to_string(), 64)]
        );
    }

    #[test]
    fn assign_drain_release_cycle() {
        let mut m = SliceMap::new(1, 64);
        m.assign(0, 7).unwrap();
        assert_eq!(m.get(0).unwrap().state, SliceState::Active);
        assert_eq!(m.get(0).unwrap().tenant, Some(7));
        m.drain(0).unwrap();
        assert_eq!(m.get(0).unwrap().state, SliceState::Draining);
        m.release(0).unwrap();
        assert_eq!(m.get(0).unwrap().state, SliceState::Free);
        assert_eq!(m.get(0).unwrap().tenant, None); // tenant cleaned on release
    }

    #[test]
    fn assign_on_active_is_rejected() {
        // Atomicity boundary: an Active slice cannot be re-assigned.
        let mut m = SliceMap::new(1, 64);
        m.assign(0, 1).unwrap();
        assert_eq!(
            m.assign(0, 2),
            Err(SliceError::BadState {
                have: SliceState::Active
            })
        );
    }

    #[test]
    fn assign_on_leased_is_rejected() {
        // DT-19: slice reserved for lease does not return to round-robin via assign.
        let mut m = SliceMap::new(1, 64);
        m.lease(0).unwrap();
        assert_eq!(
            m.assign(0, 1),
            Err(SliceError::BadState {
                have: SliceState::Leased
            })
        );
    }

    #[test]
    fn lease_unlease_cycle() {
        let mut m = SliceMap::new(1, 64);
        m.lease(0).unwrap();
        assert_eq!(m.get(0).unwrap().state, SliceState::Leased);
        m.unlease(0).unwrap();
        assert_eq!(m.get(0).unwrap().state, SliceState::Free);
    }

    #[test]
    fn illegal_jumps_rejected() {
        let mut m = SliceMap::new(1, 64);
        // Free cannot drain, release, or unlease.
        assert!(matches!(m.drain(0), Err(SliceError::BadState { .. })));
        assert!(matches!(m.release(0), Err(SliceError::BadState { .. })));
        assert!(matches!(m.unlease(0), Err(SliceError::BadState { .. })));
        // lease cannot be drained (not Active).
        m.lease(0).unwrap();
        assert!(matches!(m.drain(0), Err(SliceError::BadState { .. })));
    }

    #[test]
    fn unknown_slice_is_error() {
        let mut m = SliceMap::new(1, 64);
        assert_eq!(m.assign(9, 1), Err(SliceError::UnknownSlice));
        assert_eq!(m.drain(9), Err(SliceError::UnknownSlice));
        assert!(m.get(9).is_none());
    }
}
