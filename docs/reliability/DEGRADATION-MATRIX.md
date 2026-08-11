# Degradation Matrix — RamShared

> Modos de falha **raros mas caros** (Kahneman #5 — projetar pro pior caso).
> Âncora da disciplina #5 em [`kahneman-disciplines.md`](../methodology/kahneman-disciplines.md).
>
> Prioridade = **probabilidade × impacto**. Cenário cosmético fica
> "monitorado, não desenhado" (contra a paranoia — contra-exemplo #5 do doc).
> Cada cenário tem **detecção** (sinal observável) e **unwind** (recuperação em
> ordem inversa de alocação — idioma `goto out_err`).

## Matriz

| Cenário | Prob×Impacto | Comportamento projetado (degradação) | Detecção | Unwind / recuperação | Status |
| --- | --- | --- | --- | --- | --- |
| `dma_map_single`/`dma_alloc` falha (`-ENOMEM`) | médio × alto | abortar a operação, propagar errno; nunca expor device meio-mapeado | retorno != 0 checado | `goto out_err`: desfazer mapeamentos na ordem inversa, liberar páginas, sem leak | **desenhado** |
| OOM killer dispara com lock segurado | baixo × crítico | daemon de swap não pode depender de alloc no hot path; `mlockall` + `oom_score_adj=-1000` (SPECv3 §6.2/§11) | `dmesg` OOM; processo morto | pré-alocar tudo antes de expor device; nada de `GFP_KERNEL` em seção crítica | **desenhado** |
| **GPU-PV WDDM evicta a VRAM viva** (nosso caso) | **alto × alto** | latência 4K explode (~1,18 s medido, FASE0-FINAL); **DEMOTE**: `swapoff` só do tier VRAM, páginas caem pro VHDX, sem matar processo | canário de latência (SPECv3 §9.1, gatilho (a) p99 > K×baseline) | `swapoff` VRAM (timeout `T_demote`=30s) → estado `Demoted`; se estourar → `recover` | **desenhado** (SPECv3 §9.2) |
| **Budget WDDM cai ou `/dev/dxg` falha após ativação** | alto × alto | parar novos commits antes de `cuMemAlloc`; não trocar silenciosamente a autoridade para CUDA | `QUERYVIDEOMEMORYINFO` inválido/stale ou próximo chunk > `usable_budget` | manter NBD/chunks vivos; lower tier recebe novos writes; demote automático completo segue gate ITEM-4 | **parcial** ([SPEC autotier](../specs/no-milestone/wsl2-native-vram-autotier/SPEC.md)) |
| **Mais de 1 adapter dxg sem identidade CUDA↔LUID** | médio × alto | recusar startup; nunca consultar um adapter e alocar em outro por ordinal presumido | `AmbiguousAdapters(N)` | operador reduz visibilidade a 1 adapter; seleção explícita só após prova de LUID | **desenhado** |
| `swapoff` do VRAM trava sob eviction | médio × alto | não desconectar NBD com swap ativo (= panic); escalonar | processos em `D` > `T_stuck`; `nbd: ... timed out` no `dmesg` | `recover` escalonado (SPECv3 §13); `wsl --terminate` só em último recurso | **desenhado** |
| PCIe bus reset / device removal mid-DMA | baixo × crítico | I/O em voo → erro; marcar `Failed`; não completar I/O como sucesso parcial | erro CUDA / `dmesg` reset | parar filas, drenar/rejeitar pendentes, `recover` | **desenhado** (SPECv3 §8.2) |
| IOMMU fault | baixo × crítico | falhar a operação que causou; não mascarar | `dmesg` IOMMU fault | unwind do mapeamento ofensor; logar contexto | **monitorado** (fora do MVP WSL2) |
| CXL link-down / coerência perdida | baixo × crítico | tratar como device lost | `dmesg`/EDAC | offline do nó/tier afetado | **monitorado** (hardware futuro) |
| Race suspend/resume (S3/S4) | baixo × alto | reârmar contexto CUDA no resume ou recusar; sem assumir estado preservado | falha de `cuCtx*` pós-resume | reinicializar tier ou `Demoted` | **monitorado** (não no MVP manual) |
| **WinDrive B1 — disco/pagefile some com pagefile ativo** (surprise remove) | médio × crítico | processos com páginas no pagefile-VRAM morrem; risco residual de `KERNEL_DATA_INPAGE_ERROR` 0x7a se **kernel** paged-pool residiu no volume | BSOD 0x7a / processos mortos; `% Usage` pagefile-VRAM | **Proibido** destroy com pagefile ativo (DT-9); só VM até ITEM-8; ordered teardown first | **parcial** (2026-07-09: DT-21 PASS; B1 **safe arm PASS**; B1/B2 hot = 0x7A → DT-9) |
| **WinDrive B2 — serviço morre, disco permanece, I/O falha** (DT-10) | médio × alto | driver completa SRBs em voo com erro (`SRB_STATUS_ERROR` / not connected); storage stack não fica pendente | erros de I/O no volume; ETW/WPP do miniport | `QTeardownOnCrash` no CLEANUP/CLOSE; service restart + re-provision | **split** (2026-07-09: pagefile-hot kill → **0x7A**; DT-9 **REFUSE_KILL** hot + **REBOOT_KILL** clean after unload) |
| **WinDrive co-residência double-claim VRAM** (lease lógico ≠ free físico) | médio × alto | fail-closed: sem CREATE_DISK se `cuMemGetInfo.free < size` (DT-20); `LeaseRelease` imediato | log `coresidence_fail_closed`; lease negado/solto | dimensionar free-floor do daemon WSL2 ou parar pool antes do WinDrive | **desenhado** (teste unitário na IMPL ITEM-3/6) |
| **WinDrive `NtCreatePagingFile` build fora da allow-list** (DT-24) | baixo × médio | disco NTFS continua; **sem** pagefile secundário (degrada feature, não o host) | `PagefileError::UnsupportedBuild` | expandir allow-list só com drill VM na build nova | **desenhado** |
| **WinDrive revogação com pagefile ativo** (RNF-5 / DT-19) | médio × alto | holder-cooperative: pagefile off → (reboot se preciso) → destroy → wipe → `LeaseRelease`; sem Msg broker inventada | lease residual no broker; pagefile ainda listado | `Invoke-RevokeDrill.ps1`; nunca disconnect com pagefile ativo sem DT-9 | **desenhado** (harness pending) |
| **Windows Update / regressão ImDisk-style** (volume some no boot) | baixo × alto | smoke pós-boot desativa feature graciosamente se disco/pagefile sumiu | smoke fail + log `degrade=true` | reinstall package; não forçar pagefile cego | **monitorado** (smoke ITEM-7) |
| **Windows broker unavailable before exposure** | médio × médio | bounded named-pipe retry only for missing/busy; fail before lease/CUDA/LUN | stable broker error code 3 + retry count | reverse confirmed effects; consumer stops; broker may remain demand-ready | **validated** (VM retry/refusal matrix) |
| **Windows broker lost after Online** | baixo × alto | keep I/O pump alive; enter FailedSafe and request the existing safe teardown; never reconnect/replay | `broker_lost_online`, pipe EOF, lifecycle evidence | identity/pagefile/lock gates → flush/dismount → unregister/destroy → release if confirmable | **validated** (VM BrokerLossOnline) |
| **Windows broker restart budget exhausted** | baixo × médio | SCM restarts only abnormal broker crashes after 5/15/40 s; fourth action is NONE | SCM state + new broker instance ID | operator diagnoses config/process; consumer is never auto-replayed | **validated** (VM restart matrix) |
| **Windows package registration fails halfway** | baixo × alto | no candidate start; restore both old SCM definitions and active pointer | manufactured failure after broker registration | rollback to one complete immutable version; no in-place binary reversal | **validated** (package transaction matrix) |
| **Physical manifest drive-letter collision** | médio × crítico | refuse before install/exposure; never remove, remap, reletter or format existing volume | drive map/pagefile preflight | build a distinct immutable package using a proven-free letter | **validated** (`R:` refusal; final `S:` campaign) |

## Como usar

- Toda feature crítica nova **adiciona ou revisa** linhas aqui antes do merge
  (sinal mensurável da disciplina #5: "a matrix foi atualizada na última feature").
- Um postmortem cujo cenário **não estava** aqui → adicionar a linha como ação
  corretiva (ver [`postmortems/TEMPLATE.md`](../postmortems/TEMPLATE.md)).

## Referências

- [`kahneman-disciplines.md`](../methodology/kahneman-disciplines.md) §5
- [`wsl2-cascade-swap/SPEC.md`](../specs/no-milestone/wsl2-cascade-swap/SPEC.md) §8 (erros), §9 (eviction/DEMOTE), §13 (recovery)
- [`wsl2-fase0-final.md`](wsl2-fase0-final.md) — a medida de 1,18 s que fundou a linha de eviction

## CUDA storage-only product (windows-storport-cuda-vram)

| State | Symptom | Operator action | Auto recovery |
| --- | --- | --- | --- |
| CUDA alloc fail | probe/runtime refuses before CREATE | free VRAM / lower size_bytes | none |
| Device loss / stuck cuMemcpy | FailedSafe; health false | supervised reboot if stuck; no force kill | none |
| Pagefile on volume at stop | exit/code 7; Online preserved | clear pagefile; re-stop | none |
| Foreign process IOCTL | ACCESS_DENIED | only owner process | none |
| False backend (lab C#) | not product ImagePath | use Install-RamSharedService.ps1 Rust | none |
