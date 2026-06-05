# Runbook — Revisão periódica de ADRs e disciplinas (anti-cargo-cult)

Revisão **trimestral** (ou ao fechar milestone) que checa se as disciplinas
Kahneman e os ADRs estão vivos, não viraram ritual. É o mecanismo de
auto-aplicação de [`../methodology/KAHNEMAN-DISCIPLINES.md`](../methodology/KAHNEMAN-DISCIPLINES.md).

## Checklist

1. **Rollback triggers ativos** — para cada ADR em [`../decisions/`](../decisions/)
   com `Rollback trigger:`: a condição já disparou? Se sim, foi acionada? Condição
   disparada e nada feito = cargo cult → abrir postmortem.
2. **Adoção das disciplinas** — nos commits não-triviais (`feat|fix|refactor|perf`)
   do período, ≥ 30% citam alguma disciplina (#1–14) no body/ADR/review? Se < 30%
   por 6 meses → simplificar para Top-5 (gatilho do próprio doc Kahneman).
3. **Anchors existem** — todo arquivo citado pelas disciplinas/`ssdv3.md` existe
   (`docs/postmortems/`, `docs/reliability/`, `docs/decisions/`, `docs/LIBRARIES.md`,
   `methodology/SUPERPROMPT.md`). Verificável por grep de paths.
4. **Higiene** — ADRs superseded marcados; `DEGRADATION-MATRIX.md` atualizada na
   última feature crítica.

## Saída

Uma entrada em [`../postmortems/`](../postmortems/) (mesmo "sem desvio"),
registrando acionamentos de rollback trigger e a % de adoção medida.
