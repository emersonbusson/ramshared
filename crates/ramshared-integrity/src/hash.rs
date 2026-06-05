//! Hash por bloco (FNV-1a 64) + tabela de checksum pré-alocada (SPEC §8.1).
//! **Não é cripto** — é detecção de corrupção/leitura torn, não segurança.

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64-bit sobre os bytes do bloco.
pub fn block_hash(data: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// Tabela de checksum por índice de bloco, **pré-alocada** (sem alloc no hot path,
/// SPEC §8). `None` = bloco ainda não escrito.
pub struct ChecksumTable {
    sums: Vec<Option<u64>>,
}

impl ChecksumTable {
    pub fn new(n_blocks: usize) -> Self {
        Self {
            sums: vec![None; n_blocks],
        }
    }

    /// Grava o hash do bloco escrito. `false` se `idx` fora de faixa.
    pub fn record(&mut self, idx: usize, data: &[u8]) -> bool {
        match self.sums.get_mut(idx) {
            Some(slot) => {
                *slot = Some(block_hash(data));
                true
            }
            None => false,
        }
    }

    /// Verifica o bloco lido contra o gravado.
    /// `None` = nunca escrito (ok); `Some(true)` = casa; `Some(false)` =
    /// divergência (corrupção/torn) → o chamador retorna I/O error.
    pub fn verify(&self, idx: usize, data: &[u8]) -> Option<bool> {
        match self.sums.get(idx) {
            Some(Some(expected)) => Some(*expected == block_hash(data)),
            Some(None) => None,
            None => Some(false), // fora de faixa = inválido
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_data_same_hash_diff_data_diff_hash() {
        let a = vec![1u8; 4096];
        let mut b = a.clone();
        assert_eq!(block_hash(&a), block_hash(&b));
        b[2048] ^= 0x01;
        assert_ne!(block_hash(&a), block_hash(&b));
    }

    #[test]
    fn table_records_and_verifies() {
        let mut t = ChecksumTable::new(8);
        let data = vec![0xABu8; 4096];
        assert!(t.record(3, &data));
        assert_eq!(t.verify(3, &data), Some(true));
    }

    #[test]
    fn table_detects_corruption() {
        let mut t = ChecksumTable::new(8);
        let data = vec![0xABu8; 4096];
        t.record(3, &data);
        let mut corrupt = data.clone();
        corrupt[0] ^= 0xff;
        assert_eq!(t.verify(3, &corrupt), Some(false));
    }

    #[test]
    fn unwritten_block_is_none_oob_is_invalid() {
        let mut t = ChecksumTable::new(2);
        assert_eq!(t.verify(0, &[0u8; 4096]), None); // nunca escrito
        assert_eq!(t.verify(99, &[0u8; 4096]), Some(false)); // fora de faixa
        assert!(!t.record(99, &[0u8; 4096]));
    }
}
