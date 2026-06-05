//! Detecção de eviction WDDM por canário (SPEC §9). **Decisão pura**: alimentada
//! com amostras (latência / integridade / free), decide DEMOTE pelos gatilhos da
//! §9.3. A amostragem CUDA real e o `swapoff` do tier vivem no laço do daemon —
//! aqui fica só a lógica (testável sem GPU/root), como o `ramshared-tier`.

/// Parâmetros dos gatilhos (§9.3). Defaults calibrados pela Fase 0: o spike medido
/// foi ~330× o baseline, então `8×` por `3` amostras tem folga enorme e evita
/// falso-positivo por jitter.
#[derive(Clone, Copy, Debug)]
pub struct ResidencyConfig {
    /// (a) latência > `latency_mult` × baseline.
    pub latency_mult: u64,
    /// ...por `consecutive` amostras consecutivas.
    pub consecutive: u32,
    /// (c) `cuMemGetInfo` free abaixo deste piso → host reavendo VRAM.
    pub free_floor_bytes: u64,
}

impl Default for ResidencyConfig {
    fn default() -> Self {
        Self {
            latency_mult: 8,
            consecutive: 3,
            free_floor_bytes: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DemoteReason {
    Latency,
    Corruption,
    FreeFloor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Verdict {
    Ok,
    Demote(DemoteReason),
}

/// Estado do canário: baseline (mediana logo após `VramAllocated`) + streak de
/// amostras consecutivas acima do limiar de latência.
pub struct Canary {
    cfg: ResidencyConfig,
    baseline_us: u64,
    over_count: u32,
}

impl Canary {
    pub fn new(cfg: ResidencyConfig, baseline_us: u64) -> Self {
        Self {
            cfg,
            baseline_us,
            over_count: 0,
        }
    }

    /// Alimenta uma amostra. `content_ok=false` = canário corrompido (b);
    /// `free_bytes` = `cuMemGetInfo` livre (c); `latency_us` = round-trip do
    /// canário (a). SPEC §9.3.
    pub fn sample(&mut self, latency_us: u64, content_ok: bool, free_bytes: u64) -> Verdict {
        if !content_ok {
            return Verdict::Demote(DemoteReason::Corruption);
        }
        if free_bytes < self.cfg.free_floor_bytes {
            return Verdict::Demote(DemoteReason::FreeFloor);
        }
        let threshold = self.baseline_us.saturating_mul(self.cfg.latency_mult);
        if latency_us > threshold {
            self.over_count += 1;
            if self.over_count >= self.cfg.consecutive {
                return Verdict::Demote(DemoteReason::Latency);
            }
        } else {
            self.over_count = 0; // uma amostra boa zera o streak (anti falso-positivo)
        }
        Verdict::Ok
    }

    pub fn over_count(&self) -> u32 {
        self.over_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canary() -> Canary {
        Canary::new(ResidencyConfig::default(), 4000) // baseline 4 ms → limiar 32 ms
    }

    #[test]
    fn latency_demote_needs_consecutive() {
        let mut c = canary();
        // o spike medido na Fase 0 (1,18 s) está muito acima do limiar
        assert_eq!(c.sample(1_183_094, true, u64::MAX), Verdict::Ok); // 1
        assert_eq!(c.sample(1_183_094, true, u64::MAX), Verdict::Ok); // 2
        assert_eq!(
            c.sample(1_183_094, true, u64::MAX),
            Verdict::Demote(DemoteReason::Latency)
        ); // 3 consecutivas
    }

    #[test]
    fn good_sample_resets_streak() {
        let mut c = canary();
        c.sample(100_000, true, u64::MAX); // over (1)
        c.sample(100_000, true, u64::MAX); // over (2)
        assert_eq!(c.sample(3000, true, u64::MAX), Verdict::Ok); // boa → reseta
        assert_eq!(c.over_count(), 0);
        assert_eq!(c.sample(100_000, true, u64::MAX), Verdict::Ok); // recomeça do 1
    }

    #[test]
    fn corruption_demotes_immediately() {
        let mut c = canary();
        assert_eq!(
            c.sample(1000, false, u64::MAX),
            Verdict::Demote(DemoteReason::Corruption)
        );
    }

    #[test]
    fn free_floor_demotes() {
        let cfg = ResidencyConfig {
            free_floor_bytes: 1 << 30,
            ..Default::default()
        };
        let mut c = Canary::new(cfg, 4000);
        assert_eq!(
            c.sample(1000, true, 256 * 1024 * 1024),
            Verdict::Demote(DemoteReason::FreeFloor)
        );
    }

    #[test]
    fn normal_latency_stays_ok() {
        let mut c = canary();
        for _ in 0..100 {
            assert_eq!(c.sample(3500, true, u64::MAX), Verdict::Ok);
        }
    }
}
