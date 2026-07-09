//! Broker model types (PRD §7 / SPECv2 ITEM-3) — exactly one place.
//!
//! `SliceState` includes `Leased` (DT-19: slice reservation for lease, outside round-robin).
//! `Lease` is internal state of the broker (does not travel over the wire), hence does not derive `serde`.

/// Tenant identifier (consumer host: WSL2, civm, ...).
pub type TenantId = u32;
/// Slice identifier (`s0..s{K-1}`); the number is the suffix of the NBD device (DT-21).
pub type SliceId = u16;

/// State of a slice on the broker machine. Legal transitions in [`crate`] `slices` (ITEM-4).
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum SliceState {
    /// Free for assignment.
    Free,
    /// In use by a tenant (swap mounted).
    Active,
    /// Swapoff in flight (waiting for `SwapOffDone` + zero, DT-17) before returning to `Free`.
    Draining,
    /// Reserved for a pending/active lease (DT-19; does not return to round-robin).
    Leased,
}

/// A slice of VRAM exported as an NBD device. Disjoint offsets on the same `DeviceMem`.
///
/// Derived `PartialEq`/`Eq`: `protocol::Msg` (which derives `PartialEq` for roundtrip tests)
/// embeds `Vec<Slice>` in `StatusReply` — all fields of `Slice` are `Eq` (forced correction
/// by the type system; updated SPECv2).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Slice {
    pub id: SliceId,
    pub offset: u64,
    pub len: u64,
    pub tenant: Option<TenantId>,
    pub state: SliceState,
}

/// Memory pressure sample (`/proc/pressure/memory`, `some` line — DT-15).
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PsiSample {
    pub avg10: f32,
    pub avg60: f32,
    pub stall_us: u64,
}

/// Tenant transport (chooses the NBD endpoint in `SwapOn`, DT-25).
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum TransportKind {
    NbdUnix,
    NbdTcp,
}

/// Revocable VRAM lease (RF-B3) — internal state of the broker, not serialized.
#[derive(Clone, Debug)]
pub struct Lease {
    pub id: u32,
    pub holder: TenantId,
    pub bytes: u64,
    pub slices: Vec<SliceId>,
    pub revocable: bool,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn slice_state_roundtrips() {
        for st in [
            SliceState::Free,
            SliceState::Active,
            SliceState::Draining,
            SliceState::Leased,
        ] {
            let s = serde_json::to_string(&st).unwrap();
            let back: SliceState = serde_json::from_str(&s).unwrap();
            assert_eq!(st, back);
        }
    }

    #[test]
    fn slice_roundtrips_fields() {
        // Slice does not derive PartialEq (SPEC): check field by field.
        let sl = Slice {
            id: 3,
            offset: 192,
            len: 64,
            tenant: Some(7),
            state: SliceState::Active,
        };
        let s = serde_json::to_string(&sl).unwrap();
        let back: Slice = serde_json::from_str(&s).unwrap();
        assert_eq!(back.id, 3);
        assert_eq!(back.offset, 192);
        assert_eq!(back.len, 64);
        assert_eq!(back.tenant, Some(7));
        assert_eq!(back.state, SliceState::Active);
    }

    #[test]
    fn slice_free_has_no_tenant() {
        let sl = Slice {
            id: 0,
            offset: 0,
            len: 64,
            tenant: None,
            state: SliceState::Free,
        };
        let back: Slice = serde_json::from_str(&serde_json::to_string(&sl).unwrap()).unwrap();
        assert_eq!(back.tenant, None);
        assert_eq!(back.state, SliceState::Free);
    }

    #[test]
    fn psi_sample_default_and_roundtrip() {
        assert_eq!(PsiSample::default().avg10, 0.0);
        let p = PsiSample {
            avg10: 14.25,
            avg60: 3.5,
            stall_us: 1000,
        };
        let back: PsiSample = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn transport_kind_roundtrips() {
        for tk in [TransportKind::NbdUnix, TransportKind::NbdTcp] {
            let back: TransportKind =
                serde_json::from_str(&serde_json::to_string(&tk).unwrap()).unwrap();
            assert_eq!(tk, back);
        }
    }
}
