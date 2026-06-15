//! Tipos do modelo do broker (PRD §7 / SPECv2 ITEM-3) — exatamente um lugar.
//!
//! `SliceState` inclui `Leased` (DT-19: reserva de slice para lease, fora do round-robin).
//! `Lease` é estado interno do broker (não trafega no fio), por isso não deriva `serde`.

/// Identificador de tenant (host consumidor: WSL2, civm, ...).
pub type TenantId = u32;
/// Identificador de slice (`s0..s{K-1}`); o número é o sufixo do device NBD (DT-21).
pub type SliceId = u16;

/// Estado de uma slice na máquina do broker. Transições legais em [`crate`] `slices` (ITEM-4).
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum SliceState {
    /// Livre para atribuição.
    Free,
    /// Em uso por um tenant (swap montado).
    Active,
    /// Swapoff em voo (aguardando `SwapOffDone` + zero, DT-17) antes de voltar a `Free`.
    Draining,
    /// Reservada a um lease pendente/ativo (DT-19; não volta ao round-robin).
    Leased,
}

/// Uma fatia da VRAM exportada como device NBD. Offsets disjuntos no mesmo `DeviceMem`.
///
/// `PartialEq`/`Eq` derivados: `protocol::Msg` (que deriva `PartialEq` p/ os testes de roundtrip)
/// embute `Vec<Slice>` em `StatusReply` — todos os campos da `Slice` são `Eq` (correção forçada
/// pelo type system; SPECv2 atualizado).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Slice {
    pub id: SliceId,
    pub offset: u64,
    pub len: u64,
    pub tenant: Option<TenantId>,
    pub state: SliceState,
}

/// Amostra de pressão de memória (`/proc/pressure/memory`, linha `some` — DT-15).
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PsiSample {
    pub avg10: f32,
    pub avg60: f32,
    pub stall_us: u64,
}

/// Transporte do tenant (escolhe o endpoint NBD no `SwapOn`, DT-25).
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum TransportKind {
    NbdUnix,
    NbdTcp,
}

/// Lease de VRAM revogável (RF-B3) — estado interno do broker, não serializado.
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
        // Slice não deriva PartialEq (SPEC): confere campo a campo.
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
