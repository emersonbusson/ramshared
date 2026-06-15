//! Canário de residência dedicado (§9.4): região-canário separada da swap +
//! sonda de conteúdo em cadência. **Lógica de I/O pura sobre a VRAM**: a decisão
//! de DEMOTE (streak/histerese) vive no [`crate::residency::ResidencySampler`];
//! a cronometragem de DEMOTE por latência segue por-request no daemon.
//!
//! SPEC: `docs/008-vram-residency-canary/SPECv3.md` (DT-1, DT-2, DT-4, DT-12).
//! A `Cadence` é testável sem GPU; o round-trip real de [`CanaryProbe`] exige
//! VRAM (coberto por teste `--ignored` na composição do daemon).

use ramshared_integrity::{Pattern, fill_block, verify_block};
use ramshared_vram::{VramError, VramMemory};

/// Tamanho do round-trip da sonda: 1 página (alinhado ao `BLOCK_SIZE`). DT-1.
pub const CANARY_BYTES: usize = 4096;
/// Cadência da sonda de conteúdo/free: 1 a cada `CANARY_EVERY` requests. DT-2.
pub const CANARY_EVERY: u32 = 64;

/// Cadência pura: dispara a cada `every` ticks e recomeça do zero.
pub struct Cadence {
    every: u32,
    counter: u32,
}

impl Cadence {
    pub fn new(every: u32) -> Self {
        Self { every, counter: 0 }
    }

    /// Conta um tick; retorna `true` (e zera) quando completa `every`.
    pub fn tick(&mut self) -> bool {
        self.counter += 1;
        if self.counter >= self.every {
            self.counter = 0;
            true
        } else {
            false
        }
    }
}

/// Sonda da região-canário. Possui a região de VRAM (`M: VramMemory`) dedicada
/// (separada da swap, **não endereçável** por NBD). Reusa as sentinelas
/// reprodutíveis do `ramshared-integrity` (DT-4).
pub struct CanaryProbe<M> {
    region: M,
    wbuf: Vec<u8>,
    rbuf: Vec<u8>,
    seq: u64,
}

impl<M: VramMemory> CanaryProbe<M> {
    pub fn new(region: M) -> Self {
        Self {
            region,
            wbuf: vec![0u8; CANARY_BYTES],
            rbuf: vec![0u8; CANARY_BYTES],
            seq: 0,
        }
    }

    /// Um ciclo de conteúdo: `fill(seq)` → `write_at(0)` → `read_at(0)` →
    /// `verify(seq)`. O `seq` por ciclo também pega leitura stale. Retorna
    /// `content_ok`; erro de VRAM é propagado (tratado como amostra degradada
    /// pelo sampler, DT-11). A latência da sonda **não** é exportada (a detecção
    /// por latência segue por-request no daemon).
    pub fn check_content(&mut self) -> Result<bool, VramError> {
        self.seq += 1;
        fill_block(&mut self.wbuf, self.seq, Pattern::Random);
        self.region.write_at(0, &self.wbuf)?;
        self.region.read_at(0, &mut self.rbuf)?;
        Ok(verify_block(&self.rbuf, self.seq, Pattern::Random))
    }

    /// Zera a região-canário (teardown §11, DT-12). A região fica encapsulada
    /// aqui, então o daemon delega o zero por este método.
    pub fn zero(&mut self) -> Result<(), VramError> {
        self.region.zero()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cadence_fires_every_n() {
        let mut cad = Cadence::new(64);
        for _ in 0..63 {
            assert!(!cad.tick(), "não deve disparar antes do 64º");
        }
        assert!(cad.tick(), "deve disparar no 64º tick");
    }

    #[test]
    fn cadence_resets() {
        let mut cad = Cadence::new(4);
        for _ in 0..3 {
            assert!(!cad.tick());
        }
        assert!(cad.tick()); // 4º → dispara e reseta
        // recomeça do zero: mais 3 falsos antes do próximo disparo
        for _ in 0..3 {
            assert!(!cad.tick());
        }
        assert!(cad.tick());
    }
}
