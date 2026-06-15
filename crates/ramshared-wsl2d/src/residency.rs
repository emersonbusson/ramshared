//! Detecção de eviction WDDM por canário (SPEC §9). **Decisão pura**: alimentada
//! com amostras (latência / integridade / free), decide DEMOTE pelos gatilhos da
//! §9.3. A amostragem CUDA real e o `swapoff` do tier vivem no laço do daemon —
//! aqui fica só a lógica (testável sem GPU/root), como o `ramshared-tier`.

/// Parâmetros dos gatilhos (§9.3). Calibração (DT-31): a eviction WDDM spike ~330× o baseline
/// (Fase 0), MAS a latência de serve sob CARGA pesada chega a ~17× (medido no e2e cross-host
/// civm). `8×` dava falso-positivo e derrubava o swap sob a própria carga que deveria suportar.
/// `64×` tem folga dos dois lados (>>17× de carga, <<330× de eviction); a **sonda de conteúdo
/// §9.4** é o detector AUTORITATIVO de eviction (a latência é só um hint rápido e grosseiro).
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
            latency_mult: 64, // DT-31: 8× falso-positivava sob carga (~17×); 64× < eviction (330×)
            consecutive: 3,
            // DT-3: piso de "GPU criticamente cheia". Conservador e tunável; com a
            // histerese do `ResidencySampler` (DT-9) o risco de falso-positivo cai.
            free_floor_bytes: 64 * 1024 * 1024,
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

/// Amostrador da sonda dedicada (§9.4) com histerese. Diferente do [`Canary`]
/// (latência por-request), este recebe conteúdo + free e decide:
/// - corrupção confirmada (`content = Some(false)`) ⇒ DEMOTE **imediato** (raro,
///   inequívoco; DT-9);
/// - free abaixo do piso **OU** amostra degradada (erro de sonda/`mem_info`) ⇒
///   incrementa `bad_streak`; só demove em `bad_streak >= consecutive` (DT-9/DT-11);
/// - amostra boa zera o streak.
///
/// SPEC: `docs/008-vram-residency-canary/SPECv3.md` DT-9/DT-10/DT-11.
pub struct ResidencySampler {
    cfg: ResidencyConfig,
    bad_streak: u32,
}

impl ResidencySampler {
    pub fn new(cfg: ResidencyConfig) -> Self {
        Self { cfg, bad_streak: 0 }
    }

    /// Alimenta uma amostra da sonda em cadência.
    /// - `content`: `Some(true)` = ok, `Some(false)` = corrupção (imediato),
    ///   `None` = erro de sonda (degradada, DT-11).
    /// - `free`: `Some(bytes)` ou `None` (erro de `mem_info`, degradada, DT-11).
    pub fn sample(&mut self, content: Option<bool>, free: Option<u64>) -> Verdict {
        // Corrupção é o único gatilho imediato: raro e inequívoco.
        if content == Some(false) {
            return Verdict::Demote(DemoteReason::Corruption);
        }
        // Sinal fraco/transiente: free baixo, erro de sonda ou erro de mem_info.
        let degraded = content.is_none()
            || free.is_none()
            || free.is_some_and(|f| f < self.cfg.free_floor_bytes);
        if degraded {
            self.bad_streak += 1;
            if self.bad_streak >= self.cfg.consecutive {
                return Verdict::Demote(DemoteReason::FreeFloor);
            }
        } else {
            self.bad_streak = 0; // amostra boa zera o streak (anti falso-positivo)
        }
        Verdict::Ok
    }

    pub fn bad_streak(&self) -> u32 {
        self.bad_streak
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canary() -> Canary {
        Canary::new(ResidencyConfig::default(), 4000) // baseline 4 ms → limiar 256 ms (64×, DT-31)
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
        c.sample(500_000, true, u64::MAX); // over o limiar de 256 ms (1)
        c.sample(500_000, true, u64::MAX); // over (2)
        assert_eq!(c.sample(3000, true, u64::MAX), Verdict::Ok); // boa → reseta
        assert_eq!(c.over_count(), 0);
        assert_eq!(c.sample(500_000, true, u64::MAX), Verdict::Ok); // recomeça do 1
    }

    #[test]
    fn load_spike_below_threshold_stays_ok() {
        // Regressão DT-31: spike de CARGA ~17× o baseline (não eviction) NÃO pode demover.
        // Com 8× isto disparava e derrubava o swap sob carga (bug do e2e civm); com 64×, fica Ok.
        let mut c = canary(); // baseline 4 ms → limiar 256 ms
        for _ in 0..10 {
            assert_eq!(c.sample(4000 * 17, true, u64::MAX), Verdict::Ok); // 68 ms = 17× < 256 ms
        }
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

#[cfg(test)]
mod sampler_tests {
    use super::*;

    fn sampler() -> ResidencySampler {
        // default: consecutive=3, free_floor_bytes=64 MiB (DT-3).
        ResidencySampler::new(ResidencyConfig::default())
    }

    // Kahneman ITEM-5 (#13 ilusão de validade): corrupção devolve dado errado
    // apesar de "data-safe" → guarda que demove na hora, sem streak.
    #[test]
    fn corruption_is_immediate() {
        let mut s = sampler();
        assert_eq!(
            s.sample(Some(false), Some(u64::MAX)),
            Verdict::Demote(DemoteReason::Corruption)
        );
        assert_eq!(s.bad_streak(), 0); // corrupção não passa pelo streak
    }

    // Kahneman ITEM-6 (#5 worst-case): 1 leitura de free baixa é ruído; só
    // `consecutive` baixas configuram pressão GPU-wide (DT-10).
    #[test]
    fn free_floor_needs_consecutive() {
        let mut s = sampler();
        let low = Some(8 * 1024 * 1024); // abaixo do piso de 64 MiB
        assert_eq!(s.sample(Some(true), low), Verdict::Ok); // 1
        assert_eq!(s.sample(Some(true), low), Verdict::Ok); // 2
        assert_eq!(
            s.sample(Some(true), low),
            Verdict::Demote(DemoteReason::FreeFloor)
        ); // 3 consecutivas
    }

    // Kahneman ITEM-6 (#5 worst-case): um erro CUDA/`mem_info` isolado não é
    // perda de residência (DT-11) — conta para o streak, não demove sozinho.
    #[test]
    fn transient_error_needs_consecutive() {
        let mut s = sampler();
        assert_eq!(s.sample(None, Some(u64::MAX)), Verdict::Ok); // 1 (erro de sonda)
        assert_eq!(s.sample(Some(true), None), Verdict::Ok); // 2 (erro de mem_info)
        assert_eq!(
            s.sample(None, None),
            Verdict::Demote(DemoteReason::FreeFloor)
        ); // 3 degradadas
    }

    #[test]
    fn good_sample_resets_streak() {
        let mut s = sampler();
        let low = Some(8 * 1024 * 1024);
        s.sample(Some(true), low); // degradada (1)
        s.sample(Some(true), low); // degradada (2)
        assert_eq!(s.bad_streak(), 2);
        assert_eq!(s.sample(Some(true), Some(u64::MAX)), Verdict::Ok); // boa → reseta
        assert_eq!(s.bad_streak(), 0);
        // recomeça do 1: 2 degradadas não bastam para demover
        assert_eq!(s.sample(Some(true), low), Verdict::Ok); // 1
        assert_eq!(s.sample(Some(true), low), Verdict::Ok); // 2
        assert_eq!(s.bad_streak(), 2);
    }
}
