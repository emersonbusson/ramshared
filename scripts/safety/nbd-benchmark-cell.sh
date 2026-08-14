#!/usr/bin/env bash
# Run or aggregate one sealed WSL2 NBD benchmark cell. Live mode is root-only
# and requires both the Windows watchdog marker and an exact release/size scope.
set -euo pipefail
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH

ACTION=""
MODE=""
CONDITION=""
TIER_MIB=""
ARTIFACT_DIR=""
SAMPLES=""
OUT=""
SWAP_FIXTURE=""
RUNS=3
SAMPLE_TIMEOUT_SEC=""
SAMPLE_TIMEOUT_MAX_SEC=600
INTEGRITY_FINALIZATION_TIMEOUT_SEC=""
CELL_SETUP_CLEANUP_TIMEOUT_SEC=300
CELL_OUTER_TIMEOUT_MIN_SEC=900
CELL_OUTER_TIMEOUT_MAX_SEC=3900
CELL_OUTER_TIMEOUT_SEC=""
SAMPLE_BASELINE_REASON=""
SAMPLE_ZRAM_DEVICE=""
ALLOCATE_MIB=""
MEMORY_HIGH_MIB=1200
MEMORY_MAX_MIB=""
CHUNK_MIB=64
NBD_MKSWAP_OVERHEAD_KIB=8
SEALED_RELEASE_ROOT=""
RELEASE_VERSION=""
EXPECTED_SOURCE_COMMIT=""
EXPECTED_MANIFEST_SHA256=""
EXPECTED_INPUT_BUNDLE_MANIFEST_SHA256=""
PAIR_ID=""
UTC_STARTED=""
LOWER_SINK_CANONICAL=""
LOWER_SINK_IDENTITY_SHA256=""
LOWER_SINK_TYPE=""
SCRATCH_IDENTITY_SHA256=""
ACTION_BINARY_MATCH="N/A"
NBD_IDENTITY_REASON=""
NBD_DEVICE=""
NBD_BLOCK_MAJOR_MINOR=""
NBD_SIZE_KIB=""
NBD_USABLE_SIZE_KIB=""
NBD_CAPACITY_SECTORS=""
NBD_PRIORITY=""
NBD_SERVER_PID=""
NBD_DAEMON_MANIFEST_SHA256=""
NBD_SECOND_TIER_IDENTITY_SHA256=""
IDENTITY_FIXTURE_ROOT=""
IDENTITY_LOWER_SINK_SHA256=""
CUSTODY_INVENTORY_CANDIDATE=""
CUSTODY_ENVELOPE_CANDIDATE=""
CUSTODY_CANDIDATES_READY=0
CUSTODY_FRONTIER=0
CUSTODY_INVENTORY_PUBLISHED=0
CUSTODY_INVENTORY_IDENTITY=""
CUSTODY_INVENTORY_SHA256=""
LIVE_SEAM_PRESENT=0
for seam in RAMSHARED_PRODUCT_ROOT RAMSHARED_BENCHMARK_CGROUP_ROOT \
  WORKER_TERM_GRACE_SEC WORKER_KILL_GRACE_SEC; do
  [[ -v $seam ]] && LIVE_SEAM_PRESENT=1
done
# The benchmark approval is the only RAMSHARED_NBD_* input admitted to live
# mode. Every other name is a preflight override or a manufactured-test seam,
# including names added by later preflight revisions.
for seam in "${!RAMSHARED_NBD_@}"; do
  [[ $seam == RAMSHARED_NBD_BENCHMARK_APPROVAL ]] || LIVE_SEAM_PRESENT=1
done
WORKER_TERM_GRACE_SEC=${WORKER_TERM_GRACE_SEC:-5}
WORKER_KILL_GRACE_SEC=${WORKER_KILL_GRACE_SEC:-5}
PRODUCT_ROOT=${RAMSHARED_PRODUCT_ROOT:-/opt/ramshared}
CG_ROOT=${RAMSHARED_BENCHMARK_CGROUP_ROOT:-/sys/fs/cgroup}
SWAPS_FILE=${RAMSHARED_NBD_SWAPS_FILE:-/proc/swaps}
PID_FILE=${RAMSHARED_NBD_PID_FILE:-/run/ramshared/ramsharedd.pid}
ZRAM_RECORD=${RAMSHARED_NBD_ZRAM_RECORD:-/run/ramshared/zram-dev}
PROC_ROOT=${RAMSHARED_NBD_PROC_ROOT:-/proc}
DEV_ROOT=${RAMSHARED_NBD_DEV_ROOT:-/dev}
SYS_BLOCK_ROOT=${RAMSHARED_NBD_SYS_BLOCK_ROOT:-/sys/block}
FAILURE_REASON=""

refuse() {
  [[ -n $FAILURE_REASON ]] || FAILURE_REASON=$1
  printf 'NBD_BENCHMARK_STATE=REFUSED\n'
  printf 'NBD_BENCHMARK_REASON=%s\n' "$1"
  exit 2
}

require_shared_host_approval() {
  [[ ${RAMSHARED_SHARED_HOST_APPROVAL:-} == I_ACCEPT_BOUNDED_SHARED_HOST_PRESSURE ]] || \
    refuse SHARED_HOST_APPROVAL_MISSING
}

derive_sample_timeout_sec() {
  case $1 in
    1024) printf '120\n' ;;
    2048) printf '240\n' ;;
    4096) printf '600\n' ;;
    *) return 1 ;;
  esac
}

# Integrity finalization starts only after the HOLD receipt/TERM boundary.
# This is an explicit tier policy, not a measured performance allowance.
derive_integrity_finalization_timeout_sec() {
  derive_sample_timeout_sec "$1"
}

validate_worker_grace_values() {
  local variable value
  for variable in WORKER_TERM_GRACE_SEC WORKER_KILL_GRACE_SEC; do
    value=${!variable:-}
    [[ $value =~ ^([1-9]|[12][0-9]|30)$ ]] || return 1
  done
}

configure_timeout_budget() {
  local derived_timeout derived_finalization_timeout
  derived_timeout=$(derive_sample_timeout_sec "$TIER_MIB") || refuse SAMPLE_TIMEOUT_TIER_INVALID
  derived_finalization_timeout=$(derive_integrity_finalization_timeout_sec "$TIER_MIB") || \
    refuse INTEGRITY_FINALIZATION_TIMEOUT_TIER_INVALID
  if [[ -n $SAMPLE_TIMEOUT_SEC ]]; then
    [[ $SAMPLE_TIMEOUT_SEC =~ ^[1-9][0-9]{0,2}$ ]] || refuse SAMPLE_TIMEOUT_INVALID
    (( SAMPLE_TIMEOUT_SEC <= SAMPLE_TIMEOUT_MAX_SEC )) || refuse SAMPLE_TIMEOUT_INVALID
    [[ $SAMPLE_TIMEOUT_SEC == "$derived_timeout" ]] || refuse SAMPLE_TIMEOUT_TIER_MISMATCH
  else
    SAMPLE_TIMEOUT_SEC=$derived_timeout
  fi
  INTEGRITY_FINALIZATION_TIMEOUT_SEC=$derived_finalization_timeout
  CELL_OUTER_TIMEOUT_SEC=$((RUNS * (SAMPLE_TIMEOUT_SEC + INTEGRITY_FINALIZATION_TIMEOUT_SEC) + CELL_SETUP_CLEANUP_TIMEOUT_SEC))
  (( CELL_OUTER_TIMEOUT_SEC < CELL_OUTER_TIMEOUT_MIN_SEC )) && CELL_OUTER_TIMEOUT_SEC=$CELL_OUTER_TIMEOUT_MIN_SEC
  (( CELL_OUTER_TIMEOUT_SEC > 0 && CELL_OUTER_TIMEOUT_SEC <= CELL_OUTER_TIMEOUT_MAX_SEC )) \
    || refuse CELL_TIMEOUT_BUDGET_INVALID
}

while [[ $# -gt 0 ]]; do
  case $1 in
    --aggregate) ACTION=aggregate; shift ;;
    --run) ACTION=run; shift ;;
    --assert-shared-host-approval) ACTION=assert-shared-host-approval; shift ;;
    --mode) MODE=${2:-}; shift 2 ;;
    --condition) CONDITION=${2:-}; shift 2 ;;
    --tier-mib) TIER_MIB=${2:-}; shift 2 ;;
    --artifact-dir) ARTIFACT_DIR=${2:-}; shift 2 ;;
    --samples) SAMPLES=${2:-}; shift 2 ;;
    --out) OUT=${2:-}; shift 2 ;;
    --classify-swap-fixture) ACTION=classify-swap-fixture; shift ;;
    --swap-fixture) SWAP_FIXTURE=${2:-}; shift 2 ;;
    --runs) RUNS=${2:-}; shift 2 ;;
    --sample-timeout-sec) SAMPLE_TIMEOUT_SEC=${2:-}; shift 2 ;;
    --sealed-release-root) SEALED_RELEASE_ROOT=${2:-}; shift 2 ;;
    --release-version) RELEASE_VERSION=${2:-}; shift 2 ;;
    --expected-source-commit) EXPECTED_SOURCE_COMMIT=${2:-}; shift 2 ;;
    --expected-manifest-sha256) EXPECTED_MANIFEST_SHA256=${2:-}; shift 2 ;;
    --pair-id) PAIR_ID=${2:-}; shift 2 ;;
    --validate-evidence) ACTION=validate-evidence; shift ;;
    --validate-nbd-identity-fixture) ACTION=validate-nbd-identity-fixture; shift ;;
    --identity-fixture-root) IDENTITY_FIXTURE_ROOT=${2:-}; shift 2 ;;
    --lower-sink-identity-sha256) IDENTITY_LOWER_SINK_SHA256=${2:-}; shift 2 ;;
    *) refuse UNSUPPORTED_ARGUMENT ;;
  esac
done

[[ $ACTION == aggregate || $ACTION == run || $ACTION == validate-evidence || \
  $ACTION == validate-nbd-identity-fixture || $ACTION == classify-swap-fixture || \
  $ACTION == assert-shared-host-approval ]] \
  || refuse ACTION_REQUIRED
if [[ $ACTION == aggregate || $ACTION == run ]]; then
  [[ $MODE == disk-only || $MODE == nbd ]] || refuse MODE_INVALID
  [[ $CONDITION == idle || $CONDITION == bounded ]] || refuse CONDITION_INVALID
  [[ $TIER_MIB =~ ^(1024|2048|4096)$ ]] || refuse TIER_SIZE_INVALID
  [[ $RUNS == 3 ]] || refuse RUN_COUNT_INVALID
  configure_timeout_budget
  ALLOCATE_MIB=$((TIER_MIB + 2560))
  MEMORY_MAX_MIB=$((ALLOCATE_MIB + 512))
elif [[ $ACTION == validate-nbd-identity-fixture ]]; then
  [[ $TIER_MIB =~ ^(1024|2048|4096)$ ]] || refuse TIER_SIZE_INVALID
elif [[ $ACTION == classify-swap-fixture ]]; then
  [[ ${RAMSHARED_NBD_ALLOW_MANUFACTURED_SWAP_CLASSIFIER_TEST:-} == 1 ]] \
    || refuse SWAP_CLASSIFIER_FIXTURE_FORBIDDEN
  [[ $SWAP_FIXTURE == /* && -f $SWAP_FIXTURE && ! -L $SWAP_FIXTURE ]] \
    || refuse SWAP_CLASSIFIER_FIXTURE_INVALID
fi

if [[ $ACTION == assert-shared-host-approval ]]; then
  require_shared_host_approval
  printf 'NBD_BENCHMARK_STATE=APPROVED\n'
  exit 0
fi

aggregate_samples() {
  [[ -f $SAMPLES && -n $OUT ]] || refuse AGGREGATE_INPUT_INVALID
  python3 - "$SAMPLES" "$OUT" "$MODE" "$CONDITION" "$TIER_MIB" "$SAMPLE_TIMEOUT_SEC" \
    "$INTEGRITY_FINALIZATION_TIMEOUT_SEC" \
    "$CELL_SETUP_CLEANUP_TIMEOUT_SEC" "$CELL_OUTER_TIMEOUT_SEC" <<'PY'
import json
import hashlib
import math
import os
import statistics
import sys

samples_path, output_path, mode, condition, tier_text, sample_timeout_text, finalization_timeout_text, setup_cleanup_text, outer_timeout_text = sys.argv[1:]
tier_mib = int(tier_text)
sample_timeout_sec = int(sample_timeout_text)
integrity_finalization_timeout_sec = int(finalization_timeout_text)
setup_cleanup_timeout_sec = int(setup_cleanup_text)
cell_outer_timeout_sec = int(outer_timeout_text)
rows = []
try:
    with open(samples_path, encoding="utf-8") as source:
        for line in source:
            if line.strip():
                rows.append(json.loads(line))
except (OSError, ValueError, json.JSONDecodeError) as exc:
    raise SystemExit(f"sample_input_invalid:{exc}")

if len(rows) != 3 or {row.get("run") for row in rows} != {1, 2, 3}:
    raise SystemExit("sample_cardinality_invalid")

elapsed = []
for row in rows:
    exact = (
        row.get("schema") == 1
        and row.get("mode") == mode
        and row.get("condition") == condition
        and row.get("tier_mib") == tier_mib
        and row.get("pattern") == "shake256-v1"
        and row.get("allocation_chunk_bytes") == 64 * 1024 * 1024
        and row.get("worker_threads") == 1
        and row.get("workload") == "anonymous_memory_sequential_write"
        and row.get("allocated_mib") == tier_mib + 2560
        and row.get("memory_high_mib") == 1200
        and row.get("memory_max_mib") == tier_mib + 3072
        and row.get("checksum_match") is True
        and row.get("ghost_swap") is False
    )
    if not exact:
        raise SystemExit("sample_contract_mismatch")
    value = row.get("allocation_to_hold_ms")
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value) or value <= 0:
        raise SystemExit("sample_elapsed_invalid")
    if mode == "nbd":
        if (
            row.get("binary_match") != "PASS"
            or row.get("max_zram_delta_kib", 0) < 1024 * 1024 - 8192
            or row.get("max_nbd_delta_kib", 0) < tier_mib * 1024 - 8192
            or row.get("max_disk_delta_kib", 0) <= 8192
        ):
            raise SystemExit("nbd_activity_or_identity_missing")
    else:
        if (
            row.get("binary_match") != "N/A"
            or row.get("max_zram_delta_kib", 0) < 1024 * 1024 - 8192
            or row.get("max_nbd_delta_kib") != 0
            or row.get("max_scratch_delta_kib", 0) < tier_mib * 1024 - 8192
        ):
            raise SystemExit("disk_control_activity_invalid")
    elapsed.append(float(value))

ordered = sorted(elapsed)
summary = {
    "schema": 1,
    "status": "PASS",
    "mode": mode,
    "condition": condition,
    "tier_mib": tier_mib,
    "n": 3,
    "unit": "ms",
    "median_allocation_to_hold_ms": statistics.median(ordered),
    "p99_allocation_to_hold_ms": ordered[math.ceil(0.99 * len(ordered)) - 1],
    "population_stddev_allocation_to_hold_ms": statistics.pstdev(ordered),
    "pattern": "shake256-v1",
    "measurement": "allocation_to_hold_latency",
    "allocation_chunk_bytes": 64 * 1024 * 1024,
    "worker_threads": 1,
    "workload": "anonymous_memory_sequential_write",
    "allocated_mib": tier_mib + 2560,
    "memory_high_mib": 1200,
    "memory_max_mib": tier_mib + 3072,
    "checksum": "PASS",
    "ghost_swap": False,
    "binary_match": "PASS" if mode == "nbd" else "N/A",
    "terminal_state": "PRODUCT_OFF",
    "timeout_budget": {
        "sample_timeout_sec": sample_timeout_sec,
        "integrity_finalization_timeout_sec": integrity_finalization_timeout_sec,
        "samples": 3,
        "setup_cleanup_timeout_sec": setup_cleanup_timeout_sec,
        "cell_outer_timeout_sec": cell_outer_timeout_sec,
    },
    "samples": rows,
}
context_path = os.path.join(os.path.dirname(samples_path), "context.json")
if os.path.isfile(context_path):
    with open(context_path, "rb") as source:
        summary["context_sha256"] = hashlib.sha256(source.read()).hexdigest()
temporary = f"{output_path}.tmp"
with open(temporary, "w", encoding="utf-8") as target:
    json.dump(summary, target, sort_keys=True, separators=(",", ":"))
    target.write("\n")
os.replace(temporary, output_path)
PY
  printf 'NBD_BENCHMARK_STATE=PASS\n'
  printf 'NBD_BENCHMARK_SUMMARY=%s\n' "$OUT"
}

validate_artifact_inventory() {
  local inventory_path=${1:-"$ARTIFACT_DIR/artifact-inventory.json"}
  python3 - "$ARTIFACT_DIR" "$inventory_path" <<'PY'
import hashlib
import json
import os
import stat
import sys

root = os.path.realpath(sys.argv[1])
inventory_path = os.path.abspath(sys.argv[2])
required = {
    "before.txt", "action.txt", "after.txt", "context.json", "samples.jsonl", "summary.json",
}
try:
    if os.path.dirname(inventory_path) != root:
        raise ValueError("inventory_path")
    excluded = {"artifact-inventory.json", "evidence-envelope.json", os.path.basename(inventory_path)}
    metadata = os.lstat(inventory_path)
    if not stat.S_ISREG(metadata.st_mode):
        raise ValueError("inventory_not_regular")
    with open(inventory_path, encoding="utf-8") as source:
        inventory = json.load(source)
    if inventory.get("schema") != 2 or not isinstance(inventory.get("files"), list):
        raise ValueError("inventory_schema")
    rows = inventory["files"]
    names = []
    for row in rows:
        name = row.get("name")
        if not isinstance(name, str) or not name or "/" in name or "\\" in name or name in excluded:
            raise ValueError("inventory_name")
        if not isinstance(row.get("bytes"), int) or row["bytes"] < 0:
            raise ValueError("inventory_bytes")
        digest = row.get("sha256")
        if not isinstance(digest, str) or len(digest) != 64 or any(ch not in "0123456789abcdef" for ch in digest.lower()):
            raise ValueError("inventory_hash_format")
        names.append(name)
    if names != sorted(names) or len(names) != len(set(names)):
        raise ValueError("inventory_order")
    actual = []
    for name in sorted(os.listdir(root)):
        if name in excluded:
            continue
        path = os.path.join(root, name)
        metadata = os.lstat(path)
        if not stat.S_ISREG(metadata.st_mode):
            raise ValueError("artifact_not_regular")
        actual.append(name)
    if names != actual or not required.issubset(set(names)):
        raise ValueError("inventory_completeness")
    for row in rows:
        path = os.path.join(root, row["name"])
        metadata = os.lstat(path)
        with open(path, "rb") as source:
            digest = hashlib.sha256(source.read()).hexdigest()
        if metadata.st_size != row["bytes"] or digest != row["sha256"].lower():
            raise ValueError("inventory_hash_mismatch")
except (OSError, ValueError, json.JSONDecodeError) as exc:
    raise SystemExit(f"artifact_inventory_invalid:{exc}")
PY
}

validate_evidence_envelope() {
  local inventory_path=${1:-"$ARTIFACT_DIR/artifact-inventory.json"}
  local envelope_path=${2:-"$ARTIFACT_DIR/evidence-envelope.json"}
  python3 - "$ARTIFACT_DIR" "$inventory_path" "$envelope_path" <<'PY'
import hashlib
import json
import os
import stat
import sys

root = os.path.realpath(sys.argv[1])
inventory_path = os.path.abspath(sys.argv[2])
envelope_path = os.path.abspath(sys.argv[3])

def direct_regular(path):
    if os.path.dirname(path) != root:
        raise ValueError("candidate_path")
    metadata = os.lstat(path)
    if not stat.S_ISREG(metadata.st_mode):
        raise ValueError("candidate_not_regular")

def digest_path(path):
    with open(path, "rb") as source:
        return hashlib.sha256(source.read()).hexdigest()

try:
    direct_regular(inventory_path)
    direct_regular(envelope_path)
    with open(inventory_path, encoding="utf-8") as source:
        inventory = json.load(source)
    with open(envelope_path, encoding="utf-8") as source:
        envelope = json.load(source)
    with open(os.path.join(root, "context.json"), encoding="utf-8") as source:
        context = json.load(source)
    with open(os.path.join(root, "context.json"), "rb") as source:
        context_sha = hashlib.sha256(source.read()).hexdigest()
    with open(os.path.join(root, "summary.json"), "rb") as source:
        summary_bytes = source.read()
    summary_sha = hashlib.sha256(summary_bytes).hexdigest()
    summary = json.loads(summary_bytes.decode("utf-8"))
    inventory_sha = digest_path(inventory_path)
    required = {
        "schema_version", "pair_id", "mode", "release", "context_sha256", "summary_sha256",
        "artifact_inventory_sha256", "artifacts", "binary_match", "watchdog", "classification", "timeout_budget",
    }
    if set(envelope) != required or envelope.get("schema_version") != "ramshared-nbd-cell-evidence/v1":
        raise ValueError("envelope_schema")
    release = envelope["release"]
    context_release = context.get("release")
    if not isinstance(release, dict) or not isinstance(context_release, dict):
        raise ValueError("release_schema")
    release_required = {"version", "source_commit", "manifest_sha256", "input_bundle_manifest_sha256"}
    if set(release) != release_required:
        raise ValueError("release_schema")
    for key in release_required:
        if release[key] != context_release.get(key):
            raise ValueError("release_mismatch")
    if envelope["pair_id"] != context.get("pair_id") or envelope["mode"] != context.get("mode"):
        raise ValueError("envelope_context_mismatch")
    if envelope["binary_match"] != context.get("binary_match") or envelope["watchdog"] != context.get("watchdog"):
        raise ValueError("envelope_context_mismatch")
    if (envelope["timeout_budget"] != context.get("timeout_budget") or
            summary.get("timeout_budget") != context.get("timeout_budget")):
        raise ValueError("timeout_budget_mismatch")
    if envelope["classification"] != "INCOMPARABLE":
        raise ValueError("classification")
    if envelope["context_sha256"] != context_sha or envelope["summary_sha256"] != summary_sha:
        raise ValueError("receipt_hash")
    if envelope["artifact_inventory_sha256"] != inventory_sha:
        raise ValueError("inventory_hash")
    expected = [{"path": row["name"], "bytes": row["bytes"], "sha256": row["sha256"]} for row in inventory["files"]]
    if envelope.get("artifacts") != expected:
        raise ValueError("envelope_inventory_mismatch")
    encoded = json.dumps(envelope, sort_keys=True, separators=(",", ":"))
    for forbidden in ("/home/", "/mnt/c/Users/", "C:\\Users\\", "\\\\wsl$"):
        if forbidden in encoded:
            raise ValueError("private_path")
except (OSError, ValueError, KeyError, TypeError, json.JSONDecodeError) as exc:
    raise SystemExit(f"evidence_envelope_invalid:{exc}")
PY
}

if [[ $ACTION == validate-evidence ]]; then
  [[ -n $ARTIFACT_DIR && -d $ARTIFACT_DIR && ! -L $ARTIFACT_DIR ]] || refuse EVIDENCE_ARTIFACT_DIR_INVALID
  validate_artifact_inventory || refuse EVIDENCE_INVENTORY_INVALID
  validate_evidence_envelope || refuse EVIDENCE_ENVELOPE_INVALID
  printf 'NBD_BENCHMARK_EVIDENCE=PASS\n'
  exit 0
fi

if [[ $ACTION == aggregate ]]; then
  aggregate_samples
  exit 0
fi

identity_fail() {
  NBD_IDENTITY_REASON=$1
  return 1
}

read_manifest_daemon_hash() {
  local manifest=$1 line digest relative count=0
  NBD_DAEMON_MANIFEST_SHA256=""
  [[ -f $manifest && ! -L $manifest ]] || { identity_fail NBD_IDENTITY_MANIFEST_INVALID; return 1; }
  while IFS= read -r line || [[ -n $line ]]; do
    [[ $line =~ ^([[:xdigit:]]{64})\ \ \./([A-Za-z0-9][A-Za-z0-9._/-]*)$ ]] \
      || { identity_fail NBD_IDENTITY_MANIFEST_INVALID; return 1; }
    digest=${BASH_REMATCH[1],,}
    relative=${BASH_REMATCH[2]}
    if [[ $relative == bin/ramsharedd ]]; then
      count=$((count + 1))
      NBD_DAEMON_MANIFEST_SHA256=$digest
    fi
  done <"$manifest"
  [[ $count == 1 && $NBD_DAEMON_MANIFEST_SHA256 =~ ^[0-9a-f]{64}$ ]] \
    || { identity_fail NBD_IDENTITY_MANIFEST_INVALID; return 1; }
}

derive_nbd_second_tier_identity() {
  local swaps_file=$1 sys_block_root=$2 dev_root=$3 proc_root=$4 pid_file=$5 daemon_path=$6
  local manifest_path=$7 lower_sink_identity=$8 fixture_mode=$9
  local line filename type size_kib used_kib priority remainder nbd_index="" nbd_name="" candidate_count=0
  local device_path sys_device_path sys_size_path daemon_hash raw_exe resolved_exe pid
  local expected_size_kib expected_capacity_sectors
  local -a capacity_lines=()

  NBD_IDENTITY_REASON=""
  NBD_DEVICE=""
  NBD_BLOCK_MAJOR_MINOR=""
  NBD_SIZE_KIB=""
  NBD_USABLE_SIZE_KIB=""
  NBD_CAPACITY_SECTORS=""
  NBD_PRIORITY=""
  NBD_SERVER_PID=""
  NBD_DAEMON_MANIFEST_SHA256=""
  NBD_SECOND_TIER_IDENTITY_SHA256=""
  [[ $lower_sink_identity =~ ^[0-9a-f]{64}$ ]] || { identity_fail NBD_IDENTITY_LOWER_SINK_INVALID; return 1; }
  [[ -r $swaps_file && ! -L $swaps_file ]] || { identity_fail NBD_IDENTITY_SWAPS_UNREADABLE; return 1; }
  [[ -d $sys_block_root && ! -L $sys_block_root && -d $dev_root && ! -L $dev_root ]] \
    || { identity_fail NBD_IDENTITY_FOREIGN_DEVICE; return 1; }
  [[ -d $proc_root && ! -L $proc_root ]] || { identity_fail NBD_IDENTITY_PROC_UNREADABLE; return 1; }
  [[ -f $pid_file && ! -L $pid_file && -f $daemon_path && ! -L $daemon_path && -x $daemon_path ]] \
    || { identity_fail NBD_IDENTITY_DAEMON_EXECUTABLE_MISMATCH; return 1; }

  while IFS= read -r line || [[ -n $line ]]; do
    read -r filename type size_kib used_kib priority remainder <<<"$line"
    [[ $filename != Filename ]] || continue
    if [[ $filename == /dev/nbd* && ! $filename =~ ^/dev/nbd[0-9]+$ ]]; then
      identity_fail NBD_IDENTITY_FOREIGN_DEVICE
      return 1
    fi
    [[ $filename =~ ^/dev/nbd([0-9]+)$ ]] || continue
    nbd_index=${BASH_REMATCH[1]}
    [[ $line != *'(deleted)'* && $type == partition && $size_kib =~ ^[0-9]+$ && $used_kib =~ ^[0-9]+$ \
      && $priority =~ ^-?[0-9]+$ && -z $remainder ]] \
      || { identity_fail NBD_IDENTITY_FOREIGN_DEVICE; return 1; }
    candidate_count=$((candidate_count + 1))
    nbd_name=nbd$nbd_index
    NBD_DEVICE=$filename
    NBD_USABLE_SIZE_KIB=$size_kib
    NBD_PRIORITY=$priority
  done <"$swaps_file"
  (( candidate_count >= 1 )) || { identity_fail NBD_IDENTITY_MISSING; return 1; }
  (( candidate_count == 1 )) || { identity_fail NBD_IDENTITY_DUPLICATE; return 1; }
  [[ $NBD_DEVICE == "/dev/$nbd_name" ]] || { identity_fail NBD_IDENTITY_FOREIGN_DEVICE; return 1; }
  expected_size_kib=$((TIER_MIB * 1024))
  expected_capacity_sectors=$((expected_size_kib * 2))
  [[ $NBD_PRIORITY == 100 ]] || { identity_fail NBD_IDENTITY_PRIORITY_MISMATCH; return 1; }

  device_path="$dev_root/$nbd_name"
  sys_device_path="$sys_block_root/$nbd_name/dev"
  sys_size_path="$sys_block_root/$nbd_name/size"
  if [[ $fixture_mode == 1 ]]; then
    [[ -f $device_path && ! -L $device_path ]] || { identity_fail NBD_IDENTITY_FOREIGN_DEVICE; return 1; }
  else
    [[ -b $device_path && ! -L $device_path ]] || { identity_fail NBD_IDENTITY_FOREIGN_DEVICE; return 1; }
  fi
  [[ -f $sys_device_path && ! -L $sys_device_path ]] || { identity_fail NBD_IDENTITY_FOREIGN_DEVICE; return 1; }
  NBD_BLOCK_MAJOR_MINOR=$(tr -d '[:space:]' <"$sys_device_path")
  [[ $NBD_BLOCK_MAJOR_MINOR =~ ^[0-9]+:[0-9]+$ ]] || { identity_fail NBD_IDENTITY_FOREIGN_DEVICE; return 1; }
  [[ -f $sys_size_path && ! -L $sys_size_path ]] \
    || { identity_fail NBD_IDENTITY_SYSFS_CAPACITY_INVALID; return 1; }
  mapfile -t capacity_lines <"$sys_size_path" \
    || { identity_fail NBD_IDENTITY_SYSFS_CAPACITY_INVALID; return 1; }
  (( ${#capacity_lines[@]} == 1 )) \
    || { identity_fail NBD_IDENTITY_SYSFS_CAPACITY_INVALID; return 1; }
  NBD_CAPACITY_SECTORS=${capacity_lines[0]}
  # The supported tiers need at most seven decimal digits. Bound untrusted
  # decimal text before Bash arithmetic so an over-width value cannot wrap.
  [[ $NBD_CAPACITY_SECTORS =~ ^[1-9][0-9]{0,7}$ ]] \
    || { identity_fail NBD_IDENTITY_SYSFS_CAPACITY_INVALID; return 1; }
  [[ $NBD_CAPACITY_SECTORS == "$expected_capacity_sectors" ]] \
    || { identity_fail NBD_IDENTITY_CAPACITY_MISMATCH; return 1; }
  [[ $NBD_USABLE_SIZE_KIB =~ ^[1-9][0-9]{0,7}$ ]] \
    || { identity_fail NBD_IDENTITY_USABLE_SIZE_INVALID; return 1; }
  (( NBD_USABLE_SIZE_KIB >= expected_size_kib - NBD_MKSWAP_OVERHEAD_KIB && \
    NBD_USABLE_SIZE_KIB <= expected_size_kib )) \
    || { identity_fail NBD_IDENTITY_USABLE_SIZE_INVALID; return 1; }
  # Keep the context and identity normalized to the exact block capacity. The
  # /proc/swaps value is an observed mkswap usable-size detail, not capacity.
  NBD_SIZE_KIB=$expected_size_kib

  grep -qx '[1-9][0-9]*' "$pid_file" || { identity_fail NBD_IDENTITY_SERVER_PID_INVALID; return 1; }
  pid=$(tr -d '[:space:]' <"$pid_file")
  [[ -d $proc_root/$pid && ! -L $proc_root/$pid && -L $proc_root/$pid/exe ]] \
    || { identity_fail NBD_IDENTITY_SERVER_PID_STALE; return 1; }
  raw_exe=$(readlink -- "$proc_root/$pid/exe" 2>/dev/null || true)
  resolved_exe=$(readlink -f -- "$proc_root/$pid/exe" 2>/dev/null || true)
  [[ -n $raw_exe && $raw_exe != *'(deleted)'* && $raw_exe == "$daemon_path" && $resolved_exe == "$daemon_path" ]] \
    || { identity_fail NBD_IDENTITY_DAEMON_EXECUTABLE_MISMATCH; return 1; }
  read_manifest_daemon_hash "$manifest_path" || return 1
  daemon_hash=$(sha256sum -- "$daemon_path" | awk '{print $1}')
  [[ $daemon_hash == "$NBD_DAEMON_MANIFEST_SHA256" ]] || { identity_fail NBD_IDENTITY_DAEMON_HASH_MISMATCH; return 1; }

  NBD_SERVER_PID=$pid
  NBD_SECOND_TIER_IDENTITY_SHA256=$(printf '%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s' \
    ramshared-nbd-second-tier/v1 "$NBD_DEVICE" "$NBD_BLOCK_MAJOR_MINOR" "$NBD_SIZE_KIB" \
    "$NBD_PRIORITY" "$NBD_SERVER_PID" "$daemon_path" "$NBD_DAEMON_MANIFEST_SHA256" | sha256sum | awk '{print $1}')
  [[ $NBD_SECOND_TIER_IDENTITY_SHA256 =~ ^[0-9a-f]{64}$ ]] || { identity_fail NBD_IDENTITY_HASH_INVALID; return 1; }
  [[ $NBD_SECOND_TIER_IDENTITY_SHA256 != "$lower_sink_identity" ]] \
    || { identity_fail NBD_IDENTITY_SINK_HASH_SUBSTITUTION; return 1; }
}

swap_used() {
  python3 - "$SWAPS_FILE" <<'PY'
import re
import sys
zram = nbd = disk = 0
ghost = 0
with open(sys.argv[1], encoding="utf-8") as source:
    next(source, None)
    for line in source:
        if "(deleted)" in line:
            ghost = 1
        fields = line.split()
        if len(fields) < 5:
            continue
        name, used = fields[0], int(fields[3])
        if re.fullmatch(r"/dev/zram[0-9]+", name):
            zram += used
        elif re.fullmatch(r"/dev/(?:nbd[0-9]+|ublkb[0-9]+)", name):
            nbd += used
        else:
            disk += used
print(zram, nbd, disk, ghost)
PY
}

capture_sample_zram_device() {
  awk '
    NR == 1 { next }
    $1 ~ /^\/dev\/zram[0-9]+$/ && $2 == "partition" && $5 == 200 && NF == 5 {
      device = $1
      found += 1
    }
    END { if (found == 1) print device; else exit 1 }
  ' "$SWAPS_FILE"
}

if [[ $ACTION == validate-nbd-identity-fixture ]]; then
  [[ ${RAMSHARED_NBD_ALLOW_MANUFACTURED_IDENTITY_TEST:-} == 1 ]] || refuse IDENTITY_FIXTURE_FORBIDDEN
  [[ $IDENTITY_FIXTURE_ROOT == /* && -d $IDENTITY_FIXTURE_ROOT && ! -L $IDENTITY_FIXTURE_ROOT ]] \
    || refuse IDENTITY_FIXTURE_ROOT_INVALID
  fixture_root=$(readlink -f -- "$IDENTITY_FIXTURE_ROOT" 2>/dev/null || true)
  [[ $fixture_root == "$IDENTITY_FIXTURE_ROOT" ]] || refuse IDENTITY_FIXTURE_ROOT_INVALID
  if ! derive_nbd_second_tier_identity "$fixture_root/proc/swaps" "$fixture_root/sys/block" \
    "$fixture_root/dev" "$fixture_root/proc" "$fixture_root/run/ramsharedd.pid" \
    "$fixture_root/release/bin/ramsharedd" "$fixture_root/release/SHA256SUMS" \
    "$IDENTITY_LOWER_SINK_SHA256" 1; then
    printf 'NBD_IDENTITY_STATE=REFUSED\n'
    printf 'NBD_IDENTITY_REASON=%s\n' "$NBD_IDENTITY_REASON"
    exit 2
  fi
  printf 'NBD_IDENTITY_STATE=PASS\n'
  printf 'NBD_DEVICE=%s\n' "$NBD_DEVICE"
  printf 'NBD_BLOCK_MAJOR_MINOR=%s\n' "$NBD_BLOCK_MAJOR_MINOR"
  printf 'NBD_CAPACITY_SECTORS=%s\n' "$NBD_CAPACITY_SECTORS"
  printf 'NBD_USABLE_SIZE_KIB=%s\n' "$NBD_USABLE_SIZE_KIB"
  printf 'NBD_SIZE_KIB=%s\n' "$NBD_SIZE_KIB"
  printf 'NBD_PRIORITY=%s\n' "$NBD_PRIORITY"
  printf 'NBD_SERVER_PID=%s\n' "$NBD_SERVER_PID"
  printf 'NBD_DAEMON_MANIFEST_SHA256=%s\n' "$NBD_DAEMON_MANIFEST_SHA256"
  printf 'NBD_SECOND_TIER_IDENTITY_SHA256=%s\n' "$NBD_SECOND_TIER_IDENTITY_SHA256"
  printf 'NBD_LOWER_SINK_IDENTITY_SHA256=%s\n' "$IDENTITY_LOWER_SINK_SHA256"
  exit 0
fi

if [[ $ACTION == classify-swap-fixture ]]; then
  SWAPS_FILE=$SWAP_FIXTURE
  read -r zram_used nbd_used disk_used ghost_used <<<"$(swap_used)"
  printf 'SWAP_USED_ZRAM_KIB=%s\n' "$zram_used"
  printf 'SWAP_USED_NBD_KIB=%s\n' "$nbd_used"
  printf 'SWAP_USED_DISK_KIB=%s\n' "$disk_used"
  printf 'SWAP_USED_GHOST=%s\n' "$ghost_used"
  exit 0
fi

[[ $LIVE_SEAM_PRESENT == 0 && $PRODUCT_ROOT == /opt/ramshared && $CG_ROOT == /sys/fs/cgroup && $SWAPS_FILE == /proc/swaps \
  && $PID_FILE == /run/ramshared/ramsharedd.pid && $ZRAM_RECORD == /run/ramshared/zram-dev \
  && $PROC_ROOT == /proc && $DEV_ROOT == /dev && $SYS_BLOCK_ROOT == /sys/block ]] \
  || refuse LIVE_TEST_SEAM_FORBIDDEN
validate_worker_grace_values || refuse WORKER_GRACE_INVALID
[[ -n $SEALED_RELEASE_ROOT && -n $RELEASE_VERSION && -n $EXPECTED_SOURCE_COMMIT && -n $EXPECTED_MANIFEST_SHA256 && -n $PAIR_ID ]] \
  || refuse REVIEWED_RELEASE_BINDING_REQUIRED
[[ $RELEASE_VERSION =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$ \
  && $EXPECTED_SOURCE_COMMIT =~ ^[0-9a-f]{40}$ \
  && $EXPECTED_MANIFEST_SHA256 =~ ^[0-9a-f]{64}$ \
  && $PAIR_ID =~ ^[A-Za-z0-9][A-Za-z0-9._-]{2,127}$ ]] \
  || refuse REVIEWED_RELEASE_BINDING_INVALID
[[ $SEALED_RELEASE_ROOT == "/opt/ramshared/releases/$RELEASE_VERSION" && -d $SEALED_RELEASE_ROOT && ! -L $SEALED_RELEASE_ROOT ]] \
  || refuse REVIEWED_RELEASE_BINDING_INVALID
[[ $(readlink -f -- "$SEALED_RELEASE_ROOT" 2>/dev/null || true) == "$SEALED_RELEASE_ROOT" ]] \
  || refuse REVIEWED_RELEASE_BINDING_INVALID
RELEASE=$SEALED_RELEASE_ROOT
VERSION=$RELEASE_VERSION
[[ $(id -u) == 0 ]] || refuse ROOT_REQUIRED
[[ -n $ARTIFACT_DIR && ! -e $ARTIFACT_DIR ]] || refuse ARTIFACT_DIR_MUST_BE_FRESH
require_shared_host_approval
[[ ${RAMSHARED_WINDOWS_WATCHDOG_ARMED:-} == 1 ]] || refuse WINDOWS_WATCHDOG_NOT_ARMED
[[ ${RAMSHARED_NBD_BENCHMARK_APPROVAL:-} == "benchmark:$VERSION:$TIER_MIB:$CONDITION:$MODE" ]] || refuse BENCHMARK_APPROVAL_MISSING

PREFLIGHT="$RELEASE/scripts/safety/nbd-product-preflight.sh"
CLI="$RELEASE/bin/ramshared"
DAEMON="$RELEASE/bin/ramsharedd"
WORKER="$RELEASE/scripts/safety/cascade_pressure_integrity_worker.py"
CGROUP_LAUNCH="$RELEASE/scripts/safety/nbd-benchmark-cgroup-launch.sh"
BENCHMARK_LIB="$RELEASE/scripts/safety/nbd-benchmark-lib.sh"
[[ -x $CLI && -x $DAEMON && -x $PREFLIGHT && -x $WORKER \
  && -x $CGROUP_LAUNCH && -f $BENCHMARK_LIB && ! -L $BENCHMARK_LIB \
  && -x $RELEASE/scripts/safety/cascade-up.sh && -x $RELEASE/scripts/safety/cascade-down.sh \
  && -f $RELEASE/SOURCE_COMMIT && ! -L $RELEASE/SOURCE_COMMIT \
  && -f $RELEASE/SOURCE_BRANCH && ! -L $RELEASE/SOURCE_BRANCH \
  && -f $RELEASE/SOURCE_TREE_STATE && ! -L $RELEASE/SOURCE_TREE_STATE ]] \
  || refuse RELEASE_BENCHMARK_LAYOUT_INVALID
SOURCE_COMMIT=$(tr -d '[:space:]' <"$RELEASE/SOURCE_COMMIT")
SOURCE_BRANCH=$(tr -d '[:space:]' <"$RELEASE/SOURCE_BRANCH")
SOURCE_TREE_STATE=$(tr -d '[:space:]' <"$RELEASE/SOURCE_TREE_STATE")
[[ $SOURCE_COMMIT =~ ^[0-9a-f]{40}$ && $SOURCE_BRANCH =~ ^[A-Za-z0-9._/-]{1,200}$ \
  && $SOURCE_TREE_STATE == clean && $SOURCE_COMMIT == "$EXPECTED_SOURCE_COMMIT" ]] \
  || refuse RELEASE_SOURCE_IDENTITY_INVALID
[[ $(sha256sum -- "$RELEASE/SHA256SUMS" | awk '{print $1}') == "$EXPECTED_MANIFEST_SHA256" ]] \
  || refuse REVIEWED_MANIFEST_MISMATCH
[[ -f $RELEASE/INPUT_BUNDLE_SHA256SUMS && ! -L $RELEASE/INPUT_BUNDLE_SHA256SUMS ]] \
  || refuse INPUT_BUNDLE_MANIFEST_INVALID
EXPECTED_INPUT_BUNDLE_MANIFEST_SHA256=$(sha256sum -- "$RELEASE/INPUT_BUNDLE_SHA256SUMS" | awk '{print $1}')
[[ $EXPECTED_INPUT_BUNDLE_MANIFEST_SHA256 =~ ^[0-9a-f]{64}$ ]] \
  || refuse INPUT_BUNDLE_MANIFEST_INVALID
# shellcheck source=nbd-benchmark-lib.sh
source "$BENCHMARK_LIB"
mkdir -m 0700 -- "$ARTIFACT_DIR"
SAMPLES="$ARTIFACT_DIR/samples.jsonl"
OUT="$ARTIFACT_DIR/summary.json"
CG="$CG_ROOT/ramshared-nbd-benchmark-$$"
UTC_STARTED=$(date -u +%Y-%m-%dT%H:%M:%SZ)
WORKER_PID=""
SCRATCH_SWAP=""
SCRATCH_IDENTITY=""
SCRATCH_SWAP_ACTIVE=0
CLEANUP_OK=1
PRODUCT_OFF_PHASES=""
INITIAL_EPOCH_STATE=unattempted
FINAL_EPOCH_STATE=unattempted
PREFLIGHT_TIMEOUT_SEC=15

pinned_preflight() {
  env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin "RAMSHARED_NBD_VRAM_MIB=$TIER_MIB" "$PREFLIGHT" --check \
    --sealed-release-root "$RELEASE" --release-version "$VERSION" \
    --expected-source-commit "$EXPECTED_SOURCE_COMMIT" \
    --expected-manifest-sha256 "$EXPECTED_MANIFEST_SHA256"
}

preflight_field() {
  local path=$1 key=$2
  awk -F= -v key="$key" '$1 == key { print $2; found = 1 } END { exit(found ? 0 : 1) }' "$path"
}

load_bound_lower_sink() {
  local config sink configured_type configured_identity canonical metadata actual_identity
  config="$RELEASE/scripts/safety/cascade.conf.example"
  sink=$(awk -F= '$1 == "NBD_LOWER_SINK" { print $2; exit }' "$config")
  configured_type=$(awk -F= '$1 == "NBD_LOWER_SINK_TYPE" { print $2; exit }' "$config")
  configured_identity=$(awk -F= '$1 == "NBD_LOWER_SINK_IDENTITY_SHA256" { print $2; exit }' "$config")
  [[ $sink == /* && $sink =~ ^/[A-Za-z0-9._/-]{1,480}$ && ! -L $sink && -d $sink ]] \
    || refuse LOWER_TIER_SINK_IDENTITY_INVALID
  canonical=$(readlink -f -- "$sink" 2>/dev/null || true)
  [[ $canonical == "$sink" && $configured_type == directory && $configured_identity =~ ^[0-9a-f]{64}$ ]] \
    || refuse LOWER_TIER_SINK_IDENTITY_INVALID
  metadata=$(stat -c '%d:%i:%u:%g:%a:%F' -- "$canonical" 2>/dev/null || true)
  [[ $metadata =~ ^[0-9]+:[0-9]+:[0-9]+:[0-9]+:[0-7]{3,4}:directory$ ]] \
    || refuse LOWER_TIER_SINK_IDENTITY_INVALID
  actual_identity=$(printf '%s\0%s\0%s' "$canonical" directory "$metadata" | sha256sum | awk '{print $1}')
  [[ $actual_identity == "$configured_identity" ]] || refuse LOWER_TIER_SINK_IDENTITY_INVALID
  LOWER_SINK_CANONICAL=$canonical
  LOWER_SINK_TYPE=directory
  LOWER_SINK_IDENTITY_SHA256=$actual_identity
}

scratch_identity() {
  [[ -n $SCRATCH_SWAP ]] || return 1
  nbd_scratch_identity "$SCRATCH_SWAP"
}

scratch_used_kib() {
  [[ -n $SCRATCH_SWAP ]] || { printf '0\n'; return; }
  awk -v target="$SCRATCH_SWAP" 'NR > 1 && $1 == target { print $4; found = 1 } END { if (!found) print 0 }' "$SWAPS_FILE"
}

create_disk_scratch() {
  local identity
  [[ -n $LOWER_SINK_CANONICAL && $LOWER_SINK_TYPE == directory ]] || refuse SCRATCH_ROOT_INVALID
  SCRATCH_SWAP="$LOWER_SINK_CANONICAL/.ramshared-benchmark-swap-$TIER_MIB-$CONDITION-$$"
  python3 - "$SCRATCH_SWAP" <<'PY'
import os
import sys

path = sys.argv[1]
flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW
fd = os.open(path, flags, 0o600)
os.close(fd)
PY
  chmod 0600 -- "$SCRATCH_SWAP" || refuse SCRATCH_MODE_FAILED
  identity=$(scratch_identity) || refuse SCRATCH_IDENTITY_INVALID
  [[ $identity == *':0:0:600:8180' ]] || refuse SCRATCH_IDENTITY_INVALID
  SCRATCH_IDENTITY=$identity
  SCRATCH_IDENTITY_SHA256=$(printf '%s' "$SCRATCH_IDENTITY" | sha256sum | awk '{print $1}')
  fallocate -l 8G -- "$SCRATCH_SWAP" || refuse SCRATCH_ALLOCATION_FAILED
  [[ $(scratch_identity) == "$SCRATCH_IDENTITY" ]] || refuse SCRATCH_IDENTITY_INVALID
  mkswap -- "$SCRATCH_SWAP" >"$ARTIFACT_DIR/scratch-mkswap.txt" || refuse SCRATCH_MKSWAP_FAILED
  swapon -p 100 -- "$SCRATCH_SWAP" || refuse SCRATCH_SWAPON_FAILED
  awk -v target="$SCRATCH_SWAP" 'NR > 1 && $1 == target { found += 1 } END { exit(found == 1 ? 0 : 1) }' "$SWAPS_FILE" || refuse SCRATCH_SWAP_NOT_PUBLISHED
  SCRATCH_SWAP_ACTIVE=1
  printf 'SCRATCH_IDENTITY_SHA256=%s\n' "$SCRATCH_IDENTITY_SHA256" >"$ARTIFACT_DIR/scratch-identity.txt"
}

create_zram_control() {
  local zdev="" algo output
  [[ ! -e $ZRAM_RECORD && ! -L $ZRAM_RECORD ]] || refuse ZRAM_RECORD_ALREADY_EXISTS
  modprobe zram || refuse ZRAM_MODULE_UNAVAILABLE
  for algo in zstd lz4 lzo-rle lzo; do
    if output=$(zramctl --find --size 1024M --algorithm "$algo" 2>/dev/null); then
      zdev=${output%%[[:space:]]*}
      break
    fi
  done
  [[ $zdev =~ ^/dev/zram[0-9]+$ && -b $zdev ]] || refuse ZRAM_DEVICE_INVALID
  if ! mkswap -- "$zdev" >"$ARTIFACT_DIR/zram-mkswap.txt"; then
    zramctl -r -- "$zdev" 2>/dev/null || true
    refuse ZRAM_MKSWAP_FAILED
  fi
  install -d -m 0700 -o root -g root -- "${ZRAM_RECORD%/*}"
  if ! ( set -o noclobber; printf '%s\n' "$zdev" >"$ZRAM_RECORD" ) 2>/dev/null; then
    zramctl -r -- "$zdev" 2>/dev/null || true
    refuse ZRAM_RECORD_CREATE_FAILED
  fi
  if ! swapon -p 200 -- "$zdev"; then
    rm -f -- "$ZRAM_RECORD"
    zramctl -r -- "$zdev" 2>/dev/null || true
    refuse ZRAM_SWAPON_FAILED
  fi
  if ! awk -v target="$zdev" 'NR > 1 && $1 == target && $5 == 200 { found += 1 } END { exit(found == 1 ? 0 : 1) }' \
    "$SWAPS_FILE"; then
    swapoff -- "$zdev" 2>/dev/null || true
    rm -f -- "$ZRAM_RECORD"
    zramctl -r -- "$zdev" 2>/dev/null || true
    refuse ZRAM_SWAP_NOT_PUBLISHED
  fi
  printf 'ZRAM_CONTROL_DEVICE=%s\n' "$zdev" >"$ARTIFACT_DIR/zram-control.txt"
}

republish_sample_baseline() {
  local zdev=$SAMPLE_ZRAM_DEVICE lower lower_type original_nbd_identity transaction_output
  SAMPLE_BASELINE_REASON="BASELINE_REPUBLICATION_FAILED"
  [[ $zdev =~ ^/dev/zram[0-9]+$ ]] \
    || { SAMPLE_BASELINE_REASON=NBD_REPUBLICATION_ZRAM_IDENTITY_INVALID; return 1; }
  if [[ $MODE == nbd ]]; then
    lower=$NBD_DEVICE
    lower_type=partition
    original_nbd_identity=$NBD_SECOND_TIER_IDENTITY_SHA256
    if ! transaction_output=$(nbd_preserved_connection_republish_swap_pair \
      "$SWAPS_FILE" "$zdev" "$lower" /sbin/swapoff /sbin/mkswap /sbin/swapon 2>&1); then
      SAMPLE_BASELINE_REASON=$(nbd_republication_reason_from_output "$transaction_output")
      return 1
    fi
  else
    lower=$SCRATCH_SWAP
    lower_type=file
    nbd_republish_swap_pair "$SWAPS_FILE" "$zdev" "$lower" "$lower_type" /sbin/swapoff /sbin/swapon \
      || return 1
  fi
  if [[ $MODE == nbd ]]; then
    derive_nbd_second_tier_identity "$SWAPS_FILE" "$SYS_BLOCK_ROOT" "$DEV_ROOT" "$PROC_ROOT" \
      "$PID_FILE" "$DAEMON" "$RELEASE/SHA256SUMS" "$LOWER_SINK_IDENTITY_SHA256" 0 \
      || { SAMPLE_BASELINE_REASON=${NBD_IDENTITY_REASON:-NBD_REPUBLICATION_IDENTITY_INVALID}; return 1; }
    [[ $NBD_SECOND_TIER_IDENTITY_SHA256 == "$original_nbd_identity" ]] \
      || { SAMPLE_BASELINE_REASON=NBD_REPUBLICATION_IDENTITY_DRIFT; return 1; }
    pinned_preflight | grep -q '^NBD_BINARY_MATCH=PASS$' \
      || { SAMPLE_BASELINE_REASON=NBD_REPUBLICATION_BINARY_MATCH_FAILED; return 1; }
  else
    [[ $(scratch_identity) == "$SCRATCH_IDENTITY" ]] || return 1
  fi
}

write_live_context_v2() {
  local preflight kernel manifest_sha zram_device zram_name zram_algorithm zram_size_kib zram_priority
  local lower_kind lower_identity binary_match input_bundle_manifest_sha
  local nbd_device nbd_block_major_minor nbd_size_kib nbd_usable_size_kib nbd_capacity_sectors
  local nbd_priority nbd_server_pid nbd_daemon_manifest_sha
  preflight="$ARTIFACT_DIR/preflight-off.txt"
  [[ $MODE == nbd ]] && preflight="$ARTIFACT_DIR/preflight-ready.txt"
  kernel=$(uname -r)
  manifest_sha=$(sha256sum -- "$RELEASE/SHA256SUMS" | awk '{print $1}')
  input_bundle_manifest_sha=$(preflight_field "$preflight" NBD_INPUT_BUNDLE_MANIFEST_SHA256 || true)
  [[ $input_bundle_manifest_sha =~ ^[0-9a-f]{64}$ ]] || refuse INPUT_BUNDLE_MANIFEST_DIGEST_INVALID
  [[ $(preflight_field "$preflight" NBD_INSTALL_PROVENANCE || true) == PASS ]] || refuse INSTALL_PROVENANCE_INVALID
  zram_device=$(awk 'NR > 1 && $1 ~ /\/zram[0-9]+$/ { print $1; exit }' "$SWAPS_FILE")
  [[ $zram_device =~ ^/dev/zram[0-9]+$ ]] || refuse ZRAM_CONTEXT_MISSING
  zram_name=$(basename -- "$zram_device")
  zram_algorithm=$(sed -n 's/.*\[\([^]]*\)\].*/\1/p' "/sys/block/$zram_name/comp_algorithm")
  zram_size_kib=$(awk -v target="$zram_device" 'NR > 1 && $1 == target { print $3; exit }' "$SWAPS_FILE")
  zram_priority=$(awk -v target="$zram_device" 'NR > 1 && $1 == target { print $5; exit }' "$SWAPS_FILE")
  [[ $zram_algorithm =~ ^[A-Za-z0-9_-]+$ && $zram_size_kib =~ ^[0-9]+$ && $zram_priority =~ ^[0-9-]+$ ]] \
    || refuse ZRAM_CONTEXT_MISSING
  if [[ $MODE == nbd ]]; then
    lower_kind=nbd
    lower_identity=$NBD_SECOND_TIER_IDENTITY_SHA256
    binary_match=PASS
    nbd_device=$NBD_DEVICE
    nbd_block_major_minor=$NBD_BLOCK_MAJOR_MINOR
    nbd_size_kib=$NBD_SIZE_KIB
    nbd_usable_size_kib=$NBD_USABLE_SIZE_KIB
    nbd_capacity_sectors=$NBD_CAPACITY_SECTORS
    nbd_priority=$NBD_PRIORITY
    nbd_server_pid=$NBD_SERVER_PID
    nbd_daemon_manifest_sha=$NBD_DAEMON_MANIFEST_SHA256
  else
    lower_kind=scratch
    lower_identity=$SCRATCH_IDENTITY_SHA256
    binary_match=N/A
    nbd_device=""
    nbd_block_major_minor=""
    nbd_size_kib=""
    nbd_usable_size_kib=""
    nbd_capacity_sectors=""
    nbd_priority=""
    nbd_server_pid=""
    nbd_daemon_manifest_sha=""
  fi
  python3 - "$ARTIFACT_DIR/context.json" "$preflight" "$MODE" "$CONDITION" "$TIER_MIB" \
    "$ALLOCATE_MIB" "$MEMORY_HIGH_MIB" "$MEMORY_MAX_MIB" "$CHUNK_MIB" "$VERSION" "$kernel" "$manifest_sha" \
    "$SOURCE_COMMIT" "$SOURCE_BRANCH" "$SOURCE_TREE_STATE" "$PAIR_ID" "$UTC_STARTED" "$RELEASE" "$zram_name" \
    "$zram_algorithm" "$zram_size_kib" "$zram_priority" "$lower_kind" "$lower_identity" \
    "$LOWER_SINK_TYPE" "$LOWER_SINK_IDENTITY_SHA256" "$binary_match" "$input_bundle_manifest_sha" \
    "$nbd_device" "$nbd_block_major_minor" "$nbd_size_kib" "$nbd_usable_size_kib" \
    "$nbd_capacity_sectors" "$nbd_priority" "$nbd_server_pid" \
    "$nbd_daemon_manifest_sha" "$SAMPLE_TIMEOUT_SEC" "$INTEGRITY_FINALIZATION_TIMEOUT_SEC" \
    "$CELL_SETUP_CLEANUP_TIMEOUT_SEC" \
    "$CELL_OUTER_TIMEOUT_SEC" <<'PY'
import hashlib
import json
import os
import sys

(out, preflight_path, mode, condition, tier, allocated, memory_high, memory_max, chunk, version,
 kernel, manifest_sha, source_commit, source_branch, source_tree_state, pair_id, utc_started, release_root,
 zram_name, zram_algorithm, zram_size_kib, zram_priority, lower_kind, lower_identity,
 sink_type, sink_identity, binary_match, input_bundle_manifest_sha, nbd_device, nbd_block_major_minor,
 nbd_size_kib, nbd_usable_size_kib, nbd_capacity_sectors, nbd_priority, nbd_server_pid,
 nbd_daemon_manifest_sha, sample_timeout_sec, integrity_finalization_timeout_sec, setup_cleanup_timeout_sec, cell_outer_timeout_sec) = sys.argv[1:]
fields = {}
with open(preflight_path, encoding="utf-8") as source:
    for line in source:
        key, separator, value = line.rstrip("\n").partition("=")
        if separator and key.startswith("NBD_"):
            fields[key] = value
script_names = (
    "nbd-benchmark-cell.sh",
    "nbd-benchmark-cgroup-launch.sh",
    "nbd-benchmark-lib.sh",
    "cascade_pressure_integrity_worker.py",
    "nbd-product-preflight.sh",
    "cascade-up.sh",
    "cascade-down.sh",
)
script_sha256 = {}
for name in script_names:
    with open(os.path.join(release_root, "scripts", "safety", name), "rb") as source:
        script_sha256[name] = hashlib.sha256(source.read()).hexdigest()
zram_identity = hashlib.sha256(
    f"{zram_name}:{zram_size_kib}:{zram_priority}:{zram_algorithm}".encode("ascii")
).hexdigest()
argv = [
    "nbd-benchmark-cell.sh", "--run",
    "--mode", mode, "--condition", condition, "--tier-mib", int(tier),
    "--artifact-dir", "<campaign-artifact-dir>",
    "--sealed-release-root", release_root,
    "--release-version", version,
    "--expected-source-commit", source_commit,
    "--expected-manifest-sha256", manifest_sha,
    "--pair-id", pair_id,
    "--runs", 3, "--sample-timeout-sec", int(sample_timeout_sec),
]
record = {
    "schema": 2,
    "utc": {"started": utc_started},
    "pair_id": pair_id,
    "mode": mode,
    "condition": condition,
    "tier_mib": int(tier),
    "release": {
        "root": release_root,
        "version": version,
        "source_commit": source_commit,
        "source_branch": source_branch,
        "source_tree_state": source_tree_state,
        "manifest_sha256": manifest_sha,
        "input_bundle_manifest_sha256": input_bundle_manifest_sha,
    },
    "script_sha256": script_sha256,
    "kernel_release": kernel,
    "zram": {
        "device": zram_name,
        "algorithm": zram_algorithm,
        "size_kib": int(zram_size_kib),
        "priority": int(zram_priority),
        "identity_sha256": zram_identity,
    },
    "lower": {
        "type": lower_kind,
        "identity_sha256": lower_identity,
        "sink_type": sink_type,
        "sink_identity_sha256": sink_identity,
        "free_bytes": int(fields["NBD_LOWER_FREE_BYTES"]),
        "required_bytes": int(fields["NBD_LOWER_REQUIRED_BYTES"]),
    },
    "binary_match": binary_match,
    "watchdog": {"armed": True, "outcome": "not_fired"},
    "timeout_budget": {
        "sample_timeout_sec": int(sample_timeout_sec),
        "integrity_finalization_timeout_sec": int(integrity_finalization_timeout_sec),
        "samples": 3,
        "setup_cleanup_timeout_sec": int(setup_cleanup_timeout_sec),
        "cell_outer_timeout_sec": int(cell_outer_timeout_sec),
    },
    "argv": argv,
    "argv_redactions": ["artifact-dir"],
    "workload": {
        "name": "anonymous_memory_sequential_write",
        "pattern": "shake256-v1",
        "allocated_mib": int(allocated),
        "memory_high_mib": int(memory_high),
        "memory_max_mib": int(memory_max),
        "allocation_chunk_bytes": int(chunk) * 1024 * 1024,
        "worker_threads": 1,
    },
}
if mode == "nbd":
    record["nbd"] = {
        "device": nbd_device,
        "block_major_minor": nbd_block_major_minor,
        "size_kib": int(nbd_size_kib),
        "usable_size_kib": int(nbd_usable_size_kib),
        "capacity_sectors": int(nbd_capacity_sectors),
        "priority": int(nbd_priority),
        "server_pid": int(nbd_server_pid),
        "daemon_executable_relative_path": "bin/ramsharedd",
        "daemon_manifest_sha256": nbd_daemon_manifest_sha,
        "identity_sha256": lower_identity,
    }
temporary = out + ".tmp"
with open(temporary, "w", encoding="utf-8") as target:
    json.dump(record, target, sort_keys=True, separators=(",", ":"))
    target.write("\n")
os.replace(temporary, out)
PY
}

write_artifact_inventory() {
  local output=${1:-"$ARTIFACT_DIR/artifact-inventory.json"}
  local temporary="${output}.tmp"
  [[ $output == "$ARTIFACT_DIR"/* && ${output#"$ARTIFACT_DIR"/} != */* &&
    ! -e $output && ! -L $output && ! -e $temporary && ! -L $temporary ]] || return 1
  python3 - "$ARTIFACT_DIR" "$output" <<'PY'
import hashlib
import json
import os
import stat
import sys

root = os.path.realpath(sys.argv[1])
out = os.path.abspath(sys.argv[2])
temporary = out + ".tmp"
try:
    if os.path.dirname(out) != root:
        raise ValueError("inventory_path")
    excluded = {"artifact-inventory.json", "evidence-envelope.json", os.path.basename(out)}
    rows = []
    for name in sorted(os.listdir(root)):
        if name in excluded:
            continue
        path = os.path.join(root, name)
        metadata = os.lstat(path)
        if not stat.S_ISREG(metadata.st_mode):
            raise ValueError("artifact_inventory_non_regular")
        with open(path, "rb") as source:
            rows.append({"name": name, "bytes": metadata.st_size, "sha256": hashlib.sha256(source.read()).hexdigest()})
    fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as target:
            json.dump({"schema": 2, "files": rows}, target, sort_keys=True, separators=(",", ":"))
            target.write("\n")
            target.flush()
            os.fsync(target.fileno())
    except BaseException:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass
        raise
except (OSError, ValueError) as exc:
    raise SystemExit(f"artifact_inventory_write_failed:{exc}")
PY
  if ! publish_owned_file "$temporary" "$output"; then
    rm -f -- "$temporary"
    return 1
  fi
  validate_artifact_inventory "$output"
}

write_evidence_envelope() {
  local inventory_path=${1:-"$ARTIFACT_DIR/artifact-inventory.json"}
  local output=${2:-"$ARTIFACT_DIR/evidence-envelope.json"}
  local temporary="${output}.tmp"
  [[ $inventory_path == "$ARTIFACT_DIR"/* && ${inventory_path#"$ARTIFACT_DIR"/} != */* &&
    $output == "$ARTIFACT_DIR"/* && ${output#"$ARTIFACT_DIR"/} != */* &&
    -f $inventory_path && ! -L $inventory_path && ! -e $output && ! -L $output &&
    ! -e $temporary && ! -L $temporary ]] || return 1
  python3 - "$ARTIFACT_DIR" "$MODE" "$PAIR_ID" "$inventory_path" "$output" <<'PY'
import hashlib
import json
import os
import sys

root, mode, pair_id, inventory_path, out = sys.argv[1:]
root = os.path.realpath(root)
inventory_path = os.path.abspath(inventory_path)
out = os.path.abspath(out)
temporary = out + ".tmp"
allow_fault = os.environ.get("RAMSHARED_NBD_ALLOW_MANUFACTURED_PRODUCT_OFF_TEST") == "1"
fault = os.environ.get("RAMSHARED_NBD_TEST_EVIDENCE_ENVELOPE_FAULT", "") if allow_fault else ""

def digest_path(path):
    with open(path, "rb") as source:
        return hashlib.sha256(source.read()).hexdigest()

try:
    if os.path.dirname(inventory_path) != root or os.path.dirname(out) != root:
        raise ValueError("envelope_path")
    if fault == "write":
        raise OSError("injected envelope write failure")
    with open(os.path.join(root, "context.json"), encoding="utf-8") as source:
        context = json.load(source)
    with open(inventory_path, encoding="utf-8") as source:
        inventory = json.load(source)
    artifacts = [{"path": row["name"], "bytes": row["bytes"], "sha256": row["sha256"]} for row in inventory["files"]]
    record = {
        "schema_version": "ramshared-nbd-cell-evidence/v1",
        "pair_id": pair_id,
        "mode": mode,
        "release": {
            "version": context["release"]["version"],
            "source_commit": context["release"]["source_commit"],
            "manifest_sha256": context["release"]["manifest_sha256"],
            "input_bundle_manifest_sha256": context["release"]["input_bundle_manifest_sha256"],
        },
        "context_sha256": digest_path(os.path.join(root, "context.json")),
        "summary_sha256": digest_path(os.path.join(root, "summary.json")),
        "artifact_inventory_sha256": digest_path(inventory_path),
        "artifacts": artifacts,
        "binary_match": context["binary_match"],
        "watchdog": context["watchdog"],
        "timeout_budget": context["timeout_budget"],
        "classification": "INCOMPARABLE",
    }
    fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as target:
            json.dump(record, target, sort_keys=True, separators=(",", ":"))
            target.write("\n")
            target.flush()
            if fault == "fsync":
                raise OSError("injected envelope fsync failure")
            os.fsync(target.fileno())
        if fault == "link":
            raise OSError("injected envelope link failure")
    except BaseException:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass
        raise
except (OSError, ValueError, KeyError, TypeError, json.JSONDecodeError) as exc:
    raise SystemExit(f"evidence_envelope_write_failed:{exc}")
PY
  if ! publish_owned_file "$temporary" "$output"; then
    rm -f -- "$temporary"
    return 1
  fi
  if [[ ${RAMSHARED_NBD_ALLOW_MANUFACTURED_PRODUCT_OFF_TEST:-} == 1 &&
    ${RAMSHARED_NBD_TEST_EVIDENCE_ENVELOPE_FAULT:-} == validation ]]; then
    return 1
  fi
  validate_evidence_envelope "$inventory_path" "$output"
}

cleanup_disk_scratch() {
  [[ -n $SCRATCH_SWAP ]] || return 0
  if (( SCRATCH_SWAP_ACTIVE == 1 )) && [[ $(nbd_swap_exact_count "$SWAPS_FILE" "$SCRATCH_SWAP") != 1 ]]; then
    return 1
  fi
  timeout --foreground --kill-after=5s 30s bash -c 'source "$1"; nbd_cleanup_scratch "$2" "$3" "$4" /sbin/swapoff' _ \
    "$BENCHMARK_LIB" "$SCRATCH_SWAP" "$SCRATCH_IDENTITY" "$SWAPS_FILE" || return 1
  SCRATCH_SWAP_ACTIVE=0
  SCRATCH_SWAP=""
  SCRATCH_IDENTITY=""
}

publish_owned_file() {
  local candidate=$1 destination=$2
  [[ -f $candidate && ! -L $candidate && ! -e $destination && ! -L $destination ]] || return 1
  sync -f -- "$candidate" 2>/dev/null || return 1
  ln -- "$candidate" "$destination" 2>/dev/null || return 1
  rm -f -- "$candidate"
}

inventory_matches_ownership() {
  local path=$1 identity digest
  [[ -f $path && ! -L $path ]] || return 1
  identity=$(stat -c '%d:%i:%s' -- "$path" 2>/dev/null) || return 1
  digest=$(sha256sum -- "$path" 2>/dev/null | awk '{print $1}')
  [[ $identity == "${CUSTODY_INVENTORY_IDENTITY:-}" &&
    $digest == "${CUSTODY_INVENTORY_SHA256:-}" ]]
}

rollback_published_inventory() {
  local destination="$ARTIFACT_DIR/artifact-inventory.json"
  (( ${CUSTODY_INVENTORY_PUBLISHED:-0} == 1 )) || return 0
  if [[ ! -e $destination && ! -L $destination ]]; then
    CUSTODY_INVENTORY_PUBLISHED=0
    return 0
  fi
  inventory_matches_ownership "$destination" || return 1
  rm -f -- "$destination" || return 1
  CUSTODY_INVENTORY_PUBLISHED=0
}

discard_custody_candidates() {
  local candidate
  for candidate in "${CUSTODY_INVENTORY_CANDIDATE:-}" "${CUSTODY_ENVELOPE_CANDIDATE:-}"; do
    [[ -n $candidate && $candidate == "$ARTIFACT_DIR"/.ramshared-custody-* ]] || continue
    rm -f -- "$candidate" "${candidate}.tmp"
  done
  CUSTODY_INVENTORY_CANDIDATE=""
  CUSTODY_ENVELOPE_CANDIDATE=""
  CUSTODY_CANDIDATES_READY=0
}

prepare_custody_candidates() {
  local inventory_destination="$ARTIFACT_DIR/artifact-inventory.json"
  local envelope_destination="$ARTIFACT_DIR/evidence-envelope.json"
  CUSTODY_INVENTORY_CANDIDATE="$ARTIFACT_DIR/.ramshared-custody-inventory-$$.json"
  CUSTODY_ENVELOPE_CANDIDATE="$ARTIFACT_DIR/.ramshared-custody-envelope-$$.json"
  CUSTODY_CANDIDATES_READY=0
  [[ ! -e $inventory_destination && ! -L $inventory_destination &&
    ! -e $envelope_destination && ! -L $envelope_destination &&
    ! -e $CUSTODY_INVENTORY_CANDIDATE && ! -L $CUSTODY_INVENTORY_CANDIDATE &&
    ! -e ${CUSTODY_INVENTORY_CANDIDATE}.tmp && ! -L ${CUSTODY_INVENTORY_CANDIDATE}.tmp &&
    ! -e $CUSTODY_ENVELOPE_CANDIDATE && ! -L $CUSTODY_ENVELOPE_CANDIDATE &&
    ! -e ${CUSTODY_ENVELOPE_CANDIDATE}.tmp && ! -L ${CUSTODY_ENVELOPE_CANDIDATE}.tmp ]] || return 1
  write_artifact_inventory "$CUSTODY_INVENTORY_CANDIDATE" || return 1
  write_evidence_envelope "$CUSTODY_INVENTORY_CANDIDATE" "$CUSTODY_ENVELOPE_CANDIDATE" || return 1
  CUSTODY_CANDIDATES_READY=1
}

publish_custody_candidates() {
  local inventory_destination="$ARTIFACT_DIR/artifact-inventory.json"
  local envelope_destination="$ARTIFACT_DIR/evidence-envelope.json"
  local envelope_rc=0
  [[ $CUSTODY_CANDIDATES_READY == 1 &&
    $CUSTODY_INVENTORY_CANDIDATE == "$ARTIFACT_DIR"/.ramshared-custody-inventory-*.json &&
    $CUSTODY_ENVELOPE_CANDIDATE == "$ARTIFACT_DIR"/.ramshared-custody-envelope-*.json ]] || return 1
  CUSTODY_INVENTORY_IDENTITY=$(stat -c '%d:%i:%s' -- "$CUSTODY_INVENTORY_CANDIDATE") || return 1
  CUSTODY_INVENTORY_SHA256=$(sha256sum -- "$CUSTODY_INVENTORY_CANDIDATE" | awk '{print $1}') || return 1
  if [[ ${RAMSHARED_NBD_ALLOW_MANUFACTURED_PRODUCT_OFF_TEST:-} == 1 &&
    ${RAMSHARED_NBD_TEST_CUSTODY_INVENTORY_RACE:-} == publish-preexisting ]]; then
    ( set -o noclobber; printf '%s\n' 'foreign inventory must survive' >"$inventory_destination" ) 2>/dev/null || true
  fi
  if ! publish_owned_file "$CUSTODY_INVENTORY_CANDIDATE" "$inventory_destination"; then
    if inventory_matches_ownership "$inventory_destination"; then
      CUSTODY_INVENTORY_PUBLISHED=1
      rollback_published_inventory || true
    fi
    return 1
  fi
  CUSTODY_INVENTORY_PUBLISHED=1
  if [[ ${RAMSHARED_NBD_ALLOW_MANUFACTURED_PRODUCT_OFF_TEST:-} == 1 &&
    ${RAMSHARED_NBD_TEST_CUSTODY_ENVELOPE_RACE:-} == publish-preexisting ]]; then
    ( set -o noclobber; printf '%s\n' 'foreign envelope must survive' >"$envelope_destination" ) 2>/dev/null || true
  fi
  if [[ ${RAMSHARED_NBD_ALLOW_MANUFACTURED_PRODUCT_OFF_TEST:-} == 1 &&
    ${RAMSHARED_NBD_TEST_CUSTODY_ENVELOPE_FAULT:-} == fail ]]; then
    envelope_rc=1
  else
    publish_owned_file "$CUSTODY_ENVELOPE_CANDIDATE" "$envelope_destination" || envelope_rc=$?
  fi
  if (( envelope_rc != 0 )); then
    rollback_published_inventory || return 1
    return 1
  fi
  # The envelope link is the sole commit marker; cleanup is no longer allowed
  # to append a receipt or remove either committed artifact.
  CUSTODY_FRONTIER=1
  CUSTODY_INVENTORY_CANDIDATE=""
  CUSTODY_ENVELOPE_CANDIDATE=""
  CUSTODY_CANDIDATES_READY=0
}

publish_preflight_output() {
  local phase=$1 number=$2 content=$3 candidate destination
  candidate="$ARTIFACT_DIR/.${phase}-preflight-${number}.$$.tmp"
  destination="$ARTIFACT_DIR/${phase}-preflight-${number}.txt"
  [[ ! -e $candidate && ! -L $candidate && ! -e $destination && ! -L $destination ]] || return 1
  ( set -o noclobber; printf '%s\n' "$content" >"$candidate" ) || return 1
  publish_owned_file "$candidate" "$destination" || return 1
  if [[ $phase == initial && $number == 1 ]]; then
    [[ ! -e $ARTIFACT_DIR/preflight-off.txt && ! -L $ARTIFACT_DIR/preflight-off.txt ]] || return 1
    ln -- "$destination" "$ARTIFACT_DIR/preflight-off.txt" 2>/dev/null || return 1
  fi
}

exact_product_off() {
  local output line key value expected_vram_bytes expected_margin expected_required_bytes
  local lower_free_bytes lower_required_bytes
  local -A seen=()
  local -a required_keys=(
    NBD_RELEASE_VERSION
    NBD_RELEASE_SOURCE_COMMIT
    NBD_RELEASE_MANIFEST_SHA256
    NBD_INPUT_BUNDLE_MANIFEST_SHA256
    NBD_INSTALL_PROVENANCE
    NBD_RELEASE_GATE
    NBD_SELECTOR
    NBD_LOWER_FREE_BYTES
    NBD_LOWER_REQUIRED_BYTES
    NBD_LOWER_TIER_TYPE
    NBD_LOWER_TIER_IDENTITY_SHA256
    NBD_LOWER_TIER_BINDING
    NBD_LOWER_TIER_CAPACITY
    NBD_RELAY_GATE
    NBD_BINARY_MATCH
    NBD_UBLK_MODULE
    NBD_TRANSPORT
    NBD_PRODUCT_STATE
    NBD_READINESS_REASON
  )
  output=$(cat)
  while IFS= read -r line; do
    [[ $line =~ ^(NBD_[A-Z0-9_]+)=(.*)$ ]] || return 1
    key=${BASH_REMATCH[1]}
    value=${BASH_REMATCH[2]}
    case $key in
      NBD_RELEASE_VERSION|NBD_RELEASE_SOURCE_COMMIT|NBD_RELEASE_MANIFEST_SHA256|\
      NBD_INPUT_BUNDLE_MANIFEST_SHA256|NBD_INSTALL_PROVENANCE|NBD_RELEASE_GATE|\
      NBD_SELECTOR|NBD_LOWER_FREE_BYTES|NBD_LOWER_REQUIRED_BYTES|NBD_LOWER_TIER_TYPE|\
      NBD_LOWER_TIER_IDENTITY_SHA256|NBD_LOWER_TIER_BINDING|NBD_LOWER_TIER_CAPACITY|\
      NBD_RELAY_GATE|NBD_BINARY_MATCH|NBD_UBLK_MODULE|NBD_TRANSPORT|\
      NBD_PRODUCT_STATE|NBD_READINESS_REASON) ;;
      *) return 1 ;;
    esac
    [[ -z ${seen[$key]+present} ]] || return 1
    seen[$key]=$value
  done <<<"$output"
  for key in "${required_keys[@]}"; do
    [[ -n ${seen[$key]+present} ]] || return 1
  done
  [[ ${#seen[@]} == ${#required_keys[@]} ]] || return 1
  [[ $TIER_MIB =~ ^(1024|2048|4096)$ ]] || return 1
  [[ $EXPECTED_INPUT_BUNDLE_MANIFEST_SHA256 =~ ^[0-9a-f]{64}$ &&
    $LOWER_SINK_IDENTITY_SHA256 =~ ^[0-9a-f]{64}$ ]] || return 1
  [[ ${seen[NBD_RELEASE_VERSION]} == "$VERSION" &&
    ${seen[NBD_RELEASE_SOURCE_COMMIT]} == "$EXPECTED_SOURCE_COMMIT" &&
    ${seen[NBD_RELEASE_MANIFEST_SHA256]} == "$EXPECTED_MANIFEST_SHA256" &&
    ${seen[NBD_INPUT_BUNDLE_MANIFEST_SHA256]} == "$EXPECTED_INPUT_BUNDLE_MANIFEST_SHA256" &&
    ${seen[NBD_INSTALL_PROVENANCE]} == PASS &&
    ${seen[NBD_RELEASE_GATE]} == PASS &&
    ${seen[NBD_SELECTOR]} == PASS &&
    ${seen[NBD_LOWER_TIER_TYPE]} == directory &&
    ${seen[NBD_LOWER_TIER_IDENTITY_SHA256]} == "$LOWER_SINK_IDENTITY_SHA256" &&
    ${seen[NBD_LOWER_TIER_BINDING]} == bound &&
    ${seen[NBD_LOWER_TIER_CAPACITY]} == PASS &&
    ${seen[NBD_RELAY_GATE]} == PASS &&
    ${seen[NBD_BINARY_MATCH]} == NOT_APPLICABLE &&
    ${seen[NBD_TRANSPORT]} == none &&
    ${seen[NBD_PRODUCT_STATE]} == PRODUCT_OFF &&
    ${seen[NBD_READINESS_REASON]} == product_off ]] || return 1
  case ${seen[NBD_UBLK_MODULE]} in
    ABSENT|LOADED_INERT) ;;
    *) return 1 ;;
  esac
  [[ ${seen[NBD_LOWER_FREE_BYTES]} =~ ^[0-9]{1,16}$ &&
    ${seen[NBD_LOWER_REQUIRED_BYTES]} =~ ^[0-9]{1,16}$ ]] || return 1
  expected_vram_bytes=$((10#$TIER_MIB * 1024 * 1024))
  expected_margin=$(((expected_vram_bytes + 9) / 10))
  (( expected_margin >= 512 * 1024 * 1024 )) || expected_margin=$((512 * 1024 * 1024))
  expected_required_bytes=$((expected_vram_bytes + expected_margin))
  lower_free_bytes=$((10#${seen[NBD_LOWER_FREE_BYTES]}))
  lower_required_bytes=$((10#${seen[NBD_LOWER_REQUIRED_BYTES]}))
  (( lower_required_bytes == expected_required_bytes && lower_free_bytes >= lower_required_bytes ))
}

product_off_epoch() {
  local phase=$1 candidate_out candidate_err destination_out destination_err first second rc=0 down_rc=0 state
  [[ $phase == initial || $phase == final || $phase == failure ]] || return 1
  if [[ $phase == initial ]]; then state=$INITIAL_EPOCH_STATE; else state=$FINAL_EPOCH_STATE; fi
  [[ $state == completed ]] && return 0
  [[ $state == unattempted ]] || return 1
  if [[ $phase == initial ]]; then INITIAL_EPOCH_STATE=attempted; else FINAL_EPOCH_STATE=attempted; fi
  candidate_out="$ARTIFACT_DIR/.${phase}-down.out.$$.tmp"
  candidate_err="$ARTIFACT_DIR/.${phase}-down.err.$$.tmp"
  destination_out="$ARTIFACT_DIR/${phase}-down.out"
  destination_err="$ARTIFACT_DIR/${phase}-down.err"
  if [[ $PRODUCT_OFF_PHASES != *" $phase "* ]]; then
    [[ ! -e $candidate_out && ! -L $candidate_out && ! -e $candidate_err && ! -L $candidate_err ]] || return 1
    ( set -o noclobber; : >"$candidate_out"; : >"$candidate_err" ) || return 1
    timeout --foreground --kill-after=5s 30s "$CLI" down >"$candidate_out" 2>"$candidate_err" || down_rc=$?
    publish_owned_file "$candidate_out" "$destination_out" || return 1
    publish_owned_file "$candidate_err" "$destination_err" || return 1
    PRODUCT_OFF_PHASES+=" $phase "
    if (( down_rc != 0 )); then
      if [[ $phase == initial ]]; then INITIAL_EPOCH_STATE=failed; else FINAL_EPOCH_STATE=failed; fi
      return 1
    fi
  fi
  first=$(timeout --foreground --kill-after=5s "${PREFLIGHT_TIMEOUT_SEC}s" env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin "RAMSHARED_NBD_VRAM_MIB=$TIER_MIB" "$PREFLIGHT" --check --sealed-release-root "$RELEASE" \
    --release-version "$VERSION" --expected-source-commit "$EXPECTED_SOURCE_COMMIT" \
    --expected-manifest-sha256 "$EXPECTED_MANIFEST_SHA256" 2>&1) || rc=1
  second=$(timeout --foreground --kill-after=5s "${PREFLIGHT_TIMEOUT_SEC}s" env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin "RAMSHARED_NBD_VRAM_MIB=$TIER_MIB" "$PREFLIGHT" --check --sealed-release-root "$RELEASE" \
    --release-version "$VERSION" --expected-source-commit "$EXPECTED_SOURCE_COMMIT" \
    --expected-manifest-sha256 "$EXPECTED_MANIFEST_SHA256" 2>&1) || rc=1
  exact_product_off <<<"$first" && exact_product_off <<<"$second" || rc=1
  publish_preflight_output "$phase" 1 "$first" || return 1
  publish_preflight_output "$phase" 2 "$second" || return 1
  local receipt_rc=0
  python3 - "$ARTIFACT_DIR/.${phase}-product-off.$$.tmp" "$ARTIFACT_DIR/${phase}-product-off.json" "$phase" "$rc" <<'PY' || receipt_rc=$?
import json, os, sys
tmp, out, phase, rc = sys.argv[1:]
fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
try:
    fault = os.environ.get("RAMSHARED_NBD_TEST_PRODUCT_OFF_RECEIPT_FAULT", "") if os.environ.get("RAMSHARED_NBD_ALLOW_MANUFACTURED_PRODUCT_OFF_TEST") == "1" else ""
    if fault == "dump": raise OSError("injected dump failure")
    with os.fdopen(fd, "w", encoding="utf-8") as target:
        json.dump({"schema":"ramshared-nbd-product-off/v1","phase":phase,"preflight_count":2,"preflight_budget_sec":30,"status":"PASS" if rc == "0" else "FAIL"}, target, sort_keys=True, separators=(",",":"))
        target.write("\n"); target.flush()
        if fault == "fsync": raise OSError("injected fsync failure")
        os.fsync(target.fileno())
    if fault == "link": raise OSError("injected link failure")
    if os.environ.get("RAMSHARED_NBD_TEST_PRODUCT_OFF_RECEIPT_RACE") == "publish-preexisting" and os.environ.get("RAMSHARED_NBD_ALLOW_MANUFACTURED_PRODUCT_OFF_TEST") == "1":
        race_fd = os.open(out, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        try:
            os.write(race_fd, b"concurrent receipt bytes must survive exactly")
            os.fsync(race_fd)
        finally:
            os.close(race_fd)
    os.link(tmp, out)
finally:
    try: os.unlink(tmp)
    except FileNotFoundError: pass
PY
  if (( receipt_rc != 0 )); then
    if [[ $phase == initial ]]; then INITIAL_EPOCH_STATE=failed; else FINAL_EPOCH_STATE=failed; fi
    return 1
  fi
  if (( rc == 0 )); then
    if [[ $phase == initial ]]; then INITIAL_EPOCH_STATE=completed; else FINAL_EPOCH_STATE=completed; fi
    return 0
  fi
  if [[ $phase == initial ]]; then INITIAL_EPOCH_STATE=failed; else FINAL_EPOCH_STATE=failed; fi
  return 1
}

worker_is_live() {
  local pid=$1 state
  if [[ ${RAMSHARED_NBD_ALLOW_MANUFACTURED_PRODUCT_OFF_TEST:-} == 1 &&
    ${RAMSHARED_NBD_TEST_WORKER_LIVENESS:-} == always-live ]]; then
    return 0
  fi
  kill -0 "$pid" 2>/dev/null || return 1
  if [[ -r /proc/$pid/stat ]]; then
    state=$(awk '{print $3}' "/proc/$pid/stat" 2>/dev/null || true)
    [[ $state != Z ]] || return 1
  fi
  return 0
}

stop_worker_bounded() {
  local pid=$1 term_deadline kill_deadline forced_stop=0
  [[ $pid =~ ^[1-9][0-9]*$ ]] || return 1
  if worker_is_live "$pid"; then
    kill -TERM "$pid" 2>/dev/null || true
    term_deadline=$((SECONDS + WORKER_TERM_GRACE_SEC))
    while worker_is_live "$pid" && (( SECONDS < term_deadline )); do sleep 0.1; done
  fi
  if worker_is_live "$pid"; then
    forced_stop=1
    kill -KILL "$pid" 2>/dev/null || true
    kill_deadline=$((SECONDS + WORKER_KILL_GRACE_SEC))
    while worker_is_live "$pid" && (( SECONDS < kill_deadline )); do sleep 0.1; done
  fi
  worker_is_live "$pid" && return 1
  # A child which is absent or zombie cannot make this reap wait block.
  # Waiting still preserves its exit status and prevents zombie accumulation.
  if ! wait "$pid" 2>/dev/null; then
    return 1
  fi
  (( forced_stop == 0 ))
}

capture_integrity_finalization_deadline() {
  local timeout_sec=$1
  [[ $timeout_sec =~ ^[1-9][0-9]{0,2}$ ]] || return 1
  INTEGRITY_FINALIZATION_STARTED_SEC=$SECONDS
  INTEGRITY_FINALIZATION_DEADLINE_SEC=$((INTEGRITY_FINALIZATION_STARTED_SEC + timeout_sec))
}

stop_worker_for_integrity_deadline() {
  local pid=$1 integrity_finalization_timeout_sec=$2 integrity_finalization_deadline kill_deadline wait_rc=0 term_issued=0
  INTEGRITY_STOP_REASON=""
  INTEGRITY_WORKER_REAPED=0
  INTEGRITY_WORKER_EXIT_CODE=""
  unset INTEGRITY_FINALIZATION_STARTED_SEC INTEGRITY_FINALIZATION_DEADLINE_SEC
  [[ $pid =~ ^[1-9][0-9]*$ && $integrity_finalization_timeout_sec =~ ^[1-9][0-9]{0,2}$ ]] || {
    INTEGRITY_STOP_REASON=SAMPLE_INTEGRITY_PROCESS_FAILED
    return 1
  }
  if worker_is_live "$pid"; then
    kill -TERM "$pid" 2>/dev/null || true
    term_issued=1
    capture_integrity_finalization_deadline "$integrity_finalization_timeout_sec" || {
      INTEGRITY_STOP_REASON=SAMPLE_INTEGRITY_PROCESS_FAILED
      return 1
    }
    integrity_finalization_deadline=$INTEGRITY_FINALIZATION_DEADLINE_SEC
    while worker_is_live "$pid" && (( SECONDS < integrity_finalization_deadline )); do sleep 0.1; done
  fi
  if worker_is_live "$pid"; then
    INTEGRITY_STOP_REASON=SAMPLE_INTEGRITY_DEADLINE_EXCEEDED
    kill -KILL "$pid" 2>/dev/null || true
    kill_deadline=$((SECONDS + WORKER_KILL_GRACE_SEC))
    while worker_is_live "$pid" && (( SECONDS < kill_deadline )); do sleep 0.1; done
    if worker_is_live "$pid"; then
      INTEGRITY_STOP_REASON=SAMPLE_INTEGRITY_KILL_TIMEOUT
      return 1
    fi
    if wait "$pid" 2>/dev/null; then wait_rc=0; else wait_rc=$?; fi
    INTEGRITY_WORKER_REAPED=1
    INTEGRITY_WORKER_EXIT_CODE=$wait_rc
    return 1
  fi
  if wait "$pid" 2>/dev/null; then
    INTEGRITY_WORKER_REAPED=1
    INTEGRITY_WORKER_EXIT_CODE=0
    if (( term_issued == 0 )); then
      INTEGRITY_STOP_REASON=SAMPLE_INTEGRITY_PROCESS_FAILED
      return 1
    fi
    INTEGRITY_STOP_REASON=GRACEFUL_EXIT
    return 0
  else
    wait_rc=$?
  fi
  INTEGRITY_WORKER_REAPED=1
  INTEGRITY_WORKER_EXIT_CODE=$wait_rc
  INTEGRITY_STOP_REASON=SAMPLE_INTEGRITY_PROCESS_FAILED
  return 1
}

read_cgroup_oom_kill_count() {
  local events=$1
  [[ -f $events && ! -L $events ]] || return 1
  awk '
    NF != 2 || $1 !~ /^[a-z_]+$/ || $2 !~ /^[0-9]+$/ { invalid = 1 }
    $1 == "oom_kill" { value = $2; count += 1 }
    END {
      if (!invalid && count == 1) { print value; exit 0 }
      exit 1
    }
  ' "$events"
}

append_integrity_process_receipt() {
  local path=$1 deadline_remaining_sec=$2 oom_kill_before=$3 oom_kill_after=$4
  [[ -f $path && ! -L $path && $deadline_remaining_sec =~ ^[0-9]+$ &&
    $oom_kill_before =~ ^[0-9]+$ && $oom_kill_after =~ ^[0-9]+$ &&
    ${INTEGRITY_WORKER_REAPED:-0} == 1 && ${INTEGRITY_WORKER_EXIT_CODE:-} =~ ^[0-9]+$ ]] || return 1
  case ${INTEGRITY_STOP_REASON:-} in
    GRACEFUL_EXIT|SAMPLE_INTEGRITY_DEADLINE_EXCEEDED|SAMPLE_INTEGRITY_PROCESS_FAILED) ;;
    *) return 1 ;;
  esac
  {
    printf 'INTEGRITY_STOP_REASON=%s\n' "$INTEGRITY_STOP_REASON"
    printf 'INTEGRITY_WORKER_REAPED=PASS\n'
    printf 'INTEGRITY_WORKER_EXIT_CODE=%s\n' "$INTEGRITY_WORKER_EXIT_CODE"
    printf 'INTEGRITY_DEADLINE_REMAINING_SEC=%s\n' "$deadline_remaining_sec"
    printf 'CGROUP_OOM_KILL_BEFORE=%s\n' "$oom_kill_before"
    printf 'CGROUP_OOM_KILL_AFTER=%s\n' "$oom_kill_after"
  } >>"$path"
}

finalize_integrity_worker_after_hold() {
  local process_receipt_path=$1 oom_kill_before=$2 oom_kill_after integrity_deadline_remaining_sec=0
  if ! stop_worker_for_integrity_deadline "$WORKER_PID" "$INTEGRITY_FINALIZATION_TIMEOUT_SEC"; then
    if [[ ${INTEGRITY_WORKER_REAPED:-0} == 1 ]]; then
      WORKER_PID=""
      oom_kill_after=$(read_cgroup_oom_kill_count "$CG/memory.events") || refuse CGROUP_OOM_RECEIPT_INVALID
      # An already-exited worker never received TERM, so it has no finalization
      # deadline. Preserve its exit code and record zero remaining policy time.
      if [[ -v INTEGRITY_FINALIZATION_DEADLINE_SEC ]]; then
        integrity_deadline_remaining_sec=$((INTEGRITY_FINALIZATION_DEADLINE_SEC - SECONDS))
        (( integrity_deadline_remaining_sec >= 0 )) || integrity_deadline_remaining_sec=0
      fi
      append_integrity_process_receipt "$process_receipt_path" \
        "$integrity_deadline_remaining_sec" "$oom_kill_before" "$oom_kill_after" \
        || refuse INTEGRITY_PROCESS_RECEIPT_INVALID
    fi
    refuse "${INTEGRITY_STOP_REASON:-SAMPLE_INTEGRITY_PROCESS_FAILED}"
    return 1
  fi
  WORKER_PID=""
  oom_kill_after=$(read_cgroup_oom_kill_count "$CG/memory.events") || refuse CGROUP_OOM_RECEIPT_INVALID
  [[ -v INTEGRITY_FINALIZATION_DEADLINE_SEC ]] || {
    refuse SAMPLE_INTEGRITY_PROCESS_FAILED
    return 1
  }
  integrity_deadline_remaining_sec=$((INTEGRITY_FINALIZATION_DEADLINE_SEC - SECONDS))
  (( integrity_deadline_remaining_sec >= 0 )) || integrity_deadline_remaining_sec=0
  append_integrity_process_receipt "$process_receipt_path" \
    "$integrity_deadline_remaining_sec" "$oom_kill_before" "$oom_kill_after" \
    || refuse INTEGRITY_PROCESS_RECEIPT_INVALID
}

cleanup_cgroup() {
  [[ -d $CG ]] || return 0
  rmdir "$CG" 2>/dev/null
}

write_terminal_after() {
  {
    printf 'NBD_PRODUCT_STATE=PRODUCT_OFF\n'
    printf 'NBD_TRANSPORT=none\n'
    printf 'NBD_BINARY_MATCH=%s\n' "$ACTION_BINARY_MATCH"
  } >"$ARTIFACT_DIR/after.txt"
}

cleanup() {
  local rc=$?
  # Preserve the first exit status while a second signal cannot interrupt the
  # only cleanup epoch between worker reaping and terminal PRODUCT_OFF proof.
  trap '' INT TERM
  trap - EXIT
  if [[ ${CUSTODY_FRONTIER:-0} == 0 ]]; then
    rollback_published_inventory || { CLEANUP_OK=0; (( rc == 0 )) && rc=1; }
    discard_custody_candidates || { CLEANUP_OK=0; (( rc == 0 )) && rc=1; }
  fi
  if [[ -n $WORKER_PID ]]; then
    if ! stop_worker_bounded "$WORKER_PID"; then
      CLEANUP_OK=0
      (( rc == 0 )) && rc=1
    fi
    WORKER_PID=""
  fi
  cleanup_cgroup || { CLEANUP_OK=0; (( rc == 0 )) && rc=1; }
  if ! cleanup_disk_scratch; then
    CLEANUP_OK=0
    printf 'NBD_BENCHMARK_STATE=RED\nNBD_BENCHMARK_REASON=scratch_cleanup_failed\n' >&2
    (( rc == 0 )) && rc=1
  fi
  if ! product_off_epoch final; then
    CLEANUP_OK=0
    printf 'NBD_BENCHMARK_STATE=RED\nNBD_BENCHMARK_REASON=terminal_product_off_failed\n' >&2
    (( rc == 0 )) && rc=1
  fi
  if nbd_failure_receipt_allowed "$rc" "$CLEANUP_OK" PRODUCT_OFF 1 0; then
    nbd_write_failure_receipt "$ARTIFACT_DIR/failure-receipt.json" PRODUCT_OFF \
      "$FAILURE_REASON" "$VERSION" "$PAIR_ID" "$MODE" "$CONDITION" "$TIER_MIB" || {
      CLEANUP_OK=0
      printf 'NBD_BENCHMARK_STATE=RED\nNBD_BENCHMARK_REASON=failure_receipt_write_failed\n' >&2
      (( rc == 0 )) && rc=1
    }
  fi
  exit "$rc"
}

finish_success() {
  cleanup_disk_scratch || refuse FINAL_SCRATCH_OFF_FAILED
  product_off_epoch final || refuse FINAL_PRODUCT_OFF_FAILED
  write_terminal_after
  cleanup_cgroup || refuse FINAL_CGROUP_CLEANUP_FAILED
  aggregate_samples
  prepare_custody_candidates || refuse EVIDENCE_CUSTODY_PREPARE_FAILED
  # Both candidates are validated but unpublished. The envelope publication
  # sets the frontier; handled publication failures stay under cleanup.
  publish_custody_candidates || refuse EVIDENCE_CUSTODY_PUBLISH_FAILED
  trap - EXIT INT TERM
  printf 'NBD_BENCHMARK_MATRIX=PASS\n'
}

on_interrupt() {
  exit 130
}
on_term() {
  exit 143
}
trap cleanup EXIT
trap on_interrupt INT
trap on_term TERM

load_bound_lower_sink
product_off_epoch initial || refuse INITIAL_PRODUCT_OFF_FAILED
cp -- "$ARTIFACT_DIR/preflight-off.txt" "$ARTIFACT_DIR/before.txt"
python3 - "$ARTIFACT_DIR/action.txt" "$MODE" "$CONDITION" "$TIER_MIB" "$VERSION" "$PAIR_ID" \
  "$SAMPLE_TIMEOUT_SEC" "$INTEGRITY_FINALIZATION_TIMEOUT_SEC" "$CELL_SETUP_CLEANUP_TIMEOUT_SEC" \
  "$CELL_OUTER_TIMEOUT_SEC" <<'PY'
import json
import os
import sys

out, mode, condition, tier, version, pair_id, sample_timeout_sec, integrity_finalization_timeout_sec, setup_cleanup_timeout_sec, cell_outer_timeout_sec = sys.argv[1:]
record = {
    "schema": 1,
    "action": "sealed_nbd_benchmark_cell",
    "mode": mode,
    "condition": condition,
    "tier_mib": int(tier),
    "release_version": version,
    "pair_id": pair_id,
    "timeout_budget": {
        "sample_timeout_sec": int(sample_timeout_sec),
        "integrity_finalization_timeout_sec": int(integrity_finalization_timeout_sec),
        "samples": 3,
        "setup_cleanup_timeout_sec": int(setup_cleanup_timeout_sec),
        "cell_outer_timeout_sec": int(cell_outer_timeout_sec),
    },
}
temporary = out + ".tmp"
with open(temporary, "w", encoding="utf-8") as target:
    json.dump(record, target, sort_keys=True, separators=(",", ":"))
    target.write("\n")
os.replace(temporary, out)
PY

if [[ $MODE == nbd ]]; then
  "$CLI" up --vram "$TIER_MIB" --zram 1024 --daemon "$DAEMON" --transport nbd \
    >"$ARTIFACT_DIR/up.out" 2>"$ARTIFACT_DIR/up.err"
  pinned_preflight >"$ARTIFACT_DIR/preflight-ready.txt"
  grep -q '^NBD_PRODUCT_STATE=READY$' "$ARTIFACT_DIR/preflight-ready.txt" || refuse READY_NOT_PROVEN
  grep -q '^NBD_BINARY_MATCH=PASS$' "$ARTIFACT_DIR/preflight-ready.txt" || refuse BINARY_MATCH_FAILED
  derive_nbd_second_tier_identity "$SWAPS_FILE" "$SYS_BLOCK_ROOT" "$DEV_ROOT" "$PROC_ROOT" \
    "$PID_FILE" "$DAEMON" "$RELEASE/SHA256SUMS" "$LOWER_SINK_IDENTITY_SHA256" 0 \
    || refuse "$NBD_IDENTITY_REASON"
  ACTION_BINARY_MATCH=PASS
  expected_kib=$((TIER_MIB * 1024))
  actual_kib=$NBD_USABLE_SIZE_KIB
  [[ $actual_kib =~ ^[0-9]+$ && $actual_kib -ge $((expected_kib - NBD_MKSWAP_OVERHEAD_KIB)) \
    && $actual_kib -le $expected_kib ]] || refuse NBD_IDENTITY_USABLE_SIZE_INVALID
else
  create_zram_control
  create_disk_scratch
  nbd_disk_control_topology_exact "$SWAPS_FILE" "$SCRATCH_SWAP" || refuse DISK_CONTROL_TIERS_MISSING
fi

SAMPLE_ZRAM_DEVICE=$(capture_sample_zram_device) || refuse ZRAM_SAMPLE_IDENTITY_INVALID
readonly SAMPLE_ZRAM_DEVICE
if [[ $MODE == nbd ]]; then
  nbd_swap_pair_topology_exact "$SWAPS_FILE" "$SAMPLE_ZRAM_DEVICE" "$NBD_DEVICE" partition \
    || refuse NBD_INITIAL_SWAP_TOPOLOGY_INVALID
fi
write_live_context_v2

mkdir -- "$CG"
printf '%s\n' $((MEMORY_HIGH_MIB * 1024 * 1024)) >"$CG/memory.high"
printf '%s\n' $((MEMORY_MAX_MIB * 1024 * 1024)) >"$CG/memory.max"
[[ -f $CG/memory.swap.max ]] && printf 'max\n' >"$CG/memory.swap.max"

for run in 1 2 3; do
  read -r z0 n0 d0 ghost0 <<<"$(swap_used)"
  oom_kill_before=""
  oom_kill_after=""
  integrity_deadline_remaining_sec=""
  s0=$(scratch_used_kib)
  (( ghost0 == 0 )) || refuse GHOST_SWAP_BEFORE_SAMPLE
  result="$ARTIFACT_DIR/run-$run-integrity.json"
  log="$ARTIFACT_DIR/run-$run-worker.log"
  ready="$ARTIFACT_DIR/run-$run-cgroup-ready"
  go="$ARTIFACT_DIR/run-$run-start"
  "$CGROUP_LAUNCH" "$CG" "$ready" "$go" "$WORKER" \
    --allocate-mib "$ALLOCATE_MIB" --chunk-mib "$CHUNK_MIB" \
    --pattern shake256-v1 --result "$result" >"$log" 2>&1 &
  WORKER_PID=$!
  ready_deadline=$((SECONDS + 10))
  while [[ ! -f $ready ]] && kill -0 "$WORKER_PID" 2>/dev/null; do
    (( SECONDS < ready_deadline )) || refuse CGROUP_START_BARRIER_TIMEOUT
    sleep 0.05
  done
  [[ -f $ready ]] || refuse CGROUP_START_BARRIER_FAILED
  grep -qx -- "$WORKER_PID" "$CG/cgroup.procs" || refuse WORKER_NOT_IN_CGROUP
  printf 'CGROUP_MEMBERSHIP=PASS\n' >"$ARTIFACT_DIR/run-$run-process.txt"
  start_ms=$(date +%s%3N)
  python3 - "$go" <<'PY'
import os, sys
fd = os.open(sys.argv[1], os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
os.close(fd)
PY
  hold_deadline=$((SECONDS + SAMPLE_TIMEOUT_SEC))
  max_z=$z0 max_n=$n0 max_d=$d0 max_s=$s0 ghost_seen=$ghost0
  while kill -0 "$WORKER_PID" 2>/dev/null && ! grep -q '^HOLD ' "$log" 2>/dev/null; do
    (( SECONDS < hold_deadline )) || refuse SAMPLE_TIMEOUT
    read -r z n d ghost <<<"$(swap_used)"
    (( z > max_z )) && max_z=$z
    (( n > max_n )) && max_n=$n
    (( d > max_d )) && max_d=$d
    s=$(scratch_used_kib); (( s > max_s )) && max_s=$s
    (( ghost > ghost_seen )) && ghost_seen=$ghost
    sleep 0.2
  done
  grep -q '^HOLD ' "$log" || refuse SAMPLE_WORKER_EXITED_BEFORE_HOLD
  elapsed_ms=$(( $(date +%s%3N) - start_ms ))
  for _ in $(seq 1 10); do
    read -r z n d ghost <<<"$(swap_used)"
    (( z > max_z )) && max_z=$z
    (( n > max_n )) && max_n=$n
    (( d > max_d )) && max_d=$d
    s=$(scratch_used_kib); (( s > max_s )) && max_s=$s
    (( ghost > ghost_seen )) && ghost_seen=$ghost
    sleep 0.1
  done
  oom_kill_before=$(read_cgroup_oom_kill_count "$CG/memory.events") || refuse CGROUP_OOM_RECEIPT_INVALID
  # The HOLD deadline contains allocation only. The independent policy window
  # begins only after TERM is issued at observed HOLD and remains non-promotable on any timeout.
  finalize_integrity_worker_after_hold "$ARTIFACT_DIR/run-$run-process.txt" "$oom_kill_before"
  checksum_match=$(python3 - "$result" "$ALLOCATE_MIB" <<'PY'
import json, sys
with open(sys.argv[1], encoding="utf-8") as source:
    row = json.load(source)
ok = (
    row.get("status") == "PASS"
    and row.get("allocated_mib") == int(sys.argv[2])
    and row.get("pattern") == "shake256-v1"
    and row.get("checksum_before") == row.get("checksum_after")
)
print("true" if ok else "false")
PY
  )
  [[ $checksum_match == true ]] || refuse SAMPLE_CHECKSUM_FAILED
  max_z_delta=$((max_z - z0)); (( max_z_delta >= 0 )) || max_z_delta=0
  max_n_delta=$((max_n - n0)); (( max_n_delta >= 0 )) || max_n_delta=0
  max_d_delta=$((max_d - d0)); (( max_d_delta >= 0 )) || max_d_delta=0
  max_s_delta=$((max_s - s0)); (( max_s_delta >= 0 )) || max_s_delta=0
  python3 - "$ARTIFACT_DIR/run-$run-activity.json" "$MODE" "$TIER_MIB" \
    "$max_z_delta" "$max_n_delta" "$max_d_delta" "$max_s_delta" <<'PY'
import json, os, sys
path, mode, tier, zram, nbd, disk, scratch = sys.argv[1:]
record = {
    "schema": 1,
    "mode": mode,
    "tier_mib": int(tier),
    "observed_delta_kib": {"zram": int(zram), "nbd": int(nbd), "disk": int(disk), "scratch": int(scratch)},
    "required_delta_kib": {"zram": 1024 * 1024 - 8192, "second_tier": int(tier) * 1024 - 8192},
}
temporary = path + ".tmp"
with open(temporary, "w", encoding="utf-8") as target:
    json.dump(record, target, sort_keys=True, separators=(",", ":"))
    target.write("\n")
os.replace(temporary, path)
PY
  if [[ $MODE == nbd ]]; then
    (( max_z_delta >= 1024 * 1024 - 8192 \
      && max_n_delta >= TIER_MIB * 1024 - 8192 \
      && max_d_delta > 8192 )) || refuse NBD_ACTIVITY_MISSING
    binary_match=PASS
  else
    (( max_z_delta >= 1024 * 1024 - 8192 && max_n_delta == 0 \
      && max_s_delta >= TIER_MIB * 1024 - 8192 )) \
      || refuse DISK_CONTROL_ACTIVITY_INVALID
    binary_match=N/A
  fi
  python3 - "$SAMPLES" "$run" "$MODE" "$CONDITION" "$TIER_MIB" "$elapsed_ms" \
    "$max_z_delta" "$max_n_delta" "$max_d_delta" "$max_s_delta" "$binary_match" "$ALLOCATE_MIB" <<'PY'
import json, sys
path, run, mode, condition, tier, elapsed, zram, nbd, disk, scratch, binary, allocated = sys.argv[1:]
row = {
    "schema": 1, "run": int(run), "mode": mode, "condition": condition,
    "tier_mib": int(tier), "allocation_to_hold_ms": int(elapsed), "pattern": "shake256-v1",
    "allocation_chunk_bytes": 64 * 1024 * 1024, "worker_threads": 1,
    "workload": "anonymous_memory_sequential_write", "allocated_mib": int(allocated),
    "memory_high_mib": 1200, "memory_max_mib": int(allocated) + 512, "checksum_match": True,
    "max_zram_delta_kib": int(zram), "max_nbd_delta_kib": int(nbd),
    "max_disk_delta_kib": int(disk), "max_scratch_delta_kib": int(scratch), "ghost_swap": False,
    "binary_match": binary,
}
with open(path, "a", encoding="utf-8") as target:
    target.write(json.dumps(row, sort_keys=True, separators=(",", ":")) + "\n")
PY
  if (( run < RUNS )); then
    republish_sample_baseline || refuse "$SAMPLE_BASELINE_REASON"
  fi
done

finish_success
