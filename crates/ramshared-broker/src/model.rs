//! Broker model types (PRD §7 / SPEC ITEM-3) — exactly one place.
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
/// by the type system; updated SPEC).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Slice {
    pub id: SliceId,
    pub offset: u64,
    pub len: u64,
    pub tenant: Option<TenantId>,
    pub state: SliceState,
}

impl Slice {
    /// Validates physical hardware limits, bounds, and layout overlaps (PRD §7).
    #[allow(clippy::manual_is_multiple_of)]
    pub fn validate_layout(slices: &[Slice], max_capacity: u64) -> Result<(), std::io::Error> {
        for s in slices {
            if s.len == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Slice length cannot be zero",
                ));
            }
            if s.offset % 4096 != 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Slice offset not 4096-byte aligned",
                ));
            }
            if s.len % 4096 != 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Slice length not 4096-byte aligned",
                ));
            }
            let end = s.offset.checked_add(s.len).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Slice offset and length overflow",
                )
            })?;
            if end > max_capacity {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Slice exceeds physical VRAM capacity",
                ));
            }
        }

        let mut min_offset = u64::MAX;
        let mut max_end = 0;
        let mut total_len = 0;
        for i in 0..slices.len() {
            let a = &slices[i];
            min_offset = std::cmp::min(min_offset, a.offset);
            max_end = std::cmp::max(max_end, a.offset + a.len);
            total_len += a.len;

            for b in slices.iter().skip(i + 1) {
                let a_end = a.offset + a.len;
                let b_end = b.offset + b.len;
                if std::cmp::max(a.offset, b.offset) < std::cmp::min(a_end, b_end) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Slices overlap",
                    ));
                }
            }
        }

        if !slices.is_empty() && max_end.saturating_sub(min_offset) != total_len {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Slices are not contiguous",
            ));
        }

        Ok(())
    }
}

/// Memory pressure sample (`/proc/pressure/memory`, `some` line — DT-15).
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PsiSample {
    pub avg10: f32,
    pub avg60: f32,
    pub stall_us: u64,
}

/// Tenant transport (chooses the NBD endpoint in `SwapOn`, DT-25).
///
/// `WinDrive` is lease-only (windows-swap-driver ITEM-3 / DT-7): no NBD endpoint,
/// excluded from swap round-robin in `on_tick`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum TransportKind {
    NbdUnix,
    NbdTcp,
    /// Windows StorPort path: lease budget only, never receives `SwapOn`.
    WinDrive,
    /// Native Windows DCC consumer; lease-only, never a swap tenant.
    DccAgent,
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
    fn test_lease_construction_valid() {
        let l = Lease {
            id: 42,
            holder: 7,
            bytes: 1024,
            slices: vec![1, 2],
            revocable: true,
        };
        assert_eq!(l.id, 42);
        assert_eq!(l.holder, 7);
        assert_eq!(l.bytes, 1024);
        assert_eq!(l.slices, vec![1, 2]);
        assert_eq!(l.revocable, true);

        let dbg = format!("{:?}", l);
        assert!(dbg.contains("Lease"));
        assert!(dbg.contains("id: 42"));
        assert!(dbg.contains("holder: 7"));
        assert!(dbg.contains("bytes: 1024"));
        assert!(dbg.contains("revocable: true"));
    }

    #[test]
    fn test_lease_construction_boundary_zero() {
        let l = Lease {
            id: 0,
            holder: 0,
            bytes: 0,
            slices: vec![],
            revocable: false,
        };
        assert_eq!(l.id, 0);
        assert_eq!(l.holder, 0);
        assert_eq!(l.bytes, 0);
        assert!(l.slices.is_empty());
        assert_eq!(l.revocable, false);
    }

    #[test]
    fn test_lease_construction_boundary_max() {
        let l = Lease {
            id: u32::MAX,
            holder: u32::MAX,
            bytes: u64::MAX,
            slices: vec![u16::MAX],
            revocable: true,
        };
        assert_eq!(l.id, u32::MAX);
        assert_eq!(l.holder, u32::MAX);
        assert_eq!(l.bytes, u64::MAX);
        assert_eq!(l.slices, vec![u16::MAX]);
        assert_eq!(l.revocable, true);
    }





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
        for tk in [
            TransportKind::NbdUnix,
            TransportKind::NbdTcp,
            TransportKind::WinDrive,
            TransportKind::DccAgent,
        ] {
            let back: TransportKind =
                serde_json::from_str(&serde_json::to_string(&tk).unwrap()).unwrap();
            assert_eq!(tk, back);
        }
    }

    #[test]
    fn validate_slice_layout_rejects_overlaps_and_bounds() {
        let max_cap = 8192;
        let base_slice = Slice {
            id: 0,
            offset: 0,
            len: 4096,
            tenant: None,
            state: SliceState::Free,
        };

        let ok_slices = [
            base_slice.clone(),
            Slice {
                id: 1,
                offset: 4096,
                len: 4096,
                tenant: None,
                state: SliceState::Free,
            },
        ];
        assert!(Slice::validate_layout(&ok_slices, max_cap).is_ok());

        let mut zero_len = ok_slices.clone();
        zero_len[0].len = 0;
        let Err(e) = Slice::validate_layout(&zero_len, max_cap) else {
            panic!()
        };
        assert_eq!(e.to_string(), "Slice length cannot be zero");

        let mut bad_align_offset = ok_slices.clone();
        bad_align_offset[0].offset = 1;
        let Err(e) = Slice::validate_layout(&bad_align_offset, max_cap) else {
            panic!()
        };
        assert_eq!(e.to_string(), "Slice offset not 4096-byte aligned");

        let mut bad_align_len = ok_slices.clone();
        bad_align_len[0].len = 4095;
        let Err(e) = Slice::validate_layout(&bad_align_len, max_cap) else {
            panic!()
        };
        assert_eq!(e.to_string(), "Slice length not 4096-byte aligned");

        let mut out_of_bounds = ok_slices.clone();
        out_of_bounds[1].len = 8192;
        let Err(e) = Slice::validate_layout(&out_of_bounds, max_cap) else {
            panic!()
        };
        assert_eq!(e.to_string(), "Slice exceeds physical VRAM capacity");

        let mut overflow = ok_slices.clone();
        overflow[1].offset = u64::MAX - 4095;
        let Err(e) = Slice::validate_layout(&overflow, max_cap) else {
            panic!()
        };
        assert_eq!(e.to_string(), "Slice offset and length overflow");

        let mut overlap = ok_slices.clone();
        overlap[1].offset = 4096;
        overlap[0].len = 8192;
        let Err(e) = Slice::validate_layout(&overlap, max_cap) else {
            panic!()
        };
        assert_eq!(e.to_string(), "Slices overlap");

        let mut non_contiguous = ok_slices.clone();
        non_contiguous[1].offset = 8192;
        let Err(e) = Slice::validate_layout(&non_contiguous, 16384) else {
            panic!()
        };
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(e.to_string(), "Slices are not contiguous");
    }
}
