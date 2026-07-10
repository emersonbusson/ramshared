---
slug: wsl2-cascade-boot
title: WSL2 cascade auto-start on boot with fail-closed anti-hang
milestone: —
issues: []
---

# PRD — Cascade no boot do WSL2 (sem travar)

## 1. Summary

Quando o WSL2 sobe, o usuário quer o colchão de memória (zram → VRAM ociosa → disco) **já ligado**, e quer que a VRAM **volte para a placa** se um jogo ou render 3D no Windows precisar dela — **sem matar processos e sem travar o WSL**.

Hoje isso só funciona com `sudo ramshared up` manual. O path ublk antigo (`ramsharedd.service`) **não** é o produto Day-1 (NBD + CLI). Este PRD fecha o gap de **boot + parada ordenada + recusa se o estado estiver sujo**.

**Confirmed in codebase:** `ramshared up/down` com anti-hang (swapoff antes de matar daemon), canário free-floor/latência no daemon, DEMOTE medido.  
**Confirmed in docs:** freeze histórico por ghost swap / kill errado (`cascade.rs` contract, `validation.md`).  
**Inference (pouco):** unit systemd é a forma estável de “no boot” no WSL com `systemd=true`.

## 2. Technical context

- Day-1: `ramshared up` → zram prio 200 + NBD/CUDA prio 100 + VHDX prio -2.
- DEMOTE: `swapoff` do tier VRAM; páginas caem no disco; processos vivos.
- WDDM eviction: data-safe, latency-unsafe (~1,18 s em leitura 4K sob reclaim).
- Travamentos reais vêm de: matar daemon com nbd ativo, swap fantasma `(deleted)`, thrash host, ublk sem fix.

## 3. Recommended option

**Unit systemd `ramshared-cascade.service` (oneshot + RemainAfterExit)** que:

1. Roda **preflight NBD** (fail-closed).
2. Roda `ramshared up` com tamanhos de `/etc/ramshared/cascade.conf`.
3. No stop (incluindo `wsl --shutdown` se systemd parar units): `ramshared down` (swapoff-first).

**Não** reutilizar `ramsharedd.service` ublk como path de produto.

## 4. Functional requirements

| ID | Requirement |
| --- | --- |
| RF-1 | Install opt-in: só habilita boot depois de `ramshared check` ready e preflight OK |
| RF-2 | Boot: preflight → `up`; se preflight falhar, unit falha **sem** deixar swap sujo |
| RF-3 | Stop: sempre `down` (swapoff → nbd disconnect → daemon); nunca kill -9 com nbd em `/proc/swaps` |
| RF-4 | Config: VRAM/ZRAM MiB em conf (default conservador 1024/1024) |
| RF-5 | `up` idempotente se cascata já saudável (re-boot de unit / start duplo) |
| RF-6 | Docs humanas: o que fazer no dia a dia, o que não fazer, o que o demote custa |

## 5. Non-functional

| ID | Requirement |
| --- | --- |
| NFR-1 | Preferir **recusar start** a arriscar hang |
| NFR-2 | Timeout de stop alto o suficiente para swapoff com uso real (ex.: 600 s) |
| NFR-3 | Sem thrash no host; sem enable automático no `install` de forensics |
| NFR-4 | RNF host-safety: pressão agressiva só em VM (já regra do repo) |

## 6. Flows

1. **Primeira vez:** build → check → install-cascade-boot → enable → reboot WSL → swapon mostra 3 tiers.  
2. **Jogo no Windows:** free VRAM cai → canário → DEMOTE → VRAM tier some; apps WSL seguem.  
3. **Shutdown WSL:** systemd stop → down ordenado.  
4. **Estado sujo:** preflight/up recusa; mensagem pede `wsl --shutdown` se ghost.

## 7. Data model

- `/etc/ramshared/cascade.conf` — `VRAM_MIB`, `ZRAM_MIB`, paths de binário.  
- `/run/ramshared/*` — estado runtime (já existente).

## 8. API / Interfaces

- CLI inalterado em superfície principal: `check|doctor|up|down|status`.  
- Scripts: `install-cascade-boot.sh`, `uninstall-cascade-boot.sh`, `cascade-preflight.sh`.  
- Unit: `ramshared-cascade.service`.  
- Env opcional: `RAMSHARED_VRAM_MIB`, `RAMSHARED_ZRAM_MIB`.

## 9. Dependencies and risks

- WSL com `systemd=true` no `/etc/wsl.conf`.  
- `nbd-client`, `modprobe nbd/zram`, NVIDIA no guest.  
- Risco residual: engasgo durante DEMOTE/WDDM — **não** é freeze eterno; documentar honestamente.

## 10. Implementation strategy

1. SPEC + AUDIT-2.5 (go).  
2. Preflight NBD + unit + install.  
3. Env defaults + idempotent up.  
4. Docs humanas.  
5. IMPL + validation entry.

## 11. Documents to update

README, FAQ, ROADMAP, ARCHITECTURE, CONTRIBUTING, validation.md, this folder IMPL.

## 12. Out of scope

- Host-real Windows driver.  
- Enable automático sem opt-in.  
- ublk como path de boot.  
- Promessa de zero latência sob reclaim.

## 13. Acceptance criteria

- [ ] install opt-in documentado e scriptado  
- [ ] preflight recusa ghost / GPU sem folga / binário ausente  
- [ ] unit stop chama down  
- [ ] up idempotente com cascata já ativa  
- [ ] docs em linguagem humana; README diz o que acontece no jogo  
- [ ] testes unitários de parse env + suite workspace verde  

## 14. Validation

`cargo test -p ramshared-cli`; dry-run preflight; `docs-check`; entrada em `validation.md`.
