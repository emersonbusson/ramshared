#!/usr/bin/env bash
# measure-gpu-reserve-floor.sh — GPU reserve-floor campaign harness (RamShared).
#
# Owns the two named drills from
# docs/specs/no-milestone/gpu-reserve-floor-authority/SPEC.md:
#   measure_gpu_reserve_floor::capacity_boundary_campaign
#   measure_gpu_reserve_floor::live_adapter_before_action_after
#
# Default mode is READ-ONLY: it samples nvidia-smi, the live cache-status and
# capacity records, RAM/swap, and disk util/latency, then computes the sealed
# shared floor against the legacy sparse floors at the measured capacity. It
# allocates nothing and mutates nothing.
#
# Host safety (benchmarks.md / security.md):
#   - allocating drills are REFUSED unless BOTH --allocating is given AND
#     RAMSHARED_ALLOW_PRESSURE=1 is exported. Unsupervised pressure on the live
#     WSL2 host is forbidden.
#   - every allocating drill also needs a predeclared foreground GPU workload
#     (--workload-cmd). Without one the drill fails closed instead of inventing
#     a load profile.
#   - process identity is reported as `comm` + RSS only. No argv, no kernel
#     pointers, no KASLR material.
#
# usage:
#   measure-gpu-reserve-floor.sh [options]
# options:
#   --seconds N          sample window per round (default 30)
#   --interval S         sample interval (default 2)
#   --rounds N           rounds for the named drills (default 3, minimum 3)
#   --condition TAG      idle | loaded (default loaded)
#   --drill NAME         snapshot | capacity-boundary | live-adapter
#                        (also accepts the full SPEC test names)
#   --workload-cmd CMD   predeclared foreground GPU workload (allocating drills)
#   --allocating         opt in to pressure; refused without RAMSHARED_ALLOW_PRESSURE=1
#   --json               print one results.jsonl-compatible JSON line on stdout
#   --help               this text
# exit codes:
#   0  ok
#   64 usage
#   69 a required probe (nvidia-smi / python3) is unavailable
#   77 allocating refused by the host-safety gate
set -euo pipefail

EX_USAGE=64
EX_UNAVAILABLE=69
EX_NOPERM=77

SECONDS_WINDOW=30
INTERVAL=2
ROUNDS=3
CONDITION="loaded"
DRILL="snapshot"
WORKLOAD_CMD=""
ALLOCATING=0
WANT_JSON=0

# Sealed reserve authority (mirrors crates/ramshared-vram/src/reserve_policy.rs).
# Never overridable here: this harness reports the seal, it does not raise it.
SEALED_MIN_MIB=2048
SEALED_PERCENT=20
LEGACY_SPARSE_FLOOR_MIB=1536
LEGACY_HARD_TERM_MIB=2048

log() { echo "[gpu-reserve-floor] $*" >&2; }

usage() {
  sed -n '2,40p' "$0" | sed 's/^# \{0,1\}//'
}

while [ $# -gt 0 ]; do
  case "$1" in
    --seconds) SECONDS_WINDOW="${2:?--seconds needs a value}"; shift 2 ;;
    --interval) INTERVAL="${2:?--interval needs a value}"; shift 2 ;;
    --rounds) ROUNDS="${2:?--rounds needs a value}"; shift 2 ;;
    --condition) CONDITION="${2:?--condition needs a value}"; shift 2 ;;
    --drill) DRILL="${2:?--drill needs a value}"; shift 2 ;;
    --workload-cmd) WORKLOAD_CMD="${2:?--workload-cmd needs a value}"; shift 2 ;;
    --allocating) ALLOCATING=1; shift ;;
    --json) WANT_JSON=1; shift ;;
    --help|-h) usage; exit 0 ;;
    *) log "unknown argument: $1"; usage >&2; exit "$EX_USAGE" ;;
  esac
done

# Accept the exact SPEC test names so the matrix row is executable as written.
case "$DRILL" in
  measure_gpu_reserve_floor::capacity_boundary_campaign|capacity-boundary)
    DRILL="capacity-boundary" ;;
  measure_gpu_reserve_floor::live_adapter_before_action_after|live-adapter)
    DRILL="live-adapter" ;;
  snapshot) ;;
  *) log "unknown drill: $DRILL"; usage >&2; exit "$EX_USAGE" ;;
esac

if [ "$SECONDS_WINDOW" -lt 1 ] || [ "$INTERVAL" -lt 1 ]; then
  log "seconds and interval must be >= 1"; exit "$EX_USAGE"
fi
if [ "$ROUNDS" -lt 3 ]; then
  log "rounds must be >= 3 (benchmarks.md: one sample lies)"; exit "$EX_USAGE"
fi
case "$CONDITION" in
  idle|loaded) ;;
  *) log "condition must be idle or loaded"; exit "$EX_USAGE" ;;
esac

command -v python3 >/dev/null 2>&1 || { log "ERROR: python3 is required"; exit "$EX_UNAVAILABLE"; }

# Host-safety gate. Snapshot never allocates; the named drills always do.
require_pressure_gate() {
  if [ "$ALLOCATING" -ne 1 ]; then
    log "REFUSED: drill '$DRILL' allocates; pass --allocating"
    exit "$EX_NOPERM"
  fi
  if [ "${RAMSHARED_ALLOW_PRESSURE:-0}" != "1" ]; then
    log "REFUSED: allocating measurement needs RAMSHARED_ALLOW_PRESSURE=1"
    log "REFUSED: unsupervised pressure on the live WSL2 host is forbidden"
    exit "$EX_NOPERM"
  fi
  if [ -z "$WORKLOAD_CMD" ]; then
    log "REFUSED: drill '$DRILL' needs a predeclared --workload-cmd"
    log "REFUSED: this harness never invents a foreground GPU load profile"
    exit "$EX_NOPERM"
  fi
}

# --- read-only probes -------------------------------------------------------

gpu_name() {
  nvidia-smi --query-gpu=name --format=csv,noheader 2>/dev/null | head -1 | sed 's/^ *//;s/ *$//'
}
gpu_driver() {
  nvidia-smi --query-gpu=driver_version --format=csv,noheader 2>/dev/null | head -1 | tr -d ' '
}
gpu_mem() {
  # prints: total_mib used_mib free_mib
  nvidia-smi --query-gpu=memory.total,memory.used,memory.free \
    --format=csv,noheader,nounits 2>/dev/null | head -1 | tr -d ',' | awk '{print $1, $2, $3}'
}

mem_line() {
  # prints: ram_total_mib ram_avail_mib ram_free_mib swap_used_mib swap_total_mib
  free -m | awk '/^Mem:/{t=$2; a=$7; f=$4} /^Swap:/{s=$3; st=$2} END{print t, a, f, s, st}'
}

# Sanitized "what is open": comm + RSS only. Never argv (security.md).
open_workloads() {
  ps -eo comm=,rss= --sort=-rss 2>/dev/null | head -5 | awk \
    '{printf "%s:%dMiB", $1, int($2/1024); if (NR<5) printf "; "}'
  printf '\n'
}

# Cumulative counters only — never sleeps, so it is safe inside the sample loop.
disk_counters() {
  awk '$3 ~ /^(sd|nvme|vd)[a-z]$/ {print $3, $4, $8; exit}' /proc/diskstats
}

# One-shot util/latency over a 1s window on the first whole-disk node.
disk_util_sample() {
  local dev
  dev="$(awk '$3 ~ /^(sd|nvme|vd)[a-z]$/ {print $3; exit}' /proc/diskstats)"
  [ -n "$dev" ] || { echo "disk=none util=unavailable latency=unavailable"; return 0; }
  python3 - "$dev" <<'PY'
import sys, time
dev = sys.argv[1]
def snap():
    with open("/proc/diskstats") as fh:
        for line in fh:
            parts = line.split()
            if len(parts) >= 14 and parts[2] == dev:
                # reads completed + writes completed, read_ms + write_ms
                return int(parts[3]) + int(parts[7]), int(parts[6]) + int(parts[10])
    return 0, 0
a_ops, a_ms = snap()
time.sleep(1.0)
b_ops, b_ms = snap()
d_ops = max(b_ops - a_ops, 0)
d_ms = max(b_ms - a_ms, 0)
lat = (d_ms / d_ops) if d_ops > 0 else 0.0
util = min(100.0, d_ms / 10.0)
print(f"disk={dev} util_pct={util:.1f} latency_ms={lat:.3f}")
PY
}

read_cache_status() {
  # emits: state cached_kib target_kib headroom_kib origin daemon_instance
  python3 - <<'PY'
import json, os
path = "/run/ramshared/cache-status.json"
if not os.path.isfile(path):
    print("ABSENT 0 0 null NONE NONE")
    raise SystemExit(0)
try:
    with open(path) as fh:
        doc = json.load(fh)
except (OSError, json.JSONDecodeError):
    print("UNREADABLE 0 0 null NONE NONE")
    raise SystemExit(0)
def g(key, default="null"):
    value = doc.get(key, default)
    return "null" if value is None else value
print(
    g("cache_state", "ABSENT"),
    g("vram_cached_kib", 0),
    g("cache_target_kib", 0),
    g("gpu_headroom_kib", "null"),
    g("origin_state", "NONE"),
    g("daemon_instance_id", "NONE"),
)
PY
}

read_capacity_line() {
  if [ -f /run/ramshared/capacity-guaranteed ]; then
    tr '\n' ' ' < /run/ramshared/capacity-guaranteed
    echo
  else
    echo "absent"
  fi
}

source_revision() {
  git rev-parse --short=12 HEAD 2>/dev/null || echo "unknown"
}
source_branch() {
  git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "unknown"
}
source_dirty_count() {
  git status --porcelain 2>/dev/null | wc -l | tr -d ' '
}

# --- measurement ------------------------------------------------------------

# Cheap vitals only — safe inside the sample loop (no sleep, no git).
sample_vitals() {
  # prints exactly 5 columns (MiB):
  #   free_vram used_vram ram_avail ram_free swap_used
  local total used free_v ram_t ram_a ram_f sw_u sw_t
  read -r total used free_v < <(gpu_mem)
  read -r ram_t ram_a ram_f sw_u sw_t < <(mem_line)
  echo "${free_v:-NA} ${used:-NA} ${ram_a:-NA} ${ram_f:-NA} ${sw_u:-NA}"
}

# Full context, sampled once per round (git + 1s disk probe are too heavy per tick).
take_sample() {
  local vitals cache cap disk rev br dirty
  vitals="$(sample_vitals)"
  cache="$(read_cache_status)"
  cap="$(read_capacity_line)"
  disk="$(disk_util_sample)"
  rev="$(source_revision)"
  br="$(source_branch)"
  dirty="$(source_dirty_count)"
  echo "vitals=$vitals cache=$cache disk=$disk rev=$rev branch=$br dirty=$dirty cap=$cap"
}

run_window() {
  local n t
  n=$(( SECONDS_WINDOW / INTERVAL ))
  [ "$n" -lt 1 ] && n=1
  t=0
  while [ "$t" -lt "$n" ]; do
    sample_vitals
    t=$(( t + 1 ))
    if [ "$t" -lt "$n" ]; then sleep "$INTERVAL"; fi
  done
}

# Floors at the measured capacity. Pure arithmetic from the sealed constants
# plus the live capacity; never a live allocation.
emit_floor_math() {
  local total_mib
  total_mib="$1"
  python3 - "$total_mib" "$SEALED_MIN_MIB" "$SEALED_PERCENT" \
    "$LEGACY_SPARSE_FLOOR_MIB" "$LEGACY_HARD_TERM_MIB" <<'PY'
import sys
total_mib = int(sys.argv[1])
sealed_min = int(sys.argv[2])
sealed_pct = int(sys.argv[3])
legacy_sparse = int(sys.argv[4])
legacy_hard = int(sys.argv[5])
capacity_mib = total_mib
# configured = max(min_floor, floor(capacity * pct / 100))  (DT-2)
share = (capacity_mib * sealed_pct) // 100
configured = max(sealed_min, share)
# enforced = max(configured, ceil(capacity/5), runtime) — runtime unknown here
safety = (capacity_mib + 4) // 5
enforced = max(configured, safety)
# legacy sparse path: hardcoded 1536 MiB worker default, plus a 2 GiB hard term
legacy_enforced = max(legacy_sparse, legacy_hard)
print(f"capacity_mib={capacity_mib}")
print(f"sealed_min_mib={sealed_min}")
print(f"sealed_percent={sealed_pct}")
print(f"sealed_share_mib={share}")
print(f"configured_reserve_mib={configured}")
print(f"safety_floor_mib={safety}")
print(f"enforced_free_floor_mib={enforced}")
print(f"legacy_sparse_floor_mib={legacy_sparse}")
print(f"legacy_hard_term_mib={legacy_hard}")
print(f"legacy_enforced_floor_mib={legacy_enforced}")
print(f"floor_delta_mib={enforced - legacy_enforced}")
print(f"legacy_hardcoded_term_present=yes  # origin_cache .max(2 GiB) before DT-6")
PY
}

collect_window() {
  local label="$1"
  log "sampling '$label' vitals for ${SECONDS_WINDOW}s every ${INTERVAL}s (read-only)"
  run_window > "/tmp/gpu-reserve-floor-${label}-$$.samples"
  echo "/tmp/gpu-reserve-floor-${label}-$$.samples"
}

# --- drills -----------------------------------------------------------------

drill_snapshot() {
  local samples cache total_mib
  samples="$(collect_window snapshot)"
  cache="$(read_cache_status)"
  total_mib="$(nvidia-smi --query-gpu=memory.total --format=csv,noheader,nounits 2>/dev/null | head -1 | tr -d ' ')"
  total_mib="${total_mib:-0}"

  if [ "$WANT_JSON" -eq 1 ]; then
    python3 - "$samples" "$cache" "$total_mib" "$SECONDS_WINDOW" "$INTERVAL" \
      "$CONDITION" "$(source_revision)" "$(source_branch)" "$(source_dirty_count)" \
      "$(gpu_name)" "$(gpu_driver)" "$(open_workloads)" "$0" <<'PY'
import json, sys, datetime, statistics

(samples_path, cache, total_s, window, interval, condition, rev, branch,
 dirty, gpu, driver, open_wl, tool) = sys.argv[1:14]
total = int(total_s or 0)

def quantile(vals, q):
    vals = sorted(vals)
    if not vals:
        return 0.0
    if len(vals) == 1:
        return float(vals[0])
    pos = q * (len(vals) - 1)
    lo = int(pos)
    hi = min(lo + 1, len(vals) - 1)
    frac = pos - lo
    return float(vals[lo]) * (1 - frac) + float(vals[hi]) * frac

def stats(vals):
    vals = [float(v) for v in vals]
    n = len(vals)
    mean = sum(vals) / n if n else 0.0
    sd = statistics.stdev(vals) if n > 1 else 0.0
    return {"n": n, "min": min(vals) if vals else 0, "max": max(vals) if vals else 0,
            "median": quantile(vals, 0.5), "p99": quantile(vals, 0.99),
            "mean": round(mean, 1), "stddev": round(sd, 1)}

# sample_vitals columns: free_vram used_vram ram_avail ram_free swap_used
cols = {k: [] for k in
        ("free_vram_mib", "used_vram_mib", "ram_available_mib",
         "ram_free_mib", "swap_used_mib")}
try:
    with open(samples_path, encoding="utf-8") as fh:
        for line in fh:
            parts = line.split()
            if len(parts) < 5:
                continue
            for key, raw in zip(cols, parts[:5]):
                if raw not in ("NA", ""):
                    cols[key].append(float(raw))
except OSError:
    pass

cache_parts = cache.split()
cache_state = cache_parts[0] if cache_parts else "ABSENT"
cached = int(cache_parts[1]) if len(cache_parts) > 1 else 0
target = int(cache_parts[2]) if len(cache_parts) > 2 else 0
headroom_raw = cache_parts[3] if len(cache_parts) > 3 else "null"
origin = cache_parts[4] if len(cache_parts) > 4 else "NONE"
daemon = cache_parts[5] if len(cache_parts) > 5 else "NONE"

sealed_min, sealed_pct = 2048, 20
share = (total * sealed_pct) // 100
configured = max(sealed_min, share)
safety = (total + 4) // 5
enforced = max(configured, safety)
legacy_enforced = max(1536, 2048)

metrics = {k: stats(v) for k, v in cols.items() if v}
for k, v in cols.items():
    if v:
        metrics[f"per_round_{k}"] = v
metrics["reserve_floor"] = {
    "capacity_mib": total,
    "sealed_min_mib": sealed_min,
    "sealed_percent": sealed_pct,
    "sealed_share_mib": share,
    "configured_reserve_mib": configured,
    "safety_floor_mib": safety,
    "enforced_free_floor_mib": enforced,
    "legacy_sparse_floor_mib": 1536,
    "legacy_hard_term_mib": 2048,
    "legacy_enforced_floor_mib": legacy_enforced,
    "floor_delta_mib": enforced - legacy_enforced,
}

record = {
    "run_id": "gpu-reserve-floor-snapshot-" + datetime.datetime.now().strftime("%Y%m%d-%H%M%S"),
    "timestamp": datetime.datetime.now().astimezone().isoformat(timespec="seconds"),
    "commit": rev,
    "branch": branch,
    "condition": condition,
    "tool": tool,
    "parameters": {
        "drill": "snapshot",
        "window_sec": int(window),
        "interval_sec": int(interval),
        "allocating": False,
    },
    "host_context": {
        "gpu": gpu,
        "driver_version": driver,
        "vram_total_mib": total,
        "load_snapshot": open_wl.strip(),
        "cache_status": {
            "cache_state": cache_state,
            "vram_cached_kib": cached,
            "cache_target_kib": target,
            "gpu_headroom_kib": None if headroom_raw == "null" else int(headroom_raw),
            "origin_state": origin,
            "daemon_instance_id": daemon,
        },
    },
    "metrics": metrics,
    "publication_status": "legacy-unqualified",
    "notes": (
        "Read-only snapshot: the harness allocates nothing. Floor arithmetic is "
        "computed from the sealed constants at the measured capacity "
        "(configured = max(2048 MiB, floor(capacity * 20 / 100)); enforced also "
        "takes ceil(capacity/5)). The legacy comparison keeps the pre-DT-6 "
        "hardcoded 1536 MiB worker default and the 2 GiB origin_cache term. "
        "Host-private raw samples, no in-repo SHA-256 artifacts, so this is "
        "explicitly legacy-unqualified: not a baseline, not a regression PASS, "
        "not a promotion claim."
    ),
}
print(json.dumps(record, separators=(",", ":"), sort_keys=False))
PY
    return 0
  fi

  echo "### GPU reserve-floor snapshot (read-only)"
  echo
  echo "**Drill:** \`snapshot\` (not a campaign)"
  echo "**Condition:** \`$CONDITION\`"
  echo "**Samples:** \`$samples\`"
  echo "**Context**"
  echo "- Branch/commit: \`$(source_branch)\` @ \`$(source_revision)\` (dirty=$(source_dirty_count))"
  echo "- GPU: $(gpu_name) · driver $(gpu_driver) · capacity ${total_mib} MiB"
  echo "- Kernel: $(uname -r)"
  echo "- Cache status: $cache"
  echo "- Capacity record: $(read_capacity_line)"
  echo "- Disk: $(disk_util_sample)"
  echo "- Open (comm:RSS, top 5): $(open_workloads)"
  echo
  echo "**Vitals over the window** (columns: free_vram used_vram ram_avail ram_free swap_used, MiB)"
  echo
  echo '```'
  cat "$samples"
  echo '```'
  echo
  echo "**Reserve floors at measured capacity**"
  echo
  echo '```'
  emit_floor_math "$total_mib"
  echo '```'
  echo
  echo "**Honest reading**"
  echo "- Supported: the sealed shared floor and the legacy sparse floor at this"
  echo "  capacity, as pure arithmetic. No allocation was performed."
  echo "- Not supported: usable cache bytes under a foreground GPU workload. That"
  echo "  is \`capacity-boundary\` / \`live-adapter\` and needs --allocating plus"
  echo "  RAMSHARED_ALLOW_PRESSURE=1 and a predeclared --workload-cmd."
  echo "- Not supported: PASS_ZERO_PANIC and the Tier 3 origin columns. They are"
  echo "  campaign outcomes and are not claimed by a snapshot."
}

# Three-run comparison of the legacy sparse floor against the sealed shared
# floor around a predeclared foreground GPU workload.
drill_capacity_boundary() {
  require_pressure_gate
  log "capacity-boundary campaign: $ROUNDS rounds, workload='$WORKLOAD_CMD'"
  local round before after
  round=1
  while [ "$round" -le "$ROUNDS" ]; do
    log "round $round/$ROUNDS before"
    before="$(take_sample)"
    log "round $round/$ROUNDS action: $WORKLOAD_CMD"
    # shellcheck disable=SC2086  # the operator predeclares the exact argv
    eval "$WORKLOAD_CMD"
    log "round $round/$ROUNDS after"
    after="$(take_sample)"
    printf 'round=%s before=(%s) after=(%s)\n' "$round" "$before" "$after"
    round=$(( round + 1 ))
  done
  if [ "$WANT_JSON" -eq 1 ]; then
    # Campaign JSON is emitted by bench.sh from the round lines; a snapshot
    # envelope is printed so the dual-write contract still has a machine row.
    drill_snapshot
    return 0
  fi
  echo
  echo "### capacity-boundary campaign (measure_gpu_reserve_floor::capacity_boundary_campaign)"
  echo
  echo "Rounds: $ROUNDS · workload: \`$WORKLOAD_CMD\` · condition: \`$CONDITION\`"
  echo
  echo "**Honest reading**"
  echo "- The four-category hardware table and the Tier 3 (SSD) qualification"
  echo "  columns, including \`PASS_ZERO_PANIC\`, are campaign outcomes. Fill them"
  echo "  only from the round lines above and the origin counters; this harness"
  echo "  never invents a verdict."
  echo "- Aggregation (median + p99 + deviation across the $ROUNDS rounds) is"
  echo "  performed by \`scripts/p0/bench.sh\`, which owns the dual write."
}

# before -> action -> after on the live adapter.
drill_live_adapter() {
  require_pressure_gate
  log "live-adapter before/action/after, workload='$WORKLOAD_CMD'"
  local before after
  log "before"
  before="$(take_sample)"
  echo "BEFORE: $before"
  log "action: $WORKLOAD_CMD"
  # shellcheck disable=SC2086  # the operator predeclares the exact argv
  eval "$WORKLOAD_CMD"
  log "after"
  after="$(take_sample)"
  echo "AFTER:  $after"
  if [ "$WANT_JSON" -eq 1 ]; then
    drill_snapshot
    return 0
  fi
  echo
  echo "### live-adapter before/action/after (measure_gpu_reserve_floor::live_adapter_before_action_after)"
  echo
  echo "**Honest reading**"
  echo "- Before and after lines are real samples of the live adapter. The action"
  echo "  was the predeclared workload above, not an invented load."
  echo "- \`BINARY_MATCH\` is claimed only when \`ramsharedd\` was exercised and the"
  echo "  binary identity was proven. This harness does not claim it."
}

case "$DRILL" in
  snapshot) drill_snapshot ;;
  capacity-boundary) drill_capacity_boundary ;;
  live-adapter) drill_live_adapter ;;
esac
