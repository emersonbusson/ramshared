#!/usr/bin/env bash
# Read-only identity helpers and exact scratch cleanup transaction shared by the
# live benchmark cell and manufactured tests.

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
