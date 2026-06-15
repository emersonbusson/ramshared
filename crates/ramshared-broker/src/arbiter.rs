//! Política do árbitro **pura** (sem IO; clock injetado) — RF-B2, RF-B3, RNF-3 e o
//! counterfactual do PRD §14 com piso (DT-23).
//!
//! `tick` não muta `SliceMap`: devolve [`Action`]s que o core (ITEM-8) aplica. Uma decisão de
//! movimento por tick (no máx. 1 `MoveSlice` **ou** `RevertMove`); sob `pending_lease` os passos
//! de rebalanço (2/4) e o round-robin (5) ficam suprimidos (R9) — a reserva é o core aplicando
//! `GrantLease`→`lease()`. Ordem de revogação por **psi do dono** (proxy; o `tick` puro não tem
//! `used_kb` por slice — ver SPECv2 ITEM-4).

use std::cmp::Ordering;
use std::time::{Duration, Instant};

use crate::model::{PsiSample, Slice, SliceId, SliceState, TenantId};

/// Parâmetros do árbitro. Defaults calibrados pelo P0 (P0-RESULTS §5).
#[derive(Clone, Copy, Debug)]
pub struct ArbiterConfig {
    /// Diferencial de `some.avg10` para mover.
    pub delta_psi: f32,
    /// Ticks consecutivos acima do delta antes de mover (histerese).
    pub streak: u32,
    /// Cooldown pós-movimento normal.
    pub cooldown: Duration,
    /// "Sob pressão" (nunca-zero) e piso do counterfactual (DT-23).
    pub psi_floor: f32,
    /// Janela do counterfactual.
    pub cf_window: Duration,
    /// Fator de piora do drenado que dispara o revert.
    pub cf_factor: f32,
    /// Cooldown longo pós-revert.
    pub cf_cooldown: Duration,
}

impl Default for ArbiterConfig {
    fn default() -> Self {
        Self {
            delta_psi: 10.0,                       // P0: era 15; civm idle ~1.2 vs WSL2 carga 14
            streak: 5,                             // 5 ticks (tick=2s → 10s)
            cooldown: Duration::from_secs(60),     // PRD §14
            psi_floor: 5.0,                        // idle <5, carga ≥14
            cf_window: Duration::from_secs(60),    // PRD §14
            cf_factor: 2.0,                        // PRD §14 (>2× em 60s)
            cf_cooldown: Duration::from_secs(300), // PRD §14
        }
    }
}

/// Visão de um tenant **presente** (o core filtra ausentes, DT-20).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TenantView {
    pub id: TenantId,
    pub psi: PsiSample,
    pub slices: u16,
}

/// Ação que o core (ITEM-8) aplica ao `SliceMap` e aos agentes.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Slice `Free` → `Active(to)` (round-robin, DT-6).
    AssignFree { slice: SliceId, to: TenantId },
    /// Rebalanço: move a slice de `from` para `to` (RF-B2).
    MoveSlice {
        slice: SliceId,
        from: TenantId,
        to: TenantId,
    },
    /// Counterfactual §14 (DT-23): devolve a slice de `from` (atual) para `to` (dono original).
    RevertMove {
        slice: SliceId,
        from: TenantId,
        to: TenantId,
    },
    /// Revoga uma slice `Active` de `from` para atender um lease (RF-B3).
    RevokeForLease {
        slice: SliceId,
        from: TenantId,
        lease: u32,
    },
    /// Concede o lease quando há slices suficientes (uma única vez).
    GrantLease {
        lease: u32,
        holder: TenantId,
        slices: Vec<SliceId>,
    },
}

#[derive(Clone, Copy, Debug)]
struct MoveRecord {
    slice: SliceId,
    from: TenantId,
    to: TenantId,
    at: Instant,
    from_psi_at_move: f32,
}

/// Árbitro com estado mínimo (histerese, último movimento, cursor de round-robin, próximo lease).
pub struct Arbiter {
    cfg: ArbiterConfig,
    streak: u32,
    last_move: Option<MoveRecord>,
    cooldown_until: Option<Instant>,
    rr_cursor: usize,
    next_lease_id: u32,
}

fn owner_psi(tenants: &[TenantView], id: TenantId) -> Option<f32> {
    tenants.iter().find(|t| t.id == id).map(|t| t.psi.avg10)
}

fn first_active_of(slices: &[Slice], owner: TenantId) -> Option<SliceId> {
    slices
        .iter()
        .find(|s| s.state == SliceState::Active && s.tenant == Some(owner))
        .map(|s| s.id)
}

fn by_psi(a: f32, b: f32) -> Ordering {
    a.partial_cmp(&b).unwrap_or(Ordering::Equal)
}

impl Arbiter {
    pub fn new(cfg: ArbiterConfig) -> Self {
        Self {
            cfg,
            streak: 0,
            last_move: None,
            cooldown_until: None,
            rr_cursor: 0,
            next_lease_id: 1,
        }
    }

    fn cooldown_active(&self, now: Instant) -> bool {
        self.cooldown_until.is_some_and(|u| now < u)
    }

    /// (receiver mais pressionado, donor menos pressionado com ≥1 slice, diferencial).
    fn pressure_pair<'a>(
        &self,
        tenants: &'a [TenantView],
    ) -> Option<(&'a TenantView, &'a TenantView, f32)> {
        let receiver = tenants
            .iter()
            .max_by(|a, b| by_psi(a.psi.avg10, b.psi.avg10))?;
        let donor = tenants
            .iter()
            .filter(|t| t.slices >= 1 && t.id != receiver.id)
            .min_by(|a, b| by_psi(a.psi.avg10, b.psi.avg10))?;
        Some((receiver, donor, receiver.psi.avg10 - donor.psi.avg10))
    }

    /// Uma decisão por tick. `now` injetado (testável). Contrato (DT-20): `tenants` só os
    /// presentes; `slices` só Free/Leased ou de dono presente — nenhuma Action terá alvo ausente.
    pub fn tick(
        &mut self,
        now: Instant,
        tenants: &[TenantView],
        slices: &[Slice],
        pending_lease: Option<(TenantId, u64)>,
    ) -> Vec<Action> {
        let mut actions = Vec::new();
        let slice_len = slices.first().map_or(0, |s| s.len);

        // (1) LEASE pendente tem prioridade; suprime rebalanço (2/4) e round-robin (5) — R9.
        if let Some((holder, bytes)) = pending_lease {
            let need = if slice_len == 0 {
                0
            } else {
                bytes.div_ceil(slice_len) as usize
            };
            if need == 0 {
                return actions;
            }
            let leased: Vec<SliceId> = slices
                .iter()
                .filter(|s| s.state == SliceState::Leased)
                .map(|s| s.id)
                .collect();
            let free: Vec<SliceId> = slices
                .iter()
                .filter(|s| s.state == SliceState::Free)
                .map(|s| s.id)
                .collect();
            if leased.len() + free.len() >= need {
                let grant: Vec<SliceId> = leased
                    .iter()
                    .chain(free.iter())
                    .take(need)
                    .copied()
                    .collect();
                let lease = self.next_lease_id;
                self.next_lease_id += 1;
                actions.push(Action::GrantLease {
                    lease,
                    holder,
                    slices: grant,
                });
            } else {
                // Revoga Active das menos pressionadas primeiro (proxy por psi do dono; DT-8: o
                // lease drena além do nunca-zero). `lease` id estável até o grant (não incrementa).
                let deficit = need - (leased.len() + free.len());
                let mut active: Vec<(SliceId, TenantId, f32)> = slices
                    .iter()
                    .filter(|s| s.state == SliceState::Active)
                    .filter_map(|s| {
                        s.tenant
                            .map(|t| (s.id, t, owner_psi(tenants, t).unwrap_or(0.0)))
                    })
                    .collect();
                active.sort_by(|a, b| by_psi(a.2, b.2).then(a.0.cmp(&b.0)));
                let lease = self.next_lease_id;
                for (slice, from, _) in active.into_iter().take(deficit) {
                    actions.push(Action::RevokeForLease { slice, from, lease });
                }
            }
            return actions;
        }

        // (2) COUNTERFACTUAL (segurança; antes do cooldown). Não há counterfactual de um revert.
        let mut moved = false;
        if let Some(rec) = self.last_move {
            if now.duration_since(rec.at) > self.cfg.cf_window {
                self.last_move = None; // janela expirou
            } else if let Some(from_now) = owner_psi(tenants, rec.from)
                && from_now > self.cfg.cf_factor * rec.from_psi_at_move
                && from_now > self.cfg.psi_floor
            {
                actions.push(Action::RevertMove {
                    slice: rec.slice,
                    from: rec.to,
                    to: rec.from,
                });
                self.last_move = None;
                self.cooldown_until = Some(now + self.cfg.cf_cooldown);
                self.streak = 0;
                moved = true;
            }
        }

        // Histerese: o streak conta diferencial > delta independentemente do gate de movimento.
        let pair = self.pressure_pair(tenants);
        let over = pair.is_some_and(|(_, _, d)| d > self.cfg.delta_psi);
        if over {
            self.streak += 1;
        } else {
            self.streak = 0;
        }

        // (3)+(4) DIFERENCIAL: só se não houve revert, streak batido, sem cooldown e sem Free
        // (slices Free vão para o round-robin no passo 5; mover é p/ o caso tudo-atribuído).
        let has_free = slices.iter().any(|s| s.state == SliceState::Free);
        if !moved
            && self.streak >= self.cfg.streak
            && !self.cooldown_active(now)
            && !has_free
            && let Some((receiver, donor, _)) = pair
        {
            let donor_pressured = donor.psi.avg10 > self.cfg.psi_floor;
            // nunca-zero (RF-B2/DT-8): não drena um donor SOB PRESSÃO até zero slices.
            if !(donor_pressured && donor.slices <= 1)
                && let Some(slice) = first_active_of(slices, donor.id)
            {
                actions.push(Action::MoveSlice {
                    slice,
                    from: donor.id,
                    to: receiver.id,
                });
                self.last_move = Some(MoveRecord {
                    slice,
                    from: donor.id,
                    to: receiver.id,
                    at: now,
                    from_psi_at_move: donor.psi.avg10,
                });
                self.cooldown_until = Some(now + self.cfg.cooldown);
                self.streak = 0;
            }
        }

        // (5) ROUND-ROBIN das slices Free entre os presentes (DT-6).
        if !tenants.is_empty() {
            for s in slices.iter().filter(|s| s.state == SliceState::Free) {
                let to = tenants[self.rr_cursor % tenants.len()].id;
                self.rr_cursor = self.rr_cursor.wrapping_add(1);
                actions.push(Action::AssignFree { slice: s.id, to });
            }
        }

        actions
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn cfg() -> ArbiterConfig {
        ArbiterConfig::default()
    }

    fn tv(id: TenantId, avg10: f32, slices: u16) -> TenantView {
        TenantView {
            id,
            psi: PsiSample {
                avg10,
                avg60: avg10,
                stall_us: 0,
            },
            slices,
        }
    }

    fn slice(id: SliceId, tenant: Option<TenantId>, state: SliceState) -> Slice {
        Slice {
            id,
            offset: u64::from(id) * 64,
            len: 64,
            tenant,
            state,
        }
    }

    fn count_moves(a: &[Action]) -> usize {
        a.iter()
            .filter(|x| matches!(x, Action::MoveSlice { .. } | Action::RevertMove { .. }))
            .count()
    }

    #[test]
    fn histerese_moves_only_after_streak() {
        let mut c = cfg();
        c.streak = 3;
        let mut arb = Arbiter::new(c);
        let t0 = Instant::now();
        // sem Free: s0 do donor(1, psi 0), s1 do receiver(2, psi 20)
        let tenants = [tv(1, 0.0, 1), tv(2, 20.0, 1)];
        let slices = [
            slice(0, Some(1), SliceState::Active),
            slice(1, Some(2), SliceState::Active),
        ];
        assert_eq!(count_moves(&arb.tick(t0, &tenants, &slices, None)), 0); // streak 1
        assert_eq!(count_moves(&arb.tick(t0, &tenants, &slices, None)), 0); // streak 2
        let a = arb.tick(t0, &tenants, &slices, None); // streak 3 → move
        assert_eq!(
            a.iter()
                .find(|x| matches!(x, Action::MoveSlice { .. }))
                .cloned(),
            Some(Action::MoveSlice {
                slice: 0,
                from: 1,
                to: 2
            })
        );
    }

    #[test]
    fn cooldown_blocks_second_move() {
        let mut c = cfg();
        c.streak = 1;
        c.cooldown = Duration::from_secs(60);
        let mut arb = Arbiter::new(c);
        let t0 = Instant::now();
        let tenants = [tv(1, 0.0, 1), tv(2, 20.0, 1)];
        let slices = [
            slice(0, Some(1), SliceState::Active),
            slice(1, Some(2), SliceState::Active),
        ];
        assert_eq!(count_moves(&arb.tick(t0, &tenants, &slices, None)), 1); // move
        // dentro do cooldown: não move
        assert_eq!(
            count_moves(&arb.tick(t0 + Duration::from_secs(2), &tenants, &slices, None)),
            0
        );
        // após o cooldown: move de novo
        assert_eq!(
            count_moves(&arb.tick(t0 + Duration::from_secs(61), &tenants, &slices, None)),
            1
        );
    }

    #[test]
    fn nunca_zero_protege_donor_pressionado_com_uma_slice() {
        let mut c = cfg();
        c.streak = 1;
        let mut arb = Arbiter::new(c);
        let t0 = Instant::now();
        // donor(1) SOB PRESSÃO (8 > floor 5) com 1 slice; receiver(2) psi 20.
        let tenants = [tv(1, 8.0, 1), tv(2, 20.0, 1)];
        let slices = [
            slice(0, Some(1), SliceState::Active),
            slice(1, Some(2), SliceState::Active),
        ];
        assert_eq!(count_moves(&arb.tick(t0, &tenants, &slices, None)), 0);
    }

    #[test]
    fn counterfactual_reverte_quando_drenado_piora_2x_acima_do_piso() {
        let mut c = cfg();
        c.streak = 1;
        let mut arb = Arbiter::new(c);
        let t0 = Instant::now();
        // move s0 de A(1, psi 2) p/ B(2, psi 20)
        let t_move = [tv(1, 2.0, 1), tv(2, 20.0, 1)];
        let slices = [
            slice(0, Some(1), SliceState::Active),
            slice(1, Some(2), SliceState::Active),
        ];
        assert_eq!(count_moves(&arb.tick(t0, &t_move, &slices, None)), 1);
        // 10s depois: A piora p/ 6 (>2×2=4 E >floor 5) → revert
        let t_after = [tv(1, 6.0, 0), tv(2, 5.0, 2)];
        let a = arb.tick(t0 + Duration::from_secs(10), &t_after, &slices, None);
        assert_eq!(
            a.iter()
                .find(|x| matches!(x, Action::RevertMove { .. }))
                .cloned(),
            Some(Action::RevertMove {
                slice: 0,
                from: 2,
                to: 1
            })
        );
    }

    #[test]
    fn counterfactual_nao_reverte_por_ruido_abaixo_do_piso() {
        // DT-23: piora 2× mas abaixo do psi_floor não é pressão real → não reverte.
        let mut c = cfg();
        c.streak = 1;
        let mut arb = Arbiter::new(c);
        let t0 = Instant::now();
        let t_move = [tv(1, 2.0, 1), tv(2, 20.0, 1)];
        let slices = [
            slice(0, Some(1), SliceState::Active),
            slice(1, Some(2), SliceState::Active),
        ];
        assert_eq!(count_moves(&arb.tick(t0, &t_move, &slices, None)), 1);
        // A vai p/ 4.5: >2×2=4, mas <floor 5 → NÃO reverte
        let t_after = [tv(1, 4.5, 0), tv(2, 5.0, 2)];
        let a = arb.tick(t0 + Duration::from_secs(10), &t_after, &slices, None);
        assert_eq!(count_moves(&a), 0);
    }

    #[test]
    fn lease_revoga_alem_do_nunca_zero() {
        // DT-8: pedido de lease drena Active mesmo deixando tenant a zero.
        let mut arb = Arbiter::new(cfg());
        let t0 = Instant::now();
        // ambos pressionados (nunca-zero protegeria no rebalanço, mas o lease ignora).
        let tenants = [tv(1, 9.0, 1), tv(2, 9.0, 1)];
        let slices = [
            slice(0, Some(1), SliceState::Active),
            slice(1, Some(2), SliceState::Active),
        ];
        let a = arb.tick(t0, &tenants, &slices, Some((9, 64))); // need=1
        assert_eq!(
            a.iter()
                .filter(|x| matches!(x, Action::RevokeForLease { .. }))
                .count(),
            1
        );
        assert!(!a.iter().any(|x| matches!(x, Action::AssignFree { .. }))); // round-robin suprimido
    }

    #[test]
    fn lease_concede_de_free_sem_round_robin() {
        // R2: a slice Free é concedida ao lease, não round-robinada.
        let mut arb = Arbiter::new(cfg());
        let t0 = Instant::now();
        let tenants = [tv(1, 0.0, 0), tv(2, 0.0, 0)];
        let slices = [slice(0, None, SliceState::Free)];
        let a = arb.tick(t0, &tenants, &slices, Some((9, 64))); // need=1, 1 Free
        assert_eq!(
            a.iter()
                .find(|x| matches!(x, Action::GrantLease { .. }))
                .cloned(),
            Some(Action::GrantLease {
                lease: 1,
                holder: 9,
                slices: vec![0]
            })
        );
        assert!(!a.iter().any(|x| matches!(x, Action::AssignFree { .. })));
    }

    #[test]
    fn lease_segura_free_enquanto_revoga_para_completar() {
        // need=2, 1 Free + 1 Active → revoga 1, NÃO concede ainda, NÃO round-robina o Free (R2).
        let mut arb = Arbiter::new(cfg());
        let t0 = Instant::now();
        let tenants = [tv(1, 1.0, 1), tv(2, 0.0, 0)];
        let slices = [
            slice(0, None, SliceState::Free),
            slice(1, Some(1), SliceState::Active),
        ];
        let a = arb.tick(t0, &tenants, &slices, Some((9, 128))); // need=2
        assert_eq!(
            a.iter()
                .filter(|x| matches!(x, Action::RevokeForLease { .. }))
                .count(),
            1
        );
        assert!(!a.iter().any(|x| matches!(x, Action::GrantLease { .. })));
        assert!(!a.iter().any(|x| matches!(x, Action::AssignFree { .. })));
    }

    #[test]
    fn round_robin_distribui_free_entre_presentes() {
        let mut arb = Arbiter::new(cfg());
        let t0 = Instant::now();
        let tenants = [tv(1, 0.0, 0), tv(2, 0.0, 0)];
        let slices = [
            slice(0, None, SliceState::Free),
            slice(1, None, SliceState::Free),
        ];
        let a = arb.tick(t0, &tenants, &slices, None);
        let assigns: Vec<&Action> = a
            .iter()
            .filter(|x| matches!(x, Action::AssignFree { .. }))
            .collect();
        assert_eq!(assigns.len(), 2);
        // round-robin: s0→t1, s1→t2 (cursor avança)
        assert_eq!(assigns[0], &Action::AssignFree { slice: 0, to: 1 });
        assert_eq!(assigns[1], &Action::AssignFree { slice: 1, to: 2 });
    }

    #[test]
    fn sem_diferencial_nao_move() {
        let mut c = cfg();
        c.streak = 1;
        let mut arb = Arbiter::new(c);
        let t0 = Instant::now();
        // diferencial 2 < delta 10
        let tenants = [tv(1, 1.0, 1), tv(2, 3.0, 1)];
        let slices = [
            slice(0, Some(1), SliceState::Active),
            slice(1, Some(2), SliceState::Active),
        ];
        assert_eq!(count_moves(&arb.tick(t0, &tenants, &slices, None)), 0);
    }
}
