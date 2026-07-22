# Benchmark & Measurement Rules — RamShared

> How to capture benchmarks in an **integral and reusable** manner. Any performance measurement that
> supports a decision (P0 gate, go/no-go, regression detection) follows this. It links to **SSDV3**
> (numerical P0 gate, "number before adjective") and to the **Kahneman disciplines** (#3 number, #1 record the
> state/WYSIATI, #5 worst case / real load).

## When to apply

Any comparative or performance measurement that will support a decision (choosing a backend, approving
a phase, detecting regression). Exploratory and disposable microbenchmarks are not required — but **if the number is
cited in a doc, PR, or decision, it becomes a registered benchmark** and must follow this rule.

## Measurement validity (run correctly)

- **≥3 runs** per cell; report **median + p99 + deviation** (1 sample lies). Aligned with P0.
- **Competitors side-by-side in the SAME load snapshot** (e.g., VRAM-swap vs disk measured in the same
  window/condition — **never** at different times; comparing different moments was the bias that
  inflated conclusions in the past).
- **Fixed and versioned parameters** (bs, qd, size, runtime, ramp discarded). Parameter changed → new run.
- **Realistic load when applicable** (#5): clean idle lies if real usage is with a loaded machine.
  Every run carries a **condition tag** (`idle` | `loaded`).
- **Bounded and supervised on the live host.** Unsupervised swap/ublk thrashing on WSL2 is forbidden
  because it can freeze the host and crash user apps. Real pressure should prefer an isolated
  VM/qemu/civm. When the explicit target is the shared daily WSL2 host, it must go through
  `scripts/windows/Invoke-SharedWslPressureCampaign.ps1` with approval, a Windows-side watchdog,
  cgroup-bounded pressure, telemetry, and cleanup artifacts.

## Registration integrity (store correctly)

- **AUTOMATIC context capture** (nothing manual = nothing forgotten): timestamp, branch+commit (+dirty),
  kernel, GPU (`nvidia-smi`: VRAM used/free), RAM/swap, disk (util/latency), and **what was
  open** (Windows GUI apps + WSL2 top processes). Context **is given**: the same number means
  different things with an idle or loaded machine.
- **Dual output:** machine-readable data in **`docs/benchmarks/results.jsonl`** (1 line per run →
  trend, compare between commits, plot) **and** human input in **`docs/BENCHMARKS.md`**.
- **Append-only:** never rewrite old entries; each run = new entry with a `run-id`.
- **Raw output saved** (or reproducible) — to re-audit if a parse is incorrect.
- **Reproducible:** the harness records the exact command; re-running = same invocation.

## Artifacts (what lives where)

| Artifact | Role |
| --- | --- |
| `scripts/p0/bench.sh` | Harness: captures context + runs N times + aggregates + writes to both destinations |
| `scripts/p0/measure-*.sh` | Specific benchmarks (fio, headroom, swap-compare, ...) |
| `docs/BENCHMARKS.md` | **Human** log, append-only (template at the top) |
| `docs/benchmarks/results.jsonl` | **Machine-readable** data (1 line/run) |
| `docs/memory-broker/P0-RESULTS.md` | Consolidated decisions (go/no-go) — SSDV3 gate |

## Link with SSDV3 / Kahneman

- The SSDV3 **numerical P0 gate** (`P0-RESULTS.md`) consumes benchmarks that follow this rule.
- Disciplines: **#3** (number + unit + n + date + environment), **#1** (record the state — WYSIATI),
  **#5** (worst case / real load). **Anti-halo (#11):** the number of one phase does not "approve" the next.
