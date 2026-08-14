#!/usr/bin/env bash
# Identity predicates and narrow injected swap transactions shared by the live
# benchmark cell and manufactured tests.

nbd_first_failure_reason() {
  local current=$1 candidate=$2
  if [[ -n $current ]]; then
    printf '%s\n' "$current"
  else
    printf '%s\n' "$candidate"
  fi
}

nbd_write_failure_receipt() {
  (( $# == 8 )) || return 1
  local output=$1 terminal_state=$2 reason=$3 release_version=$4 pair_id=$5 mode=$6 condition=$7 tier_mib=$8
  [[ $terminal_state == PRODUCT_OFF ]] || return 1
  [[ ! -e $output && ! -L $output ]] || return 1
  [[ $reason =~ ^[A-Z0-9_]{1,96}$ ]] || reason=UNCLASSIFIED_FAILURE
  python3 - "$output" "$reason" "$release_version" "$pair_id" "$mode" "$condition" "$tier_mib" <<'PY'
import json
import os
import sys
import tempfile

out, reason, version, pair_id, mode, condition, tier = sys.argv[1:]
if os.path.lexists(out):
    raise SystemExit("failure_receipt_already_exists")
record = {
    "schema_version": "ramshared-nbd-cell-failure/v1",
    "status": "RED",
    "reason": reason,
    "terminal_state": "PRODUCT_OFF",
    "release_version": version,
    "pair_id": pair_id,
    "mode": mode,
    "condition": condition,
    "tier_mib": int(tier),
}

directory = os.path.dirname(out) or "."
fd, temporary = tempfile.mkstemp(prefix=".failure-receipt.", dir=directory)
publish_refused = False
try:
    with os.fdopen(fd, "w", encoding="utf-8") as target:
        json.dump(record, target, sort_keys=True, separators=(",", ":"))
        target.write("\n")
        target.flush()
        os.fsync(target.fileno())
    if os.environ.get("RAMSHARED_NBD_TEST_FAILURE_RECEIPT_RACE") == "publish-preexisting":
        race_fd = os.open(out, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        try:
            os.write(race_fd, b"concurrent receipt bytes must survive exactly")
            os.fsync(race_fd)
        finally:
            os.close(race_fd)
    os.link(temporary, out, follow_symlinks=False)
except FileExistsError:
    publish_refused = True
except BaseException:
    raise
finally:
    try:
        os.unlink(temporary)
    except FileNotFoundError:
        pass
if publish_refused:
    raise SystemExit(1)
PY
}

nbd_failure_receipt_allowed() {
  (( $# == 5 )) || return 1
  local exit_code=$1 cleanup_ok=$2 terminal_state=$3 terminal_preflight_ok=$4 daemon_count=$5
  (( exit_code != 0 && cleanup_ok == 1 && terminal_preflight_ok == 1 && daemon_count == 0 )) \
    && [[ $terminal_state == PRODUCT_OFF ]]
}

nbd_exact_daemon_count() {
  (( $# == 2 )) || return 1
  local proc_root=$1 daemon=$2 proc_dir pid raw_exe raw_without_deleted resolved count=0
  [[ -d $proc_root && ! -L $proc_root ]] || return 1
  while IFS= read -r proc_dir; do
    [[ -d $proc_dir && ! -L $proc_dir ]] || continue
    pid=${proc_dir##*/}
    [[ $pid =~ ^[1-9][0-9]*$ ]] || continue
    raw_exe=$(readlink -- "$proc_dir/exe" 2>/dev/null || true)
    [[ -n $raw_exe ]] || continue
    raw_without_deleted=${raw_exe% (deleted)}
    resolved=$(readlink -f -- "$proc_dir/exe" 2>/dev/null || true)
    if [[ $raw_without_deleted == "$daemon" || $resolved == "$daemon" ]]; then
      count=$((count + 1))
    fi
  done < <(compgen -G "$proc_root"/'[1-9]*' || true)
  printf '%s\n' "$count"
}

nbd_scratch_identity() {
  local path=$1
  [[ -f $path && ! -L $path ]] || return 1
  stat -c '%d:%i:%u:%g:%a:%f' -- "$path" 2>/dev/null
}

nbd_scratch_matches() {
  local path=$1 expected=$2 current
  current=$(nbd_scratch_identity "$path") || return 1
  [[ $current == "$expected" ]]
}

nbd_swap_exact_count() {
  local swaps_file=$1 target=$2
  awk -v target="$target" 'NR > 1 && $1 == target { found += 1 } END { print found + 0 }' "$swaps_file"
}

nbd_disk_control_topology_exact() {
  local swaps_file=$1 scratch_path=$2
  [[ -r $swaps_file && ! -L $swaps_file ]] || return 1
  awk -v scratch="$scratch_path" '
    NR == 1 { next }
    /\(deleted\)/ { ghost += 1 }
    $1 ~ /^\/dev\/zram[0-9]+$/ && $2 == "partition" && $5 == 200 { zram += 1; next }
    $1 == scratch && $2 == "file" && $5 == 100 { scratch_count += 1; next }
    $1 ~ /^\/dev\/(nbd[0-9]+|ublkb[0-9]+)$/ { managed_foreign += 1; next }
    $1 ~ /^\/dev\/zram[0-9]+$/ || $1 == scratch { invalid += 1 }
    END { exit(zram == 1 && scratch_count == 1 && managed_foreign == 0 && ghost == 0 && invalid == 0 ? 0 : 1) }
  ' "$swaps_file"
}

nbd_swap_pair_topology_exact() {
  local swaps_file=$1 zram_path=$2 lower_path=$3 lower_type=$4
  [[ -r $swaps_file && ! -L $swaps_file && $zram_path =~ ^/dev/zram[0-9]+$ ]] || return 1
  [[ $lower_type == file || $lower_type == partition ]] || return 1
  awk -v zram="$zram_path" -v lower="$lower_path" -v lower_type="$lower_type" '
    NR == 1 { next }
    /\(deleted\)/ { ghost += 1 }
    $1 == zram && $2 == "partition" && $5 == 200 && NF == 5 { zram_count += 1; next }
    $1 == lower && $2 == lower_type && $5 == 100 && NF == 5 { lower_count += 1; next }
    $1 ~ /^\/dev\/(zram[0-9]+|nbd[0-9]+|ublkb[0-9]+)$/ { foreign_managed += 1; next }
    $1 == zram || $1 == lower { invalid += 1 }
    END { exit(zram_count == 1 && lower_count == 1 && foreign_managed == 0 && ghost == 0 && invalid == 0 ? 0 : 1) }
  ' "$swaps_file"
}

nbd_republish_swap_pair() {
  local swaps_file=$1 zram_path=$2 lower_path=$3 lower_type=$4 swapoff_command=$5 swapon_command=$6
  nbd_swap_pair_topology_exact "$swaps_file" "$zram_path" "$lower_path" "$lower_type" || return 1
  "$swapoff_command" -- "$lower_path" || return 1
  "$swapoff_command" -- "$zram_path" || return 1
  [[ $(nbd_swap_exact_count "$swaps_file" "$lower_path") == 0 \
    && $(nbd_swap_exact_count "$swaps_file" "$zram_path") == 0 ]] || return 1
  "$swapon_command" -p 200 -- "$zram_path" || return 1
  "$swapon_command" -p 100 -- "$lower_path" || return 1
  nbd_swap_pair_topology_exact "$swaps_file" "$zram_path" "$lower_path" "$lower_type"
}

nbd_preserved_connection_republish_swap_pair() {
  (( $# == 6 )) || return 1
  local swaps_file=$1 zram_path=$2 nbd_path=$3
  local swapoff_command=$4 mkswap_command=$5 swapon_command=$6

  [[ $nbd_path =~ ^/dev/nbd[0-9]+$ ]] \
    || { printf 'NBD_REPUBLICATION_REASON=NBD_REPUBLICATION_DEVICE_INVALID\n' >&2; return 1; }
  nbd_swap_pair_topology_exact "$swaps_file" "$zram_path" "$nbd_path" partition \
    || { printf 'NBD_REPUBLICATION_REASON=NBD_REPUBLICATION_PRE_TOPOLOGY_INVALID\n' >&2; return 1; }
  "$swapoff_command" -- "$nbd_path" \
    || { printf 'NBD_REPUBLICATION_REASON=NBD_REPUBLICATION_SWAPOFF_NBD_FAILED\n' >&2; return 1; }
  "$swapoff_command" -- "$zram_path" \
    || { printf 'NBD_REPUBLICATION_REASON=NBD_REPUBLICATION_SWAPOFF_ZRAM_FAILED\n' >&2; return 1; }
  [[ $(nbd_swap_exact_count "$swaps_file" "$nbd_path") == 0 \
    && $(nbd_swap_exact_count "$swaps_file" "$zram_path") == 0 ]] \
    || { printf 'NBD_REPUBLICATION_REASON=NBD_REPUBLICATION_SWAP_ABSENCE_FAILED\n' >&2; return 1; }
  "$mkswap_command" -L RAMSHARED -- "$nbd_path" \
    || { printf 'NBD_REPUBLICATION_REASON=NBD_REPUBLICATION_MKSWAP_FAILED\n' >&2; return 1; }
  "$swapon_command" -p 200 -- "$zram_path" \
    || { printf 'NBD_REPUBLICATION_REASON=NBD_REPUBLICATION_ZRAM_SWAPON_FAILED\n' >&2; return 1; }
  "$swapon_command" -p 100 -- "$nbd_path" \
    || { printf 'NBD_REPUBLICATION_REASON=NBD_REPUBLICATION_NBD_SWAPON_FAILED\n' >&2; return 1; }
  nbd_swap_pair_topology_exact "$swaps_file" "$zram_path" "$nbd_path" partition \
    || { printf 'NBD_REPUBLICATION_REASON=NBD_REPUBLICATION_POST_TOPOLOGY_INVALID\n' >&2; return 1; }
}

nbd_republication_reason_from_output() {
  local output=$1 reason
  reason=$(awk -F= '
    $1 == "NBD_REPUBLICATION_REASON" { value = $2; found += 1 }
    END { if (found == 1) { print value; exit 0 } exit 1 }
  ' <<<"$output" 2>/dev/null) || reason=NBD_REPUBLICATION_TRANSACTION_FAILED
  printf '%s\n' "$reason"
}

nbd_cleanup_scratch() {
  local path=$1 expected=$2 swaps_file=$3 swapoff_command=$4
  nbd_scratch_matches "$path" "$expected" || return 1
  if [[ $(nbd_swap_exact_count "$swaps_file" "$path") == 1 ]]; then
    "$swapoff_command" -- "$path" || return 1
    [[ $(nbd_swap_exact_count "$swaps_file" "$path") == 0 ]] || return 1
  elif [[ $(nbd_swap_exact_count "$swaps_file" "$path") != 0 ]]; then
    return 1
  fi
  nbd_scratch_matches "$path" "$expected" || return 1
  rm -f -- "$path"
  [[ ! -e $path && ! -L $path ]]
}
