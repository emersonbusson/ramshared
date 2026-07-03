# Runbook — Passo 0 / Drill do pagefile em disco virtual (VM Windows)

> **Objetivo:** medir empiricamente o que o Windows faz quando o **backend de um disco virtual
> some com um pagefile secundário ativo** — contido (só processos de usuário morrem, análogo ao
> SIGBUS do Linux) ou **BSOD `KERNEL_DATA_INPAGE_ERROR` (0x7a)**. Isso resolve o risco R7 do
> `PRD.md` (o maior da feature) **sem escrever o driver do-zero** e **sem risco pro host real** —
> usando um disco virtual de prateleira (ImDisk) dentro de uma **VM Windows descartável**.
>
> **Contexto:** a pesquisa de 2026-07-03 (`MEMORY.md`) já indicou fortemente o risco de BSOD. Este
> drill **confirma ou refuta** empiricamente, e — crucialmente — testa se o caso **mediado por
> driver** (backend retorna erro de I/O, disco NÃO é fisicamente arrancado) é mais recuperável que
> o caso "disco arrancado" que a pesquisa cobriu.

## ⚠️ Regra dura de segurança

- **SÓ em VM Windows descartável, NUNCA no host real.** Uma tela azul aqui é o resultado esperado
  de um dos cenários — por isso VM. Snapshot ANTES de cada cenário destrutivo.
- Análogo exato da regra do Linux: crash-drill só em qemu/VM, nunca no WSL2 vivo (`benchmarks.md`).

## Pré-requisitos (host — precisa de você/admin)

1. **Hyper-V completo** habilitado (o WSL2 usa um subconjunto; o drill precisa do gerenciamento
   completo): PowerShell admin → `Enable-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V-All`
   (pode pedir reboot). Recursos já confirmados suficientes: C:\ ~136 GB livres, 32 GB RAM.
2. **ISO do Windows** (grátis): "Windows 11 Enterprise Evaluation" (90 dias) do site da Microsoft
   (Evaluation Center) — ~5–6 GB. Não precisa licença paga pro drill.
3. **VM Hyper-V**: 4 GB RAM, 40 GB disco, 2 vCPU. Instalar o Windows normalmente. **NÃO** precisa de
   GPU/CUDA na VM — o drill testa o COMPORTAMENTO DO PAGEFILE, não a VRAM (a VRAM só entra no
   produto real; aqui um RAM disk substitui como "backend volátil que pode sumir").
4. Dentro da VM: instalar **ImDisk Toolkit** (imdisk-toolkit no GitHub / sourceforge) — dá o RAM
   disk + a ferramenta de disco virtual controlável.

## Cenários (rode cada um a partir de um snapshot limpo)

### Cenário A — o Windows aceita pagefile secundário em disco de terceiro?
1. Na VM: criar um RAM disk ImDisk de ~2 GB, formatado NTFS, com letra (ex.: `R:`).
2. Painel de Controle → Sistema → Configurações avançadas → Desempenho → Memória Virtual →
   adicionar um pagefile gerenciado em `R:` (ex.: 1024–2048 MB). Aplicar.
3. `Get-CimInstance Win32_PageFileUsage` (ou reabrir o painel) → confirmar que `R:\pagefile.sys`
   aparece **ativo**.
   - **PASS-A:** o Windows aceitou o pagefile no disco de terceiro.
   - **FAIL-A:** o Windows recusa → a abordagem "pagefile secundário em disco nosso" morre aqui
     (nem precisa do cenário B). Registrar a mensagem exata.

### Cenário B (DECISIVO) — o que acontece quando o backend some com o pagefile ativo?
> **Snapshot antes.** Este cenário PODE dar tela azul de propósito.
1. Com o pagefile em `R:` ativo (cenário A), gerar **pressão de memória sustentada** na VM até o
   Windows de fato paginar pra `R:` (ex.: um alocador que consome > RAM da VM; confirmar via
   `Get-Counter '\Paging File(*)\% Usage'` mostrando uso em `R:` > 0).
2. **Simular a morte do backend** de duas formas (rodar as duas, cada uma de um snapshot):
   - **B1 — remoção abrupta:** `imdisk -d -m R:` (força a remoção do RAM disk) enquanto o pagefile
     está ativo e com uso > 0. = análogo a "disco arrancado".
   - **B2 — I/O error mediado (mais perto do nosso caso):** se o ImDisk tiver modo proxy, matar o
     processo servidor do proxy (o disco "existe" mas todo I/O falha) — é o cenário mais fiel ao
     nosso (StorPort miniport vivo, backend userspace morto). Se não der pra reproduzir B2 com
     ImDisk, registrar como "não testável sem o nosso driver" (aí o driver MVP precisa existir pro
     B2 real).
3. Observar (a VM tem `WSL_ENABLE_CRASH_DUMP`-equivalente? não — mas o Hyper-V permite ver a tela):
   - **Contido (bom):** só o(s) processo(s) que tinham páginas em `R:` morrem; a VM continua viva e
     responsiva. = análogo ao SIGBUS do Linux → a feature é viável com mitigações.
   - **BSOD (ruim):** tela azul `KERNEL_DATA_INPAGE_ERROR` (0x7a) ou similar → o pior caso é real;
     dispara o counterfactual do PRD §14 #2b.

## Critério de decisão (o que cada resultado significa pro projeto)

| Resultado | Significado | Ação (PRD) |
| --- | --- | --- |
| FAIL-A | Windows nem aceita pagefile em disco nosso | **Aborta** o caminho transparente; só resta app-opt-in |
| PASS-A + B contido (B1 e/ou B2) | Falha de backend é contida como o Linux | **GO** — segue pro driver MVP com mitigações (prio baixa, teardown ordenado) |
| PASS-A + B1 BSOD, B2 contido | "Disco arrancado" mata, mas "I/O error mediado" (nosso caso) não | **GO condicional** — o driver DEVE segurar/errar I/O do jeito B2; nunca deixar o disco sumir |
| PASS-A + B BSOD nos dois | Pior caso confirmado, não-mitigável só por driver | **Reavaliar**: aceitar como recurso experimental de risco consciente (prio baixíssima) OU pivotar pra app-opt-in |

## Registro

- Resultado (A, B1, B2) + prints/mensagens em `docs/BENCHMARKS.md` e entrada no `MEMORY.md`.
- Este drill é o **gate** do §10 Passo 0 do PRD: nenhum código de driver do-zero antes do resultado.

## Resultado (executado 2026-07-03)

**Ambiente:** VM Hyper-V `win11-drill` (Windows 11 Pro 25H2 pt-BR, 4 GB→2 GB RAM, Secure Boot OFF +
test-signing), instalada headless via `autounattend.xml` + Setup nativo; automação 100% por
**PowerShell Direct** (sem tela/rede). Backend volátil = **VHDX de 5 GB hot-removable** em SCSI 0:1
(substituindo o RAM disk — ImDisk foi **abandonado**: método de instalação `InstallHinfSection`
restrito no Win11 moderno + CLI com DLL faltando `0xC0000135`). Scripts em
`C:\Users\emedev\ramshared-drill\` (host).

| Cenário | Resultado | Evidência |
| --- | --- | --- |
| **A** — Windows aceita pagefile secundário no disco de backend? | **PASS-A** | `E:\pagefile.sys` alloc=4096 MB **ativo** após reboot (`Win32_PageFileUsage`), junto com/no lugar do de C: |
| **B1** — disco some (hot-remove) com pagefile ativo | **CONTIDO (3×: 194 MB@4GB, 178 MB@2GB)** | disco removido do host **e** do guest (`Test-Path E:\` = False); guest **responsivo** 120 s; **sem** `BugCheck 1001`/`MEMORY.DMP`; sem reboot |
| **B2** — erro de I/O mediado por driver (backend vivo, I/O falha) | **NÃO TESTÁVEL** sem o nosso driver | fica pro MVP do driver |

**Veredito → tabela de decisão:** linha **"PASS-A + B contido"** ⇒ **GO** (segue pro driver MVP com
mitigações) — **PORÉM** com ressalva forte, porque o drill **não** cobriu o pior caso:

- Páginas ativas no disco arrancado foram **~150-200 MB de USUÁRIO**, não escala de GB nem
  **página de kernel** (paged pool). A pesquisa do Passo 0 alertava que o BSOD
  `KERNEL_DATA_INPAGE_ERROR` vem de **página de kernel** perdida — vetor **não reproduzível** com um
  stressor userspace. ⇒ **user-workload = contido (empírico); kernel-page = não-refutado.**
- **Achado de método (importante p/ o SPEC do stressor):** a *Memory Compression* do Win11 comprime
  páginas na RAM e **mascara** a paginação quando o dado é compressível (pagefile ficava em ~2 MB
  mesmo sob pressão). Só **dado incompressível** (`[System.Security.Cryptography.RandomNumberGenerator]::GetBytes`,
  velocidade nativa) forçou páginas reais ao `E:`. O SPEC do teste kernel-page deve prever isto.

**Ação no PRD:** R7 rebaixado a MÉDIO p/ user-workload (ALTO só p/ kernel-page); §14/§Passo-0 ganharam
bloco "RESULTADO EMPÍRICO"; SPEC deve incluir teste que **force paged-pool/kernel-page** (via o nosso
driver) antes do Day-0.
