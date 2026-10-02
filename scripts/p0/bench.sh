#!/usr/bin/env bash
# bench.sh — registered benchmark harness (RamShared).
#
# Documented in .claude/rules/benchmarks.md as the harness that "captures
# context + runs N times + aggregates + writes to both destinations".
#
# What it does, in order:
#   1. captures the automatic context (timestamp, branch+commit+dirty, kernel,
#      GPU via nvidia-smi, RAM/swap, disk util/latency, what is open)
#   2. runs the wrapped measure-*.sh once per round (default 3, minimum 3 —
#      one sample lies) with FIXED parameters and saves each raw stdout
#   3. aggregates median + p99 + deviation across the rounds
#   4. appends one machine-readable line to docs/benchmarks/results.jsonl
#   5. prints a paste-ready docs/BENCHMARKS.md block, including the
#      `<!-- ramshared-benchmark-id: ... -->` marker and the two registry
#      snippets, so `node tools/ci/check-benchmark-evidence.mjs --check`
#      stays green after the operator pastes
#
# A bare measure-*.sh run has no ramshared-evidence/v1 envelope, so the
# record is written as `legacy-unqualified`. It cannot be a baseline, a
# regression PASS, or a promotion claim. The printed registry snippets say
# the same thing.
#
# Host safety: this harness only wraps a caller-chosen measure script. It
# never allocates itself and never sets RAMSHARED_ALLOW_PRESSURE. If the
# wrapped tool wants pressure, the tool is responsible for its own gate.
#
# usage:
#   bench.sh --tool PATH --run-id ID --condition idle|loaded [options] -- [tool args...]
# options:
#   --rounds N        rounds (default 3, minimum 3)
#   --title TEXT      human title for the BENCHMARKS.md heading
#   --benchmark-id X  id for the benchmark-id marker (default derived from run-id)
#   --raw-dir DIR     where raw round outputs are saved (default /tmp/bench-<run-id>)
#   --results PATH    results.jsonl to append to (default docs/benchmarks/results.jsonl)
#   --dry-run         capture + aggregate + print, but do not append to results.jsonl
#   --help            this text
# exit codes:
#   0  ok
#   64 usage
#   69 the wrapped tool is missing or unusable
#   70 a round produced no metric and no raw output
set -euo pipefail

EX_USAGE=64
EX_UNAVAILABLE=69
EX_NODATA=70

TOOL=""
RUN_ID=""
CONDITION=""
ROUNDS=3
TITLE=""
BENCH_ID=""
RAW_DIR=""
RESULTS="docs/benchmarks/results.jsonl"
DRY_RUN=0
TOOL_ARGS=()

log() { echo "[bench] $*" >&2; }

usage() {
  sed -n '2,36p' "$0" | sed 's/^# \{0,1\}//'
}

while [ $# -gt 0 ]; do
  case "$1" in
    --tool) TOOL="${2:?--tool needs a value}"; shift 2 ;;
    --run-id) RUN_ID="${2:?--run-id needs a value}"; shift 2 ;;
    --condition) CONDITION="${2:?--condition needs a value}"; shift 2 ;;
    --rounds) ROUNDS="${2:?--rounds needs a value}"; shift 2 ;;
    --title) TITLE="${2:?--title needs a value}"; shift 2 ;;
    --benchmark-id) BENCH_ID="${2:?--benchmark-id needs a value}"; shift 2 ;;
    --raw-dir) RAW_DIR="${2:?--raw-dir needs a value}"; shift 2 ;;
    --results) RESULTS="${2:?--results needs a value}"; shift 2 ;;
    --dry-run) DRY_RUN=1; shift ;;
    --help|-h) usage; exit 0 ;;
    --) shift; TOOL_ARGS=("$@"); break ;;
    *) log "unknown argument: $1"; usage >&2; exit "$EX_USAGE" ;;
  esac
done

[ -n "$TOOL" ] || { log "--tool is required"; exit "$EX_USAGE"; }
[ -n "$RUN_ID" ] || { log "--run-id is required"; exit "$EX_USAGE"; }
case "$CONDITION" in
  idle|loaded) ;;
  *) log "condition must be idle or loaded"; exit "$EX_USAGE" ;;
esac
if [ "$ROUNDS" -lt 3 ]; then
  log "rounds must be >= 3 (benchmarks.md: one sample lies)"; exit "$EX_USAGE"
fi
if [ ! -x "$TOOL" ] && [ ! -f "$TOOL" ]; then
  log "ERROR: tool not found: $TOOL"; exit "$EX_UNAVAILABLE"
fi
command -v python3 >/dev/null 2>&1 || { log "ERROR: python3 is required"; exit "$EX_UNAVAILABLE"; }

[ -n "$RAW_DIR" ] || RAW_DIR="/tmp/bench-$RUN_ID"
mkdir -p "$RAW_DIR"
[ -n "$BENCH_ID" ] || BENCH_ID="$(echo "$RUN_ID" | tr '[:upper:]' '[:lower:]' | tr -c 'a-z0-9._-' '-' | sed 's/--*/-/g;s/^-//;s/-$//')"
[ -n "$TITLE" ] || TITLE="$RUN_ID"

STAMP_ISO="$(date --iso-8601=seconds)"
STAMP_HUMAN="$(date '+%Y-%m-%d %H:%M %z')"
REPO_REV="$(git rev-parse --short=12 HEAD 2>/dev/null || echo unknown)"
REPO_BRANCH="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo unknown)"
REPO_DIRTY="$(git status --porcelain 2>/dev/null | wc -l | tr -d ' ')"

log "tool=$TOOL run_id=$RUN_ID condition=$CONDITION rounds=$ROUNDS dry_run=$DRY_RUN"
log "raw dir: $RAW_DIR"

# Each round: fixed argv, raw stdout saved. The tool may print CSV, a summary,
# or a single JSON object; the aggregator handles the three shapes it needs.
round=1
while [ "$round" -le "$ROUNDS" ]; do
  out="$RAW_DIR/round-$round.txt"
  log "round $round/$ROUNDS -> $out"
  # shellcheck disable=SC2086  # operator-supplied argv after `--` is intentional
  if ! bash "$TOOL" "${TOOL_ARGS[@]+"${TOOL_ARGS[@]}"}" >"$out" 2>"$RAW_DIR/round-$round.err"; then
    log "ERROR: round $round exited non-zero (see $RAW_DIR/round-$round.err)"
    exit "$EX_NODATA"
  fi
  if [ ! -s "$out" ]; then
    log "ERROR: round $round produced no output"; exit "$EX_NODATA"
  fi
  round=$(( round + 1 ))
done

# Aggregate and emit the machine row + the paste-ready human block.
python3 - "$RAW_DIR" "$RUN_ID" "$CONDITION" "$ROUNDS" "$STAMP_ISO" "$STAMP_HUMAN" \
  "$REPO_REV" "$REPO_BRANCH" "$REPO_DIRTY" "$TOOL" "$RESULTS" "$BENCH_ID" \
  "$TITLE" "$DRY_RUN" ${TOOL_ARGS[@]+"${TOOL_ARGS[@]}"} <<'PY'
import json, os, re, sys, statistics, glob

(raw_dir, run_id, condition, rounds, stamp_iso, stamp_human,
 rev, branch, dirty, tool, results, bench_id, title, dry_run) = sys.argv[1:16]
rounds = int(rounds)
dry_run = dry_run == "1"

def quantile(sorted_vals, q):
    if not sorted_vals:
        return 0.0
    if len(sorted_vals) == 1:
        return float(sorted_vals[0])
    pos = q * (len(sorted_vals) - 1)
    lo = int(pos)
    hi = min(lo + 1, len(sorted_vals) - 1)
    frac = pos - lo
    return float(sorted_vals[lo]) * (1 - frac) + float(sorted_vals[hi]) * frac

def stats(values):
    vals = sorted(float(v) for v in values)
    n = len(vals)
    mean = sum(vals) / n
    sd = statistics.stdev(vals) if n > 1 else 0.0
    return {
        "n": n,
        "min": vals[0],
        "max": vals[-1],
        "median": quantile(vals, 0.5),
        "p99": quantile(vals, 0.99),
        "mean": round(mean, 1),
        "stddev": round(sd, 1),
    }

# Collect per-round JSON metrics (if the tool printed one) and CSV-ish numbers.
per_round_json = []
csv_columns = {}  # name -> list of values across all rounds
for path in sorted(glob.glob(os.path.join(raw_dir, "round-*.txt"))):
    if path.endswith(".err"):
        continue
    text = open(path, encoding="utf-8", errors="replace").read()
    for line in text.splitlines():
        s = line.strip()
        if s.startswith("{") and s.endswith("}") and '"run_id"' in s:
            try:
                per_round_json.append(json.loads(s))
                continue
            except json.JSONDecodeError:
                pass
        # CSV with a header on the first matching line: name,value or a,b,c
        if "," in s and not s.startswith("#") and re.match(r"^[\w.+-]+,", s):
            parts = s.split(",")
            if len(parts) == 2 and re.match(r"^-?\d+(\.\d+)?$", parts[1]):
                csv_columns.setdefault(parts[0], []).append(float(parts[1]))

metrics = {}
if per_round_json:
    # Pool every numeric leaf of each round's "metrics" map.
    keys = set()
    for rec in per_round_json:
        m = rec.get("metrics") or {}
        for k, v in m.items():
            if isinstance(v, dict) and any(isinstance(x, (int, float)) for x in v.values()):
                keys.add(k)
            elif isinstance(v, (int, float)):
                keys.add(k)
    for key in sorted(keys):
        samples = []
        for rec in per_round_json:
            v = (rec.get("metrics") or {}).get(key)
            if isinstance(v, (int, float)):
                samples.append(v)
            elif isinstance(v, dict):
                # a round that already aggregated one sample: use its median/mean
                if "median" in v:
                    samples.append(v["median"])
                elif "mean" in v:
                    samples.append(v["mean"])
        if samples:
            metrics[key] = stats(samples)
            metrics[f"per_round_{key}"] = samples

for name, values in sorted(csv_columns.items()):
    if name not in metrics:
        metrics[name] = stats(values)
        metrics[f"per_round_{name}"] = values

if not metrics:
    # Fall back: report raw byte counts per round so the row is never empty.
    sizes = []
    for path in sorted(glob.glob(os.path.join(raw_dir, "round-*.txt"))):
        if not path.endswith(".err"):
            sizes.append(float(os.path.getsize(path)))
    if sizes:
        metrics["raw_output_bytes"] = stats(sizes)
        metrics["per_round_raw_output_bytes"] = sizes

host_context = {
    "kernel": os.uname().release,
    "branch": branch,
    "commit": rev,
    "dirty_entry_count": int(dirty),
    "raw_dir": raw_dir,
    "rounds": rounds,
}

# GPU / RAM / swap are best-effort; missing probes stay null, never invented.
def probe(cmd):
    try:
        return os.popen(cmd).read().strip()
    except OSError:
        return ""

host_context["gpu"] = probe(
    "nvidia-smi --query-gpu=name --format=csv,noheader 2>/dev/null | head -1"
) or None
host_context["driver_version"] = probe(
    "nvidia-smi --query-gpu=driver_version --format=csv,noheader 2>/dev/null | head -1"
) or None
mem = probe("free -m | awk '/^Mem:/{t=$2;a=$7} /^Swap:/{s=$3} END{print t,a,s}'")
if mem:
    parts = mem.split()
    if len(parts) >= 3:
        host_context["ram_total_mib"] = int(parts[0])
        host_context["ram_available_mib"] = int(parts[1])
        host_context["swap_used_mib"] = int(parts[2])
host_context["load_snapshot"] = probe(
    "ps -eo comm=,rss= --sort=-rss 2>/dev/null | head -5 | "
    "awk '{printf \"%s:%dMiB \", $1, int($2/1024)}'"
) or None

record = {
    "run_id": run_id,
    "timestamp": stamp_iso,
    "commit": rev,
    "branch": branch,
    "condition": condition,
    "tool": tool,
    "parameters": {
        "rounds": rounds,
        "tool_args": sys.argv[16:] if len(sys.argv) > 16 else [],
        "allocating": False,
    },
    "host_context": host_context,
    "metrics": metrics,
    "publication_status": "legacy-unqualified",
    "notes": (
        f"Registered by scripts/p0/bench.sh with {rounds} rounds under condition "
        f"'{condition}'. Raw outputs at {raw_dir}. Host-private raw output with no "
        "in-repo SHA-256 artifact manifest and no ramshared-evidence/v1 envelope, "
        "so this is explicitly legacy-unqualified: not a baseline, not a regression "
        "PASS, not a promotion claim."
    ),
}

line = json.dumps(record, separators=(",", ":"), sort_keys=False)
print("### machine row (docs/benchmarks/results.jsonl)")
print(line)
print()

if not dry_run:
    os.makedirs(os.path.dirname(results) or ".", exist_ok=True)
    with open(results, "a", encoding="utf-8") as fh:
        fh.write(line + "\n")
    print(f"(appended 1 line to {results})")
else:
    print(f"(dry-run: did not append to {results})")
print()

legacy_id = f"legacy-{run_id}"
print("### paste-ready docs/BENCHMARKS.md block")
print()
print(f"<!-- ramshared-benchmark-id: {bench_id} -->")
print(f"## {stamp_human} — {title}")
print()
print("**Context**")
print(f"- Branch/commit: `{branch}` @ `{rev}` (dirty={dirty})")
print(f"- Machine: GPU `{host_context.get('gpu')}` · driver `{host_context.get('driver_version')}` · "
      f"kernel `{host_context.get('kernel')}`")
print(f"- RAM total {host_context.get('ram_total_mib')} MiB · "
      f"avail {host_context.get('ram_available_mib')} MiB · "
      f"swap used {host_context.get('swap_used_mib')} MiB")
print(f"- Load snapshot: {host_context.get('load_snapshot')}")
print(f"- Tool/parameters: `{tool}` · rounds={rounds} · condition=`{condition}`")
print(f"- Raw outputs: `{raw_dir}`")
print()
print("**Results** (median + p99 + deviation across rounds)")
print()
print("| Metric | n | min | median | p99 | max | mean | stddev |")
print("| --- | --- | --- | --- | --- | --- | --- | --- |")
for name, st in metrics.items():
    if name.startswith("per_round_"):
        continue
    print(f"| {name} | {st['n']} | {st['min']} | {st['median']} | {st['p99']} | "
          f"{st['max']} | {st['mean']} | {st['stddev']} |")
print()
print("**Honest reading**")
print("- Fill this in from what the numbers support. A harness never writes the")
print("  interpretation for you.")
print("- This record is `legacy-unqualified`: no v1 evidence envelope and no")
print("  in-repo SHA-256 artifacts. It cannot be a baseline, a regression PASS,")
print("  or a promotion claim.")
print()
print("### registry snippets (required — without them the evidence checker goes red)")
print()
print("Append to `docs/benchmarks/legacy-unqualified.json` `entries`:")
print()
print("```json")
print(json.dumps({
    "id": legacy_id,
    "benchmark_id": bench_id,
    "run_id": run_id,
    "qualified": False,
    "reason": (
        "Host-private raw output with no in-repo SHA-256 artifact manifest and "
        "no ramshared-evidence/v1 envelope; this run cannot be a regression "
        "baseline, a regression PASS, or a promotion claim."
    ),
}, indent=2))
print("```")
print()
print("Append to `docs/benchmarks/benchmark-map.json` `entries`:")
print()
print("```json")
print(json.dumps({"benchmark_id": bench_id, "run_id": run_id}, indent=2))
print("```")
print()
print("Then run `node tools/ci/check-benchmark-evidence.mjs --check` — it must exit 0.")
PY
