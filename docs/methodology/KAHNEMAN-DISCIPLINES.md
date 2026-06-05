# Kahneman disciplines — hygiene para decisões de IA

Disciplinas derivadas de **Thinking, Fast and Slow** e **Noise**
(Kahneman, Sibony, Sunstein) aplicadas ao ciclo de desenvolvimento
assistido por IA neste monorepo.

**Por que isto existe.** LLMs falam Sistema 1 com fluência altíssima —
articulam bem até quando estão errados. O humano precisa ser o Sistema 2:
verificar, medir, questionar o que parece óbvio. Este doc cria atritos
estruturais (checklists, rubricas numéricas, counterfactuals obrigatórios)
que forçam Sistema 2 a entrar nos momentos onde Sistema 1 custaria caro.

Kahneman é explícito: vieses e ruído são propriedades do sistema, não
bugs removíveis. O que este doc faz é **higiene, não cura**. O ganho
não aparece em uma decisão isolada — aparece em redução de variância
ao longo de meses.

---

## As 14 disciplinas operacionais

Cada uma começa com o viés que combate, descreve a regra e dá o sinal
mensurável que indica que está funcionando.

### Tabela mestra (visão consolidada)

| #   | Disciplina               | Regra (1 linha)                                                                                       | Exemplo no ramshared                                                                                                                         | Sinal observável                                                                                                             |
| --- | ------------------------ | ----------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| 1   | WYSIATI                  | Declarar o não-visto antes de opinar                                                                  | "sem ter testado refresh em concorrência > 50 req/s, estimo X com confiança Y%"                                                          | resposta começa com "sem ter visto Z..."                                                                                     |
| 2   | Counterfactual           | Rollback trigger numérico em decisão não-trivial                                                      | "se latência de interrupção > 50us no dmesg, reverter" no body do patch                                                                           | commit/ADR contém critério numérico de reversão                                                                              |
| 3   | Número não adjetivo      | Métrica antes de adjetivo                                                                             | "latência DMA reduziu de 420ns → 98ns em 3 rodadas, stddev 4%" não "rápido"                                                                       | nenhum claim de perf sem unidade + n de rodadas                                                                              |
| 4   | Anchoring                | Reference class explícita antes de bottom-up                                                          | rewrite Go com `drm/amdgpu` como ancoragem (~3× estimativa inside view)                                                            | ADR/roadmap cita repo de referência                                                                                          |
| 5   | Availability heuristic   | Worst-case, não happy path                                                                            | `docs/reliability/DEGRADATION-MATRIX.md` cobre falha de rede total, OOM Killer acionado, PCIe bus reset, falha no IOMMU                                | matrix existe e foi atualizada na última feature                                                                             |
| 6   | Confiança calibrada      | Intervalo, não ponto                                                                                  | "~300 MB/s ±10%" não "300 MB/s"; "65% chega em produção" não "vai funcionar"                                                               | resposta com número carrega faixa                                                                                            |
| 7   | Hindsight bias           | Postmortem separa processo de outcome                                                                 | `docs/postmortems/` distingue "decidiu certo, deu errado" vs "funcionou por acidente"                                                    | template usado, ambas categorias presentes                                                                                   |
| 8   | Planning fallacy         | Multiplicador de reference class explícito                                                            | "estimativa inside view: 1 sem; ajustada por reference class drm/amdgpu: 3 sem"                                                    | roadmap cita inside view E adjusted                                                                                          |
| 9   | Substituição de pergunta | Qualitativa vira métrica                                                                              | "boa arquitetura?" → 0 warnings no checkpatch, ausência de deadlocks via lockdep, 0 vazamentos no kmemleak                                            | resposta qualitativa vem com critério numérico                                                                               |
| 10  | Hyperbolic discounting   | Refactor enquanto barato; sem TODO-later                                                              | `grep -r "TODO.*later\|FIXME"` retorna vazio                                                                                             | grep limpo no diff                                                                                                           |
| 11  | Halo effect              | Lib nova cita regra/ADR/evidência mensurável                                                          | dependência nova cita ADR explícita ou esta disciplina (#11)                                                                             | patch de dep referencia ADR ou disciplina                                                                                       |
| 12  | Priming em prompts       | Framing adversarial                                                                                   | "que problema você encontra?" não "está bom?"                                                                                            | template de patch review usa framing neutro/adversarial                                                                         |
| 13  | Ilusão de validade       | Teste/probe só vale se pôde falhar pelo motivo certo; valide propósito, não happy path nem existência | guard de delete privilegiado recusava root-owned e teste verde afirmava a recusa (#59/DT-v2-20); fix = integração contra root-owned real | ferramenta destrutiva tem teste de integração do modo de falha real; teste de recusa vem pareado com "input legítimo passa?" |
| 14  | Mass-refactoring fallacy | Eliminação de ruído arquitetural fatiada ortogonalmente; nunca reescrever o repo inteiro de uma vez   | "aplique o superprompt só nos hooks do drm/ramshared" ou "padronize só a struct page e flags de memória" → cada fatia um commit atômico               | patch de auditoria tem commits atômicos por serviço/componente, não um `refactor: clean codebase` gigante                       |

---

### 1. WYSIATI — What You See Is All There Is

**Viés.** Modelo responde com confiança sobre o que não viu.
Alucinação por omissão.

**Regra.** Antes de opinar em decisão crítica, listar explicitamente
o que NÃO foi visto. Pedir os arquivos antes de analisar.

**Sinal de que funciona.** Respostas começam com
"sem ter visto X, estimo Y com confiança Z%".

**Como aplicar em patch review:** se reviewer não leu o teste, reject.

### 2. Counterfactual obrigatório

**Viés.** Opinião sem condição de revisão é Sistema 1 disfarçado.

**Regra.** Toda decisão carrega `O que me faria mudar de opinião?`.
Se resposta é "nada", é Sistema 1. Resposta válida é específica:
"se p99 > 500ms, reverter" ou "se TLB shootdown stall > 1ms,
adiar promoção".

**Sinal.** Commit messages contêm o critério numérico de reversão.

**Como aplicar:** todo patch de mudança não-trivial tem `Rollback trigger:`
no body.

### 3. Sistema 1 → Sistema 2 via número

**Viés.** Substituição de pergunta: "é bom?" (difícil) vira "parece
bom?" (fácil).

**Regra.** Claim precisa de número, não adjetivo. Ruim: "mais rápido".
Bom: "p99 reduziu de 420ns para 98ns em 3 rodadas, stddev 4%".

**Sinal.** Nenhum commit message contém "rápido/elegante/eficiente/
limpo" sem número acompanhando.

**Anti-padrão explícito:** "é claro que", "obviamente", "definitivamente".

### 4. Anchoring em estimativas

**Viés.** Primeiro número que aparece ancora todos os outros.

**Regra.** Estimativas de prazo/escopo começam em **reference class**,
não bottom-up. O oráculo do ecossistema RamShared é `drm/amdgpu`
(rewrite C → Rust que levou ~3× a estimativa inicial original). Use
esse multiplicador pra projetos similares.

**Sinal.** Estimativas de sprint citam reference class explicitamente.

### 5. Availability heuristic

**Viés.** Modelo lembra do caso frequente, esquece do raro mas caro.

**Regra.** Listar explicitamente cenários raros antes de decidir: falha
de rede total, PgBouncer reiniciando, Redis cluster failover. Design
pra worst-case, não pra happy path.

**Sinal.** `docs/reliability/DEGRADATION-MATRIX.md` existe e é atualizado.

### 6. Overconfidence → confiança calibrada

**Viés.** "Exatamente X" é arrogante. Intervalo é honesto.

**Regra.** Respostas com números carregam faixa: "~300 MB/s ±10%"
em vez de "300 MB/s". Probabilidades: "chega em produção com 65%" em
vez de "vai funcionar".

**Sinal.** Respostas nunca usam "com certeza" sem número.

### 7. Hindsight bias

**Viés.** Resultado bom ⇒ processo bom. Resultado ruim ⇒ processo
ruim. Ambas falsas.

**Regra.** Decisão é avaliada pelo **processo na hora**, não pelo
outcome. "Falhou mas o processo estava certo" é válido. "Funcionou
por acidente" é alarme.

**Sinal.** Postmortems em `docs/postmortems/` separam processo de
resultado.

### 8. Planning fallacy

**Viés.** Inside view (bottom-up "3 tarefas × 2h") é sistematicamente
otimista.

**Regra.** Estimativa final multiplica a bottom-up pela taxa média de
overrun da reference class. Para rewrites Go no RamShared, esse multiplier
é ~3× (baseado em drm/amdgpu).

**Sinal.** Roadmap cita "estimativa inside view: X; ajustada por
reference class: 3X".

### 9. Substituição de pergunta

**Viés.** Pergunta difícil vira pergunta fácil sem o humano notar.
"Essa arquitetura é boa?" vira "parece organizada?".

**Regra.** Se a pergunta é qualitativa, transformar em métrica antes
de responder. "Boa arquitetura" vira "0 warnings no checkpatch, ausência de deadlocks via lockdep, 0 vazamentos no kmemleak".

**Sinal.** Respostas qualitativas sempre seguidas de critério
numérico.

### 10. Hyperbolic discounting em débito técnico

**Viés.** "Depois eu refatoro" desconta dramaticamente custo futuro.

**Regra.** Débito técnico = dívida com juros. Refactor enquanto é
barato, não quando dói. Se achou código morto, remover na hora — não
virar `TODO: refactor later`.

**Sinal.** `grep -r "TODO.*later\|FIXME"` retorna vazio.

### 11. Halo effect em ferramentas

**Viés.** "Funcionou em projeto A, vira default em B/C/D sem
reavaliação."

**Regra.** Cada lib/framework nova precisa citar a regra ou decisão
que justifica a adoção: `CLAUDE.md` para comandos, arquitetura e
contratos globais; `.claude/rules/documentation.md` para o mapa de
documentação; `.claude/rules/coding.md` para convenções, dependências
e checkpoints. Sem regra, ADR ou evidência mensurável, a lib não entra.

**Sinal.** patches com dependência nova citam o arquivo de regra, ADR ou
prova objetiva que sustenta a decisão.

### 12. Priming em prompts

**Viés.** Framing muda a resposta. "Qual problema tem esse código?"
≠ "Esse código está ok?".

**Regra.** Ao pedir review, sempre enquadrar neutro ou adversarialmente:
"que problema você encontra neste código?" em vez de "esse código
está bom?".

**Sinal.** Templates de patch review usam framing adversarial.

### 13. Ilusão de validade

**Viés.** Teste verde ou probe de existência (`--check`) dá confiança coerente
mas não-preditiva: o teste pode encodar a MESMA suposição errada do código.
Pior num boundary — afirmar uma RECUSA tranca a suposição de que o input
recusado é ilegítimo, quando ele é o propósito da ferramenta.

**Regra.** Valide contra o PROPÓSITO e o pior caso, não o happy path nem a
existência (existe ≠ funciona). Pareie todo teste de recusa com "o legítimo
ainda passa?". Comportamento privilegiado/destrutivo (sudo, rm, chown, DROP,
systemd, docker) exige gate de integração contra o modo de falha real, não
mock hermético nem probe de existência.

**Sinal.** Ferramenta destrutiva tem teste de integração do modo de falha real;
todo teste de recusa carrega o par "legítimo passa".

### 14. Falácia do Refatoramento em Massa (Mass-Refactoring Fallacy)

**Viés.** _WYSIATI_ e _Planning Fallacy_ combinados. A crença de que o Sistema 1 consegue antecipar todas as dependências ocultas de uma reescrita global.

**Regra.** Toda eliminação de "Ruído" arquitetural (usando o [`SUPERPROMPT.md`](SUPERPROMPT.md) ou IA autônoma) deve ser **fatiada ortogonalmente**. Nunca peça para a IA reescrever o repositório inteiro. O escopo deve ser fechado por padrão de ruído ou por diretório (ex: "aplique o superprompt apenas nos hooks do drm/ramshared" ou "padronize apenas a struct page e flags de memória nas patches"). Cada fatia vira um commit atômico.

**Sinal.** patches de auditoria de ruído contêm commits atômicos isolados por serviço ou componente, em vez de um gigantesco `refactor: clean codebase`.

---

## Mapeamento disciplina → rubrica/ADR no ramshared

Cada disciplina tem ancoragem operacional concreta no monorepo. A tabela
abaixo é o mapa para citar em patch review ou ADR.

> **Nota sobre rubricas:** muitas disciplinas (#1, #3, #6, #10, #11) ainda
> não têm rubrica externa formalizada — o sinal é checado em patch review
> manual e a regra vive nesta seção do doc. Mover para
> `.claude/rules/coding.md` ou criar `docs/runbooks/REVIEW-ADR.md` é
> trabalho futuro (cf. seção "Auto-aplicação" no fim).

| Disciplina                  | Rubrica/artefato no ramshared                                                                                                                                                         | Como o sinal é checado                                                                                                                       |
| --------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| #1 WYSIATI                  | patch review (manual)                                                                                                                                                                | reviewer rejeita resposta sem "sem ter visto X..." quando aplicável                                                                          |
| #2 Counterfactual           | ADRs em `docs/decisions/` com critério numérico/observável de reversão (formato livre dentro do ADR; futuro: padronizar via seção `## Rollback trigger`)                          | revisão periódica de ADRs ativos checa se há trigger numérico                                                                                |
| #3 Número não adjetivo      | patch review (regra nesta seção #3)                                                                                                                                                  | patch com claim de perf sem número é bloqueado em review                                                                                        |
| #4 Anchoring                | ADRs de rewrite citam reference class (ex.: `ADR-056-rust-for-linux-adoption.md`)                                                                                                 | ADR sem reference class é Sistema 1                                                                                                          |
| #5 Availability heuristic   | `docs/reliability/DEGRADATION-MATRIX.md` + `ADR-007-numa-nodes.md` + `ADR-008-pcie-resets.md`                                                                       | matrix versionada por feature crítica                                                                                                        |
| #6 Confiança calibrada      | patch review (regra nesta seção #6)                                                                                                                                                  | "com certeza" sem número é flag em review                                                                                                    |
| #7 Hindsight bias           | `docs/postmortems/TEMPLATE.md` (separa `## Análise de processo` de outcome)                                                                                                       | postmortem sem essa separação é incompleto                                                                                                   |
| #8 Planning fallacy         | Roadmap/ADR cita inside view + adjusted (multiplier reference class explícito)                                                                                                    | sprint sem multiplier é Sistema 1                                                                                                            |
| #9 Substituição de pergunta | SSDV3 PASSO 3 `### Validação final` (SSDV3-PROMPTS.md) — checklist verificável quando pergunta é qualitativa                                                                      | qualitativa solta vem com critério numérico                                                                                                  |
| #10 Hyperbolic discounting  | regra nesta seção #10 + patch review (TODOs novos no diff; futuro: invariante CI grep)                                                                                               | grep manual ou em diff de patch                                                                                                                 |
| #11 Halo effect             | regra nesta seção #11 (patch de dep cita ADR ou esta disciplina)                                                                                                                     | patch de dep sem citação é bloqueado em review                                                                                                  |
| #12 Priming em prompts      | Templates de patch review e SSDV3 prompts (manual; framing neutro/adversarial)                                                                                                       | review com pergunta enviesada é flag cultural                                                                                                |
| #13 Ilusão de validade      | Teste de integração `CONFIG_KASAN=y ou KVM tests` por ferramenta privilegiada (ex.: `tools/testing/selftests/ramshared/vram_test.c`); deploy valida função, não só `--check` | patch de ferramenta destrutiva sem teste de integração do modo de falha real é bloqueado; teste de recusa sem par "input legítimo passa" é flag |

Artefatos comuns confirmados: `CLAUDE.md`, `AGENTS.md`,
`.claude/rules/{coding,kernel,ssdv3,governance}.md`, `docs/decisions/`, `docs/postmortems/`,
`docs/reliability/DEGRADATION-MATRIX.md`,
`docs/SSDV3-PROMPTS.md`.

---

## Princípios de Noise (o segundo livro)

**Noise** é variação aleatória em decisões profissionais. Mesmo juiz,
dia diferente, humor diferente = sentença diferente. Código não sofre
isso exatamente como corte, mas o paralelo existe:

- **Mesma AI, prompt ligeiramente diferente, resposta drasticamente
  diferente.** Solução: prompts estruturados, com checklist fixo.
- **Mesmo código, reviewer diferente, feedback diferente.** Solução:
  rubrica de review em vez de opinião livre.
- **Mesmo produto, semana diferente, decisão de priorização
  diferente.** Solução: critério numérico escrito para priorização.

As disciplinas #1–14 acima são principalmente anti-bias. Anti-noise
precisa de **rubricas** — listas fixas de critérios que transformam
julgamento em procedimento.

### Rubricas ativas no RamShared

| Procedimento               | Rubrica                                                                             |
| -------------------------- | ----------------------------------------------------------------------------------- |
| Trabalho spec-driven       | `docs/SSDV3-PROMPTS.md`                                                             |
| patch review                  | `AGENTS.md` checklist + `.claude/rules/coding.md`                                   |
| Slice kernel/lkm     | `.claude/rules/kernel.md`, `.claude/rules/kernel.md`, `.claude/rules/testing.md` |
| Documentação e padrões     | `CLAUDE.md`, `AGENTS.md`, `.claude/rules/documentation.md`                          |
| Segurança e infraestrutura | `.claude/rules/security.md`, `.claude/rules/infra.md`, `.claude/rules/kernel.md`      |

**Sinal de que Noise reduzido:** decisões sucessivas no mesmo tipo
de problema produzem patches com forma similar.

---

## O counterfactual como gatekeeper

A meta-regra final: se você (humano ou IA) não consegue responder
**"o que me faria mudar de opinião?"** com algo específico, pare.
Você está em Sistema 1.

Exemplos válidos:

- "se benchmark com 3 rodadas mostrar regressão > 5%, reverter"
- "se TLB shootdown stall > 1ms em 7 dias, adiar promoção"
- "se fixture real do upstream falhar parsear, atualizar parser antes
  de merge"

Exemplos inválidos:

- "sei lá, depende"
- "se der errado"
- "quando as coisas mudarem"

---

## Contra-exemplos por disciplina

Toda disciplina vira **comportamento de Sistema 1 disfarçado** quando
aplicada sem juízo. Os 12 modos de falha das disciplinas anti-bias (#1–12)
agrupam em 4 padrões abaixo; #13 (ilusão de validade) e #14 (mass-refactoring
fallacy) trazem o contra-exemplo na própria seção detalhada:

| Padrão               | Disciplinas    | Sintoma comum                              |
| -------------------- | -------------- | ------------------------------------------ |
| Forma sem conteúdo   | #2, #3, #6     | compliance formal, valor zero              |
| Over-engineering     | #5, #8         | custo presente para risco hipotético       |
| Atrito injustificado | #10, #11, #12  | fricção sem ganho de qualidade             |
| Eliminação de nuance | #1, #4, #7, #9 | regra substitui pensamento em vez de guiar |

| #   | Vira...                       | Sintoma concreto no ramshared                                                                                                     | Mitigação                                                                                                             |
| --- | ----------------------------- | ----------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| 1   | Paralisia                     | listar TODA ignorância em todo PRD trava SSDV3 PASSO 1                                                                        | PRD agrupa "Inferências" em seção separada, capada em ~30% do total                                                   |
| 2   | Teatro                        | "Rollback trigger: se der errado, reverter" passa lint mas é vazio                                                            | revisão trimestral de ADRs recusa frase sem unidade/janela                                                            |
| 3   | Métrica sem contexto          | "p99 -30%" inútil se bench rodou sem `-race` em máquina diferente                                                             | SSDV3 PASSO 3 `### Validação final` (SSDV3-PROMPTS.md) exige descrever ambiente (CPU, RAM, concorrência, n rodadas)   |
| 4   | Anchor errado                 | reference class `drm/amdgpu` (rewrite Node→Go com integração externa) pode subestimar feature jurídica pesada de schema | ADR cita reference class mais próxima (`drm/nouveau`, `drivers/cxl`, `mm/hmm`) quando domínio diverge |
| 5   | Paranoia                      | DEGRADATION-MATRIX cobre cenários raros mas com custo de design alto sem priorização explícita                                | matrix prioriza por probabilidade × impacto; cenários cosméticos ficam como "monitorado, não desenhado"               |
| 6   | Escape                        | "p99 220ms ±50%" cobre tanto "passa" quanto "falha"                                                                           | stddev > 25% aciona investigação, não aceitação                                                                       |
| 7   | Desculpa                      | "Foi sorte genuína" vira racionalização de bug em postmortem                                                                  | template §"Causa raiz" exige dimensão técnica precisa antes de fechar como "sorte"                                    |
| 8   | Inflação                      | multiplier 3× drm/amdgpu vira justificativa pra qualquer estimativa inflada                                             | postmortem por feature que estourou >50% do adjusted; multiplier não é desculpa                                       |
| 9   | Eliminação de pergunta válida | forçar tudo em métrica destrói nuance UX (ex.: "qualidade percebida")                                                         | RNF aceita qualitativo se acompanhado de teste com 3+ humanos não-autores                                             |
| 10  | Scope creep                   | patch fix-only que remove TODOs antigos vira patch de 50 arquivos                                                                   | grep CI mira TODO **novo** no diff, não pré-existente                                                                 |
| 11  | Not-Invented-Here             | recusar lib boa "porque não está em coding.md" sem testar                                                                     | template de adoção é leve (2 alternativas + métrica + rollback); ADR documenta razão                                  |
| 12  | Degradação de colaboração     | "Qual problema tem?" em pair programming dá ar de desconfiança                                                                | framing rota — em patch review e SSDV3, usar; em pair, não                                                               |

**Meta-contra-exemplo: cargo cult.** Disciplinas aplicadas sem
skin-in-the-game viram ritual: ADR commita rollback trigger numérico
mas trigger nunca é executado quando condição dispara — disciplina
existe só no template, não na mente. Mitigação: revisão periódica
(futuro: formalizar em `docs/runbooks/REVIEW-ADR.md` a criar) percorre
rollback triggers ativos em `docs/decisions/` e documenta acionamentos.
Hoje, postmortems em `docs/postmortems/` capturam acionamentos quando
ocorrem (ou justificam não-acionamento).

---

## Como a IA deve usar este doc

Quando o Claude (LLM) faz uma decisão no projeto:

1. **Antes de opinar:** declarar o que não viu (WYSIATI)
2. **Com a opinião:** número, não adjetivo (Sistema 2)
3. **Depois da opinião:** counterfactual específico

Quando o Claude escreve commit message:

1. **Title:** o que mudou (imperativo)
2. **Body:** por que com número ou referência a incidente/constraint
3. **Body opcional:** rollback trigger se mudança não-trivial

Quando o Claude é pedido pra avaliar código ou arquitetura:

1. Pedir métrica específica antes de responder
2. Se o humano não tem métrica, oferecer uma rubrica
3. Se nem rubrica existe, avisar que a resposta vai ser Sistema 1

Quando o Claude cria um PR:

1. `Rollback trigger:` no body se aplicável
2. Testes cobrem counterfactual (se X, então falhar)
3. `CLAUDE.md`, `AGENTS.md` e `.claude/rules/*` atualizados se mudou
   regra de trabalho

## Não-escopo deste doc

- **Não garante qualidade.** Reduz variância. Produz ganhos
  cumulativos ao longo de meses.
- **Não substitui review humano.** Cria estrutura para review ficar
  menos enviesado.
- **Não é filosofia.** É operacional. Cada regra tem sinal mensurável
  de que funciona.

## Leituras cruzadas

- `docs/SSDV3-PROMPTS.md`
  — prompt SDD canônico
- `CLAUDE.md` e `AGENTS.md` — regras operacionais de agentes
- `.claude/rules/` — rubricas específicas de código, docs, testes,
  segurança e infraestrutura
- `ramshared/docs/decisions/` — ADRs (anti-halo)
- `ramshared/docs/postmortems/` — anti-hindsight + anti-noise em análise
  de incidentes
- Kahneman, _Thinking, Fast and Slow_ (2011)
- Kahneman, Sibony, Sunstein, _Noise_ (2021)

---

## Auto-aplicação: rollback trigger deste doc

A disciplina #2 (counterfactual obrigatório) aplicada ao próprio
documento. Se a meta-disciplina for teatro, este doc também pode
falhar — e tem condição de morte declarada:

- **Sinal de adoção real:** ≥30% dos patches não-triviais (`feat`, `fix`, `refactor`, `perf`) citam alguma disciplina (#1–14) no body, em
  comentário de review, ou em ADR vinculada.
- **Rollback trigger:** se em **6 meses (revisão obrigatória)** menos
  de 30% dos patches não-triviais citarem alguma disciplina, simplificar
  para Top-5 + 1-pager via ADR superseding. Sinal de cargo cult em
  escala.
- **Auditoria:** revisão trimestral (futuro: `docs/runbooks/REVIEW-ADR.md`
  a criar) cruza commits de patches não-triviais × disciplinas citadas.
  Hoje, métrica é manual via grep periódico em commits e patch review
  notes. Resultado documenta na próxima entrada de `docs/postmortems/`
  se houver desvio.

Se este trigger nunca disparar e disciplinas continuarem citadas, o
método está vivo. Se disparar, é Kahneman aplicado ao próprio Kahneman:
heurística que parou de funcionar é dívida.
