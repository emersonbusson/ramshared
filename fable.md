# Divisão de trabalho por modelo — RamShared

> Relatório de apoio à orquestração multi-modelo. Não é SPEC nem PRD (SSDV3 continua
> sendo o pipeline obrigatório para IMPL de locks/DMA/mm/uAPI — ver
> `.claude/rules/ssdv3.md`); é um guia de "quem deveria puxar o quê" entre
> Fable 5, Opus 4.8 e Sonnet 5, baseado no backlog real encontrado numa pesquisa de
> 2026-07-02 sobre o que falta para o projeto "funcionar de forma perfeita".

## Ressalva de calibragem

Não existe, nas regras deste ambiente nem em documentação pública acessível daqui, um
perfil de capacidades específico para o **Fable 5** — é um modelo novo (família Claude 5,
par do Sonnet 5). As atribuições de Fable abaixo são inferência a partir do formato da
tarefa (redação técnica estruturada, PT-BR, alto volume de texto narrativo), não de
benchmark publicado. Ajuste livremente se houver informação melhor.

Para Opus e Sonnet, a divisão segue o padrão já documentado no projeto
(`~/.claude/rules/*/performance.md`, mapeado da geração anterior para a atual):
**Opus** = raciocínio mais profundo, decisões arquiteturais, análise; **Sonnet** =
melhor modelo de código, execução principal.

## O problema nº1: crash-safety do daemon `ublk` sob terminação não-graciosa

Achado central da pesquisa de 2026-07-02: é a única parte do RamShared cujo modo de
falha já foi um **freeze do WSL2 inteiro** (`MEMORY.md:883-901`), não só a queda de um
processo. Layout de ABI do kernel (`crates/ramshared-wsl2d/src/ublk.rs`) verificado à
mão via `cc`, sem `bindgen`; ponteiros estacionados no kernel entre submissão
`io_uring` e CQE (`crates/ramshared-uring/src/lib.rs:268-299`).

> **Atualização 2026-07-03 (Opus concluiu o passo 1 abaixo):** a formulação original
> desta seção — "SIGKILL/crash/OOM-kill continua sem mitigação" (`MEMORY.md:939`) —
> **estava incorreta**, e a correção é o próprio resultado do trabalho do Opus. Ele leu
> o código-fonte real do kernel WSL2 (`/home/emdev/WSL2-Linux-Kernel/drivers/block/ublk_drv.c`,
> disponível localmente) em vez de confiar na inferência do MEMORY.md, e achou que o
> kernel **já** tem recuperação automática: `ublk_daemon_monitor_work`
> (`ublk_drv.c:1486-1516`) faz poll a cada 5s (`ublk_drv.c:147`), detecta o daemon morto
> via `PF_EXITING`, aborta a I/O em voo (`ublk_abort_queue`, `ublk_drv.c:1461-1484`) e
> remove o device sozinho (`ublk_stop_dev`→`del_gendisk`, `ublk_drv.c:1637-1656`). A
> afirmação do MEMORY.md nunca foi testada de fato — viés de WYSIATI (disciplina
> Kahneman #1), não fato verificado. Consequência prática: um reaper/watchdog userspace
> (a mitigação óbvia, cotada abaixo) **não agregaria nada**, porque `DEL_DEV` passa pelo
> mesmo `del_gendisk()` que o monitor do kernel já aciona sozinho — construir isso às
> cegas violaria a regra de "reuso antes de criação" do próprio SSDV3.
>
> O que continua genuinamente em aberto (nunca testado): se esse monitor **completa**
> sob swap armado + pressão real de memória, ou se trava no `del_gendisk` do mesmo jeito
> que o incidente histórico — só um experimento decisivo em qemu (E1, projetado pelo
> Opus, extensão de `scripts/kernel/qemu-ublk-daemon.sh`) decide. Detalhe completo na
> resposta do agente Opus desta sessão; PRD/SPEC formal (passo Fable, abaixo) fica
> **gated** nesse experimento — SSDV3 aqui é explícito: "nada de código de produto antes
> da Fatia 0".
>
> **Atualização 2026-07-03 (E1 rodado, 3x — `scripts/kernel/qemu-ublk-crash-e1.sh`,
> script novo, não mexe no smoke já validado): o resultado é MAIS GRAVE do que o Ramo A
> do Opus previa, não mais leve.** Cenário real (swap armado + tmpfs cheio até
> `MemFree≈4 MB` + `SIGKILL` durante I/O de swap em voo, confirmado por
> `KTEST-SWAP-ACTIVATED`/`KTEST-DD-ALIVE-AT-DECISION`), 3 rodadas (regra
> `.claude/rules/benchmarks.md` — "1 amostra mente"):
>
> | Rodada | Swap usado no kill | Device sumiu em | Resultado |
> |---|---|---|---|
> | 1 | 17.152 KB | 4,47s | **Kernel panic** |
> | 2 | 9.984 KB | 2,91s | Limpo (Ramo A) |
> | 3 | 12.544 KB | 4,94s | **Kernel panic** |
>
> A remoção do device em si foi rápida e consistente nas 3 (confirma o `monitor_work`
> como o Opus previu). Mas o mecanismo de falha real: ~4-5s depois do device sumir,
> algum processo precisa reler uma página que tinha sido despejada pro `/dev/ublkb0` —
> o kernel reporta `Read-error on swap-device` (4x, confirmado no serial log completo) —
> e **isso derrubou o PID 1 da VM de teste**, virando `panic: Attempted to kill init!`.
> Confirmado que não é bug do harness: o mesmo trecho de shell rodou sem erro minutos
> antes no mesmo script; só quebra especificamente no acesso pós-morte-do-device.
> **2 de 3 rodadas — kernel panic.** Isso é uma condição de corrida, não determinística,
> e por isso mais perigosa de avaliar/confiar do que um resultado binário limpo teria
> sido: a perda de página vira uma "mina-terrestre" que pode atingir qualquer processo
> que a toque depois, não só o dono original — o `SIGBUS` contido que o Ramo A do Opus
> presumia é otimista demais.
>
> **Ressalva honesta:** a VM de teste é minimalista (árvore de processos de 1, o próprio
> PID 1 disputa a mesma memória pressionada) — isso pode estar inflando a chance de PID 1
> especificamente ser a vítima, comparado a um host real com muito mais RAM/processos
> isolados. O mecanismo (I/O error ao reler página de swap de device morto) é real e
> generalizável de qualquer forma; só a identidade de "quem sofre" pode variar.
> **Conclusão prática (na hora): NÃO escrever "Ramo A, guarda barata" ainda — precisa
> investigar se isto é artefato do meu desenho de teste ou risco real.** Ver atualização
> seguinte, que resolve essa dúvida.

> **Atualização 2026-07-03 (investigação profunda com Opus — RESOLVIDO): era artefato do
> teste. O "Ramo A: SIGBUS contido" estava certo desde o início.** Duas evidências
> independentes e convergentes:
>
> 1. **Leitura do código-fonte do kernel** (`mm/memory.c` `do_swap_page`, `mm/page_io.c`
>    `swap_readpage`/`__end_swap_bio_read`, `mm/shmem.c`, `arch/x86/mm/fault.c`
>    `do_sigbus`): um erro de leitura de swap SEMPRE resulta em `VM_FAULT_SIGBUS`
>    entregue só ao processo que causou o fault (`force_sig_fault(SIGBUS, ...)` pra
>    `current`), com unwind limpo (sem lock vazado, sem D-state). Não existe caminho de
>    código de "página de swap perdida" para algo sistêmico.
> 2. **Experimento refeito, isolando a vítima do PID 1**
>    (`scripts/kernel/qemu-ublk-crash-e1b.sh`, script novo — os dois anteriores
>    permanecem intactos): vítima roda num cgroup v2 com `memory.max` apertado (força
>    expulsão REAL da página, não só um blip), PID 1 nunca é candidato a reclaim, um
>    "bystander" testemunha containment. **3 rodadas oficiais + 2 confirmatórias, 5/5
>    determinísticas: `Read-error on swap-device` idêntico ao E1, mas a vítima recebe
>    `SIGBUS` e sai limpa (`exit=42`), PID 1 e o bystander seguem rodando normalmente.
>    Zero panics, zero hung_task, em nenhuma das 5.**
>
> **Achado lateral relevante:** as 2 primeiras tentativas do E1b deram "NO-FAULT" —
> mesmo sob pressão global severa, a releitura ainda vinha do swapcache em RAM (o
> `do_swap_page` checa o swapcache antes do device). Só forçar expulsão real via
> cgroup produziu o teste válido. Ou seja: a "mina-terrestre" só detona se a página foi
> genuinamente expulsa da RAM — sob pressão sustentada real, não um pico passageiro.
>
> **Contraste que prova que era artefato:** mesma falha de I/O (`Read-error on
> swap-device`), mesmo device, mesmo daemon — só muda QUEM segura a página perdida.
> Vítima=PID 1 (E1, teste antigo com árvore de 1 processo) → 2/3 panic. Vítima=processo
> comum isolado por cgroup (E1b) → 0/3 panic, 3/3 contido. "Attempted to kill init!" é
> o comportamento padrão do Linux pra QUALQUER morte de PID 1, não uma propriedade do
> ublk — meu desenho original do E1 sem querer fez o próprio script-driver competir
> pela memória pressionada, o que nunca aconteceria num host real (PID 1 real —
> systemd/init do WSL2 — é um processo completamente separado dos processos-tenant que
> de fato usariam a VRAM-swap).
>
> **Recomendação de arquitetura final** (confiança alta, calibrada — ver relatório
> completo do agente para o detalhamento Kahneman):
> 1. Reaper/watchdog **continua sem valor** — o `monitor_work` do kernel já resolve
>    (device some em 1-5s, reconfirmado 5x).
> 2. **Alavanca real: prioridade de swap BAIXA** (`pri=`) pra VRAM/ublk-swap, garantindo
>    que ela só absorve overflow frio (barato de perder), não páginas "quentes".
> 3. **Contrato explícito a documentar**: crash do daemon → `SIGBUS` pros
>    processos-tenant com páginas expulsas pra lá. Aceitável pra tier de
>    scratch/overflow; não pra estado insubstituível.
> 4. Guarda opcional de baixo valor: monitor de `dmesg` por "Read-error on
>    swap-device" que faz `swapoff` proativo do device morto (não salva página
>    perdida, mas impede a ferida de crescer). Nice-to-have, não bloqueante.
> 5. **Nada de mudança de kernel, shim, ou rework do ponteiro-em-voo do io_uring** —
>    pra este modo de falha específico o kernel já falha-seguro.
>
> **Pergunta que sobra — é de produto, não de kernel, e é sua:** é aceitável, pra tese
> do RamShared, que um crash do daemon dê `SIGBUS` em processos-tenant com páginas lá?
> Isso não é mais um bloqueio técnico — é decisão de escopo (junto com o "fosso não
> validado" do Backlog nº3 abaixo).

### Opus 4.8 — decisão de arquitetura — ✅ concluído em 2026-07-03
- ~~Projetar o mecanismo de crash-safety~~ → feito, 2 passadas. 1ª: achou o
  `monitor_work` do kernel, recomendou gate em experimento (E1). 2ª (após o E1 dar 2/3
  kernel panic): investigou se era artefato de teste ou risco real, via leitura de
  código do kernel + experimento isolado (E1b, 5/5 determinístico) — **confirmou
  artefato**. Veredito final: SIGBUS contido, "Ramo A" estava certo. Recomendação de
  arquitetura final documentada acima (prioridade baixa + contrato documentado, sem
  reaper/sem mudança de kernel).
- Servir de **auditor adversarial "Passo 2.5"** da SPEC que sair da decisão final — é o
  mesmo papel que já pegou os 3 CRITICAL + 3 HIGH do SPEC.md da P2 Windows
  (`docs/memory-broker-p2-windows/SPECv2.md:1-24`) antes de virar código.

### Fable 5 — PRD/SPEC narrativo — pronto para começar
- **Já não é mais condicional** — o E1/E1b resolveram a incerteza (SIGBUS contido,
  confirmado por código + experimento). Redigir o PRD (14 seções fixas, PT-BR, template
  de `docs/SSDV3-PROMPTS.md`, slug `docs/ublk-teardown-crash-safety/`) com a
  recomendação final: prioridade de swap baixa + contrato de disponibilidade
  documentado + guarda opcional de `dmesg`. Incluir a "pergunta que sobra" (§ acima)
  como risco de escopo explícito na seção 9 (Dependências e riscos), não como bloqueio
  de IMPL — a decisão do PRD não depende dela.

### Sonnet 5 — implementação
- **Prioridade agora (E1, o experimento decisivo):** estender
  `scripts/kernel/qemu-ublk-daemon.sh` com o cenário que nunca foi testado — device
  armado como swap + pressão real de memória + `SIGKILL` (não `SIGTERM`) no daemon com
  I/O em voo, dentro da VM efêmera já existente (nunca no host — regra dura de
  `.claude/rules/benchmarks.md:23` e `guard_not_wsl2()` em
  `crates/ramshared-wsl2d/src/main.rs:1050-1058`). Mede se `/dev/ublkbN` some em ≤10s
  com a VM responsiva (Ramo A) ou trava >60s / `hung_task` (Ramo B — reproduz o freeze
  histórico isolado). Este experimento decide todo o resto — nenhuma mitigação deve ser
  escrita antes dele.
- Fechar o gap de verificação de ABI (independente de E1): teste automatizado (via
  `bindgen` ou script `cc` rodado em CI) que falhe se os offsets de
  `Params`/`CtrlCmd`/`IoDesc` em `ublk.rs` divergirem de
  `include/uapi/linux/ublk_cmd.h` do kernel WSL2 real.
- Implementar a mitigação (se E1 indicar que uma é necessária) só depois do SPEC
  pós-E1 aprovado — não antes.

## Backlog nº2: ponte Windows P2 (`docs/memory-broker-p2-windows/`)

SPEC teve **no-go** (3 CRITICAL + 3 HIGH — código assumido no lugar errado, `match`
exaustivo que quebraria compilação, "mover loop inalterado" que era falso), já
corrigido na `SPECv2.md`. 28 arquivos mapeados, **zero implementados**. Gate formal:
*"a IMPL não inicia sem os inputs do Alex"* (`SPECv2.md:23`, dados reais de uma cena
`.blend` do tester externo) — este bloqueio é humano/externo, nenhum modelo o destrava.

### Sonnet 5 — pronto para disparar assim que o gate abrir
- Os 28 arquivos da `SPECv2.md` (`ramshared-nvml`, `ramshared-config`,
  `client.rs`/`win_mem.rs`/`local.rs`, binário `ramshared-agent-win`, addon Blender)
  são implementação mecânica uma vez que o SPEC já foi auditado e aprovado — perfil de
  execução, não de decisão.

### Fable 5 — enquanto o gate não abre
- Preparar de antemão a documentação que não depende do dado do Alex: rascunho do
  `IMPL.md`, texto do PR (`.claude/rules/governance.md` — 7 seções obrigatórias,
  tabela de commits com `<details>` por linha), e sincronizar `CLAUDE.md`/`AGENTS.md`
  se algo mudar (regra de sync do `governance.md`).

### Opus 4.8 — se o dado do Alex vier e não bastar
- O próprio PRD já é honesto sobre o risco (`docs/memory-broker-p2-windows/PRD.md`,
  P2-R2: *"honesto se não destravar"*) — se o out-of-core nativo do Cycles não resolver
  a cena real do Alex, é Opus quem decide entre MVP-basta e ir para v2.

## Backlog nº3: validar o "fosso" de valor (risco mais profundo, não é bug)

Autocrítica registrada pelo próprio time (`MEMORY.md:1685-1690`): VRAM-swap só vence o
"NVMe" porque o disco deste ambiente (WSL2/VHDX) é anormalmente lento; NVIDIA/WDDM/AMD
já resolvem "GPU transborda pra RAM" no driver. O que sobra como diferencial —
arbitragem revogável de VRAM entre múltiplos tenants — **ainda não tem validação
empírica sob carga real** (`docs/BENCHMARKS.md`: falta o "decisivo Q1d", comparação
apples-to-apples sob pressão controlada).

### Opus 4.8
- Desenhar o experimento decisivo: controlar confounds, decidir o que conta como
  "vitória" do fosso, e interpretar se o resultado é direcional ou definitivo. É uma
  pergunta de tese de produto, não de implementação — cabe no papel de "pesquisa e
  análise" do Opus.

### Sonnet 5
- Executar a bateria de benchmarks desenhada pelo Opus, seguindo
  `.claude/rules/benchmarks.md` (≥3 rodadas, mediana+p99+desvio, contexto automático,
  saída dupla JSONL+MD append-only).

## Backlog nº4: manutenção rápida (bom primeiro teste de calibragem para o Fable)

`README.md:52` e `ARCHITECTURE.md:43` afirmam que `ramshared-cuda` é o único crate com
`unsafe` — falso hoje: `ramshared-vulkan` tem 38 blocos `unsafe` (quase empatado com os
41 de `ramshared-cuda`). Tarefa pequena, bem definida, baixo risco de regressão —
**boa primeira tarefa real para calibrar o Fable 5 no projeto** antes de dar a ele
trabalho maior de PRD/SPEC.

### Fable 5
- Corrigir `README.md:52` e `ARCHITECTURE.md:43` para refletir os dois crates com
  `unsafe` concentrado, com as contagens corretas.

## Backlog nº5: RamShared como swap real (Trilha 1 WSL2 + Trilha 2 Windows/Plex)

Auditoria de 2026-07-03: o tier VRAM-como-swap do WSL2, já validado exaustivamente
(Fase B PASS + crash-safety confirmado nesta sessão), **nunca rodou como serviço real**
nesta máquina (`ps aux` sem `ramsharedd`, `/dev/ublk*` ausente). Separado disso, o
usuário quer que "qualquer coisa" no Windows (ex: Plex Media Server) se beneficie de
swap-para-VRAM, como o pagefile em disco já beneficia hoje — isso é uma lacuna
arquitetural real e maior, já documentada como adiada em `docs/memory-broker/VISION.md:28`
("Windows-como-consumidor-de-swap exigiria um driver de disco Windows") mas nunca
pesquisada tecnicamente até agora.

### Sonnet 5 — Trilha 1 (WSL2) — ✅ CONCLUÍDA 2026-07-03

Houve 2 travamentos no caminho, ambos resolvidos:
- **#1** (`kernel BUG mm/memory.c:2345`): `mlockall(MCL_FUTURE)` pré-populava a VMA que o
  `dxg_map_iospace` do dxgkrnl remapeava → colisão de PTE. **Fix (definitivo, com catch de
  race):** o caminho ublk+vram usa **só `MCL_CURRENT`, nunca `MCL_FUTURE`** — o worker faz a
  init CUDA numa thread async, então "armar MCL_FUTURE depois" teria race; MCL_CURRENT não
  afeta mmaps futuros → zero colisão por construção.
- **#2** (~04:00): investigado — NÃO foi o daemon (nunca rodou) nem o host (sem Kernel-Power
  41); foi um `wsl --shutdown`. Motivou o sistema de black-box.
- **Resultado:** VRAM-swap validada end-to-end no host vivo (start→CUDA→alloc 512 MiB→
  `/dev/ublkb0`→swapon→teardown limpo), **sem freeze**. Rodando agora (`prio -3`). Sistema de
  segurança/black-box em `scripts/safety/` (ver `docs/reliability/BLACK-BOX-FORENSICS.md`).
- Pendente: auto-start no boot (gated na pergunta kernel 6.6→6.18 pós-restart) + blindagem
  anti-deadlock completa (mlock cirúrgico dos buffers de I/O, hoje só MCL_CURRENT).

### Opus 4.8 — Trilha 2, decisão de arquitetura — ✅ RESOLVIDA 2026-07-03

- **Assinatura resolvida (fonte primária + empírico no Windows real):** a política de
  abril/2026 mata só **cross-signed** (mecanismo antigo), NÃO attestation. Um driver
  attestation-signed **carrega e é confiável por padrão** no Win11 25H2 (confirmado na
  máquina real: build 26200.8655, test-signing off; e na doc MS Learn atualizada 2026-04-14).
  Custo: EV cert (~US$200-400/ano) + Partner Center, SEM testes HLK. Ver MEMORY.md.
- **DECISÃO DE ARQUITETURA: driver StorPort virtual miniport DO ZERO (Day-0), informado por
  referências.** Como a assinatura custa o mesmo pros 3 caminhos (attestation), o argumento
  "reuso reduz risco de assinatura" caiu — sobram Day-0 + licença livre + fit exato a favor do
  do-zero. Mitiga o esforço estudando **WinSpd** (StorPort miniport de verdade; licença
  GPLv3+exceção permite ESTUDAR o padrão sem copiar) e usando **GpuRamDrive** como prova de que
  o backing CUDA/VRAM funciona. Descartados: fork ImDisk (WDM legado + GPL viral, contra Day-0)
  e reviver WinSpd (adotar codebase morto de 5 anos). Escopo travado: pagefile só **secundário**
  (`NtCreatePagingFile` pós-boot); reusa a lógica CUDA do `ramshared-cuda` (portar
  `dlopen`→`LoadLibrary nvcuda.dll`) e o protocolo do broker.

### Fable 5 — Trilha 2, PRD formal — ✅ ESCRITO + AUDITADO + GATE PASSO 0 EXECUTADO 2026-07-03
- `docs/windows-vram-drive/PRD.md` (14 seções SSDV3, PT-BR) escrito e auditado (Passo 2.5 = GO).
- **Gate Passo 0 (drill em VM) EXECUTADO de verdade** (não mais só pesquisa): montei uma VM Windows 11
  Pro headless do zero (`autounattend.xml` + Setup nativo + PowerShell Direct; ver `MEMORY.md`
  2026-07-03 cont.) e rodei os cenários com um **VHDX de backend hot-removable**:
  - **A = PASS-A** (Windows aceita pagefile secundário em disco removível).
  - **B1 = CONTIDO 3×** (~150-200 MB de páginas de usuário no disco arrancado → sem BSOD; análogo ao
    SIGBUS do Linux). Ressalva: pior caso **kernel-page** e escala de GB **não-refutados** (stressor
    userspace não força página de kernel); **B2** (I/O mediado por driver) só testável com o nosso driver.
  - **Efeito:** R7 rebaixado a MÉDIO p/ user-workload (ALTO só kernel-page); o caminho transparente
    segue **viável**; o SPEC deve incluir um teste que force paged-pool/kernel-page antes do Day-0.
  - Detalhe em `PASSO0-DRILL-RUNBOOK.md §Resultado` e `PRD.md §Passo-0 empírico`.

### Próximo (pós-gate) — SPEC do driver Windows (SSDV3 Passo 2)
- Destravado agora que o Passo 0 deu GO-com-ressalva. O SPEC precisa: (a) teste kernel-page (força
  paged-pool na VRAM e mata o backend — mede se é o BSOD `0x7a` temido) usando dado incompressível;
  (b) teardown ordenado como invariante; (c) `NtCreatePagingFile` guard-por-build. Candidato a Opus
  (decisão de lock/DMA/uAPI) + Fable (prosa).

## Como orquestrar na prática

Sequência sugerida para o problema nº1 (crash-safety do `ublk`):

1. `Agent(model: "opus", ...)` — decisão de arquitetura + PRD/SPEC skeleton.
2. `Agent(model: "fable", ...)` — PRD/SPEC completo em PT-BR a partir da decisão do Opus.
3. `Agent(model: "opus", ...)` — auditoria adversarial Passo 2.5 do SPEC antes do IMPL.
4. `Agent(model: "sonnet", ...)` — IMPL (teste de ABI, drill qemu SIGKILL, mitigação).
5. `Agent(model: "fable", ...)` — MEMORY.md, IMPL.md, texto do PR.

Cada chamada é independente e pode rodar em `isolation: "worktree"` quando mexer em
código, para não conflitar com trabalho em paralelo.
