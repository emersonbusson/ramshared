#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
CELL="$ROOT/scripts/safety/nbd-benchmark-cell.sh"
CGROUP_LAUNCH="$ROOT/scripts/safety/nbd-benchmark-cgroup-launch.sh"
BENCHMARK_LIB="$ROOT/scripts/safety/nbd-benchmark-lib.sh"
TMP=$(mktemp -d)
declare -a TEST_CHILD_PIDS=()
cleanup_test_children() {
  local pid
  for pid in "${TEST_CHILD_PIDS[@]}"; do
    kill -TERM "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  done
  rm -rf -- "$TMP"
}
trap cleanup_test_children EXIT

pass_count=0

pass() {
  pass_count=$((pass_count + 1))
  printf 'PASS %s\n' "$1"
}

[[ -x $CELL ]] || {
  printf 'FAIL benchmark cell harness missing or not executable: %s\n' "$CELL" >&2
  exit 1
}
[[ -x $CGROUP_LAUNCH ]] || {
  printf 'FAIL benchmark cgroup launcher missing or not executable: %s\n' "$CGROUP_LAUNCH" >&2
  exit 1
}
[[ -f $BENCHMARK_LIB && ! -L $BENCHMARK_LIB ]] || {
  printf 'FAIL benchmark library missing or symlinked: %s\n' "$BENCHMARK_LIB" >&2
  exit 1
}
# shellcheck source=nbd-benchmark-lib.sh
source "$BENCHMARK_LIB"

make_identity_fixture() {
  local root=$1 daemon_hash
  mkdir -p "$root/proc/4242" "$root/sys/block/nbd0" "$root/dev" "$root/run" "$root/release/bin"
  printf 'manufactured ramsharedd\n' >"$root/release/bin/ramsharedd"
  chmod 0700 "$root/release/bin/ramsharedd"
  daemon_hash=$(sha256sum -- "$root/release/bin/ramsharedd" | awk '{print $1}')
  printf '%s  ./bin/ramsharedd\n' "$daemon_hash" >"$root/release/SHA256SUMS"
  ln -s "$root/release/bin/ramsharedd" "$root/proc/4242/exe"
  printf '4242\n' >"$root/run/ramsharedd.pid"
  # A 1 GiB block device has 2,097,152 512-byte sectors.  mkswap may
  # expose a small amount less through /proc/swaps, so the fixture starts
  # with the observed 4 KiB overhead instead of conflating the two sizes.
  printf 'Filename Type Size Used Priority\n/dev/nbd0 partition 1048572 0 100\n' >"$root/proc/swaps"
  printf '43:0\n' >"$root/sys/block/nbd0/dev"
  printf '2097152\n' >"$root/sys/block/nbd0/size"
  : >"$root/dev/nbd0"
}

run_identity_fixture() {
  local root=$1 lower_sink_identity=$2 tier_mib=${3:-1024}
  RAMSHARED_NBD_ALLOW_MANUFACTURED_IDENTITY_TEST=1 \
    "$CELL" --validate-nbd-identity-fixture --identity-fixture-root "$root" \
    --tier-mib "$tier_mib" --lower-sink-identity-sha256 "$lower_sink_identity"
}

assert_identity_refusal() {
  local name=$1 expected_reason=$2 root=$3 lower_sink_identity=$4 output rc
  set +e
  output=$(run_identity_fixture "$root" "$lower_sink_identity" 2>&1)
  rc=$?
  set -e
  [[ $rc -ne 0 && $output == *"NBD_IDENTITY_REASON=$expected_reason"* ]] || {
    printf 'FAIL %s: expected refusal %s, rc=%s output=%s\n' "$name" "$expected_reason" "$rc" "$output" >&2
    exit 1
  }
}

identity_root="$TMP/identity-fixture"
lower_sink_identity=$(printf 'manufactured installed lower sink\n' | sha256sum | awk '{print $1}')
make_identity_fixture "$identity_root"
identity_output=$(run_identity_fixture "$identity_root" "$lower_sink_identity")
expected_identity=$(python3 - "$identity_root" <<'PY'
import hashlib
import sys

root = sys.argv[1]
with open(root + "/release/bin/ramsharedd", "rb") as source:
    daemon_hash = hashlib.sha256(source.read()).hexdigest()
values = (
    "ramshared-nbd-second-tier/v1",
    "/dev/nbd0",
    "43:0",
    "1048576",
    "100",
    "4242",
    root + "/release/bin/ramsharedd",
    daemon_hash,
)
print(hashlib.sha256(b"\0".join(value.encode("utf-8") for value in values)).hexdigest())
PY
)
[[ $identity_output == *'NBD_IDENTITY_STATE=PASS'* &&
  $identity_output == *'NBD_DEVICE=/dev/nbd0'* &&
  $identity_output == *'NBD_BLOCK_MAJOR_MINOR=43:0'* &&
  $identity_output == *'NBD_CAPACITY_SECTORS=2097152'* &&
  $identity_output == *'NBD_USABLE_SIZE_KIB=1048572'* &&
  $identity_output == *'NBD_SIZE_KIB=1048576'* &&
  $identity_output == *'NBD_PRIORITY=100'* &&
  $identity_output == *'NBD_SERVER_PID=4242'* &&
  $identity_output == *"NBD_SECOND_TIER_IDENTITY_SHA256=$expected_identity"* &&
  $identity_output == *"NBD_LOWER_SINK_IDENTITY_SHA256=$lower_sink_identity"* ]] || {
  printf 'FAIL manufactured NBD identity was not derived from the exact tuple: %s\n' "$identity_output" >&2
  exit 1
}
pass nbd_second_tier_identity_is_observed_and_separate_from_lower_sink

exact_usable_identity_root="$TMP/identity-usable-exact"
make_identity_fixture "$exact_usable_identity_root"
sed -i 's/1048572 0 100/1048576 0 100/' "$exact_usable_identity_root/proc/swaps"
exact_usable_output=$(run_identity_fixture "$exact_usable_identity_root" "$lower_sink_identity")
[[ $exact_usable_output == *'NBD_IDENTITY_STATE=PASS'* &&
  $exact_usable_output == *'NBD_CAPACITY_SECTORS=2097152'* &&
  $exact_usable_output == *'NBD_USABLE_SIZE_KIB=1048576'* &&
  $exact_usable_output == *'NBD_SIZE_KIB=1048576'* ]] || {
  printf 'FAIL exact mkswap usable size was not accepted independently of capacity: %s\n' \
    "$exact_usable_output" >&2
  exit 1
}
pass nbd_identity_accepts_bounded_mkswap_overhead_and_exact_usable_size

for tier_mib in 2048 4096; do
  tier_identity_root="$TMP/identity-tier-$tier_mib"
  make_identity_fixture "$tier_identity_root"
  usable_kib=$((tier_mib * 1024 - 4))
  capacity_sectors=$((tier_mib * 2048))
  sed -i "s/1048572 0 100/$usable_kib 0 100/" "$tier_identity_root/proc/swaps"
  printf '%s\n' "$capacity_sectors" >"$tier_identity_root/sys/block/nbd0/size"
  tier_output=$(run_identity_fixture "$tier_identity_root" "$lower_sink_identity" "$tier_mib")
  [[ $tier_output == *"NBD_CAPACITY_SECTORS=$capacity_sectors"* &&
    $tier_output == *"NBD_USABLE_SIZE_KIB=$usable_kib"* &&
    $tier_output == *"NBD_SIZE_KIB=$((tier_mib * 1024))"* ]] || {
    printf 'FAIL tier %s capacity/usable identity mismatch: %s\n' "$tier_mib" "$tier_output" >&2
    exit 1
  }
done
pass nbd_capacity_contract_covers_all_supported_tiers

missing_identity_root="$TMP/identity-missing"
make_identity_fixture "$missing_identity_root"
printf 'Filename Type Size Used Priority\n' >"$missing_identity_root/proc/swaps"
assert_identity_refusal nbd_identity_missing NBD_IDENTITY_MISSING "$missing_identity_root" "$lower_sink_identity"

duplicate_identity_root="$TMP/identity-duplicate"
make_identity_fixture "$duplicate_identity_root"
printf 'Filename Type Size Used Priority\n/dev/nbd0 partition 1048576 0 100\n/dev/nbd1 partition 1048576 0 100\n' \
  >"$duplicate_identity_root/proc/swaps"
assert_identity_refusal nbd_identity_duplicate NBD_IDENTITY_DUPLICATE "$duplicate_identity_root" "$lower_sink_identity"

foreign_identity_root="$TMP/identity-foreign"
make_identity_fixture "$foreign_identity_root"
rm -- "$foreign_identity_root/dev/nbd0"
ln -s ../foreign "$foreign_identity_root/dev/nbd0"
assert_identity_refusal nbd_identity_foreign_device NBD_IDENTITY_FOREIGN_DEVICE "$foreign_identity_root" "$lower_sink_identity"

trailing_identity_root="$TMP/identity-trailing-field"
make_identity_fixture "$trailing_identity_root"
sed -i 's/1048572 0 100/1048572 0 100 trailing/' "$trailing_identity_root/proc/swaps"
assert_identity_refusal nbd_identity_trailing_swap_field NBD_IDENTITY_FOREIGN_DEVICE \
  "$trailing_identity_root" "$lower_sink_identity"

capacity_identity_root="$TMP/identity-capacity"
make_identity_fixture "$capacity_identity_root"
printf '2097136\n' >"$capacity_identity_root/sys/block/nbd0/size"
assert_identity_refusal nbd_identity_capacity_mismatch NBD_IDENTITY_CAPACITY_MISMATCH \
  "$capacity_identity_root" "$lower_sink_identity"

usable_identity_root="$TMP/identity-usable-loss"
make_identity_fixture "$usable_identity_root"
sed -i 's/1048572 0 100/1048567 0 100/' "$usable_identity_root/proc/swaps"
assert_identity_refusal nbd_identity_excessive_usable_loss NBD_IDENTITY_USABLE_SIZE_INVALID \
  "$usable_identity_root" "$lower_sink_identity"

overflow_usable_identity_root="$TMP/identity-usable-overflow"
make_identity_fixture "$overflow_usable_identity_root"
# 2^64 + 1048576 would wrap to the nominal 1 GiB value in unchecked Bash
# arithmetic. It must be refused as untrusted decimal input before arithmetic.
sed -i 's/1048572 0 100/18446744073710600192 0 100/' \
  "$overflow_usable_identity_root/proc/swaps"
assert_identity_refusal nbd_usable_size_overflow NBD_IDENTITY_USABLE_SIZE_INVALID \
  "$overflow_usable_identity_root" "$lower_sink_identity"
pass nbd_usable_size_overflow_refuses_before_bash_arithmetic

malformed_capacity_identity_root="$TMP/identity-capacity-malformed"
make_identity_fixture "$malformed_capacity_identity_root"
printf 'not-a-sector-count\n' >"$malformed_capacity_identity_root/sys/block/nbd0/size"
assert_identity_refusal nbd_identity_malformed_sysfs_capacity NBD_IDENTITY_SYSFS_CAPACITY_INVALID \
  "$malformed_capacity_identity_root" "$lower_sink_identity"

whitespace_capacity_identity_root="$TMP/identity-capacity-whitespace"
make_identity_fixture "$whitespace_capacity_identity_root"
printf '2 097152\n' >"$whitespace_capacity_identity_root/sys/block/nbd0/size"
assert_identity_refusal nbd_identity_noncanonical_sysfs_capacity NBD_IDENTITY_SYSFS_CAPACITY_INVALID \
  "$whitespace_capacity_identity_root" "$lower_sink_identity"

priority_identity_root="$TMP/identity-priority"
make_identity_fixture "$priority_identity_root"
sed -i 's/1048572 0 100/1048572 0 -2/' "$priority_identity_root/proc/swaps"
assert_identity_refusal nbd_identity_priority_mismatch NBD_IDENTITY_PRIORITY_MISMATCH "$priority_identity_root" "$lower_sink_identity"

server_identity_root="$TMP/identity-server"
make_identity_fixture "$server_identity_root"
printf '4243\n' >"$server_identity_root/run/ramsharedd.pid"
assert_identity_refusal nbd_identity_server_mismatch NBD_IDENTITY_SERVER_PID_STALE "$server_identity_root" "$lower_sink_identity"

exe_identity_root="$TMP/identity-exe"
make_identity_fixture "$exe_identity_root"
printf 'foreign executable\n' >"$exe_identity_root/foreign-daemon"
chmod 0700 "$exe_identity_root/foreign-daemon"
rm -- "$exe_identity_root/proc/4242/exe"
ln -s "$exe_identity_root/foreign-daemon" "$exe_identity_root/proc/4242/exe"
assert_identity_refusal nbd_identity_executable_mismatch NBD_IDENTITY_DAEMON_EXECUTABLE_MISMATCH "$exe_identity_root" "$lower_sink_identity"

manifest_identity_root="$TMP/identity-manifest"
make_identity_fixture "$manifest_identity_root"
printf '%064d  ./bin/ramsharedd\n' 0 >"$manifest_identity_root/release/SHA256SUMS"
assert_identity_refusal nbd_identity_manifest_hash_mismatch NBD_IDENTITY_DAEMON_HASH_MISMATCH \
  "$manifest_identity_root" "$lower_sink_identity"

sink_substitution_root="$TMP/identity-sink-substitution"
make_identity_fixture "$sink_substitution_root"
sink_substitution_identity=$(python3 - "$sink_substitution_root" <<'PY'
import hashlib
import sys

root = sys.argv[1]
with open(root + "/release/bin/ramsharedd", "rb") as source:
    daemon_hash = hashlib.sha256(source.read()).hexdigest()
values = (
    "ramshared-nbd-second-tier/v1",
    "/dev/nbd0",
    "43:0",
    "1048576",
    "100",
    "4242",
    root + "/release/bin/ramsharedd",
    daemon_hash,
)
print(hashlib.sha256(b"\0".join(value.encode("utf-8") for value in values)).hexdigest())
PY
)
assert_identity_refusal nbd_identity_sink_hash_substitution NBD_IDENTITY_SINK_HASH_SUBSTITUTION \
  "$sink_substitution_root" "$sink_substitution_identity"
pass nbd_second_tier_identity_refuses_missing_duplicate_foreign_and_substitution

run_swap_classifier_fixture() {
  local swaps_file=$1
  RAMSHARED_NBD_ALLOW_MANUFACTURED_SWAP_CLASSIFIER_TEST=1 \
    "$CELL" --classify-swap-fixture --swap-fixture "$swaps_file"
}

classifier_swaps="$TMP/swap-classifier-swaps"
cat >"$classifier_swaps" <<'EOF'
Filename Type Size Used Priority
/dev/zram0 partition 1048576 64 200
/dev/nbd0 partition 1048576 128 100
/dev/ublkb0 partition 1048576 384 99
/var/lib/ramshared/nbd/.ramshared-benchmark-swap-1024-idle-42 file 8388608 512 100
/mnt/nbd-control.swap file 8388608 256 -2
/dev/zramish0 file 8388608 128 -2
/dev/nbd0.backup file 8388608 64 -2
EOF
set +e
classifier_output=$(run_swap_classifier_fixture "$classifier_swaps" 2>&1)
classifier_rc=$?
set -e
[[ $classifier_rc == 0 && $classifier_output == *'SWAP_USED_ZRAM_KIB=64'* &&
  $classifier_output == *'SWAP_USED_NBD_KIB=512'* &&
  $classifier_output == *'SWAP_USED_DISK_KIB=960'* &&
  $classifier_output == *'SWAP_USED_GHOST=0'* ]] || {
  printf 'FAIL swap_device_classifier_requires_exact_device_names: rc=%s output=%s\n' \
    "$classifier_rc" "$classifier_output" >&2
  exit 1
}
pass swap_device_classifier_requires_exact_device_names

cat >"$TMP/samples.jsonl" <<'EOF'
{"schema":1,"run":1,"mode":"nbd","condition":"idle","tier_mib":1024,"allocation_to_hold_ms":100,"pattern":"shake256-v1","allocation_chunk_bytes":67108864,"worker_threads":1,"workload":"anonymous_memory_sequential_write","allocated_mib":3584,"memory_high_mib":1200,"memory_max_mib":4096,"checksum_match":true,"max_zram_delta_kib":1041000,"max_nbd_delta_kib":1041000,"max_disk_delta_kib":100000,"max_scratch_delta_kib":0,"ghost_swap":false,"binary_match":"PASS"}
{"schema":1,"run":2,"mode":"nbd","condition":"idle","tier_mib":1024,"allocation_to_hold_ms":200,"pattern":"shake256-v1","allocation_chunk_bytes":67108864,"worker_threads":1,"workload":"anonymous_memory_sequential_write","allocated_mib":3584,"memory_high_mib":1200,"memory_max_mib":4096,"checksum_match":true,"max_zram_delta_kib":1042000,"max_nbd_delta_kib":1042000,"max_disk_delta_kib":110000,"max_scratch_delta_kib":0,"ghost_swap":false,"binary_match":"PASS"}
{"schema":1,"run":3,"mode":"nbd","condition":"idle","tier_mib":1024,"allocation_to_hold_ms":400,"pattern":"shake256-v1","allocation_chunk_bytes":67108864,"worker_threads":1,"workload":"anonymous_memory_sequential_write","allocated_mib":3584,"memory_high_mib":1200,"memory_max_mib":4096,"checksum_match":true,"max_zram_delta_kib":1043000,"max_nbd_delta_kib":1043000,"max_disk_delta_kib":120000,"max_scratch_delta_kib":0,"ghost_swap":false,"binary_match":"PASS"}
EOF
printf '{"schema":1}\n' >"$TMP/context.json"

"$CELL" --aggregate --samples "$TMP/samples.jsonl" --out "$TMP/summary.json" \
  --mode nbd --condition idle --tier-mib 1024
python3 - "$TMP/summary.json" <<'PY'
import json
import math
import sys

with open(sys.argv[1], encoding="utf-8") as source:
    result = json.load(source)
assert result["status"] == "PASS", result
assert result["n"] == 3, result
assert result["median_allocation_to_hold_ms"] == 200, result
assert result["p99_allocation_to_hold_ms"] == 400, result
assert math.isclose(result["population_stddev_allocation_to_hold_ms"], 124.72191289246472), result
assert result["terminal_state"] == "PRODUCT_OFF", result
assert len(result["context_sha256"]) == 64, result
PY
pass benchmark_aggregation_is_exact_and_requires_three_runs

head -n 2 "$TMP/samples.jsonl" >"$TMP/two.jsonl"
if "$CELL" --aggregate --samples "$TMP/two.jsonl" --out "$TMP/two-summary.json" \
  --mode nbd --condition idle --tier-mib 1024 >/dev/null 2>&1; then
  echo 'FAIL aggregate accepted n<3' >&2
  exit 1
fi

sed '3s/"run":3/"run":2/' "$TMP/samples.jsonl" >"$TMP/duplicate.jsonl"
if "$CELL" --aggregate --samples "$TMP/duplicate.jsonl" --out "$TMP/duplicate-summary.json" \
  --mode nbd --condition idle --tier-mib 1024 >/dev/null 2>&1; then
  echo 'FAIL aggregate accepted duplicate run' >&2
  exit 1
fi

sed '2s/"checksum_match":true/"checksum_match":false/' "$TMP/samples.jsonl" >"$TMP/corrupt.jsonl"
if "$CELL" --aggregate --samples "$TMP/corrupt.jsonl" --out "$TMP/corrupt-summary.json" \
  --mode nbd --condition idle --tier-mib 1024 >/dev/null 2>&1; then
  echo 'FAIL aggregate accepted checksum failure' >&2
  exit 1
fi

sed '2s/"max_nbd_delta_kib":1042000/"max_nbd_delta_kib":0/' "$TMP/samples.jsonl" >"$TMP/no-nbd.jsonl"
if "$CELL" --aggregate --samples "$TMP/no-nbd.jsonl" --out "$TMP/no-nbd-summary.json" \
  --mode nbd --condition idle --tier-mib 1024 >/dev/null 2>&1; then
  echo 'FAIL aggregate accepted NBD sample without NBD activity' >&2
  exit 1
fi

sed '2s/"max_zram_delta_kib":1042000/"max_zram_delta_kib":0/' "$TMP/samples.jsonl" >"$TMP/no-zram.jsonl"
if "$CELL" --aggregate --samples "$TMP/no-zram.jsonl" --out "$TMP/no-zram-summary.json" \
  --mode nbd --condition idle --tier-mib 1024 >/dev/null 2>&1; then
  echo 'FAIL aggregate accepted NBD sample without zram activity' >&2
  exit 1
fi

sed '2s/"ghost_swap":false/"ghost_swap":true/' "$TMP/samples.jsonl" >"$TMP/ghost.jsonl"
if "$CELL" --aggregate --samples "$TMP/ghost.jsonl" --out "$TMP/ghost-summary.json" \
  --mode nbd --condition idle --tier-mib 1024 >/dev/null 2>&1; then
  echo 'FAIL aggregate accepted ghost swap' >&2
  exit 1
fi

sed '2s/"binary_match":"PASS"/"binary_match":"FAIL"/' "$TMP/samples.jsonl" >"$TMP/binary-mismatch.jsonl"
if "$CELL" --aggregate --samples "$TMP/binary-mismatch.jsonl" --out "$TMP/binary-mismatch-summary.json" \
  --mode nbd --condition idle --tier-mib 1024 >/dev/null 2>&1; then
  echo 'FAIL aggregate accepted BINARY_MATCH failure' >&2
  exit 1
fi

cat >"$TMP/disk.jsonl" <<'EOF'
{"schema":1,"run":1,"mode":"disk-only","condition":"idle","tier_mib":1024,"allocation_to_hold_ms":101,"pattern":"shake256-v1","allocation_chunk_bytes":67108864,"worker_threads":1,"workload":"anonymous_memory_sequential_write","allocated_mib":3584,"memory_high_mib":1200,"memory_max_mib":4096,"checksum_match":true,"max_zram_delta_kib":1041000,"max_nbd_delta_kib":0,"max_disk_delta_kib":100000,"max_scratch_delta_kib":1041000,"ghost_swap":false,"binary_match":"N/A"}
{"schema":1,"run":2,"mode":"disk-only","condition":"idle","tier_mib":1024,"allocation_to_hold_ms":102,"pattern":"shake256-v1","allocation_chunk_bytes":67108864,"worker_threads":1,"workload":"anonymous_memory_sequential_write","allocated_mib":3584,"memory_high_mib":1200,"memory_max_mib":4096,"checksum_match":true,"max_zram_delta_kib":1042000,"max_nbd_delta_kib":0,"max_disk_delta_kib":110000,"max_scratch_delta_kib":1042000,"ghost_swap":false,"binary_match":"N/A"}
{"schema":1,"run":3,"mode":"disk-only","condition":"idle","tier_mib":1024,"allocation_to_hold_ms":103,"pattern":"shake256-v1","allocation_chunk_bytes":67108864,"worker_threads":1,"workload":"anonymous_memory_sequential_write","allocated_mib":3584,"memory_high_mib":1200,"memory_max_mib":4096,"checksum_match":true,"max_zram_delta_kib":1043000,"max_nbd_delta_kib":0,"max_disk_delta_kib":120000,"max_scratch_delta_kib":1043000,"ghost_swap":false,"binary_match":"N/A"}
EOF
"$CELL" --aggregate --samples "$TMP/disk.jsonl" --out "$TMP/disk-summary.json" \
  --mode disk-only --condition idle --tier-mib 1024
sed '2s/"max_scratch_delta_kib":1042000/"max_scratch_delta_kib":0/' "$TMP/disk.jsonl" >"$TMP/no-scratch.jsonl"
if "$CELL" --aggregate --samples "$TMP/no-scratch.jsonl" --out "$TMP/no-scratch-summary.json" \
  --mode disk-only --condition idle --tier-mib 1024 >/dev/null 2>&1; then
  echo 'FAIL aggregate accepted disk control without scratch activity' >&2
  exit 1
fi
pass disk_control_and_nbd_candidate_share_one_workload_contract

write_tier_samples() {
  local path=$1 mode=$2 tier=$3
  python3 - "$path" "$mode" "$tier" <<'PY'
import json
import sys

path, mode, tier_text = sys.argv[1:]
tier = int(tier_text)
allocated = tier + 2560
rows = []
for run, elapsed in enumerate((101, 202, 303), start=1):
    row = {
        "schema": 1,
        "run": run,
        "mode": mode,
        "condition": "idle",
        "tier_mib": tier,
        "allocation_to_hold_ms": elapsed,
        "pattern": "shake256-v1",
        "allocation_chunk_bytes": 64 * 1024 * 1024,
        "worker_threads": 1,
        "workload": "anonymous_memory_sequential_write",
        "allocated_mib": allocated,
        "memory_high_mib": 1200,
        "memory_max_mib": tier + 3072,
        "checksum_match": True,
        "max_zram_delta_kib": 1024 * 1024,
        "max_nbd_delta_kib": tier * 1024 if mode == "nbd" else 0,
        "max_disk_delta_kib": 16384,
        "max_scratch_delta_kib": tier * 1024 if mode == "disk-only" else 0,
        "ghost_swap": False,
        "binary_match": "PASS" if mode == "nbd" else "N/A",
    }
    rows.append(row)
with open(path, "w", encoding="utf-8") as target:
    for row in rows:
        target.write(json.dumps(row, sort_keys=True, separators=(",", ":")) + "\n")
PY
}

assert_tier_summary() {
  local path=$1 mode=$2 tier=$3
  python3 - "$path" "$mode" "$tier" <<'PY'
import json
import sys

path, mode, tier_text = sys.argv[1:]
tier = int(tier_text)
with open(path, encoding="utf-8") as source:
    summary = json.load(source)
assert summary["status"] == "PASS", summary
assert summary["tier_mib"] == tier, summary
assert summary["allocated_mib"] == tier + 2560, summary
assert len(summary["samples"]) == 3, summary
for row in summary["samples"]:
    assert row["tier_mib"] == tier, row
    assert row["allocated_mib"] == tier + 2560, row
    assert row["max_zram_delta_kib"] >= 1024 * 1024 - 8192, row
    if mode == "nbd":
        assert row["max_nbd_delta_kib"] >= tier * 1024 - 8192, row
        assert row["binary_match"] == "PASS", row
    else:
        assert row["max_nbd_delta_kib"] == 0, row
        assert row["max_scratch_delta_kib"] >= tier * 1024 - 8192, row
        assert row["binary_match"] == "N/A", row
PY
}

test_aggregate_size_occupancy_contract() {
  local tier mode samples summary fixed
  for tier in 1024 2048 4096; do
    for mode in nbd disk-only; do
      samples="$TMP/$mode-$tier-samples.jsonl"
      summary="$TMP/$mode-$tier-summary.json"
      write_tier_samples "$samples" "$mode" "$tier"
      "$CELL" --aggregate --samples "$samples" --out "$summary" \
        --mode "$mode" --condition idle --tier-mib "$tier"
      assert_tier_summary "$summary" "$mode" "$tier"
    done
  done

  for tier in 2048 4096; do
    fixed="$TMP/nbd-$tier-fixed-3584.jsonl"
    write_tier_samples "$fixed" nbd "$tier"
    python3 - "$fixed" <<'PY'
import json
import sys

path = sys.argv[1]
rows = []
with open(path, encoding="utf-8") as source:
    for line in source:
        row = json.loads(line)
        row["allocated_mib"] = 3584
        rows.append(row)
with open(path, "w", encoding="utf-8") as target:
    for row in rows:
        target.write(json.dumps(row, sort_keys=True, separators=(",", ":")) + "\n")
PY
    if "$CELL" --aggregate --samples "$fixed" --out "$TMP/nbd-$tier-fixed-3584-summary.json" \
      --mode nbd --condition idle --tier-mib "$tier" >/dev/null 2>&1; then
      printf 'FAIL aggregate accepted fixed allocated_mib=3584 for tier=%s\n' "$tier" >&2
      exit 1
    fi
  done

  pass benchmark_start_barrier_and_size_occupancy_contract
}

test_aggregate_size_occupancy_contract

test_tier_derived_timeout_budget_contract() {
  local tier expected_sample expected_outer samples summary wrong_sample
  for tier in 1024 2048 4096; do
    case $tier in
      1024|2048) expected_sample=120; expected_outer=900 ;;
      4096) expected_sample=600; expected_outer=2100 ;;
      *) echo "FAIL unsupported manufactured timeout tier" >&2; exit 1 ;;
    esac
    samples="$TMP/timeout-$tier-samples.jsonl"
    summary="$TMP/timeout-$tier-summary.json"
    write_tier_samples "$samples" nbd "$tier"
    "$CELL" --aggregate --samples "$samples" --out "$summary" \
      --mode nbd --condition idle --tier-mib "$tier" --sample-timeout-sec "$expected_sample"
    python3 - "$summary" "$expected_sample" "$expected_outer" <<'PY'
import json
import sys

path, sample_text, outer_text = sys.argv[1:]
with open(path, encoding="utf-8") as source:
    summary = json.load(source)
budget = summary["timeout_budget"]
assert budget == {
    "sample_timeout_sec": int(sample_text),
    "samples": 3,
    "setup_cleanup_timeout_sec": 300,
    "cell_outer_timeout_sec": int(outer_text),
}, summary
PY
    wrong_sample=600
    [[ $expected_sample == 600 ]] && wrong_sample=120
    if "$CELL" --aggregate --samples "$samples" --out "$TMP/timeout-$tier-wrong.json" \
      --mode nbd --condition idle --tier-mib "$tier" --sample-timeout-sec "$wrong_sample" >/dev/null 2>&1; then
      printf 'FAIL aggregate accepted mismatched timeout tier=%s sample=%s\n' "$tier" "$wrong_sample" >&2
      exit 1
    fi
  done
  grep -Fq 'SAMPLE_TIMEOUT_MAX_SEC=600' "$CELL"
  grep -Fq 'CELL_SETUP_CLEANUP_TIMEOUT_SEC=300' "$CELL"
  grep -Fq 'CELL_OUTER_TIMEOUT_MIN_SEC=900' "$CELL"
  grep -Fq 'CELL_OUTER_TIMEOUT_MAX_SEC=2100' "$CELL"
  pass tier_derived_timeout_budget_is_bounded_and_refuses_mismatch
}

test_tier_derived_timeout_budget_contract

evidence_dir="$TMP/evidence"
mkdir -p "$evidence_dir"
python3 - "$evidence_dir" <<'PY'
import hashlib
import json
import os
import sys

root = sys.argv[1]
contents = {
    "before.txt": "before\n",
    "action.txt": "action\n",
    "after.txt": "after\n",
    "context.json": "{}\n",
    "samples.jsonl": "{}\n",
    "summary.json": "{}\n",
}
for name, value in contents.items():
    with open(os.path.join(root, name), "w", encoding="utf-8") as target:
        target.write(value)
rows = []
for name in sorted(contents):
    path = os.path.join(root, name)
    with open(path, "rb") as source:
        data = source.read()
    rows.append({"name": name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()})
inventory = {"schema": 2, "files": rows}
inventory_path = os.path.join(root, "artifact-inventory.json")
with open(inventory_path, "w", encoding="utf-8") as target:
    json.dump(inventory, target, sort_keys=True, separators=(",", ":"))
    target.write("\n")
def digest(name):
    with open(os.path.join(root, name), "rb") as source:
        return hashlib.sha256(source.read()).hexdigest()
timeout_budget = {
    "sample_timeout_sec": 120,
    "samples": 3,
    "setup_cleanup_timeout_sec": 300,
    "cell_outer_timeout_sec": 900,
}
context = {
    "schema": 2,
    "pair_id": "manufactured-pair",
    "mode": "disk-only",
    "binary_match": "N/A",
    "watchdog": {"armed": True, "outcome": "not_fired"},
    "timeout_budget": timeout_budget,
    "release": {
        "version": "manufactured-v1",
        "source_commit": "a" * 40,
        "manifest_sha256": "b" * 64,
        "input_bundle_manifest_sha256": "c" * 64,
    },
}
with open(os.path.join(root, "context.json"), "w", encoding="utf-8") as target:
    json.dump(context, target, sort_keys=True, separators=(",", ":"))
    target.write("\n")
with open(os.path.join(root, "summary.json"), "w", encoding="utf-8") as target:
    json.dump({"timeout_budget": timeout_budget}, target, sort_keys=True, separators=(",", ":"))
    target.write("\n")
for row in rows:
    path = os.path.join(root, row["name"])
    with open(path, "rb") as source:
        data = source.read()
    row["bytes"] = len(data)
    row["sha256"] = hashlib.sha256(data).hexdigest()
with open(inventory_path, "w", encoding="utf-8") as target:
    json.dump({"schema": 2, "files": rows}, target, sort_keys=True, separators=(",", ":"))
    target.write("\n")
envelope = {
    "schema_version": "ramshared-nbd-cell-evidence/v1",
    "pair_id": "manufactured-pair",
    "mode": "disk-only",
    "release": {
        "version": "manufactured-v1",
        "source_commit": "a" * 40,
        "manifest_sha256": "b" * 64,
        "input_bundle_manifest_sha256": "c" * 64,
    },
    "context_sha256": digest("context.json"),
    "summary_sha256": digest("summary.json"),
    "artifact_inventory_sha256": digest("artifact-inventory.json"),
    "artifacts": [{"path": row["name"], "bytes": row["bytes"], "sha256": row["sha256"]} for row in rows],
    "binary_match": "N/A",
    "watchdog": {"armed": True, "outcome": "not_fired"},
    "timeout_budget": timeout_budget,
    "classification": "INCOMPARABLE",
}
with open(os.path.join(root, "evidence-envelope.json"), "w", encoding="utf-8") as target:
    json.dump(envelope, target, sort_keys=True, separators=(",", ":"))
    target.write("\n")
PY
"$CELL" --validate-evidence --artifact-dir "$evidence_dir"
pass evidence_inventory_hashes_and_internal_cell_envelope_are_validated
pass cell_custody_envelope_is_internal_and_nonpromotable
sed -i 's/ramshared-nbd-cell-evidence\/v1/ramshared-evidence\/v1/' "$evidence_dir/evidence-envelope.json"
if "$CELL" --validate-evidence --artifact-dir "$evidence_dir" >/dev/null 2>&1; then
  echo 'FAIL evidence validator accepted a public envelope for one cell' >&2
  exit 1
fi
sed -i 's/ramshared-evidence\/v1/ramshared-nbd-cell-evidence\/v1/' "$evidence_dir/evidence-envelope.json"
printf 'tamper\n' >>"$evidence_dir/after.txt"
if "$CELL" --validate-evidence --artifact-dir "$evidence_dir" >/dev/null 2>&1; then
  echo 'FAIL evidence validator accepted a tampered inventory artifact' >&2
  exit 1
fi

set +e
env -u RAMSHARED_SHARED_HOST_APPROVAL -u RAMSHARED_WINDOWS_WATCHDOG_ARMED \
  "$CELL" --run --mode disk-only --condition idle --tier-mib 1024 \
  --artifact-dir "$TMP/refused" >/dev/null 2>&1
rc=$?
set -e
[[ $rc -ne 0 ]] || { echo 'FAIL live action accepted without approval' >&2; exit 1; }

set +e
seam_output=$(RAMSHARED_PRODUCT_ROOT="$TMP/fake-product" \
  "$CELL" --run --mode disk-only --condition idle --tier-mib 1024 \
  --artifact-dir "$TMP/seam-refused" 2>&1)
rc=$?
set -e
[[ $rc -ne 0 && $seam_output == *'NBD_BENCHMARK_REASON=LIVE_TEST_SEAM_FORBIDDEN'* ]] || {
  echo 'FAIL approved live mode did not reject a fixture seam before action' >&2
  exit 1
}
set +e
seam_output=$(RAMSHARED_NBD_LOWER_SINK=/var/lib/ramshared/nbd \
  "$CELL" --run --mode disk-only --condition idle --tier-mib 1024 \
  --artifact-dir "$TMP/canonical-seam-refused" 2>&1)
rc=$?
set -e
[[ $rc -ne 0 && $seam_output == *'NBD_BENCHMARK_REASON=LIVE_TEST_SEAM_FORBIDDEN'* ]] || {
  echo 'FAIL approved live mode accepted a canonical-valued fixture override' >&2
  exit 1
}

# A reviewed controller must bind one immutable release, not let the runner
# rediscover `current` after approval. This invocation cannot reach a host
# action: it deliberately omits the required reviewed-release binding.
set +e
binding_output=$(env \
  RAMSHARED_SHARED_HOST_APPROVAL=I_ACCEPT_BOUNDED_SHARED_HOST_PRESSURE \
  RAMSHARED_WINDOWS_WATCHDOG_ARMED=1 \
  RAMSHARED_NBD_BENCHMARK_APPROVAL='benchmark:unbound:1024:idle:disk-only' \
  "$CELL" --run --mode disk-only --condition idle --tier-mib 1024 \
  --artifact-dir "$TMP/binding-refused" 2>&1)
rc=$?
set -e
[[ $rc -ne 0 && $binding_output == *'NBD_BENCHMARK_REASON=REVIEWED_RELEASE_BINDING_REQUIRED'* ]] || {
  echo 'FAIL live action did not require reviewed release binding before approval/action' >&2
  exit 1
}

set +e
old_approval_output=$(env RAMSHARED_SHARED_HOST_APPROVAL=I_ACCEPT_WSL_TERMINATION \
  "$CELL" --assert-shared-host-approval 2>&1)
rc=$?
set -e
[[ $rc -ne 0 && $old_approval_output == *'NBD_BENCHMARK_REASON=SHARED_HOST_APPROVAL_MISSING'* ]] || {
  echo 'FAIL legacy shared-host approval token was accepted' >&2
  exit 1
}
new_approval_output=$(env RAMSHARED_SHARED_HOST_APPROVAL=I_ACCEPT_BOUNDED_SHARED_HOST_PRESSURE \
  "$CELL" --assert-shared-host-approval)
[[ $new_approval_output == *'NBD_BENCHMARK_STATE=APPROVED'* ]] || {
  echo 'FAIL bounded shared-host approval token was not accepted' >&2
  exit 1
}
pass shared_host_approval_token_is_exact

for forbidden in 'shutdown.exe' 'Restart-Computer' 'Stop-Computer' 'rmmod' 'modprobe -r'; do
  ! grep -Fq -- "$forbidden" "$CELL" || {
    printf 'FAIL forbidden product action: %s\n' "$forbidden" >&2
    exit 1
  }
done
grep -Fq 'PRODUCT_OFF' "$CELL"
grep -Fq 'BINARY_MATCH' "$CELL"
grep -Fq 'BASELINE_REPUBLICATION_FAILED' "$CELL"
grep -Fq 'managed_swap_count' "$CELL"
grep -Fq 'O_CREAT | os.O_EXCL | os.O_NOFOLLOW' "$CELL"
grep -Fq 'SCRATCH_IDENTITY' "$CELL"
grep -Fq '"$swapoff_command" -- "$path"' "$BENCHMARK_LIB"
grep -Fq 'SCRATCH_SWAP_ACTIVE' "$CELL"
grep -Fq 'CGROUP_START_BARRIER' "$CELL"
grep -Fq 'create_zram_control' "$CELL"
grep -Fq 'LIVE_TEST_SEAM_FORBIDDEN' "$CELL"
grep -Fq 'allocation_to_hold_ms' "$CELL"
grep -Fq 'SOURCE_COMMIT' "$CELL"
context_writer_definitions=$(grep -Ec '^write_live_context(_v2)?\(\)[[:space:]]*\{' "$CELL" || true)
[[ $context_writer_definitions == 1 ]] || {
  printf 'FAIL context writer definitions=%s expected=1\n' "$context_writer_definitions" >&2
  exit 1
}
! grep -Eq '^write_live_context\(\)' "$CELL" || {
  printf 'FAIL deprecated schema-1 context writer remains\n' >&2
  exit 1
}
context_writer_calls=$(grep -Exc 'write_live_context_v2' "$CELL" || true)
[[ $context_writer_calls == 1 ]] || {
  printf 'FAIL context writer calls=%s expected=1\n' "$context_writer_calls" >&2
  exit 1
}
context_schema_count=$(awk '/^write_live_context_v2\(\)/,/^}/' "$CELL" | grep -Fc '"schema": 2,' || true)
[[ $context_schema_count == 1 ]] || {
  printf 'FAIL context schema-v2 fields=%s expected=1\n' "$context_schema_count" >&2
  exit 1
}
pass cell_context_writer_is_unique_and_schema_v2
grep -Fq 'write_artifact_inventory' "$CELL"
grep -Fq -- '--sealed-release-root' "$CELL"
grep -Fq -- '--expected-source-commit' "$CELL"
grep -Fq -- '--expected-manifest-sha256' "$CELL"
grep -Fq -- '--pair-id' "$CELL"
grep -Fq 'evidence-envelope.json' "$CELL"
grep -Fq 'ramshared-nbd-cell-evidence/v1' "$CELL"
! grep -Fq '"schema_version": "ramshared-evidence/v1"' "$CELL"

mkdir -p "$TMP/fake-cgroup"
: >"$TMP/fake-cgroup/cgroup.procs"
cat >"$TMP/fixture-worker.sh" <<'EOF'
#!/usr/bin/env bash
printf 'started\n' >"$1"
EOF
chmod 0700 "$TMP/fixture-worker.sh"
"$CGROUP_LAUNCH" "$TMP/fake-cgroup" "$TMP/cgroup-ready" "$TMP/cgroup-go" \
  "$TMP/fixture-worker.sh" "$TMP/worker-started" &
launcher_pid=$!
for _ in $(seq 1 100); do
  [[ -f $TMP/cgroup-ready ]] && break
  sleep 0.01
done
[[ -f $TMP/cgroup-ready && ! -e $TMP/worker-started ]] || {
  echo 'FAIL cgroup launcher crossed start barrier early' >&2
  exit 1
}
[[ $(<"$TMP/fake-cgroup/cgroup.procs") == "$launcher_pid" ]] || {
  echo 'FAIL cgroup launcher did not publish its exact PID first' >&2
  exit 1
}
( set -o noclobber; : >"$TMP/cgroup-go" )
wait "$launcher_pid"
[[ $(<"$TMP/worker-started") == started ]] || {
  echo 'FAIL cgroup launcher did not exec after start receipt' >&2
  exit 1
}

scratch="$TMP/scratch.swap"
: >"$scratch"
chmod 0600 "$scratch"
scratch_identity=$(nbd_scratch_identity "$scratch")
[[ $scratch_identity == *':600:8180' ]] || {
  echo 'FAIL empty scratch identity is not stable numeric regular-file metadata' >&2
  exit 1
}
truncate -s 8192 "$scratch"
[[ $(nbd_scratch_identity "$scratch") == "$scratch_identity" ]] || {
  echo 'FAIL allocated scratch changed stable identity' >&2
  exit 1
}
ln -s "$scratch" "$TMP/scratch-link"
! nbd_scratch_identity "$TMP/scratch-link" >/dev/null
! nbd_scratch_matches "$scratch" "0:0:0:0:600:8180"
printf 'Filename Type Size Used Priority\n%s file 1024 0 100\n' "$scratch" >"$TMP/swaps"
cat >"$TMP/swapoff-fail.sh" <<'EOF'
#!/usr/bin/env bash
exit 9
EOF
chmod 0700 "$TMP/swapoff-fail.sh"
if nbd_cleanup_scratch "$scratch" "$scratch_identity" "$TMP/swaps" "$TMP/swapoff-fail.sh"; then
  echo 'FAIL manufactured swapoff refusal passed cleanup' >&2
  exit 1
fi
[[ -f $scratch ]] || { echo 'FAIL refused swapoff removed scratch' >&2; exit 1; }
cat >"$TMP/swapoff-pass.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[[ $1 == -- && $2 == "$SCRATCH_FIXTURE" ]]
printf 'Filename Type Size Used Priority\n' >"$SWAPS_FIXTURE"
EOF
chmod 0700 "$TMP/swapoff-pass.sh"
SCRATCH_FIXTURE="$scratch" SWAPS_FIXTURE="$TMP/swaps" \
  nbd_cleanup_scratch "$scratch" "$scratch_identity" "$TMP/swaps" "$TMP/swapoff-pass.sh"
[[ ! -e $scratch ]] || { echo 'FAIL exact successful cleanup retained scratch' >&2; exit 1; }
scratch="$TMP/disk-control.swap"
: >"$scratch"
cat >"$TMP/disk-control-swaps" <<EOF
Filename Type Size Used Priority
/dev/sdc partition 4194304 1024 -2
/dev/zram0 partition 1048572 0 200
$scratch file 8388604 0 100
EOF
nbd_disk_control_topology_exact "$TMP/disk-control-swaps" "$scratch"
for bad_row in "/dev/zram1 partition 1048572 0 200" "/dev/nbd0 partition 1048572 0 100" "/dev/ublkb0 partition 1048572 0 100" "/dev/zram0 partition 1048572 0 199" "$scratch file 8388604 0 99" "$scratch file 8388604 0 100 extra" "$scratch (deleted) file 8388604 0 100"; do
  cp -- "$TMP/disk-control-swaps" "$TMP/disk-control-invalid"
  printf '%s\n' "$bad_row" >>"$TMP/disk-control-invalid"
  if nbd_disk_control_topology_exact "$TMP/disk-control-invalid" "$scratch"; then
    echo 'FAIL invalid disk control topology was accepted' >&2
    exit 1
  fi
done
cat >"$TMP/swapoff-republish.py" <<'PY'
#!/usr/bin/env python3
import os, sys
path = sys.argv[-1]
swaps = os.environ["SWAPS_FIXTURE"]
with open(swaps, encoding="utf-8") as source:
    lines = source.readlines()
with open(swaps, "w", encoding="utf-8") as target:
    target.writelines(line for line in lines if not line.startswith(path + " "))
with open(os.environ["ORDER_FIXTURE"], "a", encoding="utf-8") as target:
    target.write("off:" + path + "\n")
PY
cat >"$TMP/swapon-republish.py" <<'PY'
#!/usr/bin/env python3
import os, sys
priority = sys.argv[sys.argv.index("-p") + 1]
path = sys.argv[-1]
kind = "partition" if path.startswith("/dev/zram") else "file"
with open(os.environ["SWAPS_FIXTURE"], "a", encoding="utf-8") as target:
    target.write(f"{path} {kind} 1048572 0 {priority}\n")
with open(os.environ["ORDER_FIXTURE"], "a", encoding="utf-8") as target:
    target.write("on:" + priority + ":" + path + "\n")
PY
chmod 0700 "$TMP/swapoff-republish.py" "$TMP/swapon-republish.py"
cp -- "$TMP/disk-control-swaps" "$TMP/republish-swaps"
: >"$TMP/republish-order"
SWAPS_FIXTURE="$TMP/republish-swaps" ORDER_FIXTURE="$TMP/republish-order" \
nbd_republish_swap_pair "$TMP/republish-swaps" /dev/zram0 "$scratch" file \
    "$TMP/swapoff-republish.py" "$TMP/swapon-republish.py"
cat >"$TMP/expected-republish-order" <<EOF
off:$scratch
off:/dev/zram0
on:200:/dev/zram0
on:100:$scratch
EOF
cmp -s "$TMP/expected-republish-order" "$TMP/republish-order" || {
  echo 'FAIL sample baseline republication order changed' >&2
  exit 1
}

test_nbd_sample_connected_republication_contract() {
  local reconnect_swaps reconnect_order expected_order
  local swapoff_cmd mkswap_cmd swapon_cmd output rc

  swapoff_cmd="$TMP/swapoff-reconnect.py"
  mkswap_cmd="$TMP/mkswap-reconnect.py"
  swapon_cmd="$TMP/swapon-reconnect.py"
  cat >"$swapoff_cmd" <<'PY'
#!/usr/bin/env python3
import os
import sys

if tuple(sys.argv[1:]) not in (("--", "/dev/nbd0"), ("--", "/dev/zram0")):
    raise SystemExit("unexpected_swapoff_argv")
path = sys.argv[-1]
if os.environ.get("RECONNECT_FORGED_REASON") == "1":
    print("NBD_REPUBLICATION_REASON=FORGED_BY_COMMAND", file=sys.stderr)
if os.environ.get("RECONNECT_FAIL_STAGE") == "swapoff:" + path:
    raise SystemExit(31)
swaps = os.environ["SWAPS_FIXTURE"]
if os.environ.get("RECONNECT_FAIL_STAGE") == "swapoff-no-remove:" + path:
    with open(os.environ["ORDER_FIXTURE"], "a", encoding="utf-8") as target:
        target.write("off-no-remove:" + path + "\n")
    raise SystemExit(0)
with open(swaps, encoding="utf-8") as source:
    lines = source.readlines()
with open(swaps, "w", encoding="utf-8") as target:
    target.writelines(line for line in lines if not line.startswith(path + " "))
with open(os.environ["ORDER_FIXTURE"], "a", encoding="utf-8") as target:
    target.write("off:" + path + "\n")
PY
  cat >"$mkswap_cmd" <<'PY'
#!/usr/bin/env python3
import os
import sys

if sys.argv[1:] != ["-L", "RAMSHARED", "--", "/dev/nbd0"]:
    raise SystemExit("unexpected_mkswap_argv")
if os.environ.get("RECONNECT_FORGED_REASON") == "1":
    print("NBD_REPUBLICATION_REASON=FORGED_BY_COMMAND", file=sys.stderr)
if os.environ.get("RECONNECT_FAIL_STAGE") == "mkswap":
    raise SystemExit(33)
with open(os.environ["ORDER_FIXTURE"], "a", encoding="utf-8") as target:
    target.write("mkswap:-L:RAMSHARED:/dev/nbd0\n")
PY
  cat >"$swapon_cmd" <<'PY'
#!/usr/bin/env python3
import os
import sys

args = sys.argv[1:]
expected = {
    ("-p", "200", "--", "/dev/zram0"),
    ("-p", "100", "--", "/dev/nbd0"),
}
if tuple(args) not in expected:
    raise SystemExit("unexpected_swapon_argv")
priority, path = args[1], args[-1]
if os.environ.get("RECONNECT_FAIL_STAGE") == "swapon:" + priority:
    raise SystemExit(34)
with open(os.environ["SWAPS_FIXTURE"], "a", encoding="utf-8") as target:
    target.write(f"{path} partition 1048572 0 {priority}\n")
    if os.environ.get("RECONNECT_TOPOLOGY_DRIFT") == "1" and path == "/dev/nbd0":
        target.write("/dev/nbd9 partition 1048572 0 100\n")
with open(os.environ["ORDER_FIXTURE"], "a", encoding="utf-8") as target:
    target.write("on:" + priority + ":" + path + "\n")
PY
  chmod 0700 "$swapoff_cmd" "$mkswap_cmd" "$swapon_cmd"

  reconnect_swaps="$TMP/reconnect-swaps"
  reconnect_order="$TMP/reconnect-order"
  cp -- "$TMP/disk-control-swaps" "$reconnect_swaps"
  sed -i "s|$scratch file 8388604 0 100|/dev/nbd0 partition 1048572 0 100|" "$reconnect_swaps"
  : >"$reconnect_order"
  SWAPS_FIXTURE="$reconnect_swaps" ORDER_FIXTURE="$reconnect_order" \
    nbd_preserved_connection_republish_swap_pair "$reconnect_swaps" /dev/zram0 /dev/nbd0 \
      "$swapoff_cmd" "$mkswap_cmd" "$swapon_cmd"
  nbd_swap_pair_topology_exact "$reconnect_swaps" /dev/zram0 /dev/nbd0 partition || {
    echo 'FAIL NBD reconnect did not restore the exact zram/NBD pair' >&2
    exit 1
  }
  expected_order="$TMP/expected-reconnect-order"
  cat >"$expected_order" <<EOF
off:/dev/nbd0
off:/dev/zram0
mkswap:-L:RAMSHARED:/dev/nbd0
on:200:/dev/zram0
on:100:/dev/nbd0
EOF
  cmp -s "$expected_order" "$reconnect_order" || {
    echo 'FAIL NBD reconnect transaction order changed' >&2
    exit 1
  }

  assert_reconnect_refusal() {
    local name=$1 expected_reason=$2 fail_stage=${3:-} topology_drift=${4:-0}
    local pre_topology_drift=${5:-0} forged_reason=${6:-0} parsed_reason
    cp -- "$TMP/disk-control-swaps" "$reconnect_swaps"
    sed -i "s|$scratch file 8388604 0 100|/dev/nbd0 partition 1048572 0 100|" "$reconnect_swaps"
    if [[ $pre_topology_drift == 1 ]]; then
      printf '/dev/nbd9 partition 1048572 0 100\n' >>"$reconnect_swaps"
    fi
    : >"$reconnect_order"
    set +e
    output=$(SWAPS_FIXTURE="$reconnect_swaps" ORDER_FIXTURE="$reconnect_order" \
      RECONNECT_FAIL_STAGE="$fail_stage" \
      RECONNECT_TOPOLOGY_DRIFT="$topology_drift" \
      RECONNECT_FORGED_REASON="$forged_reason" \
      nbd_preserved_connection_republish_swap_pair "$reconnect_swaps" /dev/zram0 /dev/nbd0 \
        "$swapoff_cmd" "$mkswap_cmd" "$swapon_cmd" 2>&1)
    rc=$?
    set -e
    [[ $rc -ne 0 ]] || {
      printf 'FAIL %s accepted an invalid reconnect transaction: %s\n' "$name" "$output" >&2
      exit 1
    }
    parsed_reason=$(nbd_republication_reason_from_output "$output")
    [[ $parsed_reason == "$expected_reason" && $parsed_reason != *$'\n'* ]] || {
      printf 'FAIL %s did not collapse to exact reason %s: raw=%s parsed=%s\n' \
        "$name" "$expected_reason" "$output" "$parsed_reason" >&2
      exit 1
    }
    ! grep -Eq 'attach:|detach:' "$reconnect_order" || {
      printf 'FAIL %s performed an NBD attach or detach\n' "$name" >&2
      exit 1
    }
  }

  assert_reconnect_refusal pre_topology_invalid NBD_REPUBLICATION_PRE_TOPOLOGY_INVALID '' 0 1
  assert_reconnect_refusal nbd_swapoff_failure NBD_REPUBLICATION_SWAPOFF_NBD_FAILED swapoff:/dev/nbd0
  ! grep -Fq 'off:/dev/zram0' "$reconnect_order" || {
    echo 'FAIL NBD swapoff refusal advanced to zram swapoff' >&2
    exit 1
  }
  assert_reconnect_refusal zram_swapoff_failure NBD_REPUBLICATION_SWAPOFF_ZRAM_FAILED swapoff:/dev/zram0
  assert_reconnect_refusal swap_absence_failure NBD_REPUBLICATION_SWAP_ABSENCE_FAILED swapoff-no-remove:/dev/zram0
  assert_reconnect_refusal mkswap_failure NBD_REPUBLICATION_MKSWAP_FAILED mkswap
  ! grep -Fq 'on:' "$reconnect_order" || {
    echo 'FAIL mkswap refusal advanced to swapon' >&2
    exit 1
  }
  assert_reconnect_refusal zram_swapon_failure NBD_REPUBLICATION_ZRAM_SWAPON_FAILED swapon:200
  ! grep -Fq 'on:100:/dev/nbd0' "$reconnect_order" || {
    echo 'FAIL zram swapon refusal advanced to NBD swapon' >&2
    exit 1
  }
  assert_reconnect_refusal nbd_swapon_failure NBD_REPUBLICATION_NBD_SWAPON_FAILED swapon:100
  assert_reconnect_refusal post_publish_topology_drift NBD_REPUBLICATION_POST_TOPOLOGY_INVALID '' 1
  grep -Fqx 'on:100:/dev/nbd0' "$reconnect_order" || {
    echo 'FAIL topology refusal did not reach the post-publish verifier' >&2
    exit 1
  }
  assert_reconnect_refusal forged_child_reason NBD_REPUBLICATION_TRANSACTION_FAILED mkswap 0 0 1

  ! grep -Eq 'attach:|detach:' "$reconnect_order" || {
    echo 'FAIL connected NBD republication performed an attach or detach' >&2
    exit 1
  }
  if awk '
    /^nbd_preserved_connection_republish_swap_pair\(\)/ { body = 1 }
    body { print }
    body && /^}/ { exit }
  ' "$BENCHMARK_LIB" | grep -Eq 'nbd-client|attach_command|detach'; then
    echo 'FAIL connected NBD republication contains an attach/detach path' >&2
    exit 1
  fi
  grep -Fq 'original_nbd_identity=$NBD_SECOND_TIER_IDENTITY_SHA256' "$CELL"
  grep -Fq 'SAMPLE_ZRAM_DEVICE=$(capture_sample_zram_device)' "$CELL"
  grep -Fq 'local zdev=$SAMPLE_ZRAM_DEVICE' "$CELL"
  if awk '
    /^republish_sample_baseline\(\)/ { body = 1 }
    body { print }
    body && /^}/ { exit }
  ' "$CELL" | grep -Fq 'ZRAM_RECORD'; then
    echo 'FAIL sample republication still trusts the mutable zram runtime receipt' >&2
    exit 1
  fi
  grep -Fq '[[ $NBD_SECOND_TIER_IDENTITY_SHA256 == "$original_nbd_identity" ]]' "$CELL"
  grep -Fq "pinned_preflight | grep -q '^NBD_BINARY_MATCH=PASS$'" "$CELL"
  grep -Fq 'refuse "$SAMPLE_BASELINE_REASON"' "$CELL"
  pass nbd_sample_preserves_connected_device_without_reattach
}

test_nbd_sample_connected_republication_contract

test_sample_zram_identity_requires_exact_initial_pair_before_worker() {
  local swaps="$TMP/initial-pair-swaps" capture_line pair_line context_line cgroup_line extra
  printf 'Filename Type Size Used Priority\n/dev/zram0 partition 1048572 0 200\n/dev/nbd0 partition 1048572 0 100\n' >"$swaps"
  nbd_swap_pair_topology_exact "$swaps" /dev/zram0 /dev/nbd0 partition || {
    echo 'FAIL legitimate initial zram/NBD pair was refused' >&2
    exit 1
  }

  for extra in \
    '/dev/zram9 partition 1048572 0 200' \
    '/dev/zram9 partition 1048572 0 100' \
    '/dev/zram9 file 1048572 0 200' \
    '/dev/zram9 partition 1048572 0 200 extra' \
    '/dev/nbd9 partition 1048572 0 100' \
    '/dev/ublkb9 partition 1048572 0 100'; do
    printf 'Filename Type Size Used Priority\n/dev/zram0 partition 1048572 0 200\n/dev/nbd0 partition 1048572 0 100\n%s\n' \
      "$extra" >"$swaps"
    if nbd_swap_pair_topology_exact "$swaps" /dev/zram0 /dev/nbd0 partition; then
      printf 'FAIL invalid initial pair row was accepted: %s\n' "$extra" >&2
      exit 1
    fi
  done

  capture_line=$(grep -nF 'SAMPLE_ZRAM_DEVICE=$(capture_sample_zram_device)' "$CELL" | cut -d: -f1)
  pair_line=$(grep -nF 'nbd_swap_pair_topology_exact "$SWAPS_FILE" "$SAMPLE_ZRAM_DEVICE" "$NBD_DEVICE" partition' "$CELL" | cut -d: -f1)
  context_line=$(grep -nF 'write_live_context_v2' "$CELL" | tail -1 | cut -d: -f1)
  cgroup_line=$(grep -nF 'mkdir -- "$CG"' "$CELL" | cut -d: -f1)
  [[ $capture_line =~ ^[0-9]+$ && $pair_line =~ ^[0-9]+$ && $context_line =~ ^[0-9]+$ && $cgroup_line =~ ^[0-9]+$ \
    && $capture_line -lt $pair_line && $pair_line -lt $context_line && $context_line -lt $cgroup_line ]] || {
    echo 'FAIL exact initial pair is not validated after capture and before context/cgroup/worker' >&2
    exit 1
  }
  pass sample_zram_identity_requires_exact_initial_pair_before_worker
}

test_sample_zram_identity_requires_exact_initial_pair_before_worker

reconnect_calls=$(grep -Fc 'transaction_output=$(nbd_preserved_connection_republish_swap_pair' "$CELL" || true)
disk_republish_calls=$(grep -Fc 'nbd_republish_swap_pair "$SWAPS_FILE" "$zdev" "$lower" "$lower_type"' "$CELL" || true)
[[ $reconnect_calls == 1 && $disk_republish_calls == 1 ]] || {
  echo 'FAIL NBD and disk sample republication paths are not both explicit' >&2
  exit 1
}

for extra in "/dev/zram9 partition 1048572 0 200" "/dev/nbd9 partition 1048572 0 100" "/dev/ublkb9 partition 1048572 0 100"; do
  cp -- "$TMP/disk-control-swaps" "$TMP/republish-extra"
  printf '%s\n' "$extra" >>"$TMP/republish-extra"
  if nbd_swap_pair_topology_exact "$TMP/republish-extra" /dev/zram0 "$scratch" file; then
    echo 'FAIL sample republication accepted extra managed swap' >&2
    exit 1
  fi
done
pass disk_control_scratch_is_exclusive_identity_bound_and_swapoff_first
pass scratch_identity_is_stable_for_empty_and_allocated_regular_file
pass disk_control_accepts_fresh_zero_used_zram_with_exact_topology
grep -Fq 'MEMORY_HIGH_MIB=1200' "$ROOT/scripts/safety/nbd-benchmark-cell.sh"
grep -Fq 'MEMORY_MAX_MIB=$((ALLOCATE_MIB + 512))' "$ROOT/scripts/safety/nbd-benchmark-cell.sh"
grep -Fq 'MEMORY_HIGH_MIB * 1024 * 1024)) >"$CG/memory.high"' "$ROOT/scripts/safety/nbd-benchmark-cell.sh"
grep -Fq 'MEMORY_MAX_MIB * 1024 * 1024)) >"$CG/memory.max"' "$ROOT/scripts/safety/nbd-benchmark-cell.sh"
pass cgroup_high_forces_reclaim_without_hard_limit_oom
pass sample_baseline_republication_is_exact_and_ordered
grep -Fq 'run-$run-activity.json' "$ROOT/scripts/safety/nbd-benchmark-cell.sh"
grep -Fq '"required_delta_kib"' "$ROOT/scripts/safety/nbd-benchmark-cell.sh"
pass activity_refusal_persists_exact_observed_deltas
pass benchmark_cleanup_refuses_ghost_or_residual_swap
pass benchmark_start_barrier_launcher_is_in_cgroup_before_exec
pass benchmark_live_seams_are_unavailable_in_approved_mode
pass reviewed_release_binding_and_evidence_custody_refuse_drift

test_failure_receipt_contract() {
  local token receipt_dir receipt symlink_target first_reason second_reason recorded
  for token in \
    'nbd_write_failure_receipt' \
    'nbd_failure_receipt_allowed' \
    'failure-receipt.json' \
    'ramshared-nbd-cell-failure/v1' \
    'FAILURE_REASON' \
    'release_version' \
    'pair_id' \
    'terminal_state' \
    'PRODUCT_OFF'; do
    if ! grep -Fq -- "$token" "$CELL" && ! grep -Fq -- "$token" "$BENCHMARK_LIB"; then
      printf 'FAIL failure_receipt_is_sanitized_and_requires_verified_product_off: missing %s\n' \
        "$token" >&2
      exit 1
    fi
  done
  receipt_dir="$TMP/failure-receipt"
  mkdir -p "$receipt_dir"
  receipt="$receipt_dir/failure-receipt.json"
  first_reason=FIRST_FAILURE
  second_reason=SECOND_FAILURE
  recorded=""
  recorded=$(nbd_first_failure_reason "$recorded" "$first_reason")
  recorded=$(nbd_first_failure_reason "$recorded" "$second_reason")
  [[ $recorded == "$first_reason" ]] || {
    printf 'FAIL failure receipt replaced the first stable reason: %s\n' "$recorded" >&2
    exit 1
  }
  if nbd_failure_receipt_allowed 2 0 PRODUCT_OFF 0 0; then
    echo 'FAIL failure receipt gate accepted cleanup failure' >&2
    exit 1
  fi
  nbd_failure_receipt_allowed 2 1 PRODUCT_OFF 1 0 || {
    echo 'FAIL failure receipt gate refused exact terminal PRODUCT_OFF proof' >&2
    exit 1
  }
  if nbd_write_failure_receipt "$receipt" PRODUCT_READY "$recorded" release-v1 pair-1 disk-only idle 1024; then
    echo 'FAIL failure receipt accepted a non-PRODUCT_OFF terminal state' >&2
    exit 1
  fi
  [[ ! -e $receipt ]] || { echo 'FAIL failure receipt was written before PRODUCT_OFF' >&2; exit 1; }
  nbd_write_failure_receipt "$receipt" PRODUCT_OFF "$recorded" release-v1 pair-1 disk-only idle 1024
  python3 - "$receipt" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as source:
    record = json.load(source)
expected = {
    "schema_version", "status", "reason", "terminal_state", "release_version",
    "pair_id", "mode", "condition", "tier_mib",
}
assert set(record) == expected, record
assert record["schema_version"] == "ramshared-nbd-cell-failure/v1", record
assert record["status"] == "RED", record
assert record["reason"] == "FIRST_FAILURE", record
assert record["terminal_state"] == "PRODUCT_OFF", record
assert record["release_version"] == "release-v1", record
assert record["pair_id"] == "pair-1", record
assert record["mode"] == "disk-only", record
assert record["condition"] == "idle", record
assert record["tier_mib"] == 1024, record
assert "path" not in record and "pid" not in record, record
assert all("/" not in str(record[name]) and "\\" not in str(record[name]) for name in (
    "reason", "release_version", "pair_id", "mode", "condition"
)), record
PY
  printf 'preexisting receipt\n' >"$receipt"
  if nbd_write_failure_receipt "$receipt" PRODUCT_OFF "$recorded" release-v1 pair-1 disk-only idle 1024; then
    echo 'FAIL failure receipt overwrote a preexisting artifact' >&2
    exit 1
  fi
  [[ $(<"$receipt") == 'preexisting receipt' ]] || {
    echo 'FAIL failure receipt modified a preexisting artifact' >&2
    exit 1
  }
  symlink_target="$receipt_dir/preexisting-target"
  receipt="$receipt_dir/failure-receipt-symlink.json"
  printf 'preexisting symlink target\n' >"$symlink_target"
  ln -s "$(basename -- "$symlink_target")" "$receipt"
  if nbd_write_failure_receipt "$receipt" PRODUCT_OFF "$recorded" release-v1 pair-1 disk-only idle 1024; then
    echo 'FAIL failure receipt accepted a symlink destination' >&2
    exit 1
  fi
  [[ $(<"$symlink_target") == 'preexisting symlink target' ]] || {
    echo 'FAIL failure receipt modified a symlink destination target' >&2
    exit 1
  }
  receipt="$receipt_dir/failure-receipt-race.json"
  if RAMSHARED_NBD_TEST_FAILURE_RECEIPT_RACE=publish-preexisting \
    nbd_write_failure_receipt "$receipt" PRODUCT_OFF FIRST_FAILURE release-v1 \
      pair-1 disk-only idle 1024; then
    echo 'FAIL failure receipt accepted a concurrent destination publication' >&2
    exit 1
  fi
  [[ -f $receipt && ! -L $receipt ]] || {
    echo 'FAIL manufactured concurrent destination was not published' >&2
    exit 1
  }
  [[ $(<"$receipt") == 'concurrent receipt bytes must survive exactly' ]] || {
    echo 'FAIL failure receipt overwrote concurrent destination bytes' >&2
    exit 1
  }
  if compgen -G "$receipt_dir/.failure-receipt.*" >/dev/null; then
    echo 'FAIL failure receipt left a temporary candidate after concurrent publish' >&2
    exit 1
  fi
  pass failure_receipt_is_sanitized_and_requires_verified_product_off
}

test_failure_receipt_terminal_preflight_and_orphan_daemon() {
  local orphan_root orphan_count orphan_receipt preflight_line receipt_gate_line
  preflight_line=$(grep -nF 'terminal_product_off_preflight' "$CELL" | head -n1 | cut -d: -f1 || true)
  receipt_gate_line=$(grep -nF 'nbd_failure_receipt_allowed "$rc"' "$CELL" | head -n1 | cut -d: -f1 || true)
  [[ -n $preflight_line && -n $receipt_gate_line && $preflight_line -lt $receipt_gate_line ]] || {
    echo 'FAIL failure receipt gate does not require terminal pinned preflight first' >&2
    exit 1
  }
  grep -Fq 'NBD_PRODUCT_STATE=PRODUCT_OFF' "$CELL" || {
    echo 'FAIL terminal failure receipt path does not prove PRODUCT_OFF' >&2
    exit 1
  }

  orphan_root="$TMP/failure-receipt-orphan"
  mkdir -p "$orphan_root/proc/4242" "$orphan_root/release/bin"
  printf 'manufactured orphan ramsharedd\n' >"$orphan_root/release/bin/ramsharedd"
  chmod 0700 "$orphan_root/release/bin/ramsharedd"
  ln -s "$orphan_root/release/bin/ramsharedd" "$orphan_root/proc/4242/exe"
  orphan_count=$(nbd_exact_daemon_count "$orphan_root/proc" "$orphan_root/release/bin/ramsharedd")
  [[ $orphan_count == 1 ]] || {
    printf 'FAIL exact orphan daemon fixture was not detected: %s\n' "$orphan_count" >&2
    exit 1
  }

  orphan_receipt="$orphan_root/failure-receipt.json"
  if nbd_failure_receipt_allowed 2 1 PRODUCT_OFF 1 "$orphan_count"; then
    nbd_write_failure_receipt "$orphan_receipt" PRODUCT_OFF ORPHAN_DAEMON release-v1 \
      pair-1 disk-only idle 1024
  fi
  [[ ! -e $orphan_receipt ]] || {
    echo 'FAIL orphan exact daemon without PID produced a failure receipt' >&2
    exit 1
  }
  pass failure_receipt_requires_pinned_terminal_preflight_and_no_orphan_daemon
}

test_failure_receipt_contract
test_failure_receipt_terminal_preflight_and_orphan_daemon
printf 'PASS Test-NbdBenchmarkCell total=%s\n' "$pass_count"
