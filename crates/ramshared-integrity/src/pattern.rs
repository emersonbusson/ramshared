//! Padrões de teste reprodutíveis por índice de bloco (SPEC §14.2 `test-integrity`).
//! Determinísticos: `verify_block` regenera o esperado sem precisar guardar dados.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pattern {
    Zero,
    Sequential,
    Random,
}

/// Preenche `buf` com o padrão do bloco `idx`.
pub fn fill_block(buf: &mut [u8], idx: u64, kind: Pattern) {
    match kind {
        Pattern::Zero => buf.iter_mut().for_each(|b| *b = 0),
        Pattern::Sequential => {
            for (i, b) in buf.iter_mut().enumerate() {
                *b = (idx.wrapping_add(i as u64) & 0xff) as u8;
            }
        }
        Pattern::Random => {
            // xorshift64 semeado pelo índice do bloco (reprodutível, ≠ por bloco).
            let mut s = idx.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
            for b in buf.iter_mut() {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                *b = (s & 0xff) as u8;
            }
        }
    }
}

/// `true` se `buf` casa com o padrão esperado do bloco `idx`.
pub fn verify_block(buf: &[u8], idx: u64, kind: Pattern) -> bool {
    let mut expected = vec![0u8; buf.len()];
    fill_block(&mut expected, idx, kind);
    expected == buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_then_verify_round_trips() {
        for kind in [Pattern::Zero, Pattern::Sequential, Pattern::Random] {
            let mut buf = vec![0u8; 4096];
            fill_block(&mut buf, 42, kind);
            assert!(verify_block(&buf, 42, kind), "{kind:?}");
        }
    }

    #[test]
    fn corruption_breaks_verify() {
        let mut buf = vec![0u8; 4096];
        fill_block(&mut buf, 7, Pattern::Random);
        buf[1234] ^= 0x01;
        assert!(!verify_block(&buf, 7, Pattern::Random));
    }

    #[test]
    fn different_blocks_differ_and_wrong_index_fails() {
        let mut a = vec![0u8; 4096];
        let mut b = vec![0u8; 4096];
        fill_block(&mut a, 1, Pattern::Random);
        fill_block(&mut b, 2, Pattern::Random);
        assert_ne!(a, b); // padrão muda por bloco
        assert!(!verify_block(&a, 2, Pattern::Random)); // índice errado = falha
    }
}
