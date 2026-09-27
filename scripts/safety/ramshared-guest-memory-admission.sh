#!/usr/bin/env bash
set -euo pipefail

meminfo_path=${1:-}
minimum_mem_mib=${2:-}
minimum_swap_mib=${3:-}

fail() {
    local reason=$1
    local mem_available_kib=${2:-0}
    local swap_free_kib=${3:-0}
    printf '{"status":"FAIL","reason":"%s","mem_available_kib":%s,"swap_free_kib":%s}\n' \
        "$reason" "$mem_available_kib" "$swap_free_kib"
    printf 'guest_memory_admission_refused:%s\n' "$reason" >&2
    exit 2
}

if [[ $# -ne 3 || ! "$minimum_mem_mib" =~ ^[0-9]+$ || ! "$minimum_swap_mib" =~ ^[0-9]+$ ||
    "$minimum_mem_mib" -lt 1024 || "$minimum_swap_mib" -lt 1024 ]]; then
    fail guest_memory_admission_arguments_invalid
fi

read_meminfo_kib() {
    local key=$1
    awk -v key="$key:" '$1 == key { value = $2 } END {
        if (value !~ /^[0-9]+$/) exit 1
        print value
    }' "$meminfo_path" 2>/dev/null
}

mem_available_kib=$(read_meminfo_kib MemAvailable) || fail guest_memory_telemetry_invalid
swap_free_kib=$(read_meminfo_kib SwapFree) || fail guest_memory_telemetry_invalid "$mem_available_kib"
minimum_mem_kib=$((minimum_mem_mib * 1024))
minimum_swap_kib=$((minimum_swap_mib * 1024))

if (( mem_available_kib < minimum_mem_kib )); then
    fail guest_mem_available_below_reserve "$mem_available_kib" "$swap_free_kib"
fi
if (( swap_free_kib < minimum_swap_kib )); then
    fail guest_swap_free_below_reserve "$mem_available_kib" "$swap_free_kib"
fi

printf '{"status":"PASS","reason":"guest_memory_headroom_ok","mem_available_kib":%s,"swap_free_kib":%s,"minimum_mem_mib":%s,"minimum_swap_mib":%s}\n' \
    "$mem_available_kib" "$swap_free_kib" "$minimum_mem_mib" "$minimum_swap_mib"
