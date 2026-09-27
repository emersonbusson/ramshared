#!/usr/bin/env bash
# Read-only, fixture-testable guest memory and PSI guards for bounded pressure.

ramshared_guest_pressure_read_kib() {
    local path=$1 key=$2
    awk -v wanted="${key}:" '
        $1 == wanted {
            count++
            if (NF != 3 || $2 !~ /^[0-9]+$/ || $3 != "kB") {
                invalid = 1
            } else {
                value = $2
            }
        }
        END {
            if (count != 1 || invalid) exit 1
            print value
        }
    ' "$path" 2>/dev/null
}

ramshared_guest_pressure_read_psi_full_avg10() {
    local path=$1
    awk '
        $1 == "full" {
            full_count++
            avg10_count = 0
            for (i = 2; i <= NF; i++) {
                if ($i ~ /^avg10=/) {
                    avg10_count++
                    value = substr($i, 7)
                    if (value !~ /^[0-9]+([.][0-9]+)?$/ || value + 0 > 100) {
                        invalid = 1
                    }
                }
            }
            if (avg10_count != 1) invalid = 1
        }
        END {
            if (full_count != 1 || invalid) exit 1
            print value
        }
    ' "$path" 2>/dev/null
}

ramshared_guest_pressure_read_sample() {
    local meminfo_path=${1:-/proc/meminfo}
    local psi_path=${2:-/proc/pressure/memory}
    local mem_available_kib swap_free_kib psi_full_avg10

    mem_available_kib=$(ramshared_guest_pressure_read_kib "$meminfo_path" MemAvailable) || return 1
    swap_free_kib=$(ramshared_guest_pressure_read_kib "$meminfo_path" SwapFree) || return 1
    psi_full_avg10=$(ramshared_guest_pressure_read_psi_full_avg10 "$psi_path") || return 1
    printf '%s %s %s\n' "$mem_available_kib" "$swap_free_kib" "$psi_full_avg10"
}

ramshared_guest_pressure_guard_reason() {
    local mem_available_kib=$1 swap_free_kib=$2 psi_full_avg10=$3 psi_status
    if [[ ! "$mem_available_kib" =~ ^[0-9]+$ || ! "$swap_free_kib" =~ ^[0-9]+$ ]]; then
        printf 'guest_pressure_telemetry_invalid\n'
        return 0
    fi
    if (( mem_available_kib < 614400 )); then
        printf 'guest_mem_available_runtime_reserve_breached\n'
        return 0
    fi
    if (( swap_free_kib < 1048576 )); then
        printf 'guest_swap_free_runtime_reserve_breached\n'
        return 0
    fi
    if ! [[ "$psi_full_avg10" =~ ^[0-9]+([.][0-9]+)?$ ]]; then
        printf 'guest_pressure_telemetry_invalid\n'
        return 0
    fi
    psi_status=0
    awk -v value="$psi_full_avg10" 'BEGIN {
        if (value + 0 > 100) exit 2
        if (value + 0 >= 10) exit 0
        exit 1
    }' || psi_status=$?
    if (( psi_status == 0 )); then
        printf 'guest_memory_psi_full_limit_reached\n'
    elif (( psi_status != 1 )); then
        printf 'guest_pressure_telemetry_invalid\n'
    fi
}

ramshared_guest_pressure_parse_cgroup_bytes() {
    local raw=$1 quantity suffix multiplier
    if [[ ! "$raw" =~ ^([0-9]+)([kKmMgGtTpP]([iI][bB])?|[bB])?$ ]]; then
        return 1
    fi
    quantity=${BASH_REMATCH[1]}
    suffix=${BASH_REMATCH[2],,}
    if ((${#quantity} > 18)); then
        return 1
    fi
    case "$suffix" in
        ""|b) multiplier=1 ;;
        k|kib) multiplier=1024 ;;
        m|mib) multiplier=1048576 ;;
        g|gib) multiplier=1073741824 ;;
        t|tib) multiplier=1099511627776 ;;
        p|pib) multiplier=1125899906842624 ;;
        *) return 1 ;;
    esac
    quantity=$((10#$quantity))
    if ((quantity > 9223372036854775807 / multiplier)); then
        return 1
    fi
    printf '%s\n' "$((quantity * multiplier))"
}

ramshared_guest_memory_limit_bytes() {
    local mem_available_kib=$1 current_bytes=$2 configured_max_bytes=$3
    local headroom_bytes current candidate
    if [[ ! "$mem_available_kib" =~ ^[0-9]+$ || ! "$current_bytes" =~ ^[0-9]+$ || ! "$configured_max_bytes" =~ ^[0-9]+$ ]]; then
        return 1
    fi
    if ((${#mem_available_kib} > 15 || ${#current_bytes} > 18 || ${#configured_max_bytes} > 18)); then
        return 1
    fi
    mem_available_kib=$((10#$mem_available_kib))
    current=$((10#$current_bytes))
    configured_max_bytes=$((10#$configured_max_bytes))
    if ((configured_max_bytes == 0 || mem_available_kib < 614400)); then
        return 1
    fi
    headroom_bytes=$(((mem_available_kib - 614400) * 1024))
    if ((headroom_bytes > 9223372036854775807 - current)); then
        return 1
    fi
    candidate=$((current + headroom_bytes))
    if ((candidate > configured_max_bytes)); then
        candidate=$configured_max_bytes
    fi
    printf '%s\n' "$candidate"
}

ramshared_guest_swap_limit_bytes() {
    local swap_free_kib=$1 current_swap_bytes=$2 free_bytes current
    if [[ ! "$swap_free_kib" =~ ^[0-9]+$ || ! "$current_swap_bytes" =~ ^[0-9]+$ ]]; then
        return 1
    fi
    if ((${#swap_free_kib} > 15 || ${#current_swap_bytes} > 18)); then
        return 1
    fi
    swap_free_kib=$((10#$swap_free_kib))
    current=$((10#$current_swap_bytes))
    if ((swap_free_kib < 1048576)); then
        return 1
    fi
    free_bytes=$((swap_free_kib * 1024 - 1073741824))
    if ((free_bytes > 9223372036854775807 - current)); then
        return 1
    fi
    printf '%s\n' "$((current + free_bytes))"
}

ramshared_guest_swap_budget_bytes() {
    local swap_free_kib=$1
    if [[ ! "$swap_free_kib" =~ ^[0-9]+$ ]] || ((${#swap_free_kib} > 15)); then
        return 1
    fi
    swap_free_kib=$((10#$swap_free_kib))
    if ((swap_free_kib <= 1048576)); then
        return 1
    fi
    ramshared_guest_swap_limit_bytes "$swap_free_kib" 0
}
