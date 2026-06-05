# SPECv2 — Fase B — zram writeback do frio na VRAM

> Versão após auditoria do Passo 2.5. Baseline: [`SPEC.md`](SPEC.md). **DESIGN-ONLY / kernel-gated.**
> Motivo (no-go do SPEC): (C4-1) **reentrância de memória** — writeback do zram (sob reclaim) →
> backing NBD → daemon userspace → CUDA pode reentrar na RAM que o zram tenta liberar → deadlock;
> (H4-1) **DEMOTE sem drenagem segura** — não há sysfs do zram p/ forçar readback do backing antes
> de soltar a VRAM → perda de dado; (H4-2) "daemon não muda" é falso (DEMOTE = `swapoff` hardcoded,
> mas a VRAM não está em swap aqui); (M4-1) overflow VHDX mal-modelado.

## 0. Proveniência da auditoria

- **Auditado:** `SPEC.md`. **Resultado:** `no-go` (2 CRITICAL/HIGH no núcleo da proposta de valor).
- **Conclusão de design (honesta):** rotear o writeback do zram por um **backing servido em
  userspace** (NBD/ublk → daemon → CUDA) é **inseguro sob reclaim** e **remove a ação segura do
  DEMOTE**. O caminho Day-0 limpo para "zram frio → VRAM" é um **block device de VRAM no
  kernel-space** (módulo futuro), que tira o userspace do caminho de reclaim. **Até existir esse
  driver, a recomendação é MANTER a cascata de 2 tiers atual** (zram > VRAM-swap > VHDX, validada
  §14/H1), que tem DEMOTE seguro por construção (o kernel drena no `swapoff`).

## Decisões técnicas (delta sobre o SPEC)

| # | Decisão | Corrige |
| --- | --- | --- |
| DT-5 | **Anti-reentrância (C4-1):** o writeback por backing userspace NÃO é Day-0-seguro — sob reclaim do zram, o caminho daemon→CUDA pode alocar RAM (Vec por-request em `conn.rs`, staging da libcuda) e reentrar. **Mitigações exigidas SE algum dia for userspace:** (a) pool de staging **pré-alocado + `mlock`** no daemon (zero alloc no caminho de backing); (b) validação ao vivo de ausência de deadlock sob reclaim (lockdep) ANTES de adotar. **Preferido:** backing = **block device de VRAM kernel-side** (sem userspace no reclaim) — vira um item próprio de Fase C (módulo de kernel). | C4-1 |
| DT-6 | **DEMOTE sob backing (H4-1):** não existe operação do zram que force o readback de TODO o backing. Logo, soltar/zerar a VRAM-backing com páginas vivas **perde dado**. Portanto, sob o modelo de backing, o canário §9 **perde sua ação de mitigação segura** (`swapoff` não se aplica). Consequência de design: **o backing só é aceitável se os guards de free-floor/corrupção (§9.4) impedirem encher a VRAM a ponto de precisar de DEMOTE de emergência** — o que enfraquece a garantia. Por isso DT-5 prefere o caminho kernel-side e, no interim, **a cascata de 2 tiers (DEMOTE seguro) permanece o Day-0**. | H4-1 |
| DT-7 | **Daemon muda (H4-2):** corrige o claim do SPEC. Se o backing for o device do daemon, o DEMOTE deixa de ser `swapoff <nbd>` e passa a `writeback_limit 0` + (impossível) drenagem — ver DT-6. Isso confirma que o backing userspace **não** é "daemon não muda". | H4-2 |
| DT-8 | **Overflow (M4-1):** backing-cheio ≠ zram-cheio. Quando a VRAM-backing enche, o writeback falha e as páginas **ficam no zram (RAM)**; o VHDX só recebe quando o **zram inteiro** satura (prioridade de swap). O SPEC confundia os dois níveis. Modelo correto: VHDX é overflow do **zram**, não do backing. | M4-1 |

## Recomendação final (design ativo)

**NÃO implementar o writeback-via-backing-userspace.** O esteira de auditoria mostrou que ele é
inseguro (reentrância) e sem DEMOTE seguro. Dois caminhos válidos, ambos Fase C+:
1. **Block device de VRAM kernel-side** (módulo LKM) como `backing_dev` → tira o userspace do
   reclaim, DEMOTE volta a ser seguro (kernel drena). **Preferido** (alinha ao destino Ring-0).
2. **Manter a cascata de 2 tiers** (Day-0 atual) — DEMOTE seguro, já validado §14. **Interim.**

Este SPECv2 é o **registro do design** (por que o caminho ingênuo foi rejeitado), não um SPEC de
IMPL. O Passo 3 só abre quando (1) existir o driver kernel-side.

## Mapa Kahneman (corrigido)

| Etapa / ITEM | Disciplina | Link | Pergunta | Evidência mínima | Abort trigger |
| --- | --- | --- | --- | --- | --- |
| ITEM-1 (backing) | #5 Worst-case | [`KAHNEMAN`](../methodology/KAHNEMAN-DISCIPLINES.md) | "writeback sob reclaim reentra/deadlocka?" | (kernel-side) sem userspace no reclaim; ou (userspace) pool mlocked + lockdep limpo | reentrância → não adotar; manter 2 tiers |
| ITEM-2 (política writeback) | #5 | [`KAHNEMAN`](../methodology/KAHNEMAN-DISCIPLINES.md) | "writeback_limit evita saturar a VRAM?" | (kernel) bd_stat sob limite | saturação → reverter |
| ITEM-3 (DEMOTE) | #2 Counterfactual | [`KAHNEMAN`](../methodology/KAHNEMAN-DISCIPLINES.md) | "soltar backing perde dado?" | drenagem segura inexistente → manter 2-tier DEMOTE | perda → não adotar |

## Validação

- **Hoje:** N/A (design-only). O valor entregue é a **rejeição fundamentada** do caminho ingênuo +
  o redirecionamento para o design kernel-side / manutenção do Day-0 de 2 tiers.
- **Futuro (kernel-side):** §14 adaptado + lockdep sob reclaim + DEMOTE 0 corrupção.
