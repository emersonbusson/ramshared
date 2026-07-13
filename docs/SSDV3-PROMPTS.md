# SSDV3 — Spec-Driven Development: Prompts Base

Metodologia em 3 passos: **PRD → SPEC → IMPL**

Versão revisada para o stack RamShared: Kernel Linux (C/Rust for Linux) · LKM · HMM · NUMA · DRM · MMU · PCIe Gen5 · CXL 3.0 · userspace (sysfs/ioctl/mmap, daemons Rust)

Objetivo desta versão:

- preservar a fase de descoberta útil antes de decidir
- reduzir ambiguidade entre fatos do repo e propostas
- produzir PRD e SPEC executáveis no domínio kernel (locks, DMA, uAPI, IRQ)
- melhorar a passagem PRD → SPEC → código → `IMPL.md`
- incorporar guardrails cognitivos (Sistema 2) nas etapas críticas
- eliminar resíduo de stack SaaS/web (HTTP/JSON API, tenant, Prometheus-as-primary)

## Como usar

1. Use o **Passo 1** para gerar `docs/specs/no-milestone/{slug}/PRD.md` (ou pasta de milestone se existir processo formal)
2. Use o **Passo 2** para transformar o PRD em `SPEC.md` **na mesma pasta**
3. Use o **Passo 2.5** quando houver risco estrutural, operacional ou de segurança — no-go **revisa o `SPEC.md` no lugar**; o git versiona
4. Use o **Passo 3** para implementar estritamente a partir do `SPEC.md` e gravar `IMPL.md`

Se um passo encontrar ambiguidade que pertence ao passo anterior, volte um passo.

## Organização dos arquivos

**Todos os artefatos SDD (PRD.md, SPEC.md e IMPL.md) vivem sob `docs/specs/`**, nunca soltos na raiz de `docs/` (exceto legado documentado abaixo).

**Convenção de nomes (canônica):**

```text
docs/specs/
├── no-milestone/
│   └── {slug}/
│       ├── PRD.md
│       ├── SPEC.md    # único SPEC; no-go revisa in-place — git é o histórico
│       └── IMPL.md    # saída do Passo 3
└── M{NN}-{nome}/      # opcional, só se o projeto adotar milestone formal
    ├── milestone.md
    └── {slug}/
        ├── PRD.md
        ├── SPEC.md
        └── IMPL.md
```

- `{slug}`: kebab-case, curto e descritivo (`<issue>-<descricao>` quando há issue, senão só `<descricao>`)
- **`SPEC.md` é único.** No-go do Passo 2.5 → Passo 2 revisa `SPEC.md` in-place e commita. **Não** gere `SPECv2.md` / `SPECvN.md` em features novas. Histórico = `git log` / `git show`
- Se a feature já tem pasta em `docs/specs/`, reutilize-a
- Evolução incremental: reutilize a pasta ou crie subpasta semântica sob a mesma árvore

### Legado (não copiar)

| Path | Status |
| --- | --- |
| `docs/{feature-slug}/` flat | Legado (só README stub permitido). Não criar pastas novas neste formato |
| `SPECv2.md` / `SPECvN.md` | **Proibido.** Política Advoq/RamShared: só `SPEC.md`; no-go revisa in-place; histórico = git |
| Títulos H1 `SPECvN — …` | Cosmético legado; o **arquivo** deve chamar-se `SPEC.md` e o H1 preferir `# SPEC — …` |

## Frontmatter obrigatório no PRD.md

Toda `PRD.md` começa com frontmatter YAML (quando o índice de docs estiver ativo, esses campos o alimentam):

```yaml
---
slug: vram-numa-node
title: Expor VRAM como NUMA node via HMM
milestone: —
issues: []
---
# PRD — VRAM como NUMA node via HMM
```

- `slug`: igual ao nome da pasta
- `title`: humano
- `milestone`: `M14`… ou `—` se ainda não associada
- `issues`: array de números de issue do GitHub (`[]` se não houver)

Status derivado de arquivos: `PRD` → só PRD.md; `SPEC` → **apenas** `SPEC.md` presente; `DONE` → IMPL.md presente. Nunca indexar `SPECvN.md`.

Regenerar / validar o índice:

```bash
node tools/generate-docs-index.mjs          # escreve docs/INDEX.md
node tools/generate-docs-index.mjs --check  # falha se desync
node tools/check-broken-links.mjs           # links .md quebrados
./scripts/docs-check.sh                    # index --check + links
```

## Referência cognitiva

Quando a mudança envolver risco estrutural, operacional, de segurança, rollout, rollback, migração de contrato/uAPI, cache de página/TLB, secret, isolamento Ring 0 vs Ring 3, DMA/IOMMU ou hot path, o SPEC deve apontar explicitamente para [`docs/methodology/kahneman-disciplines.md`](methodology/kahneman-disciplines.md).

Cada etapa crítica responde:

- qual viés está sendo combatido
- qual pergunta obrigatória de Sistema 2 precisa ser respondida
- qual evidência mínima autoriza avançar
- qual condição objetiva exige abortar, voltar um passo ou fazer rollback

## Política Day-0 do RamShared

O RamShared ainda não possui produção viva com dados legados obrigatórios. Toda mudança deve ser a **solução principal e única**, no formato correto final para Day-0.

Por padrão é proibido: workaround, shim, dual-reader, dual-write, camada de compatibilidade com formato/ABI antigo, backfill para produção inexistente, dual-path de módulo, código morto.

Exceções só com requisito explícito e documentado (integração externa real, uAPI já publicada que não pode quebrar, rollout coordenado aprovado). A exceção registra: motivo, prazo de remoção, rollback e evidência.

## Princípios da versão 3

1. **Discovery antes de convergência** — investigação ampla; documento final com uma direção.
2. **Reuso antes de criação** — antes de novo ioctl/sysfs, struct, flag, module param ou path, prove que o existente não atende.
3. **Separar fato de proposta** — `Confirmado no codebase` · `Confirmado na documentação` · `Inferência / proposta`.
4. **Rastreabilidade** — cada RF/NFR do PRD aparece no SPEC; cada bloco de IMPL aponta para itens do SPEC.
5. **Sem criatividade estrutural no Passo 3** — decisão nova → volta ao SPEC antes do código.
6. **Sistema 2 nas etapas críticas** — disciplina Kahneman + evidência mínima + abort trigger.
7. **Número antes de adjetivo** — latência, throughput, cobertura e drills com unidade, n e ambiente (ver [`.claude/rules/benchmarks.md`](../.claude/rules/benchmarks.md)).
8. **Host safety** — pressão de swap/ublk thrashing **nunca** no WSL2 live do host de dev; carga real só em VM isolada (qemu/civm).
9. **Cover da fatia ≥80% por arquivo/crate de business logic** — média monólito do workspace **não** fecha o Passo 3.
10. **E2E ao vivo + evidências fecham o Passo 3** — unit/cover sozinho **não** autoriza `validation.md` de fechamento nem `DONE` real.

### Disciplinas → tipo de teste (gate Kahneman no Passo 3)

| # | Tipo de prova exigida | Exemplo RamShared |
| --- | --- | --- |
| **#9** | Critério numérico (status, used_kb, prio, cover%) | `ramshared status` com prios 200>100>-2 |
| **#13** | Efeito real + recusa pareada com legítimo | ghost swap recusa `up`; `up` limpo passa |
| **#15** | Retry só em assinatura transitória | NBD reconnect em `EAGAIN`; `-EINVAL` fail-fast |
| **#16** | Teste a partir da **exaustão** | demote/reclaim com VRAM já cheia / WDDM commit cap |
| **#17** | Replay 2× = efeito 1× | `down`/`up` ou `swapoff` re-emitido sem double free |

Referência: [`docs/methodology/kahneman-disciplines.md`](methodology/kahneman-disciplines.md). Auditoria adversarial: [`superprompt.md`](../superprompt.md).

### IDs de requisito

- Funcionais: **`RF-N`** (ou prefixo de domínio `RF-B1`, `RF-K2` se o PRD particionar)
- Não-funcionais: **`NFR-N`**
- Decisões técnicas no SPEC: **`DT-N`**
- Itens de implementação no SPEC: **`ITEM-N`**

Commits e `IMPL.md` citam esses IDs. (Sinônimo legado `FR-N` em docs antigos = `RF-N`.)

---

## PASSO 1 — Geração do PRD.md

### Prompt

Preciso gerar o PRD técnico para a seguinte mudança:

**[DESCREVA A FEATURE/MUDANÇA EM 1-2 FRASES]**

Objetivo:

- [qual resultado técnico deve existir ao final]

Camada(s) envolvida(s):

- [ ] Kernel core (mm / sched / pci)
- [ ] Drivers (drm / amd / nouveau / ramshared LKM)
- [ ] Firmware / BIOS / CXL / PCIe
- [ ] Módulo LKM (init/exit, ops, handlers)
- [ ] Userspace (udev / sysfs / ioctl / mmap / daemon)
- [ ] ABI / uAPI headers
- [ ] Documentação / ADR / runbook
- [ ] Benchmark / P0 gate

### Processo obrigatório

Antes de escrever o PRD final:

#### Fase 1 — Discovery

- Levante o contexto real no codebase
- Identifique o que já existe e pode ser reutilizado
- Liste opções de implementação viáveis
- Levante edge cases, riscos, dependências e impactos cross-subsystem
- **Abuse cases (mentalidade atacante / kernel):**
  - ioctl com size errado, overflow, undersize, alignment
  - TOCTOU em ponteiros user vs `copy_{from,to}_user`
  - race open/close × DMA/mmap/refcount
  - path atômico que dorme; GFP errado em IRQ
  - capability bypass (device world-writable, ioctl sem `capable`)
  - leak de endereço kernel (dmesg, sysfs, error paths)
  - unload com refs vivas / UAF no exit
  - estados ilegais de página/lease/swap forçados por sequência de syscalls

#### Fase 2 — Convergência

- Escolha uma opção principal
- Explique por que ela é a recomendada no contexto RamShared
- Liste alternativas descartadas e por quê
- Registre lacunas de contexto que permaneceram abertas

#### Fase 3 — PRD final

- Escreva o `PRD.md` refletindo apenas a opção recomendada
- Não escreva um PRD com múltiplas arquiteturas concorrentes
- Incerteza real → riscos, dependências ou fora de escopo

### Pesquisa obrigatória antes de gerar o PRD

#### 1. Codebase RamShared — contexto interno

- Leia `.claude/rules/` (`kernel.md`, `coding.md`, `ssdv3.md`, `governance.md`, `benchmarks.md`)
- Leia `CLAUDE.md` (root) e `MEMORY.md` (se existir) para topologia e estado de sessão
- Identifique o módulo/crate alvo (ex.: `drivers/ramshared/`, `crates/ramshared-*`) e leia init/exit, `file_operations`/ops, handlers e structs existentes
- Mapeie layout de memória que a feature toca (structs, flags, regiões DMA/MMIO, páginas pinned)
- Verifique spinlocks, mutexes, RCU, barreiras, ordem de locks e coerência (TLB shootdown)
- Leia Kconfig / module params existentes
- Leia IRQs, workqueues, kthreads associados
- Leia [`docs/methodology/kahneman-disciplines.md`](methodology/kahneman-disciplines.md) se houver risco estrutural/operacional/segurança
- Verifique ADRs em `docs/decisions/` relevantes
- Se tocar em privilégio: `capable()` / dono do device / modo de arquivo
- Identifique explicitamente: reutilizar · estender · criar do zero

#### 2. Documentação oficial e compatibilidade

Pesquise só o que a mudança realmente toca:

| Área | Fontes típicas |
| --- | --- |
| Linguagens / tooling | C11 kernel style, Rust for Linux, checkpatch, sparse, pahole, bindgen |
| Build | Kbuild, Kconfig, Cargo (userspace) |
| Memory | HMM, NUMA, page migration, `DEVICE_PRIVATE`, hotplug |
| Bus / device | PCIe Gen5, CXL 3.0, DMA API, IOMMU |
| Display / GPU | DRM, TTM, amdgpu/nouveau onde aplicável |
| Observabilidade | ftrace, perf, dmesg, lockdep, kmemleak, KASAN |
| Validação | kselftest, KUnit, qemu/civm drills |

#### 3. Referências de mercado e edge cases

- Implementações de referência em drivers/mainline de escala similar
- Edge cases multi-processo, hot-unplug, OOM, device removal
- Trade-offs latência vs throughput vs coerência de cache
- Threat model local (não OWASP web genérico, salvo componente HTTP real no userspace)

### Regras de qualidade do PRD

- Não invente arquitetura se o codebase já tiver padrão equivalente
- Diferencie: **Confirmado no codebase** · **Confirmado na documentação** · **Inferência / proposta**
- Lacuna no repo → declare; não invente fato
- Prefira reaproveitar structs, ops tables, locks, helpers e uAPI existentes
- Não proponha novos ioctls/sysfs/params sem justificar por que os existentes não bastam
- Aponte breaking changes de uAPI/ABI, rollout (module load order), rollback (unload/revert) e o que é **forward-only**
- Aplique Day-0: solução principal, sem shims/compat/backfill fantasma
- Em **Alternativas descartadas**, mate compatibilidade “só para preservar versão antiga sem produção viva”
- Liste documentos a atualizar no mesmo commit se impacto estrutural
- Risco alto → antecipe no PRD quais etapas pedirão disciplina Kahneman no SPEC
- Se a feature embasar go/no-go de performance, cite o gate P0 e [`.claude/rules/benchmarks.md`](../.claude/rules/benchmarks.md)
- Mantenha o PRD específico e operacional; evite texto genérico

### Saída esperada

Gere `docs/specs/no-milestone/{slug}/PRD.md` (ou sob `docs/specs/M{NN}-…/{slug}/` se milestone formal) com **EXATAMENTE** esta estrutura:

#### 1. Resumo

- O que é, por que existe, qual problema resolve
- Valor técnico (aceleração de hardware / HPC / memory tiering) no contexto RamShared

#### 2. Contexto técnico

- Módulo(s)/subsistema(s) e papel de cada um
- Estado atual: structs, ioctls/sysfs, caches e fluxos a reutilizar/estender
- Escopo de memória: kernel (`kmalloc`/`vmalloc`/page), device (VRAM/DMA/MMIO), user (`mmap`)
- Dependências (símbolos exportados, notifiers, callbacks, crates)
- O que está **confirmado no codebase**
- O que está **confirmado na documentação**
- O que está **sendo proposto**

#### 3. Opção recomendada

- Solução escolhida
- Motivo da escolha
- Alternativas descartadas
- Trade-offs aceitos

#### 4. Requisitos funcionais

Para cada requisito:

- **RF-N**: descrição objetiva sem ambiguidade
- **Critério de aceite**: condição verificável (teste, dmesg, sysfs value, drill)
- **Isolamento**: address space / capabilities / per-device, se aplicável

#### 5. Requisitos não-funcionais

- **NFR — Performance**: latência (p50, p99), throughput, tamanho de I/O; unidade + ambiente alvo
- **NFR — Segurança**: capabilities, validação de input, bounds, sem leak de endereço kernel
- **NFR — Observabilidade**: ftrace/perf hooks, `dev_*`/`pr_*`, debugfs/sysfs, contadores
- **NFR — Escalabilidade**: N processos, N devices, limites de pool/filas
- **NFR — Resiliência**: pressão de memória, falha de DMA, device removal, falhas parciais, OOM
- **NFR — Concorrência**: contexto (process/softirq/hardirq), “pode dormir?”, ordem de locks
- **NFR — Host safety**: o que **não** rodar no WSL2 live; o que exige qemu/civm

#### 6. Fluxos

**Happy path**

- Passo a passo numerado
- Componente/módulo em cada passo
- Interface (ioctl / sysfs / debugfs / netlink / mmap / netlink-like protocol)
- Caminho: syscall → VFS → handler do driver (ou daemon → backend)

**Fluxos alternativos**

- Variações válidas do happy path

**Fluxos de erro**

Para cada erro:

- Condição de trigger
- errno retornado a userspace (`-EINVAL`, `-EPERM`, `-ENODEV`, `-EBUSY`, `-ENOMEM`, …)
- Log level e campos contextuais (**sem** vazar ponteiros kernel / KASLR)
- Impacto em consistência de estado (páginas, DMA maps, refcounts)

#### 7. Modelo de dados

Estruturas em memória (não há SQL no hot path kernel):

- `struct`s novas/alteradas: campos, locks embutidos, flags, alinhamento (`__packed` se ABI)
- Regiões: DMA (`dma_addr_t`), MMIO (`ioremap`), VRAM/device, páginas pinned
- Ciclo de vida: quem aloca/libera, refcount, ordem de teardown (`goto out_err`)
- Layout de uAPI/ABI (tamanho e compatibilidade de structs expostas)

#### 8. API / Interfaces

**Ioctl / sysfs / debugfs novos ou modificados**

| Campo | Valor |
| --- | --- |
| Tipo | `ioctl` / `sysfs show\|store` / `debugfs` / `mmap` |
| Path / cmd | `/dev/…`, `/sys/…`, `_IOWR(…)` |
| Privilégio | `capable(CAP_SYS_ADMIN)` / dono do device / modo 0600 |
| Contexto | process / pode dormir? / atomic |
| Validação | bounds, alignment, size max, `copy_{from,to}_user` |
| Idempotência | sim/não e por quê |

**Shape do contrato userspace** (C header / Rust struct — **não** JSON de API web):

```c
struct ramshared_foo_arg {
	__u64 offset;
	__u32 len;
	__u32 flags;
};
```

**Erros**

| errno | Condição | Nota ao userspace |
| --- | --- | --- |
| `-EINVAL` | validação falha | argumento inválido |
| `-EPERM` | sem capability | requer privilégio |
| `-ENODEV` | device ausente | não encontrado |
| `-EBUSY` | lock/recurso ocupado | tente depois / estado inválido |
| `-ENOMEM` | OOM | sem memória |

**Impacto em uAPI / headers / Kconfig**

- Structs uAPI novas ou alteradas
- Compatibilidade de layout/tamanho
- Module params / `CONFIG_*` novos ou alterados

**IRQ / workqueues** (se aplicável)

| Linha / fonte | Top half | Bottom half | Pode alocar? | Locking |
| --- | --- | --- | --- | --- |
| … | handler | workqueue/tasklet | GFP_ATOMIC / não | … |

#### 9. Dependências e riscos

- Pré-requisitos
- Riscos técnicos com mitigação concreta
- Impacto em módulos/subsistemas existentes
- Breaking changes de uAPI/ABI
- Estratégia de rollout (ordem de load, flags de feature)
- Estratégia de rollback (unload, revert de commit, o que é forward-only em uAPI)
- Hipóteses que pedirão disciplina explícita no SPEC
- **Rollback trigger numérico candidato** (disciplina #2): ex. stall > X µs, oops, lockdep splat

#### 10. Estratégia de implementação

- Ordem recomendada das fatias
- Dependências entre fatias
- O que validar cedo (compile, unit, kselftest, drill)
- O que exige hardware, qemu/civm ou host GPU

#### 11. Documentos a atualizar

| Documento | Criar / Alterar / N/A | Motivo |
| --- | --- | --- |
| ADR | … | … |
| runbook | … | … |
| `docs/reliability/DEGRADATION-MATRIX.md` | … | … |
| uAPI docs / Kconfig help | … | … |
| `.claude/rules/*` | … | … |

#### 12. Fora de escopo

- O que explicitamente NÃO faz parte desta implementação
- Motivo de exclusão

#### 13. Critérios de aceite (consolidados)

- Lista checável que une RF + NFR verificáveis

#### 14. Validação prevista

- Gates: checkpatch, sparse, `make modules`, kselftest/KUnit, cargo (userspace)
- Drills: script/qemu/civm
- Benchmarks se gate P0: ≥3 runs, median+p99, tag `idle`/`loaded`, registro em `docs/BENCHMARKS.md` + `docs/benchmarks/results.jsonl`

---

## PASSO 2 — Geração do SPEC.md (a partir do PRD ou auditoria)

> Leia o `PRD.md` e produza um `SPEC.md` cirúrgico na **mesma pasta**.
> Se reexecutado após `no-go` do Passo 2.5, **revise o `SPEC.md` no lugar** — o git preserva o histórico; **não** crie `SPECv2.md`.
> O SPEC não replica o PRD; fecha decisões, remove ambiguidade e traduz requisitos em mudanças exatas no repo.

### Objetivo do Passo 2

- transformar requisitos em tarefas de código com ordem e dependências
- resolver ambiguidades do PRD antes do código
- explicitar impactos em uAPI, dados em memória, docs, testes e rollout
- ligar etapas críticas a [`docs/methodology/kahneman-disciplines.md`](methodology/kahneman-disciplines.md)

### Prompt

Leia `docs/specs/no-milestone/{slug}/PRD.md` (ou path de milestone) e gere `SPEC.md` na mesma pasta com decisões fechadas, rastreabilidade por requisito e instruções implementáveis sem interpretação.

Se voltar do Passo 2.5 com `no-go`, leia o relatório da auditoria e revise o `SPEC.md` in-place — nunca gere `SPECv2.md`.

### Regras

1. Só inclua o que será realmente implementado agora
2. Cada arquivo listado com caminho completo a partir da raiz do repo
3. Cada mudança: **o que muda**, **como muda**, **por que muda**
4. Referências a código existente com nome exato (função, struct, tipo, ops table)
5. Ordem dos itens = ordem de implementação
6. Ambiguidade do PRD → decisão explícita + justificativa no SPEC; grave sempre em `SPEC.md` (in-place após no-go)
7. Alocação em IRQ/atomic → `GFP_ATOMIC` (ou proibir alocação)
8. Todo ioctl valida size/alignment e usa `copy_{from,to}_user` antes de confiar no buffer
9. Caminho privilegiado checa `capable(CAP_SYS_ADMIN)` (ou política documentada) antes de agir
10. Sem pseudocódigo estrutural em tipos, handlers ou contratos uAPI
11. Todo RF do PRD rastreado por ID no SPEC; NFRs críticos também
12. Mudança estrutural → quais documentos atualizar no mesmo commit
13. Etapas com risco (locks, DMA, uAPI, rollout, Ring 0/3, secret, retry, hot path) → disciplina Kahneman explícita
14. Passo crítico: pergunta obrigatória + evidência mínima + abort trigger
15. Múltiplos writes / estado multi-step → **fronteira de atomicidade** explícita (o que é atômico nesta issue e o que não é)
16. Evidência mínima **executável** no repo: `./scripts/checkpatch.pl`, `make modules`, `make kselftest`, `cargo test`, diff, log `dmesg`, drill documentado — não vale evidência implícita
17. Rollback separado quando aplicável:
    - rollback de **código** (revert/unload module)
    - rollback de **contrato uAPI** (muitas vezes **forward-only** após publicação)
    - rollback de **estado** (páginas, DMA maps, sysfs)
    e o que é proibido em host live vs permitido só em lab
18. Day-0: solução principal única; shims/dual-path só com exceção documentada
19. Contratos ainda não publicados: consolide o layout final; não planeje “migration incremental corretiva” sem produção
20. Arquivo só de compat temporária → marcar para não criar ou deletar, salvo exceção Day-0
21. **Tabela de lock order** obrigatória se a feature adquirir ≥2 classes de lock ou misturar IRQ/process
22. **Matriz de contexto** (process / softirq / hardirq × pode dormir? × GFP) para cada path quente novo
23. **Rollback trigger numérico** no SPEC (disciplina #2) para mudanças non-triviais de mm/DMA/lock
24. Gate de performance → plano de medição alinhado a [`.claude/rules/benchmarks.md`](../.claude/rules/benchmarks.md)

### Guardrail cognitivo obrigatório

Em qualquer ITEM que envolva locks, DMA/IOMMU/MMIO, uAPI, migração de páginas, auth/capability, isolamento Ring 0/3, rollout, rollback, cache/TLB, secret, retry ou risco de oops/indisponibilidade, incluir bloco `Disciplina Kahneman`:

- **Disciplina**: nome exato em `docs/methodology/kahneman-disciplines.md`
- **Link**: caminho + âncora da seção quando possível
- **Pergunta obrigatória**: pergunta de Sistema 2
- **Evidência mínima**: comando/teste/log/diff reproduzível
- **Abort trigger**: condição objetiva que impede avanço ou exige rollback

Regras adicionais:

- Evidência que depende de estado anterior → descrever harness, fixture ou drill
- Rollback inseguro em host compartilhado → política `forward-only` + abort trigger
- Host safety: abort se o plano exigir thrashing de swap no WSL2 live

### Saída esperada

Após no-go, mantenha a estrutura abaixo. Opcionalmente, logo após o H1:

```markdown
> Revisado após auditoria do Passo 2.5 ({data}): {resumo objetivo dos blockers corrigidos}.
```

Arquivo único — histórico no `git log`.

#### Escopo fechado desta implementação

- O que entra agora
- O que fica explicitamente fora agora
- Dependências já assumidas como prontas

#### Matriz de rastreabilidade PRD → SPEC

| PRD | Implementação no SPEC |
| --- | --- |
| RF-1 | ITEM-3, ITEM-4 |
| NFR-2 | ITEM-5 |

#### Decisões técnicas

| # | Decisão | Justificativa |
| --- | --- | --- |
| DT-1 | … | … |

#### Fronteira de atomicidade e política de rollback

- **Atomicidade desta implementação**: o que esta issue garante; o que fica fora; estados parciais aceitos
- **Rollback**:
  - código (revert / `rmmod` / unload path)
  - uAPI (forward-only? versionamento?)
  - estado em memória/device
  - o que é proibido no host live de dev

#### Ordem de locks e contexto de execução

| Lock / recurso | Ordem (#) | Contextos permitidos | Pode dormir com o lock? | Notas |
| --- | --- | --- | --- | --- |
| … | 1 | process | sim/não | … |

| Path / função | Contexto | Pode dormir? | GFP | Locks adquiridos |
| --- | --- | --- | --- | --- |
| … | process/softirq/hardirq | … | … | … |

#### Mapa Kahneman por etapa crítica

| Etapa / ITEM | Disciplina | Link | Pergunta obrigatória | Evidência mínima | Abort trigger |
| --- | --- | --- | --- | --- | --- |
| ITEM-3 | … | `docs/methodology/kahneman-disciplines.md` | … | … | … |

#### Checklist de segurança (pré-implementação)

- [ ] Isolamento: offsets/handles/ioctl respeitam device e capabilities do caller
- [ ] OOB: toda cópia user↔kernel valida tamanho/faixa (`copy_{from,to}_user`, bounds)
- [ ] Alignment e size máximo de structs uAPI
- [ ] Privilege: caminho privilegiado com `capable()` / modo de device documentado
- [ ] TOCTOU: não revalidar ponteiro user depois de copiar; não usar `__user` após copy
- [ ] IRQ/atomic: sem sleep; `GFP_ATOMIC` ou zero alloc
- [ ] Lock order documentado; sem lockdep inversions conhecidas
- [ ] Info-leak: sem endereços kernel em logs/sysfs/uAPI; sem credencial hardcoded
- [ ] Lifetime: refcount/get/put equilibrados; exit path sem UAF
- [ ] Device removal / hot-unplug: falha segura, sem use-after-unmap
- [ ] Host safety: sem plano de thrash no WSL2 live

#### Arquivos a CRIAR

Para cada arquivo novo:

**`caminho/completo/desde/raiz/arquivo.ext`**

- **Propósito**: uma frase
- **Requisitos cobertos**: `RF-N`, `NFR-N`, `DT-N`
- **Structs/Types**: assinatura exata
- **Funções**: assinatura exata + lógica em passos
- **Dependências internas / externas**
- **Padrão de referência**: arquivo existente no repo
- **Testes requeridos**: arquivo e cenários mínimos
- **Disciplina Kahneman** (se crítico): Disciplina / Link / Pergunta / Evidência / Abort

#### Arquivos a MODIFICAR

Para cada arquivo existente:

**`caminho/completo/desde/raiz/arquivo.ext`**

- **O que muda**: cirúrgico
- **Requisitos cobertos**: `RF-N`, `NFR-N`, `DT-N`
- **Função/bloco afetado**: nome exato
- **Antes** / **Depois**: shape relevante
- **Por quê**: vínculo ao PRD
- **Impacto**: quebra uAPI/ABI? callers? docs?
- **Testes requeridos**
- **Disciplina Kahneman** se crítico

#### Arquivos a DELETAR (se houver)

| Arquivo | Motivo |
| --- | --- |
| `path/to/file` | substituído por X / morto Day-0 |

#### Observabilidade

**Contadores / debugfs / ftrace** (se aplicável)

- nome, unidade, onde incrementa, quem lê

**Logs**

| Evento | Level | Campos (sem ponteiros kernel) |
| --- | --- | --- |
| resource ready | `dev_info` | `dev`, `size_bytes`, `flags` |

#### Contratos e documentação viva

| Documento | Atualização | Motivo |
| --- | --- | --- |
| uAPI / headers docs | Criar / Alterar / N/A | ioctl/sysfs novo? |
| Kconfig help | Alterar / N/A | novo `CONFIG_` / param? |
| `CLAUDE.md` / `.claude/rules/*` | Alterar / N/A | padrão estrutural? |
| `docs/decisions/ADR-NNN-*.md` | Criar / N/A | decisão arquitetural? |
| `docs/methodology/kahneman-disciplines.md` | Alterar / N/A | nova âncora? |
| `docs/reliability/DEGRADATION-MATRIX.md` | Alterar / N/A | novo modo de falha? |
| `docs/BENCHMARKS.md` + `results.jsonl` | Alterar / N/A | gate P0? |

#### Ordem de implementação

Lista numerada, verificável, sem gaps. Exemplo de esqueleto:

1. Structs e headers (incl. uAPI)
2. Núcleo de estado (alocação, locks, refcount)
3. Ops tables / handlers (ioctl/read/write/sysfs)
4. Integração de subsistema (DRM/PCIe/HMM) se aplicável
5. Observabilidade (ftrace, contadores, `dev_*`)
6. Testes unitários (KUnit / Rust unit)
7. Testes de integração (kselftest / drill qemu)
8. Documentação viva + `IMPL.md` no Passo 3

#### Plano de testes

**Kernel / LKM**

- unitários (KUnit): casos
- integração (kselftest): casos
- concorrência / lockdep: casos
- error paths / device remove: casos
- isolation Ring 0 vs Ring 3: casos

**Userspace (crates / daemons)**

- unitários: casos
- integração in-process: casos
- drills qemu/civm: casos (nunca thrash no WSL2 live)

**Manuais / lab**

- sequência mínima de sysfs/ioctl
- cenários de erro
- evidências objetivas do mapa Kahneman

#### Gate P0 / benchmarks (se aplicável)

- Hipótese numérica
- Harness (`scripts/p0/…` ou drill)
- n ≥ 3; reportar median + p99 + desvio
- tag `idle` | `loaded`
- destino: `docs/BENCHMARKS.md` + `docs/benchmarks/results.jsonl`

#### Checklist de validação

**C / módulo**

- [ ] `./scripts/checkpatch.pl -f <arquivo.c>` (ou path do projeto)
- [ ] `make W=1 C=1` / sparse onde configurado
- [ ] `make modules` (ou target documentado do repo)
- [ ] kselftest / KUnit relevantes

**Rust (kernel ou userspace)**

- [ ] `cargo test` no workspace/crate afetado
- [ ] `cargo clippy` com warnings do projeto
- [ ] `rustfmt --check` se aplicável

**Gates cognitivos / segurança**

- [ ] Etapas críticas → Kahneman + pergunta + evidência + abort
- [ ] Lock order e matriz de contexto preenchidas se ≥2 locks / hot path
- [ ] Rollback trigger numérico registrado se non-trivial
- [ ] Sem linguagem vaga em pontos críticos

---

## PASSO 2.5 — Auditoria do SPEC (opcional por risco)

> Use quando a implementação tiver risco estrutural, operacional ou de segurança.
> Existe para reduzir ambiguidade **antes** do código, não para burocratizar mudanças locais.

### Quando usar

- capabilities, secrets ou paths privilegiados
- isolamento Ring 0 vs Ring 3 ou acesso cross-processo
- locks, filas, cache de página/TLB, invalidação, RCU
- DMA / IOMMU / MMIO / hotplug
- contratos uAPI/ABI ou integração entre módulos
- rollout coordenado, ordem de load, janela operacional
- risco alto de oops, deadlock, perda de dados em device, thrash de host

### Quando pode pular

- poucos arquivos, mudança local
- sem lock/DMA/uAPI novos
- sem rollout especial
- sem impacto em privilégio ou isolamento

### Prompt

Revise `docs/specs/no-milestone/{slug}/SPEC.md` (ou path de milestone) como auditoria pré-implementação.

Quero revisão de lacunas com foco em:

- ambiguidades técnicas ainda não resolvidas
- fronteira de atomicidade implícita, ambígua ou incompatível com o código real
- riscos de rollout, unload, device removal e rollback
- evidência mínima sem caminho executável no repo
- rollback genérico sem separar código / uAPI / estado
- dependências não mapeadas entre módulos, configs, docs e automação
- **gaps de segurança (kernel):**
  - **Subversão de fluxo**: lógica permite estados impossíveis (página/lease/swap/device)?
  - **Concorrência**: race em open/close, mmap, IRQ vs process, TOCTOU?
  - **Smuggling de args**: flags/bits/size de ioctl não validados (análogo a mass assignment)?
  - **Assimetria**: userspace “confia” sem espelho de validação no handler?
  - **Handle/offset confuso**: acesso a recurso de outro processo/device sem checagem (análogo a IDOR)?
  - **Info-leak / KASLR**: endereço kernel ou layout interno exposto?
  - **Lifetime / UAF**: get/put e exit paths equilibrados?
- **gaps de escalabilidade / hot path:**
  - **O(n) em hot path**: loops, TLB shootdown em massa, alocação crônica sob pressão?
  - **Lock contention**: lock grosso no fast path sem necessidade?
- inconsistências entre requisitos, DTs, arquivos, testes e validação final
- item do SPEC que ainda exige interpretação na implementação
- ausência de disciplina Kahneman em etapa crítica
- linguagem vaga (`validar`, `garantir`, `confirmar`, `se necessário`) sem critério observável
- violação Day-0 (shim, dual-path, compat sem exceção)
- plano que thrasha swap/ublk no WSL2 live
- falta de lock order / matriz de contexto quando o código claramente precisa

### Formato da resposta

1. Findings primeiro, por severidade
2. Para cada finding, cite a seção exata do SPEC
3. `Open questions`
4. Pronto para implementação? sim/não
5. Feche com `go` ou `no-go`

### Persistência da auditoria (obrigatória)

Grave o resultado para não perder go/no-go no chat:

**`docs/specs/…/{slug}/AUDIT-2.5.md`** (preferido, mesma pasta do SPEC)

ou `docs/reviews/YYYY-MM-DD-{slug}.md` se a pasta de reviews for usada.

Template mínimo:

```markdown
# AUDIT 2.5 — {slug} — {YYYY-MM-DD}

**SPEC:** `SPEC.md`
**Verdict:** go | no-go

## Findings (by severity)

| Sev | Section | Finding | Action |
| --- | --- | --- | --- |
| HIGH | … | … | SPEC change / open Q |

## Open questions

- …

## Blockers addressed (if re-audit after no-go)

- …
```

### Regra de saída

- Finding que exige decisão nova → volte ao **Passo 2**
- `no-go` → **no mesmo turno**, volte ao Passo 2 e **revise `SPEC.md` in-place**; nunca crie `SPECv2.md`
- Ao revisar após no-go, registre no topo do SPEC (changelog de uma linha) e no `AUDIT-2.5.md` quais blockers foram endereçados
- Etapa crítica sem Kahneman / evidência / abort → `no-go`
- Violação Day-0 sem exceção → `no-go`
- `go` → **Passo 3** (com `AUDIT-2.5.md` gravado)

---

## PASSO 3 — Implementação + IMPL.md

> Leia o `SPEC.md` e execute-o passo a passo.
> O Passo 3 **não** fecha lacunas arquiteturais; implementa o que já foi decidido.
> Ao terminar (ou ao fechar uma fatia reviewable), grave/atualize `IMPL.md` na mesma pasta.

### Prompt

Implemente a feature descrita no `SPEC.md` de `docs/specs/no-milestone/{slug}/` (ou path de milestone).

Ao final, escreva `IMPL.md` com o que foi feito, validação numérica e gaps restantes.

### Regras de execução

1. Siga a ordem de implementação do SPEC
2. Use assinaturas e contratos do SPEC como base
3. Não adicione funcionalidade fora do escopo fechado
4. Gap estrutural → volte ao Passo 2 antes de continuar
5. Alteração de contrato → docs/headers no mesmo ciclo
6. Alteração de dado, capability, isolamento ou cache de página → teste
7. Não refatore adjacente sem necessidade funcional
8. Item crítico → execute o bloco Kahneman antes da próxima fatia
9. Day-0 limpo: sem shims, fallbacks, dual-path ou dead code
10. Se parecer necessário manter duas versões → pare e volte ao SPEC para exceção Day-0
11. Quando o SPEC consolidar estrutura Day-0, reescreva/remova o antigo em vez de preservar caminho morto
12. Commits citam `RF-N` / `NFR-N` / `ITEM-N` quando não-triviais; body com `Rollback trigger:` se tocar locks/DMA/mm
13. **Gate de cobertura da fatia ≥80%** em business logic **por arquivo** (ou por crate quando a fatia é crate-scoped): `cargo llvm-cov -p <crate> --summary-only` / report line-level. Média do workspace monólito **não** conta. Boilerplate de wiring puro pode ser `N/A — boilerplate` no IMPL.
14. Todo “Testes requeridos” do SPEC deve existir como `#[test]` / `#[tokio::test]` nomeado; path de hang/swapoff/ghost sem teste de recusa+legítimo = fatia incompleta
15. Evidência Kahneman #13/#15/#16/#17 exige o _tipo_ da tabela acima (efeito, recusa+legítimo, exaustão, replay 2×) — smoke sem assert não fecha
16. **E2E ao vivo + evidências fecham o Passo 3** (não é follow-up opcional). Ordem fixa: (a) unit/cover/docs verdes; (b) binário da fatia **deployado** (cascade/daemon com inode do `target/release` atual — `BINARY_MATCH=OK`, sem exe deleted); (c) jornada real: `ramshared status` / cascade-health / drill do SPEC com ≥1 cenário legítimo + recusas (ghost swap, used_kb>0, preflight fail); (d) artefactos em `docs/specs/.../evidence/` ou paths no `validation.md` (JSON health, `cat /proc/swaps`, screenshot de drill UI se houver); (e) só então `validation.md` + `IMPL.md` com números e veredito. Unit/cover sozinho **não** fecha Passo 3.

### Ritual de execução por fatia

1. Implementar só o item atual
2. Validar compilação
3. Validar testes relacionados (**escrever se o SPEC listar** — TDD)
4. Rodar gate de cover nos crates/arquivos da fatia (`--min 80` / line ≥80%)
5. Validar pergunta / evidência / abort Kahneman se houver
6. Comparar com o SPEC (matriz de testes)
7. Só então avançar

### Checklist durante a implementação

- [ ] Código compila sem erros
- [ ] checkpatch / lint / clippy do escopo passa
- [ ] Testes existentes continuam passando
- [ ] Novos testes da fatia adicionados (todos os “Testes requeridos” do SPEC)
- [ ] Gate de cobertura da fatia ≥80% nos arquivos/crates de business logic tocados
- [ ] Isolamento (address space / capabilities) mantido
- [ ] uAPI/ABI atualizada quando necessário
- [ ] Docs atualizadas quando o item exige
- [ ] Etapas críticas coerentes com Kahneman
- [ ] Provas #13/#15/#16/#17 presentes quando a disciplina se aplica

### Quando voltar ao SPEC

- Índice/campo/lock não previsto
- Handler precisa de campo extra
- Edge case não coberto
- Ordem de implementação não fecha
- Mudou layout de struct uAPI ou contrato ioctl/sysfs
- Rollout/rollback não descrito
- Etapa crítica exige decisão que o mapa Kahneman não fechou
- Necessidade de shim/dual-path/compat não documentada como exceção Day-0

> **Regra absoluta:** se o código precisar decidir algo que o SPEC não decidiu, pare e atualize o `SPEC.md` primeiro (in-place).

### Validação final

Execute o checklist do SPEC. Esqueleto típico RamShared:

```bash
# C / LKM (ajuste paths ao módulo real)
./scripts/checkpatch.pl -f path/to/file.c
make modules
# make W=1 C=1 M=...   # se sparse/W=1 estiver no fluxo do repo
# make kselftest       # alvos relevantes

# Userspace Rust
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
# Cover da fatia (exemplo; liste crates do SPEC):
cargo llvm-cov -p ramshared-cli -p ramshared-tier -p ramshared-dxg --summary-only
# ou por arquivo: cargo llvm-cov report --json | jq ...

# Drill isolado (nunca thrash no WSL2 live)
# scripts/kernel/qemu-*.sh  ou  civm job documentado no SPEC
```

**IMPL.md (fechamento)** — registre com números (Kahneman #3/#9):

- comando(s) de cover e exit code
- tabela crate/arquivo → % (ou output do gate)
- lista “Testes requeridos do SPEC → `test_name` implementado”
- residual &lt;80% só com `N/A — boilerplate` ou gap env-bound explícito

### Validação ponta a ponta e evidências (bloqueante do Passo 3)

> **Ordem obrigatória:** unit/cover/docs → **E2E ao vivo + artefactos** → `validation.md`
> / `IMPL.md` → commits. Entrada parcial “unit verde; E2E pendente” **não** fecha o Passo 3.

Com o código da fatia **no binário em execução** (não só no git):

1. estado inicial: `ramshared status`, `/proc/swaps`, `cascade-health.sh`, PID + `readlink /proc/$pid/exe` (= `BINARY_MATCH`);
2. ação real do SPEC (`up`/`down`/`demote`/drill de lab — **não** thrash no WSL diário se for pressão destrutiva);
3. resultado: prios, used_kb, flags.ghost/order_ok, exit codes;
4. ≥1 legítimo + recusas do SPEC (ghost, used_kb>0, preflight sem binário, WDDM fail-closed);
5. paths dos artefactos no `validation.md` (JSON, swaps dump, log de drill).

Classe **hang/freeze** (ghost ublk, free com used_kb≠0, kill -9 do daemon com swap ativo, postmortem falso CRASH) exige prova #13/#16 no E2E ou integration — “código existe” ≠ “proteção ativa”.

### Saída obrigatória — `IMPL.md`

Gere/atualize `docs/specs/no-milestone/{slug}/IMPL.md` com **EXATAMENTE** esta estrutura:

```markdown
# IMPL — {título}

> SSDV3 PASSO 3. Implementa `SPEC.md` em `docs/specs/.../{slug}/`.
> Branch: `{branch}`. PR: {link ou "ainda não"}.

## Status

{implementado | parcial} · gates: {lista com ✓/✗}

## Arquivos (RF/ITEM → mudança)

| Arquivo | ITEM / RF | O que foi feito |
| --- | --- | --- |
| `path` | ITEM-1 (RF-1) | … |

## Decisões pequenas (sem nova ADR)

- …

## Validação (números)

- testes: {n pass / n fail}
- checkpatch / clippy: {limpo | findings}
- drill: {PASS/FAIL + script}
- benchmark (se houver): median / p99 / n / tag idle|loaded · run-id

## Gaps

- fechados nesta sessão: …
- env-bound (precisa hardware/civm/GPU): …
- abertos: …

## Rollback trigger

{condição numérica/observável; alinhar ao SPEC e ao body do commit}

## Traceability

| PRD | SPEC ITEM | Commit(s) |
| --- | --- | --- |
| RF-1 | ITEM-3 | `abc1234` |
```

`DONE` no índice de specs = presença de `IMPL.md` coerente com o SPEC e gates documentados.

---

## Critérios de saída entre passos

### PRD → SPEC

Só avance se:

- houver uma opção recomendada clara
- requisitos funcionais fechados
- riscos estruturais explícitos
- fora de escopo definido
- abuse cases pelo menos listados quando houver uAPI/locks/DMA

### SPEC → Implementação

Só avance se:

- cada RF do PRD rastreado
- ordem de implementação fechada
- arquivos a criar/modificar explícitos
- plano de testes e docs definidos
- etapas críticas com Kahneman + pergunta + evidência + abort
- lock order / matriz de contexto se aplicável
- se risco alto, Passo 2.5 com `go`

### Implementação → Commit / DONE

Só avance se:

- código, testes e docs consistentes com o SPEC
- validações finais executadas (ou gaps env-bound explícitos no IMPL)
- **cover da fatia ≥80%** nos arquivos/crates de business logic tocados
- **E2E ao vivo** com binário deployado e evidências no `validation.md` / `evidence/`
- sem drift entre uAPI/headers, implementação e testes
- `IMPL.md` gravado/atualizado (com cover + E2E, não só narrativa)
- commits non-triviais com rollback trigger quando exigido

> **Nota sobre o índice (`DONE`):** presença de `IMPL.md` no `docs/INDEX.md` é artefato, **não** qualidade. Fechar SSDV3 sem cover/E2E/Kahneman de teste é violação do Passo 3 mesmo com DONE no índice.

---

## Referência rápida — Stack RamShared

| Camada | Tecnologia |
| --- | --- |
| Linguagens | C11 (Linux kernel style) + Rust for Linux / Rust userspace |
| Subsistemas | mm (HMM/NUMA), DRM, MMU, PCIe Gen5, CXL |
| Build | Kbuild / Makefiles; Cargo (tooling e daemons) |
| Validação | checkpatch.pl, sparse, lockdep, kmemleak, KASAN, kselftest/KUnit, cargo test |
| Observabilidade | ftrace, perf, dmesg, `dev_*` / `pr_*`, debugfs |
| Userspace (MVP) | Rust (libcuda via FFI, NBD/ublk paths, broker/agent) |
| Lab | qemu drills, civm — **não** thrash no WSL2 live |

---

## Regra de ouro

> O **PRD** decide o que e por quê.
> O **SPEC** fecha como, onde, em que ordem e com quais guardrails.
> A **implementação** executa sem reinventar a decisão.
> O **IMPL.md** registra o que foi feito, com números e gaps honestos.

## Quando iterar

- Se o Passo 3 achar um gap real, volte ao Passo 2
- Se o Passo 2 achar ambiguidade insolúvel, volte ao Passo 1
- Nunca resolva gap estrutural só no código
- Nunca crie `PRD.md` / `SPEC.md` / `IMPL.md` fora de `docs/specs/…`
- Nunca introduza `SPECv2.md` em feature nova
