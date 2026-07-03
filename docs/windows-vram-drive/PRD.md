# PRD — RamShared P4 / Trilha 2: swap-para-VRAM no Windows nativo (StorPort virtual miniport)

> **Escopo:** este PRD detalha o item **"driver de swap Windows"** da fase **P4 — Gated** do PRD
> unificado (`docs/memory-broker/PRD.md` §10) — a **Trilha 2** do rollout (Trilha 1 = WSL2, já
> validada). Objetivo: **qualquer processo Windows não-modificado** (Plex Media Server, jogos,
> navegadores) se beneficiar de "swap para VRAM" de forma transparente, como o `pagefile.sys` em
> disco já beneficia hoje — sem tocar nos apps.
>
> **SSDV3:** a feature cria **driver de kernel novo + uAPI nova** (disco virtual exposto ao
> storage stack + protocolo driver↔serviço) e **produto instalável** → PRD obrigatório
> (categorias 4 e 5 de `.claude/rules/ssdv3.md`). Próximo passo após aprovação: SPEC.
> **Gate de IMPL (anti-halo):** PRD aprovado **+** SPEC aprovado via auditoria adversarial
> "Passo 2.5" (Opus) **antes de qualquer código de driver**. Zero código antes disso.
>
> **Auditoria 2.5 do PRD (Opus, 2026-07-03) = GO com 2 achados incorporados.** As 3 alegações
> `[Confirmado no codebase]` centrais foram verificadas contra o código real e batem exatamente
> (`ramshared-cuda::DeviceMem::{zero,write_at,read_at}` L237/248/263; trait `VramMemory`;
> `broker::Msg::{LeaseRequest,LeaseRelease,LeaseGranted,LeaseDenied}` L42/45/64/68) — sem os erros
> estruturais que derrubaram o SPEC da P2. Achados incorporados: **(H1)** adicionado o **Passo 0
> "Fase 0 Windows"** (§10) — kill-test barato do pagefile-em-disco-virtual-de-terceiro + crash,
> ANTES de escrever o driver do-zero; **(M1)** corrigido o gate numérico (RNF-2/§13.3): o valor é
> **capacidade** (VRAM perde pro NVMe saudável em latência, dado Linux P0 §3), não velocidade.
>
> ## ⚠️ PASSO 0 — RESULTADO PRELIMINAR (pesquisa, 2026-07-03) — RISCO DE BSOD, PIOR QUE O LINUX
>
> O Passo 0 foi respondido de forma barata por PESQUISA (sem rodar o crash-test no host, sem risco):
> **o pior caso no Windows NÃO é contido como o SIGBUS do Linux.** Quando um disco com pagefile
> ativo some, o Windows termina o **processo de usuário** cujas páginas se perderam (OK, análogo ao
> SIGBUS) — MAS se o kernel referencia uma página de **kernel** que estava no pagefile perdido, ele
> não consegue continuar → **bugcheck `KERNEL_DATA_INPAGE_ERROR` (0x7a) = tela azul**. O nosso
> backing é um **serviço userspace que pode crashar sozinho** com o SO vivo → estritamente pior que
> um RAM disk. E até RAM disks de prateleira (**SoftPerfect**) **não suportam pagefile** e avisam
> contra — usuários batem em BSOD. **Implicação:** o caminho "pagefile transparente pra qualquer
> processo (Plex)" carrega risco sério e difícil de mitigar de BSOD no crash do backend — o
> counterfactual de aborto do §14 #2b está fortemente indicado ANTES mesmo do drill em VM. O
> caminho Windows de **menor risco** é o modelo **app-opt-in** (o addon Blender/DCC da P2, onde o
> app PEDE VRAM ao broker) — que **não toca o pagefile do kernel** e portanto não tem esse vetor de
> BSOD. Ver R7 e §14 #2 atualizados.
>
> ## ✅ PASSO 0 — RESULTADO EMPÍRICO (drill em VM Windows 11 Pro, 2026-07-03) — REFINA A PESQUISA
>
> O drill do runbook foi **executado de verdade** numa VM Hyper-V Windows 11 Pro 25H2 descartável
> (Secure Boot off + test-signing; PowerShell Direct; pagefile secundário num VHDX de backend
> hot-removable substituindo o disco volátil). Detalhes/scripts: `docs/windows-vram-drive/PASSO0-DRILL-RUNBOOK.md` §Resultado.
> - **Cenário A = PASS-A:** o Windows **aceita e ativa** um pagefile secundário num disco removível
>   de terceiro (`E:\pagefile.sys` ativo após reboot, confirmado via `Win32_PageFileUsage`).
> - **Cenário B1 (remoção-surpresa) = CONTIDO, repetido 3×** (194 MB @4 GB RAM; 178 MB @2 GB RAM):
>   arrancar o disco (`Remove-VMHardDiskDrive` a quente) enquanto ele carrega **~150-200 MB de
>   páginas de USUÁRIO ativas** (dado incompressível, forçado ao pagefile) **NÃO deu BSOD** — o
>   guest perde o `E:`, continua responsivo, zero `BugCheck 1001`/`MEMORY.DMP`. **Análogo ao SIGBUS
>   contido do Linux**, não ao BSOD que a pesquisa temia.
> - **Ressalvas (honestidade — o drill NÃO é conclusivo p/ o pior caso):** (1) só ~150-200 MB de
>   páginas (não escala de GB — o Windows mantém o working set residente e o stressor userspace é
>   lento); (2) **páginas de usuário, não de kernel** — e a pesquisa alertava especificamente sobre
>   **página de kernel** (paged pool) → `KERNEL_DATA_INPAGE_ERROR`. Não foi possível forçar
>   paginação de kernel com stressor userspace. (3) **Cenário B2** (erro de I/O mediado por driver, o
>   mais fiel ao nosso caso) **não é testável sem o nosso driver** — fica pro MVP.
> - **Achado técnico de método:** a *Memory Compression* do Win11 + dado compressível **mascarava** a
>   paginação (pagefile ficava em ~2 MB); só **dado aleatório incompressível** (`RandomNumberGenerator.GetBytes`)
>   forçou páginas reais ao disco. Registrar p/ o SPEC do stressor.
> - **Efeito no gate:** o counterfactual de aborto do §14 #2b **NÃO** dispara pelo caso comum (user-page
>   é contido). O caminho transparente segue **viável com mitigações**, mas o **pior caso kernel-page
>   permanece não-refutado** → o SPEC deve incluir um teste que force paged-pool/kernel-page (via
>   nosso driver) antes do Day-0. R7 rebaixado a MÉDIO p/ user-workload; ALTO só p/ kernel-page.
>
> **DECISÃO DE ESCOPO (usuário, 2026-07-03): CAMINHO TRANSPARENTE (pagefile), "como o Windows faz"
> — ajudar TODO processo automaticamente, não só apps integrados.** O opt-in fica só como
> **fallback** registrado (§12). Racional do usuário: o objetivo é o modelo nativo do SO (o
> gerenciador de memória do Windows decide quando/o-que paginar, igual ao pagefile em HD; o
> RamShared só provê a VRAM como swap confiável + prioridade + partilha — não reimplementa o
> "quando swapar"). Consequência aceita conscientemente: o risco residual de BSOD (backend morre
> sujo com página de kernel na VRAM) é **inerente** e não-eliminável 100% por software (a VRAM é
> liberada quando o processo CUDA morre → dado perdido, igual a disco arrancado). Mitigações
> obrigatórias antes de qualquer uso no host real: prioridade baixíssima (pouca coisa crítica cai
> lá) + serviço à prova de crash com auto-restart + teardown SEMPRE ordenado + **o drill em VM
> Windows descartável é GATE obrigatório** (mede o comportamento real do crash mediado por driver,
> que PODE ser mais recuperável que disco-arrancado — o driver pode SEGURAR o I/O em vez de falhar
> na hora). Postura idêntica à do Linux hoje: recurso experimental de risco consciente, prioridade
> baixa, supervisionado, NÃO no boot.

## 1. Resumo

No **Windows nativo**, o RamShared passa a oferecer o equivalente do caminho já validado no
Linux/WSL2 (daemon `ramsharedd` servindo block device `ublk` respaldado por VRAM via CUDA, usado
como swap — e2e em 2026-07-03): um **driver StorPort virtual miniport** escrito **do zero
(Day-0)** expõe um **disco virtual** ao storage stack do Windows (formatável NTFS); cada I/O de
bloco é delegado a um **serviço userspace (Rust)** que respalda leituras/escritas em **VRAM** via
`nvcuda.dll` (port da lógica CUDA existente) — análogo ao que o `ublk` faz no Linux: kernel cuida
da uAPI de bloco, userspace decide o que fazer com cada I/O. Sobre esse disco, o serviço ativa um
**pagefile SECUNDÁRIO pós-boot** via `NtCreatePagingFile` — o mecanismo pelo qual qualquer
processo se beneficia sem modificação. O serviço é **mais um tenant do broker** existente (lease
revogável), pra VRAM ser arbitrada entre WSL2, civm e Windows. Distribuição por instalador
próprio, **attestation-signed** (não Windows Update).

Expectativa honesta: isto **não** vira "`pagefile.sys` transparente desde o boot" (impossibilidade
estrutural do Windows, §2) — vira um **pagefile secundário que ajuda quando o Windows decide
usá-lo**, ainda assim um ganho real pra qualquer processo sob pressão sustentada, como **tier
frio/overflow** (PCIe em µs, não RAM em ns).

## 2. Contexto técnico

- **Confirmado no codebase + docs — o caminho Linux existe e foi validado e2e (2026-07-03):**
  `crates/ramshared-wsl2d` (`ramsharedd`) serve `ublk` respaldado por VRAM, usado como swap. O
  pior caso já foi medido: crash do daemon com swap ativo produz **SIGBUS contido, 5/5
  determinístico** (`scripts/kernel/qemu-ublk-crash-e1b.sh`; `MEMORY.md` 2026-07-03) — o kernel
  Linux falha-seguro sem reaper extra; recomendação registrada: prioridade de swap **baixa**
  (tier frio/overflow). A Trilha 2 replica esse contrato no Windows.
- **Confirmado em docs — sem driver de kernel, não há caminho transparente no Windows:**
  `docs/memory-broker/VISION.md:28` ("Windows-como-consumidor-de-swap exigiria um driver de disco
  Windows") e auditoria de 2026-07-03 (`MEMORY.md`): o árbitro
  (`crates/ramshared-broker/src/arbiter.rs`) só enxerga tenants que se **registram** pelo
  protocolo; processos Windows arbitrários (Plex, Steam, Edge) não falam protocolo nenhum.
  Interceptar I/O de bloco de processos não-modificados exige modo kernel; não existe API
  userspace pública pra isso.
- **Confirmado em docs — pagefile primário é estruturalmente impossível:** o pagefile PRIMÁRIO é
  inicializado pelo `smss.exe` **antes** de qualquer serviço userspace poder estar pronto — nem o
  VHD nativo da própria Microsoft hospeda pagefile primário. Único caminho: **pagefile SECUNDÁRIO
  adicionado pós-boot** via `NtCreatePagingFile` (API não-documentada; a mesma que o Painel de
  Controle usa pra redimensionar pagefile sem reboot), mantendo um pagefile mínimo em `C:`.
  Fragilidade conhecida: **1 caso documentado** de quebra desse mecanismo após update do Windows
  (ImDisk Toolkit issue #38) — a monitorar (§9).
- **Confirmado em docs + empírico — assinatura por attestation é viável hoje:** a política
  Windows de abril/2026 remove confiança só dos drivers **cross-signed** (mecanismo dos anos
  2000, certs expirados), **não** dos attestation-signed. Fonte primária: MS Learn "Driver
  Signing Options" (atualizada 2026-04-14; tabela mostra "Attestation dashboard signed = **Yes**"
  pra Windows Desktop). Empírico na máquina-alvo: Windows 11 25H2 **build 26200.8655**,
  test-signing OFF (enforcement real ativo) — um driver attestation-signed **carrega e é
  confiável por padrão**. Custo: certificado **EV** (~US$ 200–400/ano) + conta Partner Center
  (Hardware Dev Center) + submissão, **sem HLK/WHQL completos**. Ressalva honesta: a MS enquadra
  attestation como "for testing scenarios / not Windows Certified" — funciona hoje, mas há risco
  de política futura apertar; o caminho future-proof (e único que permite Windows Update) é o
  WHCP completo (HLK-tested), mais caro/lento (§9, §14).
- **Confirmado no codebase — a lógica CUDA já existe e é portável:**
  `crates/ramshared-cuda/src/driver.rs` encapsula a CUDA Driver API via `dlopen` — `Cuda::load()`
  (L79), `create_context()` (L154), `Context::mem_info()` (L189, `cuMemGetInfo_v2`),
  `Context::alloc()` (L198), `DeviceMem::{zero, write_at, read_at}` (L237/L248/L263), RAII na
  ordem inversa de alocação. No Windows, a **mesma** Driver API existe como `nvcuda.dll`; portar
  = trocar `dlopen`/`dlsym` por `LoadLibraryW`/`GetProcAddress` com a **mesma tabela de símbolos**
  (`cuInit`, `cuDeviceGet`, `cuCtxCreate_v2`, `cuMemAlloc_v2`, `cuMemcpyHtoD_v2`/`DtoH_v2`,
  `cuMemGetInfo_v2`). A fronteira genérica a reusar é o trait `VramProvider`
  (`crates/ramshared-vram/src/lib.rs:61`: `alloc` + `mem_info`; regiões `VramMemory` com
  `zero/read_at/write_at` e afinidade de thread documentada — o daemon Linux já roda todo I/O de
  VRAM numa thread só).
- **Confirmado no codebase — protocolo do broker pronto:**
  `crates/ramshared-broker/src/protocol.rs`: `Msg::LeaseRequest{bytes}` (L42),
  `Msg::LeaseRelease{lease}` (L45), `Msg::LeaseGranted` (L64), `Msg::LeaseDenied` (L68); wire
  **JSON-lines UTF-8** sobre TCP, teto 64 KiB anti-DoS (`MAX_LINE_BYTES`), `PROTO_VERSION = 1`.
  O agente/serviço Windows vira **mais um tenant/consumidor** desse mesmo broker
  (`docs/memory-broker/VISION.md`).
- **Confirmado em docs — referências de ESTUDO (não de cópia), pesquisa de reuso 2026-07-03
  (`MEMORY.md`):** **WinSpd** (github.com/winfsp/winspd) é um StorPort virtual miniport de
  verdade, licença GPLv3 + exceção FLOSS (permite estudar o padrão), mas **morto há ~5 anos e
  nunca saiu de Beta**. **GpuRamDrive** (github.com/prsyahmi/GpuRamDrive, MIT) é PoC abandonada
  (2022) que já **provou** o padrão "disco virtual respaldado por CUDA/VRAM" funcionando (via
  proxy do ImDisk). **ImDisk (fork DavidXanatos)** é ativo, mas WDM legado e GPL-2.0 (§3).
- **Confirmado em docs — ambiente-alvo real (auditoria 2026-07-03):** host com RTX 2060
  (**6144 MiB** de VRAM, ~1,7 GiB já em uso de baseline) e 32 GB de RAM; Plex Media Server roda
  nativo. Dimensionamento do tier tem de ser conservador (orçamento líquido ~4–5 GiB no melhor
  caso).
- **Confirmado em docs — física da latência:** RAM (ns) ≫ PCIe (µs)
  (`docs/memory-broker/VISION.md`); nenhum tier muda isso. O produto administra **capacidade**,
  não física — mesma lição da Fase 0 do Linux.

## 3. Opção recomendada

**Escrever um driver StorPort *virtual miniport* DO ZERO (Day-0)**, usando WinSpd e GpuRamDrive
como **referências de estudo** (padrão de miniport e padrão CUDA/VRAM-backed disk, ambos já
provados por terceiros) — em vez de reusar/forkar driver existente.

Alternativas avaliadas e descartadas:

- **Alternativa A — fork do ImDisk (DavidXanatos), modo proxy.** Menor esforço aparente: o driver
  já existe e é ativo, e o GpuRamDrive provou ImDisk-proxy + CUDA funcionando [Confirmado em
  docs]. **Descartada** por três razões: (i) é driver **WDM legado**, não o padrão StorPort
  moderno recomendado pela Microsoft pra storage novo [Confirmado em docs]; (ii) licença
  **GPL-2.0** = viral pra um produto instalável (winget/`.exe`) [Confirmado em docs]; (iii) falar
  o protocolo proxy genérico do ImDisk (desenhado pra servir imagens de disco) é baggage que
  viola a política **Day-0** do projeto (`CLAUDE.md`: sem shims, sem camadas de compatibilidade;
  solução limpa e definitiva) [Confirmado em docs].
- **Alternativa B — reviver o WinSpd.** Arquiteturalmente é o caminho certo (StorPort miniport
  real), mas adotar um codebase **morto há ~5 anos que nunca saiu de Beta** = assumir a
  manutenção de código de terceiro sem maturidade comprovada [Confirmado em docs]. **Descartada
  como base; mantida como referência de estudo.**
- **Fator decisivo que derrubou o argumento pró-reuso:** a assinatura de driver custa **o mesmo**
  (attestation) pros três caminhos — fork, revival ou do-zero [Confirmado em docs + empírico,
  §2]. Logo, "reuso reduz risco/custo de assinatura" é **falso**. Sobram a favor do do-zero:
  política Day-0, licença livre de amarras e fit exato ao problema (o driver só precisa de UM
  modo: disco virtual delegado a userspace).

## 4. Requisitos funcionais (RF)

- **RF-1 — Driver kernel-mode: StorPort virtual miniport.** Expõe um **disco virtual** ao Windows
  Storage stack — o Windows enxerga um disco comum, formata **NTFS** e pode hospedar um pagefile
  secundário nele. O driver **não contém lógica CUDA**: serve cada I/O de bloco delegando ao
  serviço userspace (RF-3) via o protocolo de backend (RF-2) — análogo ao `ublk_drv` no Linux
  (kernel cuida da uAPI de bloco; userspace decide o que fazer com cada I/O). *(A construir;
  padrão provado pelas referências de estudo — Confirmado em docs.)*
- **RF-2 — Protocolo driver↔serviço (backend).** IOCTL/shared-memory **dedicado** (device
  interface própria): fila de requests de bloco (`read`/`write`/`flush`, offset, len),
  completions, timeout e teardown definidos. Desenho Day-0 (protocolo próprio, não o proxy do
  ImDisk). *(Inferência: formato exato de fila/handshake a fixar na SPEC.)*
- **RF-3 — Serviço userspace (Rust, Windows).** Implementa o lado servidor do protocolo (RF-2),
  respaldando cada leitura/escrita de bloco lendo/escrevendo **VRAM** via a lógica CUDA portada
  (RF-4). Respeita a afinidade de thread do provider (I/O de VRAM numa thread só, como o daemon
  Linux — Confirmado no codebase). Roda como serviço do Windows (auto-start). *(A construir;
  papel espelha o `ramsharedd` — Confirmado no codebase.)*
- **RF-4 — Port da camada CUDA pra `nvcuda.dll`.** Loader Windows (`LoadLibraryW` +
  `GetProcAddress`) com a **mesma tabela de símbolos** do `ramshared-cuda` atual, implementando o
  trait `VramProvider` (`alloc` + `mem_info`) e `VramMemory` (`zero/read_at/write_at`)
  [Confirmado no codebase — §2]. Nenhuma API nova é inventada: é a mesma CUDA Driver API.
- **RF-5 — Tenant do broker (lease de VRAM).** O serviço pede o lease antes de provisionar
  (`LeaseRequest{bytes}`) e devolve no teardown (`LeaseRelease{lease}`), reusando
  `ramshared-broker::Msg` (JSON-lines/TCP) [Confirmado no codebase — §2]. A VRAM fica arbitrada
  entre WSL2, civm e Windows pelo mesmo broker. Política de **revogação com pagefile ativo** a
  fixar na SPEC *(Inferência; ver risco R8)*.
- **RF-6 — Ativação do pagefile secundário pós-boot.** Via `NtCreatePagingFile` no volume do
  disco virtual, com **tamanho e prioridade conservadores** e mantendo pagefile mínimo em `C:`
  [Confirmado em docs — §2]. Inclui verificação pós-update do Windows (smoke que detecta a
  regressão tipo ImDisk #38) *(mecânica do smoke: Inferência, a fixar na SPEC)*.
- **RF-7 — Teardown seguro e contenção de falha.** (a) Desativação **ordenada**: remover/desativar
  o pagefile → drenar I/O → destruir o disco → **wipe** da VRAM (`zero()`, reuso da disciplina
  DT-17 já existente — Confirmado no codebase) → `LeaseRelease`. (b) **Crash do serviço com
  pagefile ativo** tem comportamento definido no driver (falhar I/O pendente de forma
  determinística, nunca travar o storage stack) e validado por drill em VM — análogo ao
  SIGBUS-contido do Linux (§2, §14 #5). *(Comportamento exato do Windows nesse cenário é
  Inferência até o drill — ver §6 fluxo 4 e R7.)*
- **RF-8 — Instalador attestation-signed.** Driver (INF/CAT assinados via attestation) + serviço
  empacotados num instalador próprio (`.exe`/winget, padrão já previsto em
  `docs/memory-broker/VISION.md` — Confirmado em docs). Distribuição pelo próprio instalador —
  **não** Windows Update (attestation não vai pra WU pro público) [Confirmado em docs — §2].

## 5. Requisitos não-funcionais (RNF)

- **RNF-1 — Zero BSOD (gate de estabilidade).** Bug de driver de kernel vira BSOD, não crash
  contido — por isso: stress + **fuzzing do caminho de I/O** e do IOCTL surface, Driver Verifier
  ativo durante a validação, e **N horas de stress sem BSOD em VM** antes de tocar o host real
  (N numérico a fixar na SPEC). *(Ferramentas: prática padrão de driver Windows — Inferência
  leve; o requisito em si deriva do risco R1.)*
- **RNF-2 — Números, não adjetivos (latência/throughput).** Toda medição segue
  `.claude/rules/benchmarks.md` [Confirmado em docs]: ≥3 rodadas, mediana + p99 + desvio, tag
  `idle`/`loaded`, registro em `docs/BENCHMARKS.md` + `docs/benchmarks/results.jsonl`.
  Comparação **lado-a-lado na mesma janela**: pagefile-VRAM vs pagefile em disco no mesmo host.
  **Correção de gate (auditoria 2.5):** o dado real do Linux mostra que VRAM-swap (~241 µs) **NÃO
  bate NVMe saudável (~80 µs)** — só ganhou do VHDX-WSL2 anormalmente lento (~2114 µs)
  (`MEMORY.md` 2026-06-16, P0 §3). Num host Windows com SSD/NVMe real, VRAM-sobre-PCIe
  provavelmente **perde** em latência. Logo o gate de promoção **NÃO é "≥ paridade em latência"**
  (seria inconsistente com §2/R6, que posicionam VRAM como tier frio que não compete em latência).
  O gate honesto é: **(a) alívio de capacidade real** (o pagefile-VRAM absorve páginas que
  senão iriam pro disco/OOM, contadores de uso > 0 sob pressão) **e (b) não catastroficamente
  mais lento** (p99 de page-in dentro de um teto **Kx** o do disco — K a fixar na SPEC pela
  primeira medição), não "mais rápido que o disco". *(Limiares K: Inferência até medir.)*
  Honestidade: não compete com RAM (física, §2) nem com NVMe saudável (dado Linux); o valor é
  **capacidade** (VRAM ociosa vira tier de overflow), não velocidade.
- **RNF-3 — Day-0.** Protocolo de backend próprio e definitivo (RF-2); sem shim, sem camada de
  compatibilidade, sem protocolo alheio; cada superfície entregue na forma final [Confirmado em
  docs — política do `CLAUDE.md`].
- **RNF-4 — Validação de entrada na fronteira kernel.** Todo IOCTL do driver valida buffers,
  bounds e alinhamento antes de usar (equivalente Windows das validações `copy_from_user` que a
  SPEC SSDV3 exige); acesso à device interface de controle restrito ao serviço
  (SYSTEM/Administrators). *(Detalhe por handler: a fixar na SPEC.)*
- **RNF-5 — Lease revogável respeitado.** VRAM do tier Windows é emprestada e revogável como a de
  qualquer tenant [Confirmado em docs — VISION.md]; o serviço nunca "prende" o lease além do
  teardown; caminho de degradação com pagefile ativo documentado na SPEC (pior caso aceito e
  explícito — ver R8).
- **RNF-6 — Não-disruptivo no host vivo.** Pressão de memória pesada, fuzzing e drills de crash
  rodam **só em VM Windows isolada** — análogo à regra que proíbe thrash no WSL2 vivo
  [Confirmado em docs — `.claude/rules/benchmarks.md`]. O host real só recebe builds que passaram
  o gate de estabilidade em VM, e o primeiro uso é supervisionado.
- **RNF-7 — Assinatura de release.** Artefatos de release **attestation-signed** carregando com
  test-signing OFF (referência: build 26200.8655) [Confirmado em docs + empírico — §2];
  test-signing só em VM de desenvolvimento.
- **RNF-8 — Zero regressão no lado Linux.** O port da camada CUDA (RF-4) não muda o
  comportamento do caminho Linux/WSL2; smokes existentes continuam verdes.

## 6. Fluxos

1. **Provisionamento (start do serviço):** serviço inicia → lê config → `LeaseRequest{bytes}` ao
   broker → `LeaseGranted` → carrega `nvcuda.dll` + `alloc` da região (RF-4) → conecta ao driver
   e ativa o disco virtual (RF-1/RF-2) → volume NTFS disponível (formatação na primeira vez) →
   `NtCreatePagingFile` adiciona o pagefile secundário com tamanho/prioridade conservadores
   (RF-6) → telemetria ativa.
2. **Caminho quente (page-out/page-in):** processo sob pressão sustentada (ex.: Plex) → Memory
   Manager pagina pro pagefile secundário → storage stack → miniport enfileira o request →
   serviço consome via shared memory → `write_at` (HtoD) / `read_at` (DtoH) na VRAM → completion.
   Latência medida conforme RNF-2.
3. **Broker precisa da VRAM (ex.: render no mesmo host):** broker sinaliza revogação → serviço
   aplica a política fixada na SPEC (degradar/encolher/desativar o pagefile antes de devolver) →
   `LeaseRelease`. Enquanto a política não é trivial (pagefile ativo), o pior caso aceito fica
   documentado (R8).
4. **Pior caso — crash do serviço com pagefile ativo (Kahneman #5):** driver detecta backend
   morto → falha os I/Os pendentes de forma definida (RF-7) → **o que o Windows faz com erro de
   I/O em paging é pergunta de validação obrigatória em VM** (drill análogo ao
   `qemu-ublk-crash-e1b.sh` do Linux, que provou SIGBUS contido 5/5 — Confirmado em docs §2).
   Hipótese a testar: pode variar de "processos com páginas lá morrem" (análogo aceitável do
   SIGBUS) até bugcheck (`KERNEL_DATA_INPAGE_ERROR`) — se o drill mostrar bugcheck sem mitigação
   possível, dispara o counterfactual de aborto (§14 #2). *(Inferência até o drill.)*
5. **Desativação ordenada (stop/uninstall):** desativar o pagefile secundário (remoção a quente
   não é garantida pelo Windows; caminho conservador pode exigir reboot — a fixar na SPEC,
   *Inferência*) → drenar I/O → destruir disco → `zero()` (wipe da VRAM antes de devolver — o
   pagefile contém memória de processos) → `LeaseRelease` → serviço para.
6. **Update do Windows (fragilidade conhecida):** após update, smoke automático re-verifica disco
   + pagefile (RF-6); regressão tipo ImDisk #38 → desativa a feature graciosamente (pagefile só
   em disco) e loga — nunca degrada o boot do usuário.

## 7. Modelo de dados

- **`VirtualDisk{size_bytes, block_size, serial}`** — parâmetros do disco exposto pelo miniport
  (RF-1). *(Campos exatos: a fixar na SPEC.)*
- **`IoRequest{op: read|write|flush, offset, len}` + `IoCompletion{status}`** — unidade do
  protocolo de backend (RF-2), carregada via shared memory/IOCTL. *(Layout: a fixar na SPEC.)*
- **`Lease{holder, bytes, revocable}`** — reuso do modelo do broker [Confirmado no codebase —
  `ramshared-broker`]; o holder passa a poder ser o serviço Windows (novo `transport` de tenant,
  precedente do `DccAgent` da P2 — nome da variante a fixar na SPEC, *Inferência*).
- **`PagefileConfig{volume_path, min_bytes, max_bytes, priority}`** — mapeia os parâmetros de
  `NtCreatePagingFile` (RF-6); defaults conservadores derivados do orçamento real (§2: ~4–5 GiB
  líquidos na GPU-alvo).
- **Config TOML** — o TOML único da plataforma (RF-P3 da P2, Confirmado em docs) ganha a seção do
  tier Windows: `[win_drive] size, pagefile_min, pagefile_max, priority, broker, tenant`.
  *(Seção nova: Inferência, alinhada ao loader já especificado na P2.)*

## 8. API / Interfaces

- **Driver (kernel):** (a) interface **StorPort padrão** pro storage stack (o Windows enxerga um
  disco; nenhuma API pública nova desse lado); (b) **device interface de controle** dedicada pro
  serviço — IOCTLs + shared memory do protocolo de backend (RF-2), com validação por handler
  (RNF-4). GUIDs, códigos de IOCTL e layout das filas são definidos na SPEC. *(Inferência:
  detalhes; o formato "miniport + IOCTL" em si é o padrão provado pelo WinSpd — Confirmado em
  docs.)*
- **Serviço Windows (Rust):** crate novo no workspace (nome a fixar na SPEC; target
  `x86_64-pc-windows-msvc`, padrão já usado na P2 — Confirmado em docs). Reusa
  `ramshared-broker::{Msg, write_msg, read_msg}` como cliente/tenant (RF-5).
- **Camada CUDA (RF-4):** loader `nvcuda.dll` com a mesma tabela de símbolos do
  `ramshared-cuda` (§2), atrás do trait `VramProvider`/`VramMemory` [Confirmado no codebase].
  `unsafe` isolado no binding FFI (com `// SAFETY:`), superfície segura sem `unsafe` — padrão do
  crate atual.
- **`NtCreatePagingFile` (ntdll, não-documentada):** chamada isolada num módulo único, com guard
  por versão de build do Windows e **falha-graciosa** (sem pagefile ≠ sem disco: o volume
  continua utilizável; a feature degrada, não quebra) [restrição confirmada em docs — §2;
  guard/fallback: Inferência, a fixar na SPEC].
- **Instalador:** pacote de driver (INF/CAT attestation-signed) + serviço + CLI, `.exe`/winget
  [Confirmado em docs — VISION.md]. **Nenhuma distribuição via Windows Update** (§12).

## 9. Dependências e riscos

| # | Risco | Mitigação |
|---|---|---|
| R1 | **Kernel driver = um bug vira BSOD** (não crash de processo contido) | StorPort miniport (modelo mais restrito/testável que WDM); fuzzing do caminho de I/O + IOCTLs; Driver Verifier; gate de N horas em VM (RNF-1/RNF-6); HLK opcional futuro |
| R2 | **Esforço do do-zero é real** — WinSpd ("só" miniport+IOCTL) levou 1 pessoa até Beta e parou [Confirmado em docs] | estudar WinSpd + GpuRamDrive antes de codar; MVP mínimo primeiro (§10); estimativas com reference class explícita (Kahneman #4/#8) |
| R3 | **Attestation enquadrado como "testing scenarios"** — política futura pode apertar [Confirmado em docs] | plano B = WHCP completo (HLK); monitorar MS Learn; counterfactual de aborto em §14 #2 |
| R4 | **Pagefile secundário frágil a updates do Windows** (caso ImDisk #38) [Confirmado em docs] | smoke pós-update (RF-6, fluxo 6); degrade gracioso; telemetria |
| R5 | **`NtCreatePagingFile` é não-documentada** — assinatura/semântica podem mudar por build | módulo isolado com guard por versão + falha-graciosa (§8); teste por build suportado |
| R6 | **Latência: VRAM via PCIe (µs) ≫ RAM (ns)** — expectativa inflada mata a credibilidade [Confirmado em docs] | posicionar como tier frio/overflow (lição da Fase 0 Linux); gates comparativos honestos (RNF-2) |
| R7 | **Crash do backend com pagefile ativo = BSOD provável** (Passo 0 por pesquisa 2026-07-03: página de KERNEL perdida → `KERNEL_DATA_INPAGE_ERROR` 0x7a; RAM disks nem suportam pagefile por isso). PIOR que o SIGBUS-contido do Linux. **Risco elevado de MÉDIO→ALTO.** **↳ DRILL EMPÍRICO em VM (2026-07-03, ver §14 e runbook):** remoção-surpresa de disco com pagefile ativo carregando **~150-200 MB de páginas de USUÁRIO** foi **CONTIDA 3×** (guest perde o disco, sobrevive, sem BSOD/bugcheck) — o caso comum é análogo ao SIGBUS do Linux, **não** ao BSOD temido. O pior caso de **página de KERNEL** e escala de GB **não** foi reproduzível com stressor userspace → **permanece não-refutado**. Risco rebaixável a **MÉDIO** p/ workload de usuário; ALTO só p/ o vetor kernel-page. | teardown SEMPRE ordenado (desativar pagefile→migrar páginas→só então remover disco) = seguro; o vetor é morte NÃO-limpa do serviço. Mitigações: serviço à prova de crash + orientar pagefile-VRAM a páginas frias. Drill confirmou containment p/ user-pages; **o SPEC deve forçar/medir o caso kernel-page** (via nosso driver ou stressor de paged-pool) antes do Day-0 |
| R8 | **Revogação de lease com pagefile ativo** não tem caminho rápido óbvio | política explícita na SPEC (RF-5/RNF-5); pior caso aceito e documentado (revogação lenta) — nunca silencioso |
| R9 | **Dependência organizacional: EV cert + Partner Center** (custo ~US$ 200–400/ano + prazo de onboarding) [Confirmado em docs] | iniciar o trâmite em paralelo à SPEC (não bloqueia dev: test-signing em VM); orçamento aprovado antes da IMPL |

## 10. Estratégia de implementação

**Pré-requisito (gate anti-halo):** este PRD aprovado + SPEC aprovada via auditoria adversarial
"Passo 2.5" (Opus). **Zero código de driver antes disso.** Em paralelo (sem código): iniciar o
trâmite EV cert + Partner Center (R9).

Ordem (cada passo testável de forma independente, userspace antes de kernel):

0. **Fase 0 Windows — kill-test barato ANTES de escrever o driver (auditoria 2.5 / Kahneman #5
   / precedente "medir antes de codar" da Fase 0 Linux).** A maior incerteza da feature (§14 #1,
   R7) é: *o Windows sequer PERMITE um pagefile secundário num disco virtual respaldado por
   userspace, e o que ele faz quando esse backend morre com o pagefile ativo?* Isso pode ser
   respondido **sem escrever uma linha do nosso driver**, usando um disco virtual de prateleira
   (ex.: um RAM disk do ImDisk, trivial de instalar numa VM): (a) `NtCreatePagingFile` num volume
   de terceiro é aceito? (b) matar o backend do disco com o pagefile ativo → o Windows contém
   (processos com páginas lá morrem, análogo ao SIGBUS) ou dá **bugcheck**? Se der bugcheck sem
   mitigação, **a abordagem inteira morre aqui** — e economizamos todo o esforço do driver do-zero
   (R2). É o análogo exato da Fase 0 do Linux (mediu eviction WDDM em GPU real antes de construir
   qualquer coisa). *(Inferência sobre a viabilidade; o experimento é justamente o que a resolve.)*
1. **RF-4 — port CUDA** (`nvcuda.dll` loader atrás do `VramProvider`): testável **userspace-only**
   no host real (aloca, escreve, lê, mede HtoD/DtoH) — nenhum código de kernel envolvido; valida
   o pilar VRAM isoladamente e o RNF-8 (Linux intacto). Roda em paralelo à Fase 0.
2. **RF-3/RF-5 — serviço + tenant do broker:** lease e2e contra o broker existente (WSL2), VRAM
   backing local — ainda sem driver.
3. **RF-1/RF-2 — driver MVP em VM (test-signing):** disco virtual enumera → protocolo de backend
   → smoke de formatação NTFS → correção de I/O sob verificação (Driver Verifier) e fuzzing
   (RNF-1).
4. **RF-6 — pagefile secundário na VM:** `NtCreatePagingFile` + pressão sintética sustentada +
   **drill de crash do backend** (fluxo 4, RF-7) — o resultado do drill alimenta a SPEC/matriz de
   degradação antes de qualquer host real.
5. **RNF-2 — benchmarks** conforme `.claude/rules/benchmarks.md` (lado-a-lado vs pagefile em
   disco, mesma janela, ≥3 rodadas) em VM e depois no host.
6. **RF-8 — assinatura attestation + instalador:** submissão Partner Center; carga no host real
   (test-signing OFF) com primeiro uso supervisionado (RNF-6).

Disciplina: cada passo cita seu RF/RNF-ID nos commits (regra dura SSDV3 #4); IMPL.md por passo.

## 11. Documentos a atualizar

- `docs/windows-vram-drive/SPEC.md` — **próximo passo** deste PRD; `IMPL.md` por passo.
- `docs/memory-broker/PRD.md` §10/§12 — marcar o item "driver de swap Windows" (P4) como
  detalhado aqui; retirar do "fora de escopo" global.
- `docs/memory-broker/VISION.md` — a linha "fora de escopo por ora" (L28) passa a apontar pra
  este PRD.
- `docs/reliability/DEGRADATION-MATRIX.md` — novos modos de falha do lado Windows (crash do
  backend com pagefile ativo, update do Windows, revogação de lease).
- `docs/LIBRARIES.md` — toolchain de driver Windows (WDK) e deps novas do serviço.
- `docs/BENCHMARKS.md` + `docs/benchmarks/results.jsonl` — runs de validação (RNF-2).
- `README.md`/`ARCHITECTURE.md` — novo componente (Trilha 2); `MEMORY.md` — entrada de sessão.

## 12. Fora de escopo

Pagefile **primário**/boot-time (impossibilidade estrutural — `smss.exe`, §2); distribuição via
**Windows Update** e WHCP/HLK completo (fica como plano B registrado, não neste MVP); GPUs
**não-NVIDIA** (Vulkan/D3D12 → trilha P3; o trait `VramProvider` mantém a porta aberta);
interposer `nvcuda.dll` v2 (item P4 separado no doc-pai); modificar/instrumentar apps (o ponto é
transparência via SO); tiering RAM↔VRAM dentro do serviço Windows (MVP = VRAM only, como o tier
Linux); competir com RAM em latência (física, §2); auth/cripto própria (rede privada só, igual
P1/P2); compressão/dedup do conteúdo paginado.

## 13. Critérios de aceitação

1. **Gate de IMPL (anti-halo):** PRD aprovado + SPEC aprovada (auditoria adversarial "Passo 2.5"
   do Opus) **antes** de qualquer código de driver. Zero código antes disso.
2. **MVP mínimo demonstrável (e2e):** driver cria o disco virtual → serviço respalda com VRAM
   (leituras/escritas via `nvcuda.dll`) → `NtCreatePagingFile` ativa o pagefile secundário → um
   processo sob pressão sustentada **usa o pagefile-VRAM** (contadores de uso > 0) **sem BSOD**.
   Primeiro em VM, depois no host real supervisionado (RNF-6).
3. **Gates numéricos (RNF-2 corrigido pela auditoria 2.5, conforme benchmarks.md):** (a) p50/p99
   de leitura/escrita do disco-VRAM vs pagefile em disco, **mesma janela**, ≥3 rodadas — gate =
   **alívio de capacidade** (uso do pagefile-VRAM > 0 sob pressão, páginas que iriam pro
   disco/OOM) **+ p99 de page-in dentro de Kx o do disco** (K fixado na SPEC), NÃO "mais rápido
   que o disco" (VRAM perde pro NVMe saudável, dado Linux); (b) tempo de ativação do pagefile
   secundário medido (ms, do start do serviço ao pagefile ativo); (c) N horas de stress em VM sem
   BSOD (N fixado na SPEC).
4. **Drill do pior caso (Kahneman #5):** matar o serviço com pagefile ativo em VM → comportamento
   documentado + plano de teardown validado (RF-7). Se o resultado for bugcheck sem mitigação
   especificável, o counterfactual de §14 dispara (não promove).
5. **Assinatura:** driver attestation-signed **carrega** em Windows 11 25H2 (referência: build
   26200.8655) com test-signing OFF (RNF-7).
6. **Broker:** `LeaseRequest`/`LeaseRelease` observados em logs; no teardown a VRAM é **zerada**
   (`zero()`) e devolvida (RF-7); smokes do lado Linux continuam verdes (RNF-8).

## 14. Validação (Kahneman)

Referência: `docs/methodology/KAHNEMAN-DISCIPLINES.md`.

- **#3 — Número, não adjetivo:** nenhum claim "funciona/é rápido": os gates são latência de I/O
  medida (p50/p99, ≥3 rodadas, mesma janela que o concorrente), tempo de ativação do pagefile em
  ms, horas de stress sem BSOD e contadores de uso do pagefile — tudo registrado conforme
  `.claude/rules/benchmarks.md`.
- **#5 — Pior caso, não happy path:** o cenário dimensionante é **crash do serviço backend com
  pagefile ativo** — o que acontece com o Windows? O drill em VM (fluxo 4) é obrigatório antes do
  host, análogo ao experimento que provou o SIGBUS-contido no Linux
  (`qemu-ublk-crash-e1b.sh`, 5/5 — Confirmado em docs). O resultado entra na
  `DEGRADATION-MATRIX` e define o plano de teardown seguro (RF-7).
- **#2 — Counterfactual (o que faria abortar a feature):** (a) driver attestation-signed **deixar
  de carregar** em build estável do Windows (aperto de política) e o custo WHCP não se justificar
  → abortar/park; (b) pagefile secundário **instável demais** — quebra em updates consecutivos do
  Windows (padrão ImDisk #38) ou drill do pior caso mostra bugcheck sem mitigação → abortar; (c)
  pagefile-VRAM **perder** pro pagefile em disco em leitura (p50, mesma janela) → não promove pro
  host; fica registrado como experimento.
- **#1 — WYSIATI (o que NÃO foi visto):** nenhum StorPort miniport foi escrito ou carregado pelo
  projeto ainda; o comportamento do Memory Manager com pagefile em disco virtual respaldado por
  userspace **não foi verificado** nesta máquina; `NtCreatePagingFile` sobre volume de miniport
  próprio **não foi testada**. Essas lacunas são exatamente o que os passos 3–4 de §10 fecham
  antes de qualquer promoção.
- **#4/#8 — Reference class nas estimativas:** âncora explícita = WinSpd (1 pessoa, anos, parou
  em Beta). Estimativas de prazo da IMPL citam inside view + multiplicador de reference class.
- **#11 — Anti-halo:** o sucesso e2e do Linux (2026-07-03) **não aprova** a Trilha 2; cada gate
  desta trilha prova com números próprios, em VM primeiro.
