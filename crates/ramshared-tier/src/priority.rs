//! Esquema fixo de prioridades da cascata. SPEC §1, §6.2 (passo 4), §11.
//!
//! A ordem `zram > VRAM > VHDX` é o que faz a VRAM ser tier **frio** (não swap
//! quente): o achado da Fase 0 (§9.5) mostrou a VRAM latency-unsafe sob pressão,
//! então o zram (RAM comprimida) absorve o working set quente e a VRAM só pega o
//! spill frio.

use core::fmt;

/// Prioridade do tier zram (HOT, RAM comprimida). Maior = usado primeiro.
pub const ZRAM_PRIO: i32 = 200;

/// Prioridade do tier VRAM (COLD, `nbd-vram`). Sempre `< ZRAM_PRIO` e `> VHDX`.
pub const VRAM_PRIO: i32 = 100;

/// Prioridades efetivas dos três tiers da cascata.
///
/// `vhdx` é a prioridade **observada** do swap VHDX existente do WSL2
/// (tipicamente `-2`); o RamShared não a altera, só a valida.
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

/// Violações da ordem estrita da cascata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrderError {
    /// zram precisa ter prioridade estritamente maior que a VRAM.
    ZramNotAboveVram,
    /// VRAM precisa ter prioridade estritamente maior que o VHDX.
    VramNotAboveVhdx,
}

impl fmt::Display for OrderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OrderError::ZramNotAboveVram => {
                f.write_str("ordem da cascata invalida: zram deve ter prioridade > VRAM")
            }
            OrderError::VramNotAboveVhdx => {
                f.write_str("ordem da cascata invalida: VRAM deve ter prioridade > VHDX")
            }
        }
    }
}

impl core::error::Error for OrderError {}

/// Valida a ordem estrita `zram > VRAM > VHDX` exigida pela arquitetura (§6.2).
///
/// Rejeitar aqui evita o anti-padrão do v2 (VRAM como swap de prioridade máxima),
/// que a Fase 0 provou inseguro.
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
        // v2 fixava a VRAM em 32767 (swap quente). Tem que falhar se o zram não
        // estiver acima.
        let p = TierPriorities {
            zram: ZRAM_PRIO,
            vram: 32767,
            vhdx: -2,
        };
        assert_eq!(validate_order(p), Err(OrderError::ZramNotAboveVram));
    }
}
