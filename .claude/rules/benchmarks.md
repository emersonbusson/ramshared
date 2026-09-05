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
- **Public evidence envelope:** every new benchmark claim uses
  `docs/benchmarks/evidence.schema.json` (`ramshared-evidence/v1`) and passes
  `node tools/ci/check-benchmark-evidence.mjs --check`. A public PASS requires
  repository-relative, SHA-256-verifiable sanitized artifacts. Host-private or
  pre-schema evidence is explicitly `legacy-unqualified` and cannot be a
  baseline, regression PASS, or promotion claim.

## Artifacts (what lives where)

| Artifact | Role |
| --- | --- |
| `scripts/p0/bench.sh` | Harness: captures context + runs N times + aggregates + writes to both destinations |
| `scripts/p0/measure-*.sh` | Specific benchmarks (fio, headroom, swap-compare, ...) |
| `docs/BENCHMARKS.md` | **Human** log, append-only (template at the top) |
| `docs/benchmarks/results.jsonl` | **Machine-readable** data (1 line/run) |
| `docs/memory-broker/P0-RESULTS.md` | Consolidated decisions (go/no-go) — SSDV3 gate |

## Hardware Metrics Taxonomy & Regression Alarms

Every release or candidate comparison is evaluated across 4 physical domains with explicit optimization directionality:
1. **Workload & Capacity**: RAM requested, ZRAM compression, GPU VRAM offload (`[🔺 Higher is better]`), Tier 3 SSD spillover (`[🔻 Lower is better]`).
2. **Speed & Latency**: Reclaim bus throughput (`[🔺 Higher is better]`), Reclaim duration (`[🔻 Lower is better]`), Block latency (`[🔻 Lower is better]`).
3. **Pressure & Stalls**: Pressure index tolerance (`[🔺 Higher is better]`), PSI stall time (`[🔻 Lower is better]`, target 0.0%), Major page faults (`[🔻 Lower is better]`).
4. **Integrity & Stability**: Bit-exactness (100%), Zero memory leaks (< 100 MB), Zero OOM kills (0), Verdict (`PASS_ZERO_PANIC`).

### Regression Alarm Thresholds (🔴 ALARM)
- **Throughput Drop**: > 3% loss vs `docs/benchmarks/baseline.json`.
- **Latency Spike**: > 5% increase without swap volume change.
- **Unintended SSD Spill**: > 0 MB when within Tier 1+2 budget.
- **Status Failure**: Any verdict != `PASS_ZERO_PANIC`.

When an alarm triggers, the PR is blocked and the operator must execute [`docs/reliability/HARDWARE-METRICS-TRIAGE.md`](../../docs/reliability/HARDWARE-METRICS-TRIAGE.md) to diagnose and resolve the root cause before proceeding.

## Link with SSDV3 / Kahneman

- The SSDV3 **numerical P0 gate** (`P0-RESULTS.md`) consumes benchmarks that follow this rule.
- Disciplines: **#3** (number + unit + n + date + environment), **#1** (record the state — WYSIATI),
  **#5** (worst case / real load). **Anti-halo (#11):** the number of one phase does not "approve" the next.
