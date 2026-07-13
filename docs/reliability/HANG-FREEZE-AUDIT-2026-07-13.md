# Hang / freeze audit — 2026-07-13

**Disciplina:** Kahneman #13 (existe ≠ funciona), #16 (fail-safe), #18 (camada certa).  
**Superprompt:** [`superprompt.md`](../../superprompt.md).  
**SSDV3 Passo 3:** E2E + cover ≥80% obrigatórios — [`docs/SSDV3-PROMPTS.md`](../SSDV3-PROMPTS.md).

## Escopo

Travamentos percebidos no WSL2 diário (build Docker, “freeze” guest) vs. bugs de classe hang do RamShared (ghost swap, teardown, daemon deleted).

## Fatos medidos (sessão)

| Item | Valor |
| --- | --- |
| Guest mem cap | 16 GiB (`.wslconfig`) |
| Cascata pós-redeploy | zram 2G prio 200, nbd0 4G prio 100, sdc 8G prio -2; used=0 |
| Daemon | PID novo, `BINARY_MATCH=OK` → `target/release/ramsharedd` |
| cascade-health | `ok:true`, `ghost:false`, `order_ok:true` |
| Ollama | residual 203/EXEC **removido** (unit fantasma) |
| Docker images/cache | limpos; stack Advoq down até rebuild |
| Go/Rust caches | limpos; toolchains go1.26.5 / rustc 1.97.0 |

## Classes de hang — status

| Classe | Severidade | Estado | Evidência / mitigação |
| --- | --- | --- | --- |
| Ghost ublk/nbd após kill daemon | CRITICAL | **Mitigado em código** (`cascade.rs` refuse/recover) | Postmortem 2026-07-09; testes parse_proc_swaps |
| Free sparse com used_kb≠0 | HIGH | **Mitigado** (WDDM Phase 1 MEMORY 2026-07-11) | Teardown retries até used_kb==0 |
| WDDM commit refuse sem fallback no write | HIGH | **Mitigado** (EIO → swapoff bounded) | MEMORY 2026-07-11 |
| Daemon inode deleted vs disk binary | HIGH | **Fechado nesta sessão** | Rebuild + restart; BINARY_MATCH=OK |
| Postmortem “CRASH” por Call Trace / OOM memcg / ollama spam | MEDIUM | **Mitigado** (postmortem.sh classifica kernel vs OOM; remove Call Trace) | #13 |
| OOM memcg postgres Docker | MEDIUM | **Ambiental** (não cascade) | OOM em container; não é ghost swap |
| BuildKit hang build web | MEDIUM | **Ambiental / path Docker** | Não OOM guest limpo; rebuild full sem cache |
| Pressure probe no WSL diário | HIGH se repetido | **Política** — proibido no host live | MEMORY 2026-07-11 audit |

## Gaps abertos (não fingir verde)

1. **Cover workspace monólito &lt;80%** historicamente (36% agregado em 2026-07-11). Crates dxg/autotier já 100% naquela fatia; **re-medir** após rustc 1.97 (ver validation.md desta data).
2. **ITEM-8 / StorPort INF** — LUN no Get-Disk ainda env-bound lab.
3. **Drill de pressão / demote destrutivo** — só VM isolada; não re-rodar no Ubuntu diário.
4. **Volumes Docker nomeados** (~6G) e dados Advoq — não apagados na limpeza de imagens.
5. **I:\** ainda ~70%+ — monitorar swap vhdx; não é bug de cascade.

## Gate SSDV3 para fechar hang-class

Antes de declarar “cascade safe” em qualquer PR:

- [ ] `cargo test` nos crates tocados
- [ ] cover ≥80% nos arquivos/crates da fatia (não monólito)
- [ ] `BINARY_MATCH=OK` + `cascade-health ok:true`
- [ ] ≥1 recusa (#13): ghost ou used_kb>0 se o path tocar teardown/up
- [ ] entrada em `validation.md` com números

## Rollback trigger (desta auditoria)

Se `cascade-health` `ok:false` ou `ghost:true` ou BINARY_MATCH falhar após boot → parar workloads pesados, `systemctl stop ramshared-cascade` (swapoff-first), reavaliar before `up`.
