# Benchmark & Measurement Rules — RamShared

> Como capturar benchmarks de forma **íntegra e reaproveitável**. Toda medição de performance que
> embasa uma decisão (gate P0, go/no-go, detecção de regressão) segue isto. Liga-se ao **SSDV3**
> (gate numérico P0, "número não adjetivo") e às **disciplinas Kahneman** (#3 número, #1 registrar o
> estado/WYSIATI, #5 pior caso / carga real).

## Quando aplicar

Qualquer medição comparativa ou de performance que vá embasar uma decisão (escolher backend, aprovar
fase, detectar regressão). Microbench exploratório e descartável não precisa — mas **se o número for
citado em doc, PR ou decisão, vira benchmark registrado** e segue esta regra.

## Validade da medição (rodar certo)

- **≥3 rodadas** por célula; reportar **mediana + p99 + desvio** (1 amostra mente). Alinhado ao P0.
- **Concorrentes lado-a-lado no MESMO snapshot de carga** (ex.: VRAM-swap vs disco medidos na mesma
  janela/condição — **nunca** em momentos diferentes; comparar momentos distintos foi o viés que
  inflou conclusões no passado).
- **Parâmetros fixos e versionados** (bs, qd, size, runtime, ramp descartado). Mudou parâmetro → run nova.
- **Carga realista quando for o caso** (#5): idle limpo mente se o uso real é com a máquina cheia.
  Toda run leva uma **tag de condição** (`idle` | `loaded`).
- **Bounded e não-disruptivo no host vivo.** NUNCA thrash de swap/ublk no WSL2 (CONGELA o host +
  derruba os apps do usuário). Pressão real só em VM/qemu/civm **isolada** (DT-29).

## Integridade do registro (guardar certo)

- **Captura de contexto AUTOMÁTICA** (nada manual = nada esquecido): horário, branch+commit (+dirty),
  kernel, GPU (`nvidia-smi`: VRAM usado/livre), RAM/swap, disco (util/latência) e **o que estava
  aberto** (apps GUI do Windows + top procs WSL2). O contexto **é dado**: o mesmo número significa
  coisas diferentes com a máquina ociosa ou cheia.
- **Saída dupla:** dados máquina-legíveis em **`docs/benchmarks/results.jsonl`** (1 linha por run →
  trendar, comparar entre commits, plotar) **e** entrada humana em **`docs/BENCHMARKS.md`**.
- **Append-only:** nunca reescrever entradas antigas; cada run = entrada nova com `run-id`.
- **Raw output guardado** (ou reproduzível) — para reauditar se um parse estiver errado.
- **Reprodutível:** o harness grava o comando exato; re-rodar = mesma invocação.

## Artefatos (onde mora o quê)

| Artefato | Papel |
| --- | --- |
| `scripts/p0/bench.sh` | Harness: captura contexto + roda N vezes + agrega + escreve nos 2 destinos |
| `scripts/p0/measure-*.sh` | Benchmarks específicos (fio, headroom, swap-compare, …) |
| `docs/BENCHMARKS.md` | Log **humano**, append-only (template no topo) |
| `docs/benchmarks/results.jsonl` | Dados **máquina-legíveis** (1 linha/run) |
| `docs/memory-broker/P0-RESULTS.md` | Decisões consolidadas (go/no-go) — gate SSDV3 |

## Ligação com SSDV3 / Kahneman

- O **gate numérico P0** do SSDV3 (`P0-RESULTS.md`) consome benchmarks que seguem esta regra.
- Disciplinas: **#3** (número + unidade + n + data + ambiente), **#1** (registrar o estado — WYSIATI),
  **#5** (pior caso / carga real). **Anti-halo (#11):** o número de uma fase não "aprova" a próxima.
