//! Mapa de blocos em voo (SPEC §8.1): garante que uma requisição a uma faixa com
//! operação em voo na **mesma** faixa seja serializada atrás dela — sem leitura
//! torn nem write-after-write reordenada. Lógica pura; o daemon consulta antes de
//! enfileirar a cópia CUDA.

/// Conjunto de faixas `[offset, offset+len)` atualmente em voo.
#[derive(Default)]
pub struct Inflight {
    ranges: Vec<(u64, u64)>,
}

impl Inflight {
    pub fn new() -> Self {
        Self { ranges: Vec::new() }
    }

    /// `true` se `[off, off+len)` sobrepõe alguma faixa em voo.
    pub fn conflicts(&self, off: u64, len: u64) -> bool {
        let end = off.saturating_add(len);
        self.ranges.iter().any(|&(s, e)| off < e && s < end)
    }

    /// Marca a faixa como em voo. Retorna `false` se já conflita (chamador deve
    /// serializar atrás da operação existente).
    pub fn try_insert(&mut self, off: u64, len: u64) -> bool {
        if self.conflicts(off, len) {
            return false;
        }
        self.ranges.push((off, off.saturating_add(len)));
        true
    }

    /// Remove a faixa ao completar a operação.
    pub fn remove(&mut self, off: u64, len: u64) {
        let end = off.saturating_add(len);
        if let Some(i) = self.ranges.iter().position(|&r| r == (off, end)) {
            self.ranges.swap_remove(i);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_ranges_conflict() {
        let mut f = Inflight::new();
        assert!(f.try_insert(4096, 4096));
        assert!(f.conflicts(4096, 4096)); // mesma faixa
        assert!(f.conflicts(6000, 4096)); // sobreposição parcial
        assert!(!f.conflicts(8192, 4096)); // adjacente, sem overlap
    }

    #[test]
    fn try_insert_rejects_conflict_then_allows_after_remove() {
        let mut f = Inflight::new();
        assert!(f.try_insert(0, 4096));
        assert!(!f.try_insert(0, 4096)); // mesmo bloco em voo → serializa
        f.remove(0, 4096);
        assert!(f.try_insert(0, 4096)); // liberou
        assert!(!f.is_empty());
    }

    #[test]
    fn distinct_blocks_are_concurrent() {
        let mut f = Inflight::new();
        assert!(f.try_insert(0, 4096));
        assert!(f.try_insert(4096, 4096));
        assert!(f.try_insert(8192, 4096));
    }
}
