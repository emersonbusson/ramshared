---
slug: zram-writeback-vram
title: zram writeback do frio direto na VRAM (Fase B)
milestone: —
issues: [3]
---
# PRD — Fase B — zram writeback do frio direto na VRAM

> **Status: DESIGN-ONLY (Fase B, kernel-gated).** Verificado: este kernel WSL2
> (`6.6.114.1-microsoft-standard-WSL2`) **não tem** `CONFIG_ZRAM_WRITEBACK` (`# ... is not set`).
> IMPL e validação exigem **kernel custom**. Este PRD fecha o desenho; o Passo 3 fica para
> quando houver kernel com o config. Itens marcados **Inferência** não puderam ser testados.

## Resumo

Hoje a cascata trata zram e VRAM como **tiers de swap separados** (`zram 200 > nbd 100 >
VHDX -2`): uma página fria desce de zram para a VRAM **re-swapando** por outro device NBD —
caminho que passa por userspace (daemon) a cada página fria. A feature usa o **writeback
nativo do zram** (`CONFIG_ZRAM_WRITEBACK`): zram escreve as páginas **idle/incompressíveis**
direto para um `backing_dev` — apontado para a **VRAM** — sem o page-in/page-out extra por um
2º device de swap. Reduz cópias no caminho frio e libera RAM do zram mais cedo.

Valor: menos amplificação de I/O no caminho frio (Confirmado em docs: ROADMAP Fase B) e RAM do
zram liberada sob pressão; mantém a VRAM como destino frio (alinhado ao SPECv3-WSL2).

## Contexto técnico

- **Confirmado no codebase:**
  - Cascata atual: `crates/ramshared-cli/src/cascade.rs` monta `zram` (lzo-rle, prio 200) e
    `nbd0` (VRAM, prio 100) como **swaps independentes**. Sem `backing_dev`.
  - `docs/LIBRARIES.md` lista **zram-writeback** em "deliberadamente NÃO usado — exige
    `CONFIG_ZRAM_WRITEBACK` (kernel custom); cascata por prioridade resolve Day-0".
  - VRAM exposta como block device via `ramshared-wsl2d` (NBD) ou (Fase B) ublk.
- **Confirmado na documentação oficial (kernel `Documentation/admin-guide/blockdev/zram.rst`):**
  - zram aceita `backing_dev` (`echo /dev/X > /sys/block/zramN/backing_dev`) **antes** do
    `disksize`. Writeback via `echo idle|huge|huge_idle > /sys/block/zramN/writeback`.
  - `mem_limit`/`idle` marcam páginas; só páginas **incompressíveis (huge)** ou **idle** vão ao
    backing_dev. `writeback_limit` controla o volume.
  - O `backing_dev` deve ser um **block device** (não arquivo).
- **Proposto / Inferência:**
  - Apontar `backing_dev` do zram para o **device de VRAM** (o `/dev/nbdX` do daemon, ou um
    device ublk na Fase B). **Inferência:** o zram writeback emite BIOs ao backing_dev como I/O
    de bloco normal — o daemon serve via o caminho NBD existente (mesma VRAM, mesmo worker único).
  - Substituir o **2º swap tier (nbd0 prio 100)** pelo `backing_dev` do zram: a VRAM deixa de
    ser swap independente e vira o **store de writeback do zram**. **Inferência** (muda a
    arquitetura da cascata; precisa do kernel p/ validar a semântica de pressão).

## Opção recomendada

**`backing_dev` do zram = device de VRAM; VRAM deixa de ser swap tier separado.**

- `up` (Fase B): `modprobe zram` → `echo <vram_blockdev> > /sys/block/zram0/backing_dev` →
  `disksize` → `mkswap`/`swapon` só do **zram** → política `echo idle > .../writeback` em
  cadência (ou `huge` para incompressíveis na hora). VHDX permanece como tier final (`swapon`
  prio menor) para overflow do próprio backing_dev/zram.
- Por quê: elimina o page-out por um 2º swap device (o frio sai do zram direto pro backing);
  o kernel gerencia idle/huge nativamente (menos lógica userspace); reusa o device de VRAM já
  validado (H1).
- **Alternativas descartadas:**
  - **Manter 2 tiers separados (Day-0 atual)** — funciona, mas dobra o I/O no caminho frio
    (zram→swapout→nbd). É o baseline; a Fase B existe para superá-lo.
  - **`backing_dev` = arquivo no VHDX** — zram writeback aceita só block device; e mandaria o
    frio pro disco, não pra VRAM (perde o ganho).
- **Trade-offs:** muda a forma da cascata (VRAM vira backing, não swap); a observabilidade
  passa a ser via `/sys/block/zram0/bd_stat` em vez de `/proc/swaps` do nbd0; depende de
  kernel custom.

## Requisitos funcionais

- **RF-1 — Backing dev na VRAM.** `up --writeback` aponta `backing_dev` do zram para o device de
  VRAM antes do `disksize`. *Aceite:* `/sys/block/zram0/backing_dev` ecoa o device; `bd_stat`
  evolui sob writeback. **Inferência (precisa de kernel).**
- **RF-2 — Política de writeback.** Disparar `writeback` por `idle` (cadência) e/ou `huge`
  (incompressíveis na hora), com `writeback_limit` para não saturar a VRAM. *Aceite:* páginas
  idle migram pro backing; RAM do zram cai. **Inferência.**
- **RF-3 — VHDX como overflow final.** Quando a VRAM (backing) enche, o excedente vai ao VHDX.
  *Aceite:* sob pressão > VRAM, VHDX cresce; sem OOM. **Inferência.**
- **RF-4 — DEMOTE coerente.** O canário §9/§9.4 e o DEMOTE continuam protegendo a VRAM; com a
  VRAM como backing, o "DEMOTE" passa a ser **desabilitar o backing_dev** (não `swapoff` do
  nbd). *Aceite:* sob eviction, o backing é drenado/desligado sem perda. **Inferência (decisão
  de design a fechar no SPEC).**

## Requisitos não-funcionais

- **Performance:** menos uma cópia no caminho frio vs baseline (Inferência — medir no kernel).
- **Segurança:** sem novo input externo; root; a VRAM segue não endereçável fora do device.
- **Resiliência:** writeback_limit evita saturar a VRAM; VHDX como rede final.
- **Observabilidade:** `/sys/block/zram0/{bd_stat,mm_stat}`; substituir a linha "Tiers" do
  `check` para refletir backing em vez de 2º swap.

## Fluxos

**Happy path (Fase B):** `up --writeback` → zram com `backing_dev`=VRAM → carga enche o zram →
páginas idle/huge → writeback pra VRAM → RAM do zram liberada; overflow → VHDX. `down` →
`writeback_limit 0` + drena + `swapoff zram` + solta o backing + daemon zera a VRAM.

**Erro:** backing_dev cheio → writeback falha → páginas ficam no zram (RAM) → pressão sobe →
canário/VHDX. backing_dev some (DEMOTE) → desabilita writeback, mantém zram em RAM.

## Modelo de dados

- Sem struct uAPI nova (usa sysfs do zram). Estado novo: o `backing_dev` (block device de VRAM)
  + `writeback_limit`. Ciclo de vida: setar backing **antes** do disksize; soltar no teardown.

## API / Interfaces

- **Sem ioctl novo.** Usa sysfs do zram (`backing_dev`, `writeback`, `writeback_limit`,
  `idle`, `bd_stat`). CLI: `ramshared up --writeback [--wb-limit N]`.
- **Kconfig:** exige `CONFIG_ZRAM_WRITEBACK=y` (kernel custom — Fase B).

## Dependências e riscos

- **Pré-requisito duro:** kernel com `CONFIG_ZRAM_WRITEBACK` (ausente no WSL2 atual — verificado).
- **Riscos:** (a) o zram writeback a um backing NBD/ublk pode ter latência/reentrância não óbvia
  (BIO do kernel → daemon userspace → CUDA) — **Inferência**, medir; (b) semântica de DEMOTE
  muda (backing vs swap) — decisão de design; (c) deadlock se o backing_dev (VRAM via daemon)
  depender de RAM que o zram está liberando — **worst-case #5**, analisar no SPEC.
- **Rollout/rollback:** app-only + flag `--writeback` (default off → mantém o Day-0 de 2 tiers).

## Estratégia de implementação (quando houver kernel)

1. CLI `up --writeback`: setar backing_dev + disksize + política. 2. `check`/`status` refletir
backing. 3. Política de writeback (idle cadência). 4. DEMOTE adaptado (desligar backing). 5.
Aceitação §14 adaptada (bd_stat).

## Fora de escopo

- IMPL/validação agora (kernel-gated). Writeback a arquivo. Mudar o worker único do daemon.
- Substituir NBD por ublk (item 5, separado).
