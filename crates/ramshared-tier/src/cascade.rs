//! Invariante de rede de segurança do DEMOTE (finding A1). SPEC §6.2 (passo 4), §9.2.
//!
//! O DEMOTE (§9.2) faz `swapoff` só do tier VRAM quando o canário detecta
//! latência de eviction; as páginas VRAM-residentes migram para o tier de baixo.
//! Isso só é **seguro** se existir um destino abaixo da VRAM — senão o `swapoff`
//! não tem para onde escoar e pode levar a OOM. Logo: não armar a VRAM sem rede.

/// Tiers da cascata, do mais quente ao mais frio.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tier {
    /// zram — RAM comprimida, baixa latência (HOT).
    Zram,
    /// VRAM via `nbd-vram` — alto bandwidth, latência instável sob pressão (COLD).
    Vram,
    /// swap VHDX do WSL2 — último recurso.
    Vhdx,
}

/// Resultado da checagem A1: existe rede para o DEMOTE da VRAM escoar?
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SafetyNet {
    /// Há swap VHDX de prioridade menor: o DEMOTE escoa para ele.
    VhdxBelow,
    /// Sem VHDX, mas `MemAvailable >= vram_size`: o DEMOTE escoa para a RAM.
    RamHeadroom,
    /// Sem rede. Armar a VRAM exige `--force-no-safety-net` (§6.2 passo 4).
    None,
}

impl SafetyNet {
    /// `true` quando é seguro armar o tier VRAM sem `--force`.
    pub fn is_safe(self) -> bool {
        !matches!(self, SafetyNet::None)
    }
}

/// Decide a rede de segurança para o tier VRAM (A1).
///
/// Seguro se: existe swap VHDX abaixo (`vhdx_present`), **ou** a RAM disponível
/// comporta uma migração do tamanho da VRAM (`mem_available >= vram_size`).
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
        // swap=0 no .wslconfig, mas 4 GiB livres cobrem 1 GiB de VRAM.
        let net = vram_safety_net(false, 4 * GIB, GIB);
        assert_eq!(net, SafetyNet::RamHeadroom);
        assert!(net.is_safe());
    }

    #[test]
    fn no_vhdx_and_no_ram_is_unsafe() {
        // swap desligado e RAM insuficiente: armar a VRAM levaria a OOM no DEMOTE.
        let net = vram_safety_net(false, 256 * 1024 * 1024, GIB);
        assert_eq!(net, SafetyNet::None);
        assert!(!net.is_safe());
    }

    #[test]
    fn ram_exactly_equal_to_vram_is_safe() {
        assert_eq!(vram_safety_net(false, GIB, GIB), SafetyNet::RamHeadroom);
    }
}
