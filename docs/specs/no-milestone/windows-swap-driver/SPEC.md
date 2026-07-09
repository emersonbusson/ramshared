# SPEC — RamShared P4 / Trilha 2: swap-para-VRAM no Windows nativo (StorPort virtual miniport)

> **Arquivo único:** `SPEC.md` (modelo Advoq/RamShared). Histórico de revisões = `git log` deste path — **sem** `SPECvN.md`.
>
> **Revisado após auditoria do Passo 2.5 (2026-07-08):** no-go na 1ª rodada → correções in-place; re-auditoria = **GO**.
> Achados da re-auditoria: só **LOW** (L1–L4, sem decisão estrutural nova). C1–H4 da 1ª rodada re-verificados e **fechados** neste mesmo arquivo.
>
> **Candidato ativo para IMPL (Passo 3):** este arquivo. Gate residual de produto: trâmite EV/Partner
> Center (R9) em curso antes de carga no host real / ITEM-11; zero driver no host real antes do
> ITEM-8 (kernel-page) em VM.
>
> Motivo do no-go na 1ª rodada: 2 CRITICAL + 4 HIGH estruturais (paths inventados vs código real). Achados bloqueantes endereçados **in-place**:
>
> - **C1** (RNF-5 / ITEM-8 Map / `Invoke-RevokeDrill`): o SPEC alegava "broker sinaliza revoke" ao
>   holder do lease. **Falso no código:** não existe `Msg` broker→holder de force-revoke; o lease só
>   termina por `LeaseRelease` do holder (`broker_srv.rs:427`) ou **disconnect** auto-release
>   (`:456-464`). `RevokeForLease` revoga **swap de outros tenants** para *conceder* o lease, não o
>   lease em si. Este SPEC redefine RNF-5 no mecanismo real (holder-cooperative + disconnect forçado
>   como último recurso admin) — DT-19.
> - **C2** (provision / RF-5): lease `Free→Leased` **não libera** `DeviceMem` no daemon (só muda
>   estado em `slices.rs:89-95`); WinDrive faz `cuMemAlloc` **local**. Sem DT, IMPL double-claim
>   silencioso na mesma GPU. Fecha co-residência: orçamento lógico no broker + gate físico
>   `cuMemGetInfo.free` no Windows + free-floor operacional — DT-20.
> - **H1** (R8): "encolher" era vago. Fecha a sequência observável de revogação com pagefile
>   ativo (sem API mágica de shrink) — DT-19.
> - **H2** (ITEM-8): enviesar paged-pool pro pagefile-VRAM via "C: mínimo" é heurística, não
>   garantia. Gate exige `% Usage` do pagefile-VRAM > 0 **antes** de matar o serviço; senão ABORT
>   do drill (não "pass" silencioso) — DT-21.
> - **H3** (ITEM-3): heartbeat do WinDrive era "mínimo" sem shape. Fecha como `Msg::Psi` default
>   (padrão P2 DT-13); `Lib` Drop do CUDA vira `loader::close` (não `dlclose` incondicional).
> - **H4** (ITEM-5): superfície de build WDK ausente. Lista `.vcxproj`/INF/package sob
>   Arquivos a CRIAR.
>
> **Re-auditoria GO — LOWs incorporados (sem decisão nova):**
> - **L1:** DT-2 alinhado a DT-22 (events = auxiliar; wake primário = COMMIT_AND_FETCH).
> - **L2:** `RAMSHARED_DISK_PARAMS` explicitado em `protocol.h` (não só na linha do IOCTL).
> - **L3:** DT-4/buffer path sob SRBEX usa helpers StorPort (`StorPortGetSystemAddress`), não
>   `Srb->DataBuffer` classic-only.
> - **L4:** matriz RF-5/RNF-5 aponta DT-19/DT-20.
>
> **SSDV3 PASSO 2** (pós 2.5), a partir de `PRD.md` (GO) + drill Passo 0 (PASS-A + B1 contido user-page).
> **Adaptação de plataforma (DT-14):** checklist Windows-kernel (WDK/SDV/Driver Verifier/InfVerif),
> não `checkpatch.pl`/`make modules`.

## Escopo fechado desta implementação

**Entra agora (RF-1..RF-8, RNF-1..RNF-8 do PRD, num único SPEC):**

- **RF-4** — port da camada CUDA para `nvcuda.dll` (Windows), reusando a **mesma tabela de símbolos**
  do `ramshared-cuda` (ITEM-1).
- **RF-3** — serviço userspace Windows (`ramshared-winsvc`) que respalda I/O de bloco em VRAM,
  reusando o adaptador `VramBackend` promovido (ITEM-2, ITEM-3, ITEM-6).
- **RF-5** — serviço vira tenant do broker existente (`LeaseRequest`/`LeaseRelease`), novo
  `TransportKind::WinDrive` (ITEM-3).
- **RF-2** — protocolo driver↔serviço **definitivo Day-0**: par de rings SPSC (SQ/CQ) em memória do
  serviço, travada+mapeada pelo driver, + data area bounce-buffer + doorbell IOCTL (ITEM-4 ABI, ITEM-5
  driver, ITEM-6 serviço).
- **RF-1** — driver StorPort **virtual miniport** do-zero: disco virtual, control device seguro
  (ITEM-4, ITEM-5).
- **RF-6** — ativação do pagefile secundário via `NtCreatePagingFile` + smoke pós-update (ITEM-7).
- **RF-7** — teardown ordenado + contenção de crash do serviço (ITEM-5 comportamento de driver, ITEM-8
  drill).
- **RF-8** — instalador attestation-signed (ITEM-11; a mecânica de assinatura; o onboarding EV/Partner
  Center é organizacional, R9, fora do código).
- **RNF-1** (N=72h, DT-12), **RNF-2** (K na 1ª medição, DT-13), **RNF-3** (Day-0), **RNF-4** (validação
  na fronteira kernel), **RNF-5** (lease revogável com pagefile ativo), **RNF-6** (VM-only para
  pressão/fuzz/drill), **RNF-7** (attestation carrega), **RNF-8** (zero regressão Linux).

**Fora agora (Day-0, sem dual-path):**

- Pagefile **primário**/boot-time (impossibilidade estrutural, PRD §2/§12).
- Distribuição via **Windows Update** e WHCP/HLK completo (plano B registrado, não neste MVP — PRD §12,
  §14 #2a).
- GPUs **não-NVIDIA** (Vulkan/D3D12 → trilha P3; o trait `VramProvider` mantém a porta aberta, mas
  nenhum backend Vulkan-Windows entra aqui).
- Interposer `nvcuda.dll` v2; tiering RAM↔VRAM dentro do serviço (MVP = VRAM-only, igual tier Linux);
  compressão/dedup; auth/cripto própria (rede privada só, igual P1/P2).
- **Multi-lease** (broker é 1-lease-por-vez, `crates/ramshared-wsl2d/src/broker_srv.rs:403`).
- **Novo `Msg` de force-revoke de lease** (C1/DT-19 — reusa disconnect/holder release).
- **Liberar `DeviceMem` do daemon no GrantLease** (C2/DT-20 — orçamento lógico + alloc local).
- Zero-copy do buffer do SRB (bounce-buffer é a escolha Day-0 — DT-4; zero-copy é otimização futura
  gated por medição, não dual-path).

**Dependências assumidas prontas (Confirmado no codebase, verificado nesta geração):**

- `trait VramProvider` (`crates/ramshared-vram/src/lib.rs:61`, `alloc`+`mem_info`) e `trait VramMemory`
  (`:41`, `zero`/`read_at`/`write_at`), sem `unsafe`, hardware-agnósticos.
- `ramshared-cuda`: `Cuda::load()` (`driver.rs:79`), `Syms` (`ffi.rs:47`) com os símbolos `_v2`
  (`cuInit`, `cuDeviceGetCount`, `cuDeviceGet`, `cuDeviceGetName`, `cuCtxCreate_v2`, `cuCtxDestroy_v2`,
  `cuCtxSynchronize`, `cuMemAlloc_v2`, `cuMemFree_v2`, `cuMemcpyHtoD_v2`, `cuMemcpyDtoH_v2`,
  `cuMemsetD8_v2`, `cuMemGetInfo_v2`, `cuGetErrorString` opcional), RAII em ordem inversa.
- `ramshared-block`: `trait BlockBackend` (`request.rs:16`, métodos `size_bytes`/`block_size`/
  `read_at`/`write_at`/`flush`), `serve()` (`request.rs:55`, validação→`NBD_EINVAL` antes do
  backend), `pub struct IoError(pub String)` (`:13` — **struct**, não enum).
- `VramBackend<M>` (`crates/ramshared-wsl2d/src/backend.rs:11-55`): adaptador `VramMemory`→`BlockBackend`,
  **genérico e sem acoplamento a `ublk`** nas linhas 11-55 (o `use crate::ublk` em `:8` serve
  `SliceView`/`RamBackend`/testes abaixo). É o alvo de promoção (DT-6).
- `ramshared-broker`: `enum Msg` (`protocol.rs:19`) com `LeaseRequest{bytes}` (`:42`),
  `LeaseRelease{lease}` (`:45`), `LeaseGranted{lease,bytes}` (`:64`), `LeaseDenied{reason}` (`:68`),
  `Register{proto,tenant,transport}` (`:21`); **sem** Msg de force-revoke ao holder (C1);
  `write_msg`/`read_msg` (`:132`/`:144`, **monomórficos em `Msg`**, teto `MAX_LINE_BYTES=64KiB`);
  `PROTO_VERSION=1` (`:12`); `enum TransportKind` (`model.rs:48` = `NbdUnix`|`NbdTcp` hoje).
- `BrokerCore` / `endpoint_for` / `on_tick` / lease: **`crates/ramshared-wsl2d/src/broker_srv.rs`**
  (não no crate `ramshared-broker` — lição P2). `endpoint_for` L182-195; `on_tick` L573;
  1-lease L403; capacity L412; grant L628-664; disconnect auto-release L456-464.
- `SliceMap::lease/unlease` (`crates/ramshared-broker/src/slices.rs:89,99`) só mudam estado —
  **não** liberam VRAM física (C2/DT-20).
- Precedente empírico do Passo 0 (drill VM 2026-07-03, `PASSO0-DRILL-RUNBOOK.md`): PASS-A + B1 contido
  3× para **página de usuário**; **página de kernel não-refutada** (é o que ITEM-8 fecha). Achado de
  método: **dado incompressível** (`RandomNumberGenerator`) é obrigatório para forçar paginação real
  (a Memory Compression do Win11 mascara dado compressível).
- Precedente de padrão P2 (`docs/specs/no-milestone/memory-broker/SPEC.md`): `windows-service`+`windows-sys`
  sob `[target.'cfg(windows)']`, bin com `main` real + stub `not(windows)` (workspace verde no Linux),
  novo `TransportKind` quebra `match` exaustivo em `endpoint_for` e exige filtro em `on_tick`.

## Matriz de rastreabilidade PRD → SPEC

| PRD | Implementação no SPEC |
| --- | --- |
| RF-1 (StorPort virtual miniport) | ITEM-4 (ABI), ITEM-5 (driver) — DT-1, DT-17, DT-18 |
| RF-2 (protocolo driver↔serviço) | ITEM-4 (ABI/`protocol.h`+mirror), ITEM-5 (rings/doorbell/inflight no driver), ITEM-6 (`driver_link` no serviço) — DT-2, DT-3, DT-4, DT-17, DT-18 |
| RF-3 (serviço userspace Rust) | ITEM-2 (`VramBackend` promovido), ITEM-3 (skeleton+broker), ITEM-6 (loop de I/O ↔ VRAM) — DT-6, DT-15, DT-16 |
| RF-4 (port CUDA → `nvcuda.dll`) | ITEM-1 (`ramshared-cuda` cross-platform) — DT-5 |
| RF-5 (tenant do broker) | ITEM-3 (`broker_tenant` + `TransportKind::WinDrive` + `on_tick` + `endpoint_for`) — DT-7, DT-19, DT-20 |
| RF-6 (pagefile secundário + smoke) | ITEM-7 (`ntpagefile` + `smoke`) — DT-8 |
| RF-7 (teardown + contenção de crash) | ITEM-5 (contenção determinística no driver, DT-10), ITEM-8 (drill + teardown ordenado, DT-9, DT-11) |
| RF-8 (instalador attestation-signed) | ITEM-11 — organizacional R9 fora do código |
| RNF-1 (zero BSOD, N horas) | ITEM-10 (soak Driver Verifier) — DT-12, DT-14 |
| RNF-2 (números, não adjetivos; teto K) | ITEM-9 (`Measure-PagefileVram.ps1`) — DT-13 |
| RNF-3 (Day-0) | todos os ITEMs; sem shim/dual-path (DT-4/DT-5/DT-15 justificados) |
| RNF-4 (validação fronteira kernel) | ITEM-5 (validação de IOCTL + MDL untrusted) — DT-14, DT-17, DT-18 |
| RNF-5 (lease revogável c/ pagefile) | ITEM-3, ITEM-7/8 (`Invoke-RevokeDrill`, R8) — DT-19 (holder-cooperative; sem Msg revoke) |
| RNF-6 (não-disruptivo, VM-only) | ITEM-8, ITEM-10 (pressão/fuzz/drill só em VM) |
| RNF-7 (attestation carrega) | ITEM-11 (verificação em 26200.8655, test-signing OFF) |
| RNF-8 (zero regressão Linux) | ITEM-1, ITEM-2 (únicos que tocam crates compartilhados) — gate = drills/testes Linux verdes |

## Decisões técnicas

Decisões fechadas aqui que o PRD deixou como "Inferência: a fixar na SPEC".

| # | Decisão | Justificativa |
| --- | --- | --- |
| DT-1 | **RF-1 = StorPort *virtual* miniport** via `VIRTUAL_HW_INITIALIZATION_DATA` (`StorPortInitialize`), **+ control device separado** criado com `IoCreateDeviceSecure` (SDDL restrito a SYSTEM+Administrators) exposto por device-interface GUID. O disco é enumerado pelo miniport; o canal ao serviço é o control device (não o path SCSI). | Padrão exato provado pelo WinSpd (StorPort virtual miniport real + control device — PRD §2/§3). Control device separado dá superfície de IOCTL própria e segurável (RNF-4), sem misturar com o path de storage. |
| DT-2 | **RF-2 = par de rings SPSC (SQ driver→serviço, CQ serviço→driver)** em memória **do serviço**, travada e mapeada pelo driver (`MmProbeAndLockPages` + `MmGetSystemAddressForMdlSafe`), + **data area bounce-buffer** (slots fixos `queue_depth × max_io_bytes`), + doorbell `IOCTL_RAMSHARED_COMMIT_AND_FETCH` (IRP pendável). Auto-reset events no REGISTER são **sinalização auxiliar** (`KeSetEvent`); o **wake primário do serviço é o IRP pendável** (DT-22) — não dual-path de espera. Modelo `ublk` adaptado ao IOCTL/MDL do Windows. | Rejeita: **NBD-sobre-loopback**; **proxy do ImDisk**; **zero-copy** do buffer do SRB (DT-4). Ring SPSC + doorbell = "1 modo: disco delegado a userspace" (PRD §3). |
| DT-3 | **Uma thread de I/O de VRAM no serviço** (single-consumer do SQ, single-producer do CQ). | Afinidade de thread do contexto CUDA é thread-local (`ramshared-cuda` `driver.rs:176-181`; `VramMemory` doc `lib.rs:38-40`); o daemon Linux já roda todo I/O de VRAM numa thread só. Reusar o invariante evita `cuCtxSetCurrent` e corridas. |
| DT-4 | **Bounce-buffer** (driver copia buffer do SRB ↔ slot da data area: WRITE antes de postar o SQE, READ após o CQE OK), **não zero-copy**. Sob SRBEX (DT-23) o ponteiro do buffer vem de **`StorPortGetSystemAddress` / helpers StorPort** — não assumir `Srb->DataBuffer` classic como único path. | O memcpy extra é desprezível vs PCIe em µs (RNF-2/R6). Zero-copy = otimização futura gated por medição (ITEM-9), não dual-path Day-0. |
| DT-5 | **RF-4 = tornar `ramshared-cuda` cross-platform**, não crate novo: extrair a fronteira de loader (`dlopen`/`dlsym`/`dlclose` vs `LoadLibraryW`/`GetProcAddress`/`FreeLibrary`) para `loader_unix.rs`/`loader_win.rs` selecionados por `#[cfg]`; `Syms` (`ffi.rs:47`) e `driver.rs` (wrappers seguros) ficam **idênticos** e compartilhados; a lista de candidatos vira `nvcuda.dll` no Windows. **Não é dual-path:** é **uma** tabela de símbolos (os nomes `_v2` existem iguais na `nvcuda.dll`), dois loaders de SO. | RF-4 pede explicitamente "a **mesma** tabela de símbolos" (PRD §2/§8). Crate paralelo duplicaria `Syms`+`driver.rs` (viola DRY/Day-0). Custo: toca o crate CUDA validado → RNF-8 (gate = testes CUDA + roundtrip GPU Linux verdes; #14). |
| DT-6 | **Promover o adaptador genérico `VramBackend<M>` para `ramshared-block`** (crate ganha dep em `ramshared-vram`); `ramshared-wsl2d` passa a `pub use ramshared_block::VramBackend` (deleta a def local, comportamento preservado). Ambos os SOs reusam o **mesmo** adaptador testado. | Regra dura #1 (reuso) + imutabilidade/DRY: o serviço Windows precisa de `VramMemory→BlockBackend`; duplicar 45 linhas divergiria Linux/Windows. `ramshared-block` é o lar natural ("onde VRAM vira block device"). As linhas 11-55 não usam `ublk` (verificado). Gate: drills `qemu-ublk-*` verdes (RNF-8, #14). |
| DT-7 | **RF-5 = novo `TransportKind::WinDrive`** (aditivo em `crates/ramshared-broker/src/model.rs:48`, hoje só `NbdUnix`/`NbdTcp` — **`DccAgent` ainda NÃO existe no código**). Adicionar a variante **quebra o `match` exaustivo** em `endpoint_for` (`crates/ramshared-wsl2d/src/broker_srv.rs:182-195`) → braço `WinDrive => None` obrigatório; e o tenant é **excluído do round-robin/rebalance de swap** filtrando por transport em `on_tick` (`:573-584`) ao construir `present` a partir de `TenantState.transport` (`:74`). Se o P2 `DccAgent` aterrissar depois, o filtro generaliza para "transports lease-only". **`arbiter.rs` sem diff** (`TenantView` não tem transport — L50). | Reuso do padrão P2 (C1/C2/DT-5 do memory-broker SPEC), verificado no código atual. O `WinDrive` só faz lease, nunca recebe `SwapOn`. |
| DT-8 | **`NtCreatePagingFile`** isolada em `ntpagefile.rs`: allow-list **DT-24** (`26200.*`), falha-graciosa, pagefile mínimo em `C:`. Remoção: `NtSetSystemInformation` remove; se SO não liberar a quente → **reboot** (é o "shrink" real — H1/DT-19). | API não-documentada (R5); allow-list vazia era gap M3. |
| DT-9 | **Teardown NUNCA remove o disco com pagefile ativo** (é exatamente o vetor B1 de BSOD). Ordem obrigatória (RF-7a): desativar pagefile → (reboot se o SO não liberar a quente) → drenar I/O em voo → destruir o disco virtual → `VramBackend::zero()` (wipe — reuso DT-17 do Linux) → `LeaseRelease`. | O drill (`PASSO0-DRILL-RUNBOOK.md`) mostrou que arrancar o disco com pagefile ativo é o cenário perigoso; o teardown seguro é o oposto disso. Wipe antes de devolver porque o pagefile conteve memória de processos (PRD fluxo 5). |
| DT-10 | **Contenção de crash (RF-7b) = comportamento determinístico no driver.** Quando o serviço morre (fecho do handle do control device → `IRP_MJ_CLEANUP`/`CLOSE`), o driver **completa TODOS os SRBs em voo com `SRB_STATUS_ERROR`/`STATUS_DEVICE_NOT_CONNECTED`** — nunca deixa SRB pendente (isso travaria o storage stack) e nunca completa como sucesso parcial. É o análogo do SIGBUS-contido do Linux, e é o que torna o cenário **B2 (erro mediado por driver)** finalmente testável (o disco NÃO some; o I/O falha de forma limpa). | Este é o **lever** de mitigação do R7: o driver pode **errar** o I/O de paging em vez de fazer o disco sumir — a hipótese (PRD fluxo 4) de que o erro mediado é mais recuperável que "disco arrancado". Provado/refutado em ITEM-8. |
| DT-11 | **Drill de página-de-kernel** via test driver **VM-only** `ramshared-poolstress.sys`: `ExAllocatePool2(POOL_FLAG_PAGED,...)` em GB + `BCryptGenRandom` + touch + IOCTL read-back; C: pagefile mínimo (heurística); **gate de residência DT-21** antes do kill; B1 vs B2 (DT-10). | Fecha lacuna do Passo 0 (só user-page). H2: placement no pagefile-VRAM não é garantido — daí DT-21. |
| DT-12 | **RNF-1: N = 72 h agregadas** (3× 24 h independentes, espírito ≥3 rodadas do `benchmarks.md`) com **Driver Verifier Standard** ativo + fuzz do caminho de I/O e dos IOCTLs, **zero BugCheck**. | Âncora reference-class (#4/#8): durações de stress HLK/WHQL (24-72 h). 3×24 h dá variância entre rodadas em vez de 1 amostra. Número fixado; counterfactual: qualquer BugCheck aborta a promoção. |
| DT-13 | **RNF-2: K "fixado na 1ª medição real", NÃO inventado agora.** O harness `Measure-PagefileVram.ps1` mede lado-a-lado (pagefile-VRAM vs pagefile em disco) **na mesma janela**, ≥3 rodadas, p50/p99+desvio, tags `idle`/`loaded`, saída dupla `results.jsonl`+`BENCHMARKS.md`. Gate = **(a)** alívio de capacidade (uso do pagefile-VRAM > 0 sob pressão) **e (b)** p99 de page-in ≤ **K×** o do disco, com **K definido pela primeira medição** (não "mais rápido que o disco" — VRAM perde pro NVMe, dado Linux). | PRD RNF-2/§13.3 corrigido pela auditoria 2.5: o valor é **capacidade**, não velocidade. Inventar K seria anchoring (#4). O SPEC fecha **como medir**, não o número. |
| DT-14 | **Checklist de validação Windows-kernel substitui o Linux** (registrado, não silencioso — exigência da tarefa): build WDK/EWDK via MSBuild com `TreatWarningsAsErrors`+`/W4 /WX`; **Static Driver Verifier** (`msbuild /p:RunCodeAnalysis=true` + SDV) report limpo (ou waivers documentados); **Driver Verifier** runtime durante RNF-1; `InfVerif.exe /w` (INF universal); `ApiValidator`; `signtool` + submissão attestation (Partner Center); harness de integração em VM via **PowerShell Direct** (equivalente kselftest, RNF-6). Rust userspace mantém `cargo fmt/clippy/test/audit/deny`. | Não há `checkpatch.pl`/`make modules` aqui. A estrutura/rigor do checklist é preservada; as ferramentas são as reais de driver Windows. |
| DT-15 | **Config `WinDriveConfig`** própria do serviço agora (self-contained, seção `[win_drive]`); quando o `ramshared-config` da P2 aterrissar, absorve esta seção. Não é shim: é a config **desta** feature. | P2 (`ramshared-config`) é SPEC, não IMPL — não assumir pronto. Definir local mantém Day-0 e evita dual-path especulativo. |
| DT-16 | **Cross-compile gating (padrão P2 DT-12):** `ramshared-winsvc` + deps Windows (`windows`, `windows-service`, `windows-sys`, `ntapi`) sob `[target.'cfg(windows)'.dependencies]`; módulos de FFI Windows `#[cfg(windows)]`; o bin tem `#[cfg(windows)] fn main` real **e** `#[cfg(not(windows))] fn main` stub (`eprintln!`+`exit(2)`). | Mantém `cargo test --workspace` verde no host Linux (o driver C não entra no cargo; o serviço compila como stub). |
| DT-17 | **`protocol.h` (C) é a ÚNICA fonte de verdade da ABI** (structs `RAMSHARED_*`, IOCTL codes, `RAMSHARED_ABI_VERSION`). O lado Rust é um mirror `#[repr(C)]` com `const { assert!(size_of::<Sqe>()==32) }` (etc.) + um teste de golden-bytes cross-check. Igual a um uapi header do kernel Linux. | uAPI/ABI (categoria 4 SSDV3): layout exposto entre Ring-0 e Ring-3 é irreversível após release; drift C↔Rust vira corrupção silenciosa. |
| DT-18 | **O driver trata a memória mapeada (rings/data area) e todos os índices/tags como NÃO-CONFIÁVEIS** (defesa em profundidade): head/tail do CQ bounds-checked a cada iteração; cada tag de CQE validado contra a inflight table (rejeitar tag desconhecido/duplicado → nunca completar um SRB duas vezes, que seria UAF/BugCheck). | O serviço é Ring-3; um serviço bugado/comprometido não pode induzir OOB nem double-complete no Ring-0 (RNF-4, #13 ilusão de validade — validar o modo de falha real, não o happy path). |
| DT-19 | **RNF-5 / R8 = revogação holder-cooperative + disconnect** (C1). Protocolo **intocado** além de `TransportKind::WinDrive` (sem novo `Msg`). (a) **Normal:** serviço executa DT-9 completo e só então `LeaseRelease`. (b) **Admin / teste de revogação:** `Invoke-RevokeDrill.ps1` manda o **serviço** (SCM stop / named-pipe admin / CLI) iniciar (a) — **não** finge um frame broker inexistente. (c) **Último recurso:** fechar a sessão TCP (broker `CloseSession` ou kill do socket) dispara auto-release no broker; o serviço trata `read_msg` EOF como "lease perdido no papel" e **se pagefile ainda ativo** entra em DT-9 de emergência (pode precisar reboot). Abort: pagefile ativo + socket morto sem DT-9 = vetor B1 residual (documentado na DEGRADATION-MATRIX). | Código real: lease só some por `LeaseRelease` ou disconnect. Inventar `LeaseRevoke` seria mudança de uAPI do broker (fora do escopo Day-0 desta feature, e P1 deliberadamente não medi usage do holder). |
| DT-20 | **Co-residência VRAM (C2): lease é orçamento lógico; alloc é físico e local.** (1) Broker: `LeaseRequest` reserva slices `Free→Leased` (`slices.rs:89-95`) — **não** faz `cuMemFree` do `DeviceMem` do daemon; a VRAM do pool Linux continua alocada. (2) WinDrive: após `LeaseGranted{bytes}`, mede `cuMemGetInfo` **no processo Windows** e só então `alloc(min(granted.bytes, free))`; se `free < config.size_bytes` → **fail-closed** (log + `LeaseRelease` imediato + não cria disco). (3) Operação com daemon WSL2 no mesmo GPU: o operador dimensiona **free-floor do daemon ≥ size_bytes do WinDrive** (ou para o pool antes do provision Windows). Fórmula proibida: assumir que lease "transfere" bytes do pool Linux pro Windows. (4) Gate de teste: com daemon segurando pool > GPU−size, provision Windows **deve** falhar gracioso (teste `coresidence_fail_closed`). | Mesma GPU física (RTX 2060). Double-claim silencioso é o bug de IMPL mais caro; fechar no SPEC evita thrash/OOM no host. Alinhado ao modelo P2 (lease = permissão/orçamento; uso CUDA é local). |
| DT-21 | **ITEM-8 — evidência de residência no pagefile-VRAM é gate, não esperança (H2).** Antes de matar o serviço no drill de kernel-page: (i) dado **incompressível** no paged pool (`BCryptGenRandom`); (ii) contador `\Paging File(<volume-vram>)\% Usage` **> 0** (ou `Win32_PageFileUsage.CurrentUsage` do volume VRAM > 0); (iii) se após pressão o uso do pagefile-VRAM == 0, o drill **ABORTA como INCONCLUSIVO** (não conta como PASS e não conta como BSOD) — C: mínimo é heurística, o SO pode manter kernel pages em C:. Só então: kill serviço / B1 vs B2, ≥3 execuções com residência confirmada. | Passo 0 já mostrou que Memory Compression + placement opaco mascaram o teste. Sem (ii) o ITEM-8 seria teatro (#13). |
| DT-22 | **Wake path único Day-0 (H3 parcial / M1):** o serviço **só** espera trabalho via `DeviceIoControl(IOCTL_RAMSHARED_COMMIT_AND_FETCH)` pendável (loop único). Os handles `sq_event`/`cq_event` no REGISTER são **sinalização auxiliar do driver** (`KeSetEvent` no submit / opcional no CQE) para futuros waiters; o MVP do serviço **não** faz `WaitForSingleObject` neles como caminho primário. Barreiras SPSC: writer faz store-release das entries **antes** de avançar `tail` (driver: `KeMemoryBarrier`/`MemoryBarrier`; serviço Rust: `Ordering::Release` no tail mirror se usar atomics; com `volatile` C + barreira explícita). Reader carrega `tail` com acquire-equivalente antes de ler entries. | Dual-path de wake = dual-path Day-0 disfarçado. Um caminho testável. |
| DT-23 | **SRB surface (M2):** miniport declara suporte a **`STORAGE_REQUEST_BLOCK` (SRBEX)** via `VIRTUAL_HW_INITIALIZATION_DATA` / feature bits do StorPort moderno; handlers aceitam SRBEX e leem buffer via APIs StorPort (`StorPortGetSystemAddress` etc.). Fallback classic `SCSI_REQUEST_BLOCK` **só** se SDV/harness na build 26200 exigir — registrado como waiver, não como segundo produto. | Win11 25H2 + WDK atual; WinSpd histórico usa paths clássicos, mas Day-0 mira o stack atual. |
| DT-24 | **`NtCreatePagingFile` allow-list (M3):** builds suportadas no MVP = **Windows 11 25H2 `26200.*`** (a do drill e a do host). `RtlGetVersion` fora da série 26200 → `PagefileError::UnsupportedBuild`, disco continua utilizável sem pagefile (smoke RF-6). Expandir a lista só com evidência de drill em VM na build nova. | Evita allow-list vazia (interpretação na IMPL) e scope creep. |
| DT-25 | **Install Day-0 = INF + signed `.cat` + root device (`Root\RamShared`).** Lab: `Inf2Cat` + test-sign `.cat`; `certutil -addstore Root/TrustedPublisher`; `pnputil /add-driver` + **`devcon install inf Root\RamShared`** (guest pnputil sem `/add-device`). Proibido `sc create` como path de produto (conflita com PnP → status 1072). Após `StorPortInitialize`, **hook dispatch** só no control device e **forward** IRPs StorPort (DT-25). LUN 1 bus/target/LUN; CREATE → `BusChangeDetected`. **MDL data ≤ 4 MiB**. COMMIT com cancel routine. **R/W: `StorPortGetSystemAddress` apenas** (`MapBuffers=NON_READ_WRITE`); parse LBA do CDB (10/16). | Evidência 2026-07-09: sc-only → 0 disco; INF+devcon → `Get-Disk N=1 RAMSHARE VRAMDISK 64MiB`; format com DataBuffer cru → BSOD **0xD1**. |

## Fronteira de atomicidade e política de rollback

**Fronteira de atomicidade desta implementação:**

- **Atômico:** (1) **um I/O de bloco** (SQE→VRAM→CQE→completion do SRB) é completado **exatamente uma
  vez**, OK **ou** erro, nunca sucesso parcial (`serve()`/`BlockBackend` já garante isso no plano
  reusado; o driver garante o exactly-once via inflight table + DT-18). (2) O **handshake REGISTER** é
  all-or-nothing: ou a fila inteira é validada+travada+mapeada, ou `IOCTL_RAMSHARED_REGISTER_QUEUE`
  falha e **nada** fica travado (unwind em ordem inversa, idioma `goto out_err`). (3) **Lease** reusa a
  serialização 1-lease-por-vez do broker (`crates/ramshared-wsl2d/src/broker_srv.rs:403`;
  `LeaseGranted` só após slices drenadas, `:628-664`). **Force-revoke do holder NÃO existe no
  protocolo** (C1/DT-19) — ver fronteira de revogação abaixo.
- **Fora da atomicidade (eventual / multi-passo, estados parciais aceitos e documentados):**
  - **Ativação do pagefile** (`NtCreatePagingFile`) é operação de SO multi-passo, **não** transacional:
    estado parcial aceito = "disco ativo, pagefile ainda não" → a feature degrada, não quebra (DT-8).
  - **Teardown** é uma sequência (DT-9); estado parcial aceito = "pagefile desativado aguardando reboot,
    disco ainda presente" — nunca "disco removido com pagefile ativo".
  - **Revogação de lease com pagefile ativo (R8/RNF-5 / DT-19):** **holder-cooperative only** no
    protocolo atual. Caminhos reais: (1) **serviço inicia** `LeaseRelease` após teardown ordenado
    do pagefile (DT-9); (2) **disconnect** da sessão TCP → broker auto-`on_lease_release`
    (`broker_srv.rs:456-464`) — o serviço DEVE ter completado DT-9 *antes* de fechar o socket, ou
    (admin) aceitar o risco residual documentado. **Não há** `Msg::LeaseRevoke` nem "broker sinaliza
    revoke" (C1). Sequência observável (H1): `pagefile off` (`ntpagefile::remove_secondary` /
    `NtSetSystemInformation` remove) → se SO não liberar a quente, **reboot** (único shrink real;
    não inventar API de "encolher sob carga") → drain I/O → destroy disk → `zero()` →
    `LeaseRelease`. Pior caso = revogação lenta (minutos se reboot), **nunca silenciosa**.
  - **Predição de capacidade** (orçamento de VRAM vs pressão) é snapshot → margem conservadora.

**Política de rollback:**

- **Rollback de app:** desinstalar (remover driver via INF + parar/remover serviço). A config de pagefile
  reverte para `C:`-only. Cada ITEM Rust compila isolado; `git revert` do ITEM reverte a superfície
  (reverter ITEM-1/ITEM-2 exige revalidar os drills Linux — por isso o gate #14).
- **Rollback de migration:** **N/A** — não há schema/estado persistido migrável (a VRAM é volátil por
  design; o conteúdo do pagefile é transitório).
- **Rollback de dados:** **N/A** — Day-0, sem produção viva, sem dado durável (o wipe `zero()` no
  teardown é higiene, não migração).
- **Proibido / `forward-only`:** **proibido em qualquer ambiente** remover/destruir o disco virtual com
  pagefile ativo (vetor B1 de BSOD, DT-9) — restrição operacional `forward-only` explícita: uma vez o
  pagefile ativo, o único caminho seguro é desativá-lo primeiro (reboot se necessário). Abort trigger
  correspondente em ITEM-8.

## Mapa Kahneman por etapa crítica

| Etapa / ITEM | Disciplina Kahneman | Link | Pergunta obrigatória | Evidência mínima | Abort trigger |
| --- | --- | --- | --- | --- | --- |
| ITEM-1 (RF-4 loader cross-platform) | #14 Mass-Refactoring + #1 WYSIATI | [`#14`](../../../methodology/kahneman-disciplines.md#disc-14) · [`#1`](../../../methodology/kahneman-disciplines.md#disc-1) | A `nvcuda.dll` exporta os **mesmos** símbolos `_v2` do `ffi.rs`? A refação muda o caminho Linux? | Windows: `Cuda::load()` resolve os 13 símbolos + `mem_info()` retorna `free/total` plausível na RTX 2060. Linux: `cargo test -p ramshared-cuda` + `gpu_roundtrip_256mib` (`--ignored`) verdes. | Qualquer símbolo `_v2` ausente na `nvcuda.dll`, **ou** qualquer regressão nos testes/roundtrip Linux. |
| ITEM-2 (promover `VramBackend`) | #14 Mass-Refactoring | [`#14`](../../../methodology/kahneman-disciplines.md#disc-14) | A promoção muda o comportamento do daemon Linux? | Drills `qemu-ublk-daemon.sh` + `qemu-ublk-crash-e1b.sh` (SIGBUS 5/5) verdes; `cargo test -p ramshared-wsl2d` sem regressão. | Qualquer regressão de drill/teste do daemon Linux → reverter a promoção. |
| ITEM-4 (RF-2 ABI `protocol.h`+mirror) | #9 Substituição de pergunta | [`#9`](../../../methodology/kahneman-disciplines.md#disc-9) | "O protocolo está certo?" → o layout C bate byte-a-byte com o mirror Rust? | `const { assert!(...) }` de tamanho compila nos dois lados; teste golden-bytes (bytes fixos ↔ struct) passa; `sizeof` C == `size_of` Rust em CI. | Drift de tamanho/offset entre `protocol.h` e o mirror Rust. |
| ITEM-5 (driver: IOCTL surface + rings) | #13 Ilusão de validade + #5 Availability | [`#13`](../../../methodology/kahneman-disciplines.md#disc-13) · [`#5`](../../../methodology/kahneman-disciplines.md#disc-5) | REGISTER/doorbell **malformados** (buffer curto, `queue_depth` não-potência-de-2, VA nula, offset desalinhado, tag desconhecido/duplicado) são **rejeitados antes** de `MmProbeAndLockPages`/de tocar VRAM/de completar SRB? | SDV report limpo; teste sob Driver Verifier: cada entrada malformada → IOCTL falha com `STATUS_INVALID_PARAMETER`, **zero BugCheck**; teste **pareado** "entrada legítima ainda funciona". | Qualquer BugCheck a partir de entrada malformada; defeito SDV sem waiver; double-complete de SRB observável. |
| ITEM-6 + ITEM-8 (crash c/ pagefile ativo — vetor R7) | #5 Availability + #2 Counterfactual | [`#5`](../../../methodology/kahneman-disciplines.md#disc-5) · [`#2`](../../../methodology/kahneman-disciplines.md#disc-2) | Matar o serviço com **página de kernel** (paged pool, dado incompressível) **confirmada no pagefile-VRAM** → contido **ou** `KERNEL_DATA_INPAGE_ERROR` 0x7a? B2 (DT-10) vs B1? | `Invoke-KernelPageDrill.ps1`: (DT-21) `% Usage` pagefile-VRAM > 0 **antes** do kill; senão INCONCLUSIVO. ≥3 execuções com residência; B1 vs B2; captura BSOD/`MEMORY.DMP`. | **B2 produz BugCheck 0x7a sem mitigação especificável** → aborto PRD §14 #2b. Drill sem residência confirmada **não** conta como PASS. |
| ITEM-7 (`NtCreatePagingFile`, não-documentada) | #1 WYSIATI + #2 Counterfactual | [`#1`](../../../methodology/kahneman-disciplines.md#disc-1) · [`#2`](../../../methodology/kahneman-disciplines.md#disc-2) | O Windows **ativa** um pagefile secundário no volume do **nosso** miniport (não testado — WYSIATI PRD §14 #1)? Build fora da allow-list degrada gracioso? | `Win32_PageFileUsage` mostra `<vram>:\pagefile.sys` ativo pós-`NtCreatePagingFile`; teste de fallback (build não suportado → sem pagefile, disco formatável/utilizável). | Ativação dá BugCheck/corrupção, **ou** não há caminho de falha-graciosa (disco quebra junto com o pagefile). |
| ITEM-9 (RNF-2 gate numérico) | #3 Número não adjetivo + #11 Halo | [`#3`](../../../methodology/kahneman-disciplines.md#disc-3) · [`#11`](../../../methodology/kahneman-disciplines.md#disc-11) | O pagefile-VRAM **alivia capacidade** (uso > 0 sob pressão) e não é **catastroficamente** mais lento que o disco? | `results.jsonl`+`BENCHMARKS.md`: p50/p99 lado-a-lado, mesma janela, ≥3 rodadas, tags `idle`/`loaded`; contador de uso do pagefile-VRAM > 0. | Alívio de capacidade == 0 (nunca usado sob pressão) **ou** p99 > K× o do disco (K da 1ª medição) → não promove (PRD §14 #2c). |
| ITEM-10 (RNF-1 soak) | #5 Availability + #6 Confiança calibrada | [`#5`](../../../methodology/kahneman-disciplines.md#disc-5) · [`#6`](../../../methodology/kahneman-disciplines.md#disc-6) | 72 h (3×24 h) sob Driver Verifier + fuzz sem BugCheck? | Logs do Driver Verifier + harness de soak; 3 rodadas registradas com `run-id`. | Qualquer BugCheck em qualquer rodada. |
| ITEM-11 (RF-8 attestation) | #2 Counterfactual | [`#2`](../../../methodology/kahneman-disciplines.md#disc-2) | O driver attestation-signed **carrega** em build estável com test-signing OFF? | Carga em Windows 11 25H2 **26200.8655**, test-signing OFF, driver confiável por padrão (RNF-7). | Não carrega em build estável (política apertou) **e** custo WHCP não se justifica → abortar/park (PRD §14 #2a). |
| RNF-5 (revogação c/ pagefile ativo, R8) | #5 Availability + #2 Counterfactual | [`#5`](../../../methodology/kahneman-disciplines.md#disc-5) · [`#2`](../../../methodology/kahneman-disciplines.md#disc-2) | Serviço executa DT-9 e só então `LeaseRelease`, sem pagefile ativo no disconnect? | `Invoke-RevokeDrill.ps1`: SCM stop/admin → pagefile off (ou reboot path) → destroy → wipe → `LeaseRelease` observado no log do broker; tempo pior caso medido. **Não** existe frame broker de revoke (C1/DT-19). | Pagefile ainda ativo após "release"; deadlock no teardown; broker ainda mostra lease após disconnect limpo. |

## Checklist de segurança (pré-implementação)

- [ ] **Isolamento (RNF-4/DT-1):** control device criado com `IoCreateDeviceSecure` + SDDL
  `D:P(A;;GA;;;SY)(A;;GA;;;BA)` (só SYSTEM + Administrators); serviço roda como LocalSystem. Ninguém sem
  privilégio abre o device.
- [ ] **Buffer overflow / OOB (RNF-4/DT-18):** todo IOCTL `METHOD_BUFFERED` valida
  `Parameters.DeviceIoControl.InputBufferLength == sizeof(struct esperado)` **antes** de ler
  `SystemBuffer`; REGISTER valida `abi_version`, `queue_depth` (potência de 2, ≤ `RAMSHARED_MAX_QD`),
  `block_size ∈ {512,4096}`, `max_io_bytes` limitado, VAs não-nulas e comprimentos consistentes **antes**
  de `MmProbeAndLockPages`; cada SQE valida offset/len (alinhado ao `block_size`, dentro da faixa) antes
  de tocar VRAM (espelha `ramshared_block::validate`).
- [ ] **Memória mapeada não-confiável (DT-18):** head/tail do CQ bounds-checked a cada iteração; tag de
  CQE validado contra a inflight table (rejeitar desconhecido/duplicado → sem double-complete de SRB).
- [ ] **Preemption / IRQL:** cópias bounce e travamento de MDL fora de `DISPATCH_LEVEL` quando exigido;
  completion de SRB segue as regras de IRQL do StorPort; nada de alocação paginável em caminho de I/O
  quente (análogo a `GFP_ATOMIC`).
- [ ] **Input validation (serviço):** `bytes` do lease revalidado no serviço antes de encaminhar ao
  broker; o broker já recusa `> total` (`broker_srv.rs:412`).
- [ ] **`unsafe`/FFI (Rust):** CUDA-Windows (ITEM-1), `driver_link` (ITEM-6), `ntpagefile` (ITEM-7) com
  `// SAFETY:` por bloco; superfície segura sem `unsafe` (padrão `ramshared-cuda`).
- [ ] **Segredos/ponteiros:** sem credencial hardcoded; **nenhum endereço de kernel logado** (WPP/ETW
  sem ponteiros — alinhado a `coding.md`: nunca vazar KASLR); telemetria sem PII (o conteúdo do pagefile
  é memória de processos — **nunca** logar payload).
- [ ] **Kernel Oops/erro interno:** IOCTL falho retorna NTSTATUS genérico; sem vazar detalhe de
  implementação/offset interno ao Ring-3.

## Arquivos a CRIAR

### `drivers/windows/ramshared/protocol.h`  *(ITEM-4 — RF-1/RF-2, DT-17)*

- **Propósito:** fonte de verdade única da ABI driver↔serviço (uAPI Windows).
- **Requisitos cobertos:** RF-2, DT-17, DT-18.
- **Structs/Types (layout fixo `#pragma pack(push,8)`; todo `UINTxx`):**
  ```c
  #define RAMSHARED_ABI_VERSION 1u
  #define RAMSHARED_MAX_QD      256u        /* queue_depth máximo (potência de 2) */
  #define RAMSHARED_MAX_IO      (1u<<20)    /* 1 MiB por slot (bounce) */

  enum ramshared_op { RAMSHARED_OP_READ=0, RAMSHARED_OP_WRITE=1, RAMSHARED_OP_FLUSH=2 };
  /* status: 0=OK; senão errno-like alinhado ao ramshared-block */
  #define RAMSHARED_ST_OK     0
  #define RAMSHARED_ST_EIO    5
  #define RAMSHARED_ST_EINVAL 22

  typedef struct _RAMSHARED_SQE {   /* driver -> serviço, 32 bytes */
      UINT64 tag; UINT32 op; UINT32 flags;
      UINT64 offset; UINT32 len; UINT32 buf_slot;
  } RAMSHARED_SQE;

  typedef struct _RAMSHARED_CQE {   /* serviço -> driver, 16 bytes */
      UINT64 tag; INT32 status; UINT32 reserved;
  } RAMSHARED_CQE;

  typedef struct _RAMSHARED_RING_HDR { /* precede entries[]; SPSC */
      UINT32 magic; UINT32 entries;      /* entries = queue_depth (potência de 2) */
      volatile UINT32 head; volatile UINT32 tail;
  } RAMSHARED_RING_HDR;

  typedef struct _RAMSHARED_REGISTER { /* payload do IOCTL REGISTER */
      UINT32 abi_version; UINT32 disk_id; UINT32 queue_depth; UINT32 block_size;
      UINT32 max_io_bytes; UINT32 reserved;
      UINT64 sq_ring_va; UINT64 cq_ring_va;
      UINT64 data_area_va; UINT64 data_area_len;
      UINT64 sq_event_handle; UINT64 cq_event_handle; /* auxiliar (DT-22); wake primário = IRP */
  } RAMSHARED_REGISTER;

  typedef struct _RAMSHARED_DISK_PARAMS { /* IOCTL CREATE_DISK */
      UINT64 size_bytes;   /* múltiplo de block_size */
      UINT32 block_size;   /* 512 ou 4096 */
      UINT32 reserved;
      UCHAR  serial[16];   /* INQUIRY VPD / identificação estável */
  } RAMSHARED_DISK_PARAMS;
  ```
- **IOCTL codes:** `CTL_CODE(FILE_DEVICE_MASS_STORAGE, 0x800|N, METHOD_BUFFERED, FILE_READ_ACCESS|FILE_WRITE_ACCESS)`
  para `IOCTL_RAMSHARED_REGISTER_QUEUE` (N=0), `IOCTL_RAMSHARED_UNREGISTER_QUEUE` (N=1),
  `IOCTL_RAMSHARED_COMMIT_AND_FETCH` (N=2), `IOCTL_RAMSHARED_CREATE_DISK` (N=3, `RAMSHARED_DISK_PARAMS{size_bytes,block_size,serial[]}`),
  `IOCTL_RAMSHARED_DESTROY_DISK` (N=4).
- **Padrão de referência:** headers uapi do kernel Linux (struct-size estável); WinSpd `winspd.h`.
- **Testes requeridos:** compilação C emite `C_ASSERT(sizeof(RAMSHARED_SQE)==32)` etc.
- **Disciplina Kahneman:** #9 (ver Mapa — ITEM-4).

### `drivers/windows/ramshared/protocol_check.rs` *(mirror Rust; vive em `crates/ramshared-winsvc/src/proto.rs`)*  *(ITEM-4 — RF-2, DT-17)*

- **Propósito:** mirror `#[repr(C)]` exato de `protocol.h` + asserts de tamanho + golden-bytes.
- **Structs:** `#[repr(C)] pub struct Sqe { pub tag:u64, pub op:u32, pub flags:u32, pub offset:u64, pub len:u32, pub buf_slot:u32 }` (idem `Cqe`, `RingHdr`, `Register`); `pub const ABI_VERSION:u32=1; pub const MAX_QD:u32=256; pub const MAX_IO:u32=1<<20;`.
- **Funções:** `const _: () = { assert!(core::mem::size_of::<Sqe>()==32); assert!(core::mem::size_of::<Cqe>()==16); /* ... */ };`
- **Testes requeridos:** `golden_sqe_bytes` (serializa uma `Sqe` conhecida e compara com o byte-array fixo que o C produz).

### `drivers/windows/ramshared/driver.c` + `driver.h`  *(ITEM-5 — RF-1, DT-1)*

- **Propósito:** `DriverEntry`; registra o **StorPort virtual miniport** e cria o control device.
- **Requisitos cobertos:** RF-1, DT-1.
- **Funções (assinatura exata WDK):**
  - `NTSTATUS DriverEntry(PDRIVER_OBJECT, PUNICODE_STRING)` — monta `VIRTUAL_HW_INITIALIZATION_DATA`
    (callbacks abaixo) → `StorPortInitialize`; cria control device (DT-1) via `IoCreateDeviceSecure`
    (SDDL SYSTEM+Admin) + `IoRegisterDeviceInterface` (GUID próprio).
  - `ULONG HwStorFindAdapter(PVOID DevExt, ..., PPORT_CONFIGURATION_INFORMATION)` — 1 bus/target/lun
    virtual; sem I/O de porta real.
  - `BOOLEAN HwStorInitialize(PVOID DevExt)`; `BOOLEAN HwStorResetBus(PVOID,ULONG)`.
  - `BOOLEAN HwStorStartIo(PVOID DevExt, PSCSI_REQUEST_BLOCK Srb)` — na prática recebe SRBEX
    (DT-23); dispatch SCSI → `virtdisk.c`.
- **Dependências:** `storport.lib`, `ntstrsafe.lib`. **Padrão:** WinSpd (virtual miniport + control device).
- **Testes:** SDV/InfVerif no ITEM-5; enumeração do disco no harness VM.

### `drivers/windows/ramshared/virtdisk.c` + `virtdisk.h`  *(ITEM-5 — RF-1)*

- **Propósito:** estado do disco virtual + tradução de comandos SCSI.
- **Structs:** `typedef struct _VIRTUAL_DISK { UINT64 size_bytes; UINT32 block_size; UCHAR serial[16]; RAMSHARED_QUEUE queue; volatile LONG state; } VIRTUAL_DISK;`
- **Funções:** `NTSTATUS VdCreate(PVIRTUAL_DISK,const RAMSHARED_DISK_PARAMS*)`; `VOID VdTranslateSrb(PVIRTUAL_DISK,PSCSI_REQUEST_BLOCK)` — trata `SCSIOP_READ/WRITE(10|16)`, `SYNCHRONIZE_CACHE`(→FLUSH), `INQUIRY`, `READ_CAPACITY(16)`, `TEST_UNIT_READY`; READ/WRITE/FLUSH viram SQE via `queue.c`.
- **Testes:** formatação NTFS no harness VM (ITEM-5).

### `drivers/windows/ramshared/queue.c` + `queue.h`  *(ITEM-5 — RF-2, DT-2, DT-10, DT-18)*

- **Propósito:** rings SPSC, inflight table, doorbell, MDL lock/map, contenção de crash.
- **Structs:** `typedef struct _RAMSHARED_QUEUE { PMDL sq_mdl,cq_mdl,data_mdl; PRAMSHARED_RING_HDR sq,cq; PUCHAR data; PKEVENT sq_event,cq_event; RAMSHARED_INFLIGHT inflight[RAMSHARED_MAX_QD]; KSPIN_LOCK lock; PIRP pended_fetch; } RAMSHARED_QUEUE;` (inflight guarda o `PSCSI_REQUEST_BLOCK` + `op` + `buf_slot` por tag).
- **Funções:**
  - `NTSTATUS QRegister(PRAMSHARED_QUEUE,const RAMSHARED_REGISTER*,KPROCESSOR_MODE)` — **valida tudo**
    (DT-18) → `MmProbeAndLockPages`(sq/cq/data) → `MmGetSystemAddressForMdlSafe` → `ObReferenceObjectByHandle`
    dos 2 eventos. Falha → unwind em ordem inversa (nada travado, atomicidade REGISTER).
  - `NTSTATUS QSubmit(PRAMSHARED_QUEUE,PSCSI_REQUEST_BLOCK,enum ramshared_op,UINT64 off,UINT32 len)` —
    aloca tag+slot; se WRITE, copia buffer do SRB (via helper StorPort/DT-23/DT-4) → slot; publica SQE
    (barreira **antes** de avançar `tail`, DT-22); `KeSetEvent(sq_event)` auxiliar; se houver
    `pended_fetch`, completa-o (wake primário do serviço).
  - `NTSTATUS QCommitAndFetch(PRAMSHARED_QUEUE,PIRP)` — dreno do CQ (valida tag/head/tail, DT-18): para
    cada CQE, se READ+OK copia slot → buffer do SRB (helper StorPort), mapeia status→`SRB_STATUS_*`,
    `StorPortNotification(RequestComplete)`; se SQ vazio, **pend** o IRP (`pended_fetch`), senão completa
    com a contagem de SQEs novos.
  - `VOID QTeardownOnCrash(PRAMSHARED_QUEUE)` (DT-10) — em `IRP_MJ_CLEANUP`/`CLOSE`: **completa TODOS os
    SRBs em voo com `SRB_STATUS_ERROR`** (determinístico, nunca pendente); `MmUnlockPages`;
    `ObDereferenceObject` dos eventos.
- **Disciplina Kahneman:** #13+#5 (ITEM-5) e #5+#2 (ITEM-6/8) no Mapa.
- **Testes:** fuzz de IOCTL sob Driver Verifier (ITEM-5); drill de crash (ITEM-8).

### `drivers/windows/ramshared/control.c` + `control.h`  *(ITEM-5 — RF-1/RF-2, RNF-4, DT-1)*

- **Propósito:** dispatch dos IOCTLs do control device + segurança.
- **Funções:** `NTSTATUS CtlDeviceControl(PDEVICE_OBJECT,PIRP)` — `switch(ioctl)` sobre os 5 códigos;
  valida `InputBufferLength`/`OutputBufferLength` antes de usar `SystemBuffer` (RNF-4); COMMIT_AND_FETCH
  pode retornar `STATUS_PENDING`. `IRP_MJ_CLEANUP` → `QTeardownOnCrash`.
- **Testes:** entradas malformadas → `STATUS_INVALID_PARAMETER`, zero BugCheck (ITEM-5, #13).

### `drivers/windows/ramshared/ramshared.inf`  *(ITEM-5/ITEM-11 — RF-1/RF-8)*

- **Propósito:** INF **universal** (attestation-signable), instala o miniport + control device interface.
- **Testes:** `InfVerif.exe /w ramshared.inf` limpo (DT-14).

### `drivers/windows/ramshared/ramshared.vcxproj` (+ `.vcxproj.filters`, `ramshared.sln`)  *(ITEM-5 — H4, DT-14)*

- **Propósito:** superfície de build WDK/EWDK Day-0 (não deixar o implementador inventar o projeto).
- **Props:** `ConfigurationType=Driver`, `DriverType=WDM`/`MiniPort` conforme template StorPort do
  WDK, `Platform=x64`, `TreatWarningAsError=true`, `/W4 /WX`, link `storport.lib` + `ntstrsafe.lib`.
- **Targets:** `Build` (Release), `Sdv` (`RunCodeAnalysis` + Static Driver Verifier), pacote INF.
- **Testes:** build limpo no EWDK; SDV report anexável no IMPL.

### `drivers/windows/ramshared/package/` (`ramshared.inf` já listado, `ramshared.man` WPP opcional)  *(ITEM-5/11)*

- **Propósito:** layout de empacotamento attestation (`signtool` + Partner Center).

### `crates/ramshared-winsvc/` (`Cargo.toml`, `src/main.rs`, `src/service.rs`, `src/driver_link.rs`, `src/ntpagefile.rs`, `src/broker_tenant.rs`, `src/smoke.rs`, `src/config.rs`, `src/proto.rs`)  *(ITEM-3/ITEM-6/ITEM-7 — RF-3/RF-5/RF-6, DT-15, DT-16)*

- **Propósito:** serviço Windows (LocalSystem) que respalda I/O em VRAM, arbitra lease e ativa o pagefile.
- **Requisitos cobertos:** RF-3, RF-5, RF-6, DT-15, DT-16.
- **Structs/Types:**
  - `config.rs`: `#[derive(Deserialize)] struct WinDriveConfig { size_bytes:u64, block_size:u32, pagefile_min:u64, pagefile_max:u64, priority:i32, broker:SocketAddr, tenant:String }` (seção `[win_drive]`, DT-15).
  - `driver_link.rs`: `struct DriverLink { ctl: HANDLE, q: QueueMap }`; `QueueMap` possui os rings+data area (memória do serviço) e os 2 eventos; método `run_io_loop<B: BlockBackend>(&mut self, backend:&mut B)` (thread única, DT-3) — `DeviceIoControl(COMMIT_AND_FETCH)` (bloqueia) → para cada SQE novo: `match op { READ=>backend.read_at(off, slot); WRITE=>backend.write_at(off, slot); FLUSH=>backend.flush() }` → posta CQE (status mapeado de `IoError`) → recomeça. `unsafe` FFI isolado (`// SAFETY:`).
  - `ntpagefile.rs` (DT-8): `fn create_secondary(volume:&Path, min:u64, max:u64) -> Result<(),PagefileError>` (`NtCreatePagingFile`); `fn remove_secondary(volume:&Path)`; guard `supported_build() -> bool` via `RtlGetVersion` (allow-list); falha-graciosa.
  - `broker_tenant.rs` (RF-5, DT-7, DT-19, DT-20): reusa `ramshared_broker::{Msg, write_msg, read_msg}` (monomórficos em `Msg`); `Register{proto:PROTO_VERSION, tenant, transport:TransportKind::WinDrive}`; `acquire(bytes)->LeaseRequest`; `release(lease)->LeaseRelease`; trata `LeaseGranted/Denied`. **Heartbeat (H3):** `Msg::Psi { sample: PsiSample::default(), swaps: vec![], mem: None }` em intervalo configurável (default 5s) — keepalive TCP + presença; PSI é ignorado na arbitragem porque `on_tick` exclui WinDrive (DT-7). **EOF/`Error`/close:** se pagefile ativo → DT-9 de emergência (DT-19c). **Pós-Granted:** gate `cuMemGetInfo` (DT-20) antes de `alloc`.
  - `smoke.rs` (RF-6/fluxo 6): `fn post_boot_smoke() -> SmokeResult` — verifica disco enumerado + pagefile ativo (`Win32_PageFileUsage`); regressão (tipo ImDisk #38) → desativa a feature graciosamente + loga.
  - `service.rs`: `fn provision()` (fluxo 1: config → `LeaseRequest` → `LeaseGranted` → **`mem_info` free≥size** (DT-20) → CUDA `alloc` → `IOCTL_CREATE_DISK` → REGISTER → volume NTFS → `NtCreatePagingFile` allow-list 26200 (DT-24)); fail-closed em qualquer passo com `LeaseRelease` se grant já ocorreu. `fn teardown()` = DT-9. `fn on_revoke_request()` (admin/SCM) = DT-19a.
  - `main.rs`: `#[cfg(windows)] fn main()` (SCM via `windows-service`) + `#[cfg(not(windows))] fn main(){ eprintln!("ramshared-winsvc: Windows-only"); std::process::exit(2); }` (DT-16).
- **Dependências internas:** `ramshared-cuda` (RF-4), `ramshared-vram`, `ramshared-block` (`BlockBackend`+`VramBackend`), `ramshared-broker`.
- **Dependências externas (só `[target.'cfg(windows)']`):** `windows`/`windows-sys` (IOCTL, `MmXxx` via handles, `Win32_PageFileUsage`), `windows-service` (SCM), `ntapi` ou FFI própria p/ `NtCreatePagingFile`/`RtlGetVersion`, `serde`+`toml`.
- **Padrão de referência:** `ramshared-agent` (cliente do broker) + `ramshared-wsl2d/main.rs` (loop de I/O de VRAM em thread única, `run_nbd`); memory-broker SPEC P2 (cross-compile gating).
- **Testes requeridos:** `driver_link` roundtrip contra um **fake driver** (mock de `DeviceIoControl` em memória) — SQE READ/WRITE/FLUSH → backend em RAM → CQE; `broker_tenant` `LeaseRequest`→`Granted` contra fake broker; `ntpagefile` fallback (build não suportado → `Err` graciosa); `config` parse. (Puros, rodam no Linux; o bin é stub — DT-16.)
- **Disciplina Kahneman:** ITEM-6/ITEM-7 no Mapa.

### `drivers/windows/tools/poolstress/` (`poolstress.c`, `poolstress.inf`)  *(ITEM-8 — RF-7, DT-11; VM-only)*

- **Propósito:** test driver que **força página de kernel** (paged pool incompressível) ao pagefile-VRAM
  e permite page-in sob comando. **Nunca** distribuído (só test-signing em VM, RNF-6).
- **Funções:** `DriverEntry` cria control device; IOCTL `ALLOC(n_gb)` → `ExAllocatePool2(POOL_FLAG_PAGED,...)` + `BCryptGenRandom` (incompressível) + toca; IOCTL `READBACK` → lê tudo (força page-in); IOCTL `TRIM_WS` → força trim do working set (`ZwSetSystemInformation`/pressão).
- **Testes:** é o próprio instrumento do drill (ITEM-8).

### `scripts/windows/` (`Invoke-DriverSoak.ps1`, `Invoke-KernelPageDrill.ps1`, `Measure-PagefileVram.ps1`, `Invoke-RevokeDrill.ps1`, `Build-Sign-Install.ps1`)  *(ITEM-8/9/10/11 — RNF-1/RNF-2/RNF-5/RNF-6/RF-8, DT-11/DT-12/DT-13)*

- **Propósito:** harness de integração/medição em VM via **PowerShell Direct** (padrão do
  `PASSO0-DRILL-RUNBOOK.md`).
- **Funções:** `Invoke-KernelPageDrill.ps1` (carrega `poolstress`, pagefile-VRAM ativo, C: mínimo, pressão
  incompressível, mata o serviço, captura BSOD/`MEMORY.DMP`, ≥3 execuções); `Measure-PagefileVram.ps1`
  (lado-a-lado vs disco, ≥3 rodadas, contexto auto, `results.jsonl`+`BENCHMARKS.md`, DT-13);
  `Invoke-DriverSoak.ps1` (Driver Verifier Standard, 3×24 h, DT-12); `Invoke-RevokeDrill.ps1`
  (RNF-5/R8/**DT-19**: para o serviço via SCM/admin → DT-9 → confere `LeaseRelease` no broker;
  **não** envia Msg inventada).
- **Testes:** produzem as evidências dos ITEMs 8/9/10/11 e da linha RNF-5 do Mapa.

## Arquivos a MODIFICAR

### `crates/ramshared-cuda/src/ffi.rs` + `src/driver.rs` (+ novos `src/loader_unix.rs`, `src/loader_win.rs`)  *(ITEM-1 — RF-4, DT-5) — RNF-8*

- **O que muda:** extrair a fronteira de loader. Hoje `ffi.rs:13-19` declara `dlopen/dlsym/dlclose/dlerror`
  com `#[link(name="dl")]` **incondicional** (não compila no Windows). Depois: `loader_unix.rs`
  (`#[cfg(unix)]`, dlopen) e `loader_win.rs` (`#[cfg(windows)]`, `LoadLibraryW`+`GetProcAddress`+`FreeLibrary`);
  `Cuda::load()` (`driver.rs:79`) chama `loader::open`/`loader::sym`/`loader::close`.
- **Requisitos cobertos:** RF-4, DT-5.
- **Função/bloco afetado:** `ffi` (extern block unix-only), `CANDIDATES` (`driver.rs:69-75`),
  `Cuda::load`, **`Lib` Drop** (`driver.rs:52-61` — hoje chama `ffi::dlclose` **sempre**; vira
  `loader::close`, senão Windows quebra no Drop — H3).
- **Antes:** `dlopen`/`dlsym` diretos; candidatos Linux/WSL2.
- **Depois:** loader por SO; Windows `CANDIDATES=["nvcuda.dll"]`. `Syms` (`ffi.rs:47-62`, **13
  obrigatórios + 1 opcional** `cuGetErrorString`) e wrappers `driver.rs` loader-agnósticos.
- **Por quê:** RF-4 exige a MESMA tabela de símbolos na `nvcuda.dll` (PRD §2/§8); um crate só evita
  duplicar `Syms`+`driver.rs` (Day-0/DRY).
- **Impacto:** **não** quebra ABI userspace; Linux **não** muda de comportamento. `ramshared-vulkan`/`wsl2d`
  não tocados. **RNF-8** = gate.
- **Testes requeridos:** Linux: `cargo test -p ramshared-cuda` + `gpu_roundtrip_256mib --ignored` verdes
  (sem regressão). Windows: `Cuda::load()` resolve os 13 símbolos na `nvcuda.dll`; `mem_info()` plausível.
- **Disciplina Kahneman:** #14 + #1 (Mapa ITEM-1).

### `crates/ramshared-cuda/Cargo.toml`  *(ITEM-1 — RF-4, DT-16)*

- **O que muda:** deps do loader Windows sob `[target.'cfg(windows)'.dependencies]` (`windows-sys` p/
  `LoadLibraryW`/`GetProcAddress`); Linux mantém o `#[link(name="dl")]`/libc. **Impacto:** nenhum no Linux.

### `crates/ramshared-block/src/lib.rs` + novo `src/vram_backend.rs`  *(ITEM-2 — RF-3, DT-6) — RNF-8*

- **O que muda:** criar `vram_backend.rs` com o `VramBackend<M>` **promovido** (mover verbatim as linhas
  11-55 de `wsl2d/backend.rs`, que **não** usam `ublk`); `lib.rs` `pub use vram_backend::VramBackend`.
- **Requisitos cobertos:** RF-3, DT-6.
- **Antes:** `ramshared-block` não conhece VRAM; `VramBackend` vive em `wsl2d`.
- **Depois:** `ramshared-block` depende de `ramshared-vram`; expõe `VramBackend<M: VramMemory>`.
- **Por quê:** o serviço Windows (`x86_64-pc-windows-msvc`) **não** compila o `wsl2d` (Linux-only); precisa
  do adaptador de um lib compartilhado — reuso, não duplicação.
- **Impacto:** `ramshared-block/Cargo.toml` ganha `ramshared-vram`; sem quebra de API (aditivo).
- **Testes requeridos:** os testes de `backend.rs` que exercem `VramBackend` migram junto; `cargo test -p ramshared-block` verde.
- **Disciplina Kahneman:** #14 (Mapa ITEM-2).

### `crates/ramshared-wsl2d/src/backend.rs`  *(ITEM-2 — RF-3, DT-6) — RNF-8*

- **O que muda:** deletar a def local de `VramBackend` (linhas 10-55) e `pub use ramshared_block::VramBackend;`.
  `SliceView`/`RamBackend`/`use crate::ublk` **permanecem**.
- **Por quê:** comportamento preservado; o daemon Linux passa a usar o mesmo tipo compartilhado.
- **Impacto:** `main.rs` (`run_nbd`) e callers de `VramBackend` inalterados (mesmo nome/assinatura).
- **Testes requeridos:** drills `qemu-ublk-daemon.sh` + `qemu-ublk-crash-e1b.sh` verdes (gate RNF-8, #14).

### `crates/ramshared-broker/src/model.rs`  *(ITEM-3 — RF-5, DT-7)*

- **O que muda:** `enum TransportKind` ganha `WinDrive` (aditivo no wire serde). **Impacto:** aditivo,
  **mas quebra o `match` exaustivo** em `endpoint_for` → tem de vir com a modificação abaixo.

### `crates/ramshared-wsl2d/src/broker_srv.rs`  *(ITEM-3 — RF-5, DT-7)*

- **O que muda:** (a) `endpoint_for` ganha braço `TransportKind::WinDrive => None` (WinDrive não tem
  endpoint NBD; mantém o `match` exaustivo compilando); (b) `on_tick` **exclui** tenants
  `transport == WinDrive` ao construir `present` (round-robin/rebalance de swap) — se o P2 `DccAgent` já
  existir, o filtro vira "transports lease-only". **Por quê:** o `WinDrive` é lease-only (DT-7).
- **Testes requeridos:** `BrokerCore`: `windrive_nao_recebe_swap` (1 WinDrive + 1 tenant swap → só o swap
  recebe `SwapOn`); `windrive_pode_lease` (lease do WinDrive revoga o swap); **`arbiter.rs` sem diff**.

### `Cargo.toml` (workspace) / `crates/ramshared-block/Cargo.toml`  *(ITEM-2/ITEM-3, DT-16)*

- **O que muda:** workspace `members += "crates/ramshared-winsvc"`. `ramshared-block` dep `ramshared-vram`.
  `ramshared-winsvc` herda `publish=false`; deps Windows sob `[target.'cfg(windows)']` (DT-16).

## Arquivos a DELETAR

| Arquivo | Motivo |
| --- | --- |
| — | Nenhum. A def local de `VramBackend` em `wsl2d/backend.rs` é **substituída** por re-export (ITEM-2), não é arquivo a deletar. Day-0 aditivo. |

## Observabilidade

**Métricas / contadores (serviço — ETW ou perf counters):**

- `ramshared_win_io_ops_total` (Counter, labels `op=read|write|flush`) — no `run_io_loop`.
- `ramshared_win_bytes_served_total` (Counter) — por CQE OK.
- `ramshared_win_inflight_depth` (Gauge) — profundidade da inflight.
- `ramshared_win_vram_bytes{kind=free|used|total}` (Gauge) — de `mem_info()`.
- `ramshared_win_pagefile_vram_usage_bytes` (Gauge) — de `Win32_PageFileUsage` do volume-VRAM (o "alívio
  de capacidade" do gate RNF-2/DT-13).
- `ramshared_win_lease_events_total` (Counter, `event=acquire|granted|denied|release|revoke`).

**Driver (WPP tracing, sem endereços de kernel):** enumeração do disco, REGISTER/UNREGISTER, contagem de
SQE/CQE, injeção de erro em `QTeardownOnCrash`, rejeições de IOCTL malformado.

**Logs estruturados (serviço):**

| Evento | Level | Campos |
| --- | --- | --- |
| Pagefile ativado/desativado | Info | `volume`, `min`, `max`, `priority`, `build` |
| Lease acquire/granted/denied/release/revoke | Info | `tenant`, `bytes`, `lease` |
| Smoke pós-update: regressão | Warn | `check`, `detalhe`, `degrade=true` |
| Driver reportou erro em voo (crash contido) | Error | `inflight_falhos`, `op` |
| Teardown ordenado (fase) | Info | `fase` (`pagefile_off`/`drain`/`destroy`/`wipe`/`release`) |

**Benchmarks (RNF-2):** `docs/benchmarks/results.jsonl` (1 linha/run) + `docs/BENCHMARKS.md` (humano),
append-only, contexto automático (`benchmarks.md`).

## Contratos e documentação viva

| Documento | Atualização necessária | Motivo |
| --- | --- | --- |
| `docs/specs/no-milestone/windows-swap-driver/IMPL.md` | Criar (por ITEM) | rastreabilidade SSDV3 (após GO do Passo 2.5); preflight em `PREFLIGHT.md` |
| `Documentation/` (uAPI/ABI) → `drivers/windows/ramshared/protocol.h` | Criar | nova ABI Ring-0↔Ring-3 (DT-17) |
| `docs/decisions/ADR-0006-storport-virtual-miniport.md` | Criar | decisão do-zero StorPort + protocolo RF-2 (ring SPSC) — arquitetural (anti-halo #11) |
| `docs/memory-broker/PRD.md` §10/§12 | Alterar | marcar "driver de swap Windows" (P4/Trilha 2) detalhado aqui; tirar do fora-de-escopo global |
| `docs/memory-broker/VISION.md` (L28) | Alterar | a linha "fora de escopo por ora" aponta para esta feature |
| `docs/reliability/DEGRADATION-MATRIX.md` | Alterar | novos modos: crash do backend c/ pagefile ativo (B2 mediado), update do Windows (ImDisk #38), revogação de lease c/ pagefile, `NtCreatePagingFile` guard-fail |
| `docs/LIBRARIES.md` | Alterar | WDK/StorPort; `windows`/`windows-sys`/`windows-service`/`ntapi`; loader `nvcuda.dll` |
| `deny.toml` | Alterar | licenças `windows*`/`ntapi`/`toml` (MIT/Apache-2.0 — allow-list atual) |
| `CLAUDE.md` | Alterar | novo tree `drivers/windows/` (padrão estrutural) |
| `.claude/rules/*.md` | N/A | nenhuma convenção nova (kernel.md já cobre "mapear/desmapear explicitamente" — vale p/ MDL) |
| `docs/methodology/kahneman-disciplines.md` | N/A | nenhuma disciplina/âncora nova |
| `README.md`/`ARCHITECTURE.md` | Alterar | novo componente (Trilha 2); `MEMORY.md` entrada por ITEM |
| `docs/INDEX.md` | Alterar | status da feature vira `SPEC` |

## Ordem de implementação

Lista numerada, sem gaps; **userspace antes de kernel** (PRD §10); cada ITEM cita seu RF/RNF/DT nos
commits (regra dura SSDV3 #4); `IMPL.md` por ITEM. **Fase 0 (drill do Passo 0) já executada** com
ressalva (kernel-page fica pro ITEM-8).

1. **ITEM-1 — RF-4:** `ramshared-cuda` cross-platform (loader split, DT-5). Testável userspace-only no
   host real (aloca/escreve/lê VRAM via `nvcuda.dll`); valida o pilar VRAM e o RNF-8. *(PRD §10.1)*
2. **ITEM-2 — RF-3 (base):** promover `VramBackend<M>` p/ `ramshared-block` (DT-6); gate = drills Linux.
3. **ITEM-3 — RF-3/RF-5:** skeleton `ramshared-winsvc` + `broker_tenant` + `TransportKind::WinDrive`
   (`model.rs`+`endpoint_for`+`on_tick`); lease e2e contra o broker existente, VRAM local, **sem driver**.
   *(PRD §10.2)*
4. **ITEM-4 — RF-1/RF-2 (ABI):** `protocol.h` + mirror Rust `proto.rs` + asserts de tamanho + golden-bytes
   (DT-17). **Contrato congelado antes do driver** (template: structs/headers primeiro).
5. **ITEM-5 — RF-1/RF-2 (driver MVP):** StorPort virtual miniport (`driver.c`/`virtdisk.c`) + control
   device seguro (`control.c`, RNF-4) + rings/doorbell/inflight/MDL (`queue.c`, DT-2/DT-18) + contenção
   determinística (`QTeardownOnCrash`, DT-10). Em VM (test-signing): disco enumera → formata NTFS →
   SDV/InfVerif limpos → fuzz de IOCTL sob Driver Verifier. *(PRD §10.3)*
6. **ITEM-6 — RF-3 (completo):** `driver_link.rs` (lado serviço do RF-2) ligado ao `VramBackend`; e2e
   read/write/flush ↔ VRAM real na VM; Driver Verifier + fuzz do caminho de I/O.
7. **ITEM-7 — RF-6:** `ntpagefile.rs` + ativação do pagefile secundário (DT-8) + `smoke.rs` (fluxo 6). *(PRD §10.4 parte)*
8. **ITEM-8 — RF-7 (o gate do R7):** `poolstress.sys` + `Invoke-KernelPageDrill.ps1` (DT-11) + teardown
   ordenado (DT-9) + comparação B1 vs B2. **Alimenta a `DEGRADATION-MATRIX` antes de qualquer host real.**
   *(PRD §10.4)*
9. **ITEM-9 — RNF-2:** `Measure-PagefileVram.ps1` lado-a-lado vs pagefile em disco (DT-13), VM e depois host. *(PRD §10.5)*
10. **ITEM-10 — RNF-1:** `Invoke-DriverSoak.ps1` (Driver Verifier, 72 h/3×24 h, DT-12), zero BugCheck.
11. **ITEM-11 — RF-8/RNF-7:** `Build-Sign-Install.ps1` (attestation + submissão Partner Center); carga no
    host real (test-signing OFF, 26200.8655), primeiro uso supervisionado (RNF-6). *(PRD §10.6)*

## Plano de testes

**Backend / puros (rodam aqui, Linux — o bin Windows é stub, DT-16):**

- `ramshared-cuda`: sem regressão Linux (`cargo test -p ramshared-cuda`); `#[ignore]` `gpu_roundtrip_256mib`.
- `ramshared-block`: `VramBackend` migrado (write→read roundtrip; OOB→erro).
- `ramshared-winsvc`: `driver_link` roundtrip contra fake `DeviceIoControl` (READ/WRITE/FLUSH → RAM → CQE);
  `broker_tenant` LeaseRequest→Granted (fake broker); **`coresidence_fail_closed`** (DT-20: free < size →
  LeaseRelease + sem CREATE_DISK); `ntpagefile` fallback build-não-suportado; `config` parse.

- `ramshared-broker`/`wsl2d`: `BrokerCore` `windrive_nao_recebe_swap` + `windrive_pode_lease`;
  **`arbiter.rs` sem diff**; drills `qemu-ublk-*` + `qemu-broker-drill.sh` (RNF-8).

**Driver Windows (VM, test-signing — RNF-6):**

- **Estado/hooks:** enumeração do disco; INF/SDV/InfVerif/ApiValidator limpos.
- **Fluxos de bloco:** formatação NTFS; READ/WRITE/FLUSH e2e ↔ VRAM; `READ_CAPACITY`/`INQUIRY` corretos.
- **Isolamento Ring-0↔Ring-3 (RNF-4/#13):** REGISTER/doorbell malformados rejeitados (`STATUS_INVALID_PARAMETER`,
  zero BugCheck) **pareado** com "entrada legítima ainda funciona"; tag desconhecido/duplicado não
  double-completa SRB (DT-18).
- **Concorrência/atomicidade:** fila cheia (`queue_depth`); flush drena; contenção de crash (DT-10) completa
  todos os SRBs em voo com erro, storage stack não trava.
- **Pior caso (ITEM-8, #5/#2):** `Invoke-KernelPageDrill.ps1` — página de **kernel** incompressível no
  pagefile-VRAM, mata o serviço, B1 vs B2, ≥3 execuções; captura BSOD/`MEMORY.DMP`.

**Medição (RNF-2/#3):** `Measure-PagefileVram.ps1` — p50/p99 lado-a-lado, mesma janela, ≥3 rodadas,
`idle`/`loaded`, `results.jsonl`+`BENCHMARKS.md`; contador de uso do pagefile-VRAM > 0.

**Soak (RNF-1):** `Invoke-DriverSoak.ps1` — 3×24 h Driver Verifier + fuzz, zero BugCheck.

**Manuais / evidências das etapas críticas:** cargas do driver attestation-signed (RNF-7); revogação
holder-cooperative com pagefile ativo (`Invoke-RevokeDrill.ps1`, RNF-5/R8/DT-19); co-residência
fail-closed (DT-20); drill kernel-page com residência confirmada (DT-21).

## Checklist de validação

> **DT-14 — checklist Windows-kernel (substitui o Linux; registrado, não silencioso).** Estrutura/rigor
> preservados; ferramentas reais de driver Windows.

**Driver (kernel-mode, C — WDK/EWDK):**

- [ ] Build MSBuild Release x64 com `TreatWarningsAsErrors=true` + `/W4 /WX` limpo (substitui `make W=1`/`checkpatch.pl`)
- [ ] **Static Driver Verifier** (`msbuild /p:RunCodeAnalysis=true` + SDV) report limpo ou waivers documentados (substitui `sparse`)
- [ ] **Code Analysis / PREfast for drivers** sem defeito não-waivado
- [ ] `InfVerif.exe /w ramshared.inf` limpo (INF universal); `ApiValidator` limpo
- [ ] **Driver Verifier Standard** ativo durante o soak (ITEM-10) — zero BugCheck (substitui KASAN/lockdep)
- [ ] Harness de integração em VM via PowerShell Direct PASS (substitui `make kselftest`): enumeração,
  NTFS, I/O e2e, IOCTL malformado rejeitado, contenção de crash (RNF-6)
- [ ] `signtool verify` + driver attestation-signed **carrega** em 26200.8655, test-signing OFF (RNF-7)

**Serviço + libs (Rust userspace):**

- [ ] `cargo fmt --all -- --check` limpo
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` limpo (novas crates + bin stub)
- [ ] `cargo test --workspace` verde (novos testes puros + atuais sem regressão; bin Windows = stub no Linux, DT-16)
- [ ] `cargo audit` + `cargo deny check` verdes com `windows*`/`ntapi`/`toml`
- [ ] **RNF-8:** drills `qemu-ublk-daemon.sh` + `qemu-ublk-crash-e1b.sh` + `qemu-broker-drill.sh` PASS; **`arbiter.rs` sem diff**
- [ ] `#[ignore]` CUDA `nvcuda.dll` na RTX 2060 (ITEM-1) — `mem_info` plausível

**Docs:**

- [ ] `docs/INDEX.md` regenerado (status `SPEC`); links das âncoras Kahneman válidos
- [ ] `DEGRADATION-MATRIX.md`, `LIBRARIES.md`, `ADR-0006`, `IMPL.md` atualizados no mesmo commit da fatia estrutural

**Gates cognitivos:**

- [ ] Cada ITEM crítico aponta para `docs/methodology/kahneman-disciplines.md` (Mapa) com âncora exata
- [ ] Cada etapa crítica registra pergunta obrigatória, evidência mínima e abort trigger
- [ ] Nenhuma linguagem vaga em ponto crítico sem critério observável
- [ ] **Gate do R7 (ITEM-8):** o drill de página-de-kernel rodou e a `DEGRADATION-MATRIX` foi atualizada
  **antes** de qualquer carga no host real
