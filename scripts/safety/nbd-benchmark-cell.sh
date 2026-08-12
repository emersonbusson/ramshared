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
SAMPLE_TIMEOUT_SEC=120
ALLOCATE_MIB=""
MEMORY_HIGH_MIB=1200
MEMORY_MAX_MIB=""
CHUNK_MIB=64
NBD_MKSWAP_OVERHEAD_KIB=8
SEALED_RELEASE_ROOT=""
RELEASE_VERSION=""
EXPECTED_SOURCE_COMMIT=""
EXPECTED_MANIFEST_SHA256=""
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
LIVE_SEAM_PRESENT=0
for seam in RAMSHARED_PRODUCT_ROOT RAMSHARED_BENCHMARK_CGROUP_ROOT \
  RAMSHARED_NBD_SWAPS_FILE RAMSHARED_NBD_PID_FILE RAMSHARED_NBD_ZRAM_RECORD \
  RAMSHARED_NBD_LOWER_SINK RAMSHARED_NBD_VRAM_MIB RAMSHARED_NBD_LIFECYCLE_APPROVAL \
  RAMSHARED_NBD_PROC_ROOT RAMSHARED_NBD_DEV_ROOT RAMSHARED_NBD_SYS_BLOCK_ROOT; do
  [[ -v $seam ]] && LIVE_SEAM_PRESENT=1
done
PRODUCT_ROOT=${RAMSHARED_PRODUCT_ROOT:-/opt/ramshared}
CG_ROOT=${RAMSHARED_BENCHMARK_CGROUP_ROOT:-/sys/fs/cgroup}
SWAPS_FILE=${RAMSHARED_NBD_SWAPS_FILE:-/proc/swaps}
PID_FILE=${RAMSHARED_NBD_PID_FILE:-/run/ramshared/ramsharedd.pid}
ZRAM_RECORD=${RAMSHARED_NBD_ZRAM_RECORD:-/run/ramshared/zram-dev}
PROC_ROOT=${RAMSHARED_NBD_PROC_ROOT:-/proc}
DEV_ROOT=${RAMSHARED_NBD_DEV_ROOT:-/dev}
SYS_BLOCK_ROOT=${RAMSHARED_NBD_SYS_BLOCK_ROOT:-/sys/block}
NBD_RUNTIME_SOCKET=/run/ramshared/wsl2d.sock

refuse() {
  printf 'NBD_BENCHMARK_STATE=REFUSED\n'
  printf 'NBD_BENCHMARK_REASON=%s\n' "$1"
  exit 2
}

while [[ $# -gt 0 ]]; do
  case $1 in
    --aggregate) ACTION=aggregate; shift ;;
    --run) ACTION=run; shift ;;
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
  $ACTION == validate-nbd-identity-fixture || $ACTION == classify-swap-fixture ]] \
  || refuse ACTION_REQUIRED
if [[ $ACTION == aggregate || $ACTION == run ]]; then
  [[ $MODE == disk-only || $MODE == nbd ]] || refuse MODE_INVALID
  [[ $CONDITION == idle || $CONDITION == bounded ]] || refuse CONDITION_INVALID
  [[ $TIER_MIB =~ ^(1024|2048|4096)$ ]] || refuse TIER_SIZE_INVALID
  [[ $RUNS == 3 ]] || refuse RUN_COUNT_INVALID
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

aggregate_samples() {
  [[ -f $SAMPLES && -n $OUT ]] || refuse AGGREGATE_INPUT_INVALID
  python3 - "$SAMPLES" "$OUT" "$MODE" "$CONDITION" "$TIER_MIB" <<'PY'
import json
import hashlib
import math
import os
import statistics
import sys

samples_path, output_path, mode, condition, tier_text = sys.argv[1:]
tier_mib = int(tier_text)
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
  python3 - "$ARTIFACT_DIR" <<'PY'
import hashlib
import json
import os
import stat
import sys

root = os.path.realpath(sys.argv[1])
inventory_path = os.path.join(root, "artifact-inventory.json")
required = {
    "before.txt", "action.txt", "after.txt", "context.json", "samples.jsonl", "summary.json",
}
excluded = {"artifact-inventory.json", "evidence-envelope.json"}
try:
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
  python3 - "$ARTIFACT_DIR" <<'PY'
import hashlib
import json
import os
import sys

root = os.path.realpath(sys.argv[1])
try:
    with open(os.path.join(root, "artifact-inventory.json"), encoding="utf-8") as source:
        inventory = json.load(source)
    with open(os.path.join(root, "evidence-envelope.json"), encoding="utf-8") as source:
        envelope = json.load(source)
    with open(os.path.join(root, "context.json"), encoding="utf-8") as source:
        context = json.load(source)
    with open(os.path.join(root, "context.json"), "rb") as source:
        context_sha = hashlib.sha256(source.read()).hexdigest()
    with open(os.path.join(root, "summary.json"), "rb") as source:
        summary_sha = hashlib.sha256(source.read()).hexdigest()
    with open(os.path.join(root, "artifact-inventory.json"), "rb") as source:
        inventory_sha = hashlib.sha256(source.read()).hexdigest()
    required = {
        "schema_version", "pair_id", "mode", "release", "context_sha256", "summary_sha256",
        "artifact_inventory_sha256", "artifacts", "binary_match", "watchdog", "classification",
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
[[ ${RAMSHARED_SHARED_HOST_APPROVAL:-} == I_ACCEPT_WSL_TERMINATION ]] || refuse SHARED_HOST_APPROVAL_MISSING
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

pinned_preflight() {
  RAMSHARED_NBD_VRAM_MIB=$TIER_MIB "$PREFLIGHT" --check \
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

managed_swap_count() {
  awk 'NR > 1 && ($1 ~ /\/zram[0-9]+$/ || $1 ~ /\/nbd[0-9]+$/ || $1 ~ /\/ublkb[0-9]+$/) { count += 1 } END { print count + 0 }' "$SWAPS_FILE"
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
  local zdev lower lower_type original_nbd_identity
  [[ -f $ZRAM_RECORD && ! -L $ZRAM_RECORD ]] || return 1
  IFS= read -r zdev <"$ZRAM_RECORD" || return 1
  [[ $zdev =~ ^/dev/zram[0-9]+$ ]] || return 1
  if [[ $MODE == nbd ]]; then
    lower=$NBD_DEVICE
    lower_type=partition
    original_nbd_identity=$NBD_SECOND_TIER_IDENTITY_SHA256
    nbd_reconnect_republish_swap_pair "$SWAPS_FILE" "$zdev" "$lower" "$NBD_RUNTIME_SOCKET" \
      /run/ramshared/wsl2d.sock /sbin/swapoff /usr/sbin/nbd-client /sbin/mkswap /sbin/swapon \
      || return 1
  else
    lower=$SCRATCH_SWAP
    lower_type=file
    nbd_republish_swap_pair "$SWAPS_FILE" "$zdev" "$lower" "$lower_type" /sbin/swapoff /sbin/swapon \
      || return 1
  fi
  if [[ $MODE == nbd ]]; then
    derive_nbd_second_tier_identity "$SWAPS_FILE" "$SYS_BLOCK_ROOT" "$DEV_ROOT" "$PROC_ROOT" \
      "$PID_FILE" "$DAEMON" "$RELEASE/SHA256SUMS" "$LOWER_SINK_IDENTITY_SHA256" 0 \
      || return 1
    [[ $NBD_SECOND_TIER_IDENTITY_SHA256 == "$original_nbd_identity" ]] || return 1
    pinned_preflight | grep -q '^NBD_BINARY_MATCH=PASS$' || return 1
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
    "$nbd_daemon_manifest_sha" <<'PY'
import hashlib
import json
import os
import sys

(out, preflight_path, mode, condition, tier, allocated, memory_high, memory_max, chunk, version,
 kernel, manifest_sha, source_commit, source_branch, source_tree_state, pair_id, utc_started, release_root,
 zram_name, zram_algorithm, zram_size_kib, zram_priority, lower_kind, lower_identity,
 sink_type, sink_identity, binary_match, input_bundle_manifest_sha, nbd_device, nbd_block_major_minor,
 nbd_size_kib, nbd_usable_size_kib, nbd_capacity_sectors, nbd_priority, nbd_server_pid,
 nbd_daemon_manifest_sha) = sys.argv[1:]
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
    "--runs", 3, "--sample-timeout-sec", 120,
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
  python3 - "$ARTIFACT_DIR" <<'PY'
import hashlib, json, os, stat, sys

root = os.path.realpath(sys.argv[1])
rows = []
for name in sorted(os.listdir(root)):
    if name in {"artifact-inventory.json", "evidence-envelope.json"}:
        continue
    path = os.path.join(root, name)
    metadata = os.lstat(path)
    if not stat.S_ISREG(metadata.st_mode):
        raise SystemExit("artifact_inventory_non_regular")
    with open(path, "rb") as source:
        rows.append({"name": name, "bytes": metadata.st_size, "sha256": hashlib.sha256(source.read()).hexdigest()})
out = os.path.join(root, "artifact-inventory.json")
with open(out + ".tmp", "w", encoding="utf-8") as target:
    json.dump({"schema": 2, "files": rows}, target, sort_keys=True, separators=(",", ":"))
    target.write("\n")
os.replace(out + ".tmp", out)
PY
  validate_artifact_inventory || refuse EVIDENCE_INVENTORY_INVALID
}

write_evidence_envelope() {
  python3 - "$ARTIFACT_DIR" "$MODE" "$PAIR_ID" <<'PY'
import hashlib
import json
import os
import sys

root, mode, pair_id = sys.argv[1:]
def digest(name):
    with open(os.path.join(root, name), "rb") as source:
        return hashlib.sha256(source.read()).hexdigest()
with open(os.path.join(root, "context.json"), encoding="utf-8") as source:
    context = json.load(source)
with open(os.path.join(root, "artifact-inventory.json"), encoding="utf-8") as source:
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
    "context_sha256": digest("context.json"),
    "summary_sha256": digest("summary.json"),
    "artifact_inventory_sha256": digest("artifact-inventory.json"),
    "artifacts": artifacts,
    "binary_match": context["binary_match"],
    "watchdog": context["watchdog"],
    "classification": "INCOMPARABLE",
}
out = os.path.join(root, "evidence-envelope.json")
temporary = out + ".tmp"
with open(temporary, "w", encoding="utf-8") as target:
    json.dump(record, target, sort_keys=True, separators=(",", ":"))
    target.write("\n")
os.replace(temporary, out)
PY
  validate_evidence_envelope || refuse EVIDENCE_ENVELOPE_INVALID
}

cleanup_disk_scratch() {
  [[ -n $SCRATCH_SWAP ]] || return 0
  if (( SCRATCH_SWAP_ACTIVE == 1 )) && [[ $(nbd_swap_exact_count "$SWAPS_FILE" "$SCRATCH_SWAP") != 1 ]]; then
    return 1
  fi
  nbd_cleanup_scratch "$SCRATCH_SWAP" "$SCRATCH_IDENTITY" "$SWAPS_FILE" /sbin/swapoff \
    || return 1
  SCRATCH_SWAP_ACTIVE=0
  SCRATCH_SWAP=""
  SCRATCH_IDENTITY=""
}

safe_product_off() {
  local z n d ghost
  read -r z n d ghost <<<"$(swap_used)"
  if (( $(managed_swap_count) > 0 )) || [[ -f $PID_FILE ]]; then
    "$CLI" down \
      >"$ARTIFACT_DIR/down.out" 2>"$ARTIFACT_DIR/down.err" || return 1
  fi
  read -r z n d ghost <<<"$(swap_used)"
  (( z == 0 && n == 0 && ghost == 0 && $(managed_swap_count) == 0 )) || return 1
  [[ ! -f $PID_FILE ]] || return 1
}

cleanup() {
  local rc=$?
  trap - EXIT INT TERM
  if [[ -n $WORKER_PID ]] && kill -0 "$WORKER_PID" 2>/dev/null; then
    kill -TERM "$WORKER_PID" 2>/dev/null || true
    wait "$WORKER_PID" 2>/dev/null || true
  fi
  if [[ -d $CG ]]; then
    rmdir "$CG" 2>/dev/null || rc=1
  fi
  if ! cleanup_disk_scratch; then
    printf 'NBD_BENCHMARK_STATE=RED\nNBD_BENCHMARK_REASON=scratch_cleanup_failed\n' >&2
    rc=1
  fi
  if ! safe_product_off; then
    CLEANUP_OK=0
    printf 'NBD_BENCHMARK_STATE=RED\nNBD_BENCHMARK_REASON=terminal_product_off_failed\n' >&2
    rc=1
  fi
  exit "$rc"
}
trap cleanup EXIT INT TERM

safe_product_off || refuse INITIAL_PRODUCT_OFF_FAILED
pinned_preflight >"$ARTIFACT_DIR/preflight-off.txt"
grep -q '^NBD_PRODUCT_STATE=PRODUCT_OFF$' "$ARTIFACT_DIR/preflight-off.txt" || refuse PRODUCT_OFF_NOT_PROVEN
load_bound_lower_sink
cp -- "$ARTIFACT_DIR/preflight-off.txt" "$ARTIFACT_DIR/before.txt"
python3 - "$ARTIFACT_DIR/action.txt" "$MODE" "$CONDITION" "$TIER_MIB" "$VERSION" "$PAIR_ID" <<'PY'
import json
import os
import sys

out, mode, condition, tier, version, pair_id = sys.argv[1:]
record = {
    "schema": 1,
    "action": "sealed_nbd_benchmark_cell",
    "mode": mode,
    "condition": condition,
    "tier_mib": int(tier),
    "release_version": version,
    "pair_id": pair_id,
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

write_live_context_v2

mkdir -- "$CG"
printf '%s\n' $((MEMORY_HIGH_MIB * 1024 * 1024)) >"$CG/memory.high"
printf '%s\n' $((MEMORY_MAX_MIB * 1024 * 1024)) >"$CG/memory.max"
[[ -f $CG/memory.swap.max ]] && printf 'max\n' >"$CG/memory.swap.max"

for run in 1 2 3; do
  read -r z0 n0 d0 ghost0 <<<"$(swap_used)"
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
  deadline=$((SECONDS + SAMPLE_TIMEOUT_SEC))
  max_z=$z0 max_n=$n0 max_d=$d0 max_s=$s0 ghost_seen=$ghost0
  while kill -0 "$WORKER_PID" 2>/dev/null && ! grep -q '^HOLD ' "$log" 2>/dev/null; do
    (( SECONDS < deadline )) || refuse SAMPLE_TIMEOUT
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
  kill -TERM "$WORKER_PID"
  wait "$WORKER_PID" || refuse SAMPLE_INTEGRITY_PROCESS_FAILED
  WORKER_PID=""
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
    republish_sample_baseline || refuse BASELINE_REPUBLICATION_FAILED
  fi
done

cleanup_disk_scratch || refuse FINAL_SCRATCH_OFF_FAILED
safe_product_off || refuse FINAL_PRODUCT_OFF_FAILED
{
  printf 'NBD_PRODUCT_STATE=PRODUCT_OFF\n'
  printf 'NBD_TRANSPORT=none\n'
  printf 'NBD_BINARY_MATCH=%s\n' "$ACTION_BINARY_MATCH"
} >"$ARTIFACT_DIR/after.txt"
aggregate_samples
write_artifact_inventory
write_evidence_envelope
printf 'NBD_BENCHMARK_MATRIX=PASS\n'
