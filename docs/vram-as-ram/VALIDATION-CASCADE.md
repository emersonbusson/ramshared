# Validação de Aceitação — Cascata zram→VRAM→VHDX (SPECv3 §14)

Evidência empírica end-to-end no sistema vivo (RTX 2060, WSL2/GPU-PV), com a stack
Rust real (`ramshared up`/`down` + daemon `ramshared-wsl2d` servindo `/dev/nbd0`).
Pressão **confinada por cgroup v2** (blast radius limitado ao hog). Harness e RAW
em `/home/emdev/fase0/` (fora do repo, como os smokes da Fase 0):
`cascade-validate.sh`, `cascade-demote.sh`, `cascade-hog.c`.

## §14.3 — Spill sob pressão (a cascata absorve)

`cascade-validate.sh` (2026-06-05): `up --vram 512 --zram 256`; hog de 1300 MiB
(dados aleatórios, padrão por índice de página) num cgroup `memory.max=768M`.

| Métrica | Medido |
|---|---|
| Cascata montada | `zram0` prio **200** › `nbd0` prio **100** › `sdc` prio **-2** ✔ |
| Pico em `/dev/nbd0` (VRAM) | **511 MiB** |
| Integridade pós round-trip | **332.800 páginas íntegras, 0 corrupção** |
| Falso-positivo do canário | **nenhum** (latência do serve normal sob carga) |
| Teardown | `down` limpo |

Veredito: as páginas que excederam RAM+zram caíram na VRAM e **voltaram íntegras**.

## §14.4 — DEMOTE: migração segura de tier vivo

`cascade-demote.sh` (2026-06-05): hog de 1500 MiB em modo *hold* (segura as páginas
vivas na VRAM), depois `swapoff /dev/nbd0` — a **ação** do DEMOTE — com o daemon
servindo o read-back. (O *gatilho* do canário — spike de latência — é unit-testado
em `crates/ramshared-wsl2d/src/residency.rs`: o spike de 1,18 s da Fase 0 dispara
`Demote(Latency)`.)

| Métrica | Medido |
|---|---|
| Páginas vivas na VRAM antes | **481 MiB** |
| `swapoff /dev/nbd0` (DEMOTE) | **OK em 6 s** |
| `nbd0` após | **ausente** de `/proc/swaps` |
| VHDX absorveu | **1277 → 2058 MiB** |
| Integridade pós-migração | **384.000 páginas íntegras, 0 corrupção** |

Veredito: sob páginas vivas na VRAM, o DEMOTE **migra para o tier abaixo (VHDX) sem
perda nem corrupção**, enquanto o daemon serve o read-back — a mitigação central do
*latency-unsafe* (§9) validada em runtime.

## Cobertura §14

- §14.1 device round-trip — `wiring-smoke.sh` (write/readback 1 MiB na VRAM) ✔
- §14.2 montagem/desmontagem da cascata — `up`/`down` (acima) ✔
- §14.3 spill confinado — ✔ (acima)
- §14.4 DEMOTE — ✔ (acima)
