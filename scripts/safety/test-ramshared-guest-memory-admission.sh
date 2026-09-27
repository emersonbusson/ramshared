#!/usr/bin/env bash
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
gate="$root/scripts/safety/ramshared-guest-memory-admission.sh"
tmp=$(mktemp -d "${TMPDIR:-/tmp}/ramshared-guest-memory-gate.XXXXXX")
trap 'rm -rf -- "$tmp"' EXIT

assert_result() {
    local name=$1
    local expected_status=$2
    local expected_exit=$3
    local mem_kib=$4
    local swap_kib=$5
    local fixture="$tmp/$name.meminfo"
    local output="$tmp/$name.out"
    local error="$tmp/$name.err"
    printf 'MemAvailable: %s kB\nSwapFree: %s kB\n' "$mem_kib" "$swap_kib" >"$fixture"

    local actual_exit=0
    bash "$gate" "$fixture" 1024 1024 >"$output" 2>"$error" || actual_exit=$?
    [[ "$actual_exit" -eq "$expected_exit" ]]
    grep -Fq "\"status\":\"$expected_status\"" "$output"
    printf 'PASS %s\n' "$name"
}

assert_result exact_reserve_passes PASS 0 1048576 1048576
assert_result low_guest_memory_refuses FAIL 2 1048575 2097152
assert_result low_guest_swap_refuses FAIL 2 2097152 1048575

malformed="$tmp/malformed.meminfo"
printf 'MemTotal: 16384000 kB\n' >"$malformed"
malformed_output="$tmp/malformed.out"
malformed_error="$tmp/malformed.err"
malformed_exit=0
bash "$gate" "$malformed" 1024 1024 >"$malformed_output" 2>"$malformed_error" || malformed_exit=$?
[[ "$malformed_exit" -eq 2 ]]
grep -Fq '"reason":"guest_memory_telemetry_invalid"' "$malformed_output"
printf 'PASS malformed_guest_telemetry_refuses\n'

invalid_reserve_exit=0
bash "$gate" "$malformed" 1023 1024 >"$tmp/invalid-reserve.out" 2>"$tmp/invalid-reserve.err" || invalid_reserve_exit=$?
[[ "$invalid_reserve_exit" -eq 2 ]]
grep -Fq '"reason":"guest_memory_admission_arguments_invalid"' "$tmp/invalid-reserve.out"
printf 'PASS guest_reserve_cannot_be_lowered\n'

printf 'RAMSHARED_GUEST_MEMORY_ADMISSION=PASS\n'
