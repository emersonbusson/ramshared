#!/usr/bin/env bash
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
source "$root/scripts/safety/guest-pressure-runtime-guard.sh"
tmp=$(mktemp -d "${TMPDIR:-/tmp}/ramshared-guest-pressure-guard.XXXXXX")
trap 'rm -rf -- "$tmp"' EXIT

assert_eq() {
    local actual=$1 expected=$2 name=$3
    [[ "$actual" == "$expected" ]] || {
        printf 'FAIL %s: expected <%s>, got <%s>\n' "$name" "$expected" "$actual" >&2
        exit 1
    }
}

write_meminfo() {
    printf 'MemAvailable: %s kB\nSwapFree: %s kB\n' "$1" "$2" >"$tmp/meminfo"
}

write_psi() {
    printf 'some avg10=0.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=%s avg60=0.00 avg300=0.00 total=0\n' "$1" >"$tmp/psi"
}

write_meminfo 614400 2097152
write_psi 9.99
sample=$(ramshared_guest_pressure_read_sample "$tmp/meminfo" "$tmp/psi")
assert_eq "$sample" '614400 2097152 9.99' exact_runtime_floor_sample

reason=$(ramshared_guest_pressure_guard_reason 614400 1048576 9.99)
assert_eq "$reason" '' exact_runtime_floors_pass
reason=$(ramshared_guest_pressure_guard_reason 614399 2097152 0.00)
assert_eq "$reason" guest_mem_available_runtime_reserve_breached low_memavailable_refuses
reason=$(ramshared_guest_pressure_guard_reason 614400 1048575 0.00)
assert_eq "$reason" guest_swap_free_runtime_reserve_breached low_swapfree_refuses
reason=$(ramshared_guest_pressure_guard_reason 614400 2097152 10.00)
assert_eq "$reason" guest_memory_psi_full_limit_reached psi_at_limit_refuses
reason=$(ramshared_guest_pressure_guard_reason not-a-number 2097152 0.00)
assert_eq "$reason" guest_pressure_telemetry_invalid malformed_runtime_sample_refuses

assert_eq "$(ramshared_guest_swap_budget_bytes 2097152)" 1073741824 one_gibibyte_guest_swap_budget
if ramshared_guest_swap_budget_bytes 1048576 >/dev/null 2>&1; then
    printf 'FAIL exact_swap_reserve_must_not_start_pressure\n' >&2
    exit 1
fi
if ramshared_guest_swap_budget_bytes 999999999999999999999 >/dev/null 2>&1; then
    printf 'FAIL overflowing_swap_budget_must_refuse\n' >&2
    exit 1
fi

assert_eq "$(ramshared_guest_pressure_parse_cgroup_bytes 1200M)" 1258291200 parse_cgroup_megabytes
assert_eq "$(ramshared_guest_pressure_parse_cgroup_bytes 1G)" 1073741824 parse_cgroup_gigabytes
if ramshared_guest_pressure_parse_cgroup_bytes max >/dev/null 2>&1; then
    printf 'FAIL unbounded_memory_limit_must_be_rejected\n' >&2
    exit 1
fi
if ramshared_guest_pressure_parse_cgroup_bytes 999999999999999999P >/dev/null 2>&1; then
    printf 'FAIL overflowing_memory_limit_must_be_rejected\n' >&2
    exit 1
fi
assert_eq "$(ramshared_guest_memory_limit_bytes 2097152 0 1258291200)" 1258291200 cap_by_configured_mem_max
assert_eq "$(ramshared_guest_memory_limit_bytes 716800 104857600 1258291200)" 209715200 preserve_guest_memory_reserve
assert_eq "$(ramshared_guest_memory_limit_bytes 614400 104857600 1258291200)" 104857600 exact_memavailable_reserve_blocks_growth
assert_eq "$(ramshared_guest_swap_limit_bytes 2097152 0)" 1073741824 cap_by_free_swap_after_reserve
assert_eq "$(ramshared_guest_swap_limit_bytes 1572864 536870912)" 1073741824 account_for_existing_cgroup_swap
assert_eq "$(ramshared_guest_swap_limit_bytes 1048576 536870912)" 536870912 exact_swap_reserve_blocks_growth

mkdir "$tmp/failing-bin"
printf '#!/bin/sh\nexit 42\n' >"$tmp/failing-bin/awk"
chmod +x "$tmp/failing-bin/awk"
reason=$(PATH="$tmp/failing-bin:$PATH" ramshared_guest_pressure_guard_reason 614400 2097152 0.00)
assert_eq "$reason" guest_pressure_telemetry_invalid psi_evaluator_failure_refuses

printf 'MemAvailable: 614400 kB\n' >"$tmp/meminfo"
if ramshared_guest_pressure_read_sample "$tmp/meminfo" "$tmp/psi" >/dev/null 2>&1; then
    printf 'FAIL missing_swapfree_must_refuse\n' >&2
    exit 1
fi
write_meminfo 614400 2097152
printf 'full avg10=NaN avg60=0.00 avg300=0.00 total=0\n' >"$tmp/psi"
if ramshared_guest_pressure_read_sample "$tmp/meminfo" "$tmp/psi" >/dev/null 2>&1; then
    printf 'FAIL malformed_psi_must_refuse\n' >&2
    exit 1
fi
write_psi 100.01
if ramshared_guest_pressure_read_sample "$tmp/meminfo" "$tmp/psi" >/dev/null 2>&1; then
    printf 'FAIL out_of_range_psi_must_refuse\n' >&2
    exit 1
fi
printf 'MemAvailable: 614400 kB\nMemAvailable: 614400 kB\nSwapFree: 2097152 kB\n' >"$tmp/meminfo"
if ramshared_guest_pressure_read_sample "$tmp/meminfo" "$tmp/psi" >/dev/null 2>&1; then
    printf 'FAIL duplicate_meminfo_metric_must_refuse\n' >&2
    exit 1
fi
write_meminfo 614400 2097152
printf 'full avg10=1.00 avg10=2.00 avg60=0.00 avg300=0.00 total=0\n' >"$tmp/psi"
if ramshared_guest_pressure_read_sample "$tmp/meminfo" "$tmp/psi" >/dev/null 2>&1; then
    printf 'FAIL duplicate_psi_avg10_must_refuse\n' >&2
    exit 1
fi

printf 'GUEST_PRESSURE_RUNTIME_GUARD=PASS\n'
