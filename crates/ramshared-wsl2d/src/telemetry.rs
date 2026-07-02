//! Telemetria & reconciliação do broker (SPECv2 `docs/broker-telemetry-reconciliation/`).
//!
//! Tipos compartilhados entre data-plane (escreve contadores) e control-plane (lê + reconcilia) +
//! a lógica **pura** de reconciliação. O invariante é de **ocupação** (DT-4): compara a capacidade
//! emprestada (`Σ slice.len` Active|Draining) com o swap realmente ocupado nas nossas slices; o
//! throughput (`bytes_served`/`io_count`) é telemetria separada, fora do invariante. Eviction é
//! detectada pelo **canário** (`demotes_delta`), não por subtração de VRAM (DT-6).

use std::sync::atomic::AtomicU64;

/// Contadores de IO por slice: o worker (data-plane) escreve, o `BrokerCore` lê (DT-1). `Relaxed`
/// é suficiente — cada contador é independente; o par `(bytes, io)` não é lido atomicamente junto
/// (skew de um tick é aceito, telemetria, não contabilidade).
#[derive(Default)]
pub struct SliceIoCounters {
    pub bytes_served: AtomicU64,
    pub io_count: AtomicU64,
}

/// Gauge de VRAM publicado pela closure de residência do worker (DT-5). `total == 0` é a sentinela
/// de "sem dado de VRAM" (ex.: `--backend ram`, sem GPU) → os campos `vram_*` saem `None`.
#[derive(Default)]
pub struct VramGauge {
    pub free: AtomicU64,
    pub total: AtomicU64,
}

/// Veredito da reconciliação (RF-4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconcileFlag {
    /// Convergente (ocupado ≤ emprestado + tolerância).
    None,
    /// Alguma fonte ausente (degrade) — amostra parcial.
    Partial,
    /// Canário disparou DEMOTE desde a última amostra (WDDM espremeu a VRAM).
    Eviction,
    /// Slice presa em `Draining` (zero não confirmou) além do limiar.
    StuckSlice,
    /// Ocupado > emprestado + tolerância (tenant swapando fora das nossas slices / drift).
    Unaccounted,
}

/// Entrada pura da reconciliação — já coletada do core/gauge (DT-4/DT-6).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReconcileInput {
    /// `Σ slice.len` das slices `Active|Draining` (capacidade emprestada).
    pub alloc_active_bytes: u64,
    /// `Σ used` (bytes) das nossas nbd devices (DT-10), já filtrado/convertido pelo `Psi` handler.
    pub occupied_swap_bytes: u64,
    /// Alguma slice em `pending_zero` ≥ `ZERO_RETRY_ERROR` ticks.
    pub stuck_draining: bool,
    /// DEMOTEs do canário desde a última amostra (DT-6).
    pub demotes_delta: u64,
    /// Alguma fonte ausente (sem VRAM gauge, sem `mem`) → `Partial`.
    pub any_source_missing: bool,
}

/// Amostra emitida pelo **core** (sem `t`/`branch`/`commit` — DT-8). `PartialEq` p/ entrar em
/// `Outbound`; `f64` impede `Eq` (e `Outbound` não exige `Eq`).
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct TelemetryCore {
    pub tenant: Option<String>,
    pub slice: Option<u16>,
    pub swap_used: u64,
    pub alloc_active: u64,
    pub page_io_s: Option<u64>,
    pub vram_alloc_daemon: u64,
    pub vram_total_used: Option<u64>,
    pub vram_outros: Option<u64>,
    pub canario_demotes: u64,
    pub demote_reason: Option<String>,
    pub reconcile_delta: f64,
    pub flag: ReconcileFlag,
}

/// Linha final (a camada de IO embrulha o [`TelemetryCore`], adicionando `t`/`branch`/`commit` —
/// DT-8). 1 objeto JSON por linha (`docs/benchmarks/results.jsonl`, RF-5).
#[derive(Clone, Debug, serde::Serialize)]
pub struct TelemetrySample {
    /// Epoch em segundos (carimbado pela camada de IO; o core não lê relógio).
    pub t: u64,
    pub branch: Option<String>,
    pub commit: Option<String>,
    #[serde(flatten)]
    pub core: TelemetryCore,
}

/// VRAM de "outros" (gráficos/Windows) por subtração, com clamp em 0 (DT-4/DT-5). Chamar só quando
/// há dado de VRAM (`total > 0`).
pub fn vram_outros(total_used: u64, alloc_daemon: u64) -> u64 {
    total_used.saturating_sub(alloc_daemon)
}

/// Reconciliação pura (RF-4). Devolve `(delta, flag)` onde `delta = (ocupado − emprestado)/emprestado`
/// (positivo = ocupou mais do que foi emprestado). O `streak` (histerese, DT-12) é aplicado **fora**,
/// no `on_tick`. F-v2-1: `delta` é computado **antes** de qualquer retorno.
pub fn reconcile(inp: &ReconcileInput, tol_frac: f64) -> (f64, ReconcileFlag) {
    // alloc=0 (nada emprestado): a fração é indefinida → 0.0 se nada ocupado, senão 1.0 (drift
    // total). Evita reportar `occupied` cru (número gigante) no `reconcile_delta` do JSONL (M1).
    let delta = if inp.alloc_active_bytes == 0 {
        if inp.occupied_swap_bytes == 0 {
            0.0
        } else {
            1.0
        }
    } else {
        (inp.occupied_swap_bytes as f64 - inp.alloc_active_bytes as f64)
            / inp.alloc_active_bytes as f64
    };
    if inp.any_source_missing {
        return (delta, ReconcileFlag::Partial);
    }
    if inp.demotes_delta > 0 {
        // Canário é a autoridade de eviction (DT-6); subtração de VRAM não detecta evicção WDDM.
        return (delta, ReconcileFlag::Eviction);
    }
    if inp.stuck_draining {
        return (delta, ReconcileFlag::StuckSlice);
    }
    if delta > tol_frac {
        return (delta, ReconcileFlag::Unaccounted);
    }
    (delta, ReconcileFlag::None)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn base() -> ReconcileInput {
        ReconcileInput {
            alloc_active_bytes: 1 << 30,    // 1 GiB emprestado
            occupied_swap_bytes: 512 << 20, // 512 MiB ocupado (metade)
            stuck_draining: false,
            demotes_delta: 0,
            any_source_missing: false,
        }
    }

    #[test]
    fn reconcile_idle_none() {
        let (delta, flag) = reconcile(&base(), 0.10);
        assert_eq!(flag, ReconcileFlag::None);
        assert!(delta < 0.0, "ocupado < emprestado => delta negativo");
    }

    #[test]
    fn reconcile_unaccounted_when_occupied_gt_alloc() {
        let mut inp = base();
        inp.occupied_swap_bytes = inp.alloc_active_bytes + (200 << 20); // ocupa mais que emprestou
        let (delta, flag) = reconcile(&inp, 0.10);
        assert_eq!(flag, ReconcileFlag::Unaccounted);
        assert!(delta > 0.10);
    }

    #[test]
    fn reconcile_eviction_when_demotes() {
        let mut inp = base();
        inp.demotes_delta = 1; // canário disparou -> eviction tem prioridade
        inp.occupied_swap_bytes = inp.alloc_active_bytes + (200 << 20); // mesmo "over", eviction ganha
        assert_eq!(reconcile(&inp, 0.10).1, ReconcileFlag::Eviction);
    }

    #[test]
    fn reconcile_stuckslice() {
        let mut inp = base();
        inp.stuck_draining = true;
        assert_eq!(reconcile(&inp, 0.10).1, ReconcileFlag::StuckSlice);
    }

    #[test]
    fn reconcile_partial_when_missing() {
        let mut inp = base();
        inp.any_source_missing = true;
        inp.demotes_delta = 1; // partial tem prioridade sobre tudo (não dá pra confiar)
        assert_eq!(reconcile(&inp, 0.10).1, ReconcileFlag::Partial);
    }

    #[test]
    fn reconcile_delta_computed_before_partial_branch() {
        // F-v2-1: mesmo no caminho Partial, o delta sai computado (não 0/garbage).
        let mut inp = base();
        inp.any_source_missing = true;
        let (delta, _) = reconcile(&inp, 0.10);
        assert!((delta - (-0.5)).abs() < 0.01, "512MiB/1GiB - 1 = -0.5");
    }

    #[test]
    fn reconcile_alloc_zero_no_giant_delta() {
        // M1: nada emprestado → delta definido (não o `occupied` cru). Vazio=None, com swap=Unaccounted.
        let mut inp = base();
        inp.alloc_active_bytes = 0;
        inp.occupied_swap_bytes = 0;
        assert_eq!(reconcile(&inp, 0.10), (0.0, ReconcileFlag::None));
        inp.occupied_swap_bytes = 999 << 20;
        let (delta, flag) = reconcile(&inp, 0.10);
        assert_eq!(flag, ReconcileFlag::Unaccounted);
        assert!(
            (delta - 1.0).abs() < 1e-9,
            "delta = 1.0 (drift total), não occupied cru"
        );
    }

    #[test]
    fn vram_outros_clamps_at_zero() {
        assert_eq!(vram_outros(2000, 500), 1500);
        assert_eq!(vram_outros(500, 2000), 0); // clamp (skew de amostragem)
    }

    #[test]
    fn telemetry_sample_serializes_flat_jsonl() {
        // RF-5/DT-8: o `core` é achatado no nível raiz (uma linha JSON) + flag em snake_case.
        let core = TelemetryCore {
            tenant: Some("civm".into()),
            slice: None,
            swap_used: 1024,
            alloc_active: 2048,
            page_io_s: Some(512),
            vram_alloc_daemon: 4096,
            vram_total_used: Some(8192),
            vram_outros: Some(4096),
            canario_demotes: 0,
            demote_reason: None,
            reconcile_delta: -0.5,
            flag: ReconcileFlag::None,
        };
        let sample = TelemetrySample {
            t: 1718,
            branch: Some("b".into()),
            commit: Some("c".into()),
            core,
        };
        let line = serde_json::to_string(&sample).expect("serializa JSON");
        assert!(line.contains("\"t\":1718"));
        assert!(
            line.contains("\"swap_used\":1024"),
            "flatten: campo do core na raiz"
        );
        assert!(line.contains("\"flag\":\"none\""), "snake_case");
        assert!(!line.contains("\"core\":"), "flatten não aninha");
    }
}
