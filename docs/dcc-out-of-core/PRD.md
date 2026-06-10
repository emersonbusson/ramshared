# PRD — Fase C: RAM-as-VRAM, tier out-of-core para DCC (Blender/Cycles)

> **ABSORVIDO pelo PRD unificado [`docs/memory-broker/PRD.md`](../memory-broker/PRD.md)
> (2026-06-09).** Mantido como documento de origem; a SPEC sai do unificado.

> SSDV3 PASSO 1. Slug: `dcc-out-of-core`. Origem: conversa Emerson ↔ Alex Santos (2026-06-09).
> Direção **inversa** da Fase B: lá VRAM→SO (swap); aqui RAM→app de DCC (cache out-of-core).
> Tester real disponível: Alex (artista 3D, cenas reais que estouram a VRAM).

## 1. Resumo

Artistas 3D em GPUs consumer estouram a VRAM com cena grande e o render falha (CUDA OOM) ou cai
para CPU — enquanto a RAM do sistema fica ociosa. A Fase C entrega um **tier gerenciado RAM-abaixo-
da-VRAM** para o DCC: a cena maior que a VRAM renderiza usando a RAM como backing store, com
staging sob demanda para a VRAM. Produto-alvo: **addon do Blender** (mercado: SuperHive) + backend.

## 2. Contexto técnico

- **Problema (Confirmado em relato do tester):** Alex passou **dias** otimizando cena à mão
  (reduzir texturas, decimate, dividir passes) para caber na VRAM e conseguir renderizar — com a
  RAM sem uso. Dor recorrente ("já precisei várias vezes").
- **Física (Confirmado):** a GPU só computa sobre dado na VRAM. "RAM além da VRAM" = backing store
  maior + streaming pela PCIe (µs, GB/s) — nunca latência de VRAM local. O ganho é **caber**; o
  custo é streaming. Mesma lógica de tiering da Fase B, consumidor diferente (app, não SO).
- **Confirmado em docs CUDA:** oversubscription transparente por page-fault (UVM,
  `cudaMallocManaged`) só existe em **Linux** (Pascal+); no **Windows/WDDM não há demand paging**
  (modelo pré-Pascal). Relevante: DCC users (Alex) rodam Blender no Windows.
- **Confirmado em docs Blender:** Cycles/CUDA tem fallback de **memória do host** (pinned,
  zero-copy) para cena que não cabe na VRAM, com custo de performance. **A verificar na F0:**
  cobertura (texturas vs geometria/BVH), comportamento no backend **OptiX** (o default moderno) e
  por que falhou/não bastou nos casos do Alex.
- **Reuso honesto (Confirmado no codebase):** da nossa stack, o que serve aqui é a camada
  `ramshared-cuda` (FFI por `dlopen` da Driver API — portável para `nvcuda.dll`), a telemetria/
  canário e a disciplina de tiering. **ublk/io_uring/NBD NÃO se aplicam** (DCC não quer block
  device; quer alocador/cache).

## 3. Opção recomendada

**Measurement-first (F0 com as cenas reais do Alex), MVP no molde "configurar/orquestrar o que já
existe", e o tier custom como v2 diferenciado.** Os três moldes:

1. **UVM transparente** (`cudaMallocManaged`): quase zero mudança no app, mas oversubscription real
   é **Linux-only** → não atende o Alex no Windows. Vale como modo "Linux power-user".
2. **Out-of-core do próprio renderer:** Cycles já tem fallback p/ RAM (CUDA backend). Um **addon
   Python** consegue: detectar a cena que não cabe (estimar footprint vs VRAM via NVML), trocar
   OptiX→CUDA quando preciso, ativar/ajustar o fallback, gerar proxies/mipmaps de textura
   não-destrutivos, prever "cabe/não cabe" antes do render e monitorar o spill. **É vendável e
   barato — MVP.**
3. **Tier custom nosso (interposer CUDA):** lib que intercepta a Driver API (hook em
   `cuMemAlloc`/texturas — o conhecimento do nosso `ffi.rs` aplica direto) e gerencia residência
   VRAM↔RAM com política melhor que a do driver: prefetch por tile, pinning do working set,
   compressão opcional (lz4/zstd) na RAM. **Diferencial real de produto, alto risco/esforço → v2,
   gated pelos números da F0/MVP.**

**Por que nessa ordem (anti-halo / Kahneman #5):** não construir o interposer sem antes provar com
as cenas do Alex que o out-of-core nativo (a) não resolve, ou (b) resolve mal o suficiente para
justificar v2. Se o nativo resolver com boa UX, o MVP já elimina a dor (e valida o mercado).

## 4. Requisitos funcionais (RF)

- **RF-1 (F0)** Reproduzir ≥2 cenas reais do Alex que falham; registrar: SO, GPU/VRAM, RAM, versão
  do Blender, backend (OptiX/CUDA), erro exato, pico de VRAM/RAM (NVML + OS).
- **RF-2 (F0)** Medir o out-of-core nativo do Cycles nessas cenas (CUDA backend): renderiza? tempo
  vs CPU-only vs cena-otimizada-à-mão? o que transborda (texturas? geometria?)?
- **RF-3 (MVP)** Addon "render maior que a VRAM": predição cabe/não-cabe (footprint estimado vs
  VRAM livre), configuração automática do caminho out-of-core (backend + flags), proxies/mipmaps
  não-destrutivos opcionais, monitor de VRAM/spill durante o render.
- **RF-4 (MVP)** Telemetria honesta no addon: quanto foi para a RAM, custo de tempo vs estimativa.
- **RF-5 (v2, gated)** Interposer de residência (molde 3) com prefetch/pinning; só se F0/MVP
  provarem necessidade (gate numérico: cena do Alex que o MVP não destrava, ou perda de tempo >2×
  vs working-set-na-VRAM).

## 5. Requisitos não-funcionais (RNF)

- **RNF-1** Plataforma do usuário-alvo: **Windows primeiro** (onde o Alex e o mercado estão);
  Linux como bônus (lá UVM dá o modo transparente).
- **RNF-2** Addon não pode corromper cena (.blend intocado; proxies são não-destrutivos/reversíveis).
- **RNF-3** Zero dependência de driver hackeado no MVP (só APIs públicas: Python do Blender, NVML).
- **RNF-4** v2 (interposer): `unsafe` confinado no crate FFI (padrão do projeto); falha do
  interposer degrada para o caminho nativo, nunca crash do Blender.

## 6. Fluxos

1. **MVP:** artista abre cena grande → addon estima footprint vs VRAM → avisa "não cabe; habilitar
   modo out-of-core?" → configura backend/flags/proxies → render passa → relatório (spill, tempo).
2. **F0 (com o Alex):** ele roda um script/checklist nosso nas cenas que falham → coleta números →
   decide o molde com dados.
3. **v2:** mesmo fluxo do MVP, mas com o interposer dando working-set management (prefetch/pinning)
   em vez do paging genérico do driver.

## 7. Modelo de dados

MVP é addon Python (estimativas por imagem/mesh, config, log). v2: tabela de residência
`{alloc_id, tamanho, local: Vram|Ram, pinned, último_uso}` no interposer — detalhar na SPEC da v2.

## 8. API / Interfaces

- MVP: Blender Python API + NVML; sem mudança no RamShared atual.
- v2: lib interposer (Rust, `cdylib`) carregada via mecanismo de injeção a definir na SPEC
  (LD_PRELOAD no Linux; no Windows, hook de `nvcuda.dll` — pesquisa F0).

## 9. Dependências e riscos

- **Risco A — o nativo já resolve** e o diferencial v2 não se justifica → o MVP ainda é produto
  (UX/predição/proxies), mas reavaliar o investimento v2. (Por isso F0 antes de tudo.)
- **Risco B — OptiX:** se o caso do Alex exigir OptiX (RTX cores) e OptiX não suportar o
  transbordo necessário, a troca p/ CUDA custa tempo de render — medir na F0.
- **Risco C — Windows hooking (v2):** interceptar `nvcuda.dll` é frágil entre versões de driver.
  Mitigação: gate v2 + degradação para caminho nativo (RNF-4).
- **Risco D — expectativa:** "RAM vira VRAM rápida" não existe (PCIe). O addon precisa comunicar o
  trade-off (caber vs velocidade) — telemetria RF-4.
- **Dependência:** disponibilidade do Alex (cenas reais + rodar F0). Confirmada ("eu testo de boa").

## 10. Estratégia de implementação

**F0** checklist+script de medição com o Alex (sem produto) → decide molde com números. →
**MVP** addon (molde 2, RF-3/4) nas cenas dele → valida dor + mercado. → **v2** interposer (molde 3,
RF-5) somente se o gate numérico mandar. Pipeline SSDV3 por fase (SPEC antes de IMPL).

## 11. Documentos a atualizar

`docs/dcc-out-of-core/{SPEC,IMPL}.md` por fase; `MEMORY.md`; README (nova frente Fase C).

## 12. Fora de escopo

Reescrever/patch do Cycles; outros renderers no MVP (Redshift/Octane já têm out-of-core próprio);
render distribuído/rede; modelo de negócio do addon (preço/licença — decisão à parte, fora do PRD
técnico); competir com RAM em latência (física).

## 13. Critérios de aceitação

- F0: números das cenas do Alex documentados (footprint, falha, comportamento do nativo).
- MVP: ≥1 cena real do Alex que **falhava** renderiza **sem edição manual da cena**; tempo e spill
  reportados; .blend intocado.
- A dor "dias otimizando à mão" eliminada nesses casos (depoimento do tester).
- v2 só inicia se o gate do RF-5 disparar.

## 14. Validação

Suite de benchmark = cenas reais do Alex (com versões reduzidas reprodutíveis). Métricas: pico
VRAM/RAM, tempo de render (nativo vs MVP vs v2), spill (GB), crashes (zero). Kahneman #2
(counterfactual): se o modo out-of-core deixar o render >3× mais lento que a cena otimizada à mão,
o produto não elimina a dor — reavaliar antes de v2.

## Anexo — perguntas para o tester (F0)

1. SO e versão (Windows 10/11?), GPU (modelo/VRAM), RAM total.
2. Versão do Blender e backend usado (OptiX ou CUDA? sabe dizer?).
3. 1-2 arquivos .blend (ou descrição: nº de objetos, resolução/quantidade de texturas) que
   **falharam** por VRAM, e a mensagem de erro exata.
4. O que você tentou (reduzir texturas? decimate? tiles?) e quanto tempo perdeu.
5. Aceita rodar um script de medição nosso (lê VRAM/RAM durante o render, não altera a cena)?
