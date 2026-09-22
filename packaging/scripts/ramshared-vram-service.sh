#!/usr/bin/env bash
# RamShared Boot Survival & VRAM Tier Service for Linux / WSL2
# Broker/NBD capacity reserve: max(1536 MiB, 20% total VRAM), plus a separate
# 768 MiB runtime free-VRAM buffer before selecting the tier size.
# Protected cgroup v2 policy: memory.min=512M, memory.swap.max=0.
set -euo pipefail

NBD_DEV="/dev/nbd0"
SOCK_PATH="/run/ramshared/wsl2d.sock"
PID_FILE="/run/ramshared/ramsharedd.pid"
DAEMON_BIN="/usr/local/bin/ramsharedd"
SWAP_DEV_FILE="/run/ramshared/swap-dev"
ZRAM_DEV_FILE="/run/ramshared/zram-dev"
CAPACITY_STATUS_FILE="/run/ramshared/capacity-guaranteed"
LOG_FILE="/var/log/ramshared/vram-tier.log"
ZRAM_MIB=${RAMSHARED_ZRAM_MIB:-1024}

mkdir -p /run/ramshared /var/log/ramshared
chmod 0755 /run/ramshared

setup_protected_cgroup() {
    mkdir -p /sys/fs/cgroup/ramshared-protected
    if [[ -w /sys/fs/cgroup/ramshared-protected/memory.min ]]; then
        echo 536870912 > /sys/fs/cgroup/ramshared-protected/memory.min 2>/dev/null || true
    fi
    if [[ -w /sys/fs/cgroup/ramshared-protected/memory.low ]]; then
        echo 1073741824 > /sys/fs/cgroup/ramshared-protected/memory.low 2>/dev/null || true
    fi
    if [[ -w /sys/fs/cgroup/ramshared-protected/memory.swap.max ]]; then
        echo 0 > /sys/fs/cgroup/ramshared-protected/memory.swap.max 2>/dev/null || true
    fi
}

detect_vram_capacity() {
    if [[ -n "${RAMSHARED_VRAM_MIB:-}" ]]; then
        echo "$RAMSHARED_VRAM_MIB"
        return 0
    fi

    local total_mib=0 free_mib=0
    local smi_bin=""
    if [[ -x /usr/lib/wsl/lib/nvidia-smi ]]; then
        smi_bin="/usr/lib/wsl/lib/nvidia-smi"
    elif command -v nvidia-smi >/dev/null 2>&1; then
        smi_bin="nvidia-smi"
    fi

    if [[ -n "$smi_bin" ]]; then
        local raw
        raw=$($smi_bin --query-gpu=memory.total,memory.free --format=csv,noheader,nounits 2>/dev/null | head -n 1 || true)
        if [[ -n "$raw" && "$raw" =~ ^[0-9]+,[[:space:]]*[0-9]+ ]]; then
            total_mib=$(echo "$raw" | awk -F, '{print $1}' | tr -d ' ')
            free_mib=$(echo "$raw" | awk -F, '{print $2}' | tr -d ' ')
        fi
    fi

    if [[ "$total_mib" =~ ^[0-9]+$ ]] && [[ "$total_mib" -gt 0 ]]; then
        local min_reserve_floor_mib=1536
        local reserve_percent=20
        local max_slice_cap_mib=4096
        local runtime_free_headroom_mib=768
        local slice_align_mib=128
        local min_viable_tier_mib=512

        local reserve_mib=$(( total_mib * reserve_percent / 100 ))
        if [[ $reserve_mib -lt $min_reserve_floor_mib ]]; then
            reserve_mib=$min_reserve_floor_mib
        fi
        local target_mib=$(( total_mib - reserve_mib ))
        if [[ $target_mib -gt $max_slice_cap_mib ]]; then
            target_mib=$max_slice_cap_mib
        fi
        # If active free VRAM is reported, strictly preserve free headroom (SPEC §DT-1)
        if [[ "$free_mib" =~ ^[0-9]+$ ]] && [[ $free_mib -gt 0 ]]; then
            local safe_free=$(( free_mib - runtime_free_headroom_mib ))
            if [[ $safe_free -lt $target_mib ]]; then
                target_mib=$safe_free
            fi
        fi
        # Align down to boundary
        target_mib=$(( (target_mib / slice_align_mib) * slice_align_mib ))
        if [[ $target_mib -lt $min_viable_tier_mib ]]; then
            echo 0
            return 0
        fi
        echo "$target_mib"
    else
        echo 0
    fi
}

nbd_device_ready() {
    [[ -b "$NBD_DEV" ]]
}

swap_device_active() {
    local device=$1 swap_table=${2:-/proc/swaps}
    [[ -f $swap_table && -r $swap_table ]] || return 2
    local device_alias=''
    if [[ $device =~ ^/dev/(nbd|zram)[0-9]+$ ]]; then
        device_alias="/${device##*/}"
    fi
    local state
    if ! state=$(awk -v device="$device" -v device_alias="$device_alias" '
        NR == 1 { if ($1 != "Filename" || $2 != "Type") exit 3; next }
        $1 == device || ($1 == device_alias && $2 == "partition") { found = 1 }
        END { if (NR == 0) exit 3; print found ? "active" : "absent" }
    ' "$swap_table"); then
        return 2
    fi
    case $state in
        active) return 0 ;;
        absent) return 1 ;;
        *) return 2 ;;
    esac
}

swap_device_absent() {
    local result=0
    swap_device_active "$@" || result=$?
    (( result == 1 ))
}

nbd_swap_active() {
    swap_device_active "$NBD_DEV"
}

nbd_swap_absent() {
    swap_device_absent "$NBD_DEV"
}

nbd_connection_absent() {
    local sysfs_dir=${1:-/sys/block/${NBD_DEV##*/}}
    if [[ ! -e $sysfs_dir ]]; then
        [[ ! -b $NBD_DEV ]]
        return
    fi
    [[ -d $sysfs_dir && -f $sysfs_dir/size && -r $sysfs_dir/size ]] || return 1
    [[ ! -e $sysfs_dir/pid && ! -L $sysfs_dir/pid ]] || return 1
    local sectors
    sectors=$(<"$sysfs_dir/size")
    [[ $sectors =~ ^[0-9]+$ ]] && (( sectors == 0 ))
}

nbd_connection_connected() {
    local sysfs_dir=${1:-/sys/block/${NBD_DEV##*/}}
    [[ -d $sysfs_dir && -f $sysfs_dir/size && -r $sysfs_dir/size \
        && -f $sysfs_dir/pid && -r $sysfs_dir/pid ]] || return 1
    local sectors kernel_pid
    sectors=$(<"$sysfs_dir/size")
    kernel_pid=$(<"$sysfs_dir/pid")
    [[ $sectors =~ ^[0-9]+$ && $kernel_pid =~ ^[1-9][0-9]*$ ]] \
        && (( sectors > 0 ))
}

activate_nbd_tier() {
    local backend_desc=$1 backend_mb=$2
    echo "[+] Connecting $NBD_DEV to $backend_desc daemon..."
    if ! nbd_device_ready; then
        echo "[-] Refusing activation: $NBD_DEV is not a block device" >&2
        return 1
    fi
    if ! nbd-client -swap -timeout 0 -unix "$SOCK_PATH" "$NBD_DEV" >/dev/null 2>&1; then
        echo "[-] Refusing activation: NBD connection failed" >&2
        return 1
    fi
    if ! mkswap -f "$NBD_DEV" >/dev/null 2>&1; then
        echo "[-] Refusing activation: mkswap failed; NBD may remain connected" >&2
        return 1
    fi
    if ! swapon -p 50 "$NBD_DEV" 2>/dev/null; then
        echo "[-] Refusing activation: swapon failed; NBD may remain connected" >&2
        return 1
    fi
    if ! nbd_swap_active; then
        echo "[-] Refusing activation: $NBD_DEV is absent from /proc/swaps" >&2
        return 1
    fi
    echo "$NBD_DEV" > "$SWAP_DEV_FILE"
    echo "1" > "$CAPACITY_STATUS_FILE"
    echo "[+] RamShared Tier active at priority 50 on $NBD_DEV (${backend_mb} MiB) [$backend_desc]"
}

zram_swap_active() {
    local device=$1
    swap_device_active "$device"
}

any_zram_swap_active() {
    local swap_table=${1:-/proc/swaps}
    [[ -f $swap_table && -r $swap_table ]] || return 2
    local state
    if ! state=$(awk '
        NR == 1 { if ($1 != "Filename" || $2 != "Type") exit 3; next }
        $1 ~ /^\/(dev\/)?zram[0-9]+$/ && $2 == "partition" { found = 1 }
        END { if (NR == 0) exit 3; print found ? "active" : "absent" }
    ' "$swap_table"); then
        return 2
    fi
    case $state in
        active) return 0 ;;
        absent) return 1 ;;
        *) return 2 ;;
    esac
}

zram_device_ready() {
    [[ -b "$1" ]]
}

start_managed_zram() {
    if [[ ! $ZRAM_MIB =~ ^[0-9]+$ ]]; then
        echo "[-] Refusing ZRAM setup: RAMSHARED_ZRAM_MIB must be a nonnegative integer" >&2
        return 1
    fi
    (( ZRAM_MIB > 0 )) || return 0
    local existing_zram_status=0
    any_zram_swap_active || existing_zram_status=$?
    if (( existing_zram_status == 0 )); then
        echo "[+] Existing ZRAM swap is unmanaged by this service; leaving it untouched"
        return 0
    elif (( existing_zram_status != 1 )); then
        echo "[-] Refusing ZRAM setup: /proc/swaps state is unreadable" >&2
        return 1
    fi
    if ! modprobe zram 2>/dev/null; then
        echo "[-] Refusing ZRAM setup: module load failed" >&2
        return 1
    fi
    local zram_dev
    if ! zram_dev=$(zramctl --find --size "${ZRAM_MIB}M" 2>/dev/null); then
        echo "[-] Refusing ZRAM setup: device allocation failed" >&2
        return 1
    fi
    if [[ ! $zram_dev =~ ^/dev/zram[0-9]+$ ]] || ! zram_device_ready "$zram_dev"; then
        echo "[-] Refusing ZRAM setup: allocated device is invalid" >&2
        return 1
    fi
    echo "$zram_dev" > "$ZRAM_DEV_FILE"
    if ! mkswap "$zram_dev" >/dev/null 2>&1; then
        echo "[-] Refusing ZRAM setup: mkswap failed; retained device record for inspection" >&2
        return 1
    fi
    if ! swapon -p 100 "$zram_dev" 2>/dev/null; then
        echo "[-] Refusing ZRAM setup: swapon failed; retained device record for inspection" >&2
        return 1
    fi
    if ! zram_swap_active "$zram_dev"; then
        echo "[-] Refusing ZRAM setup: device is absent from /proc/swaps" >&2
        return 1
    fi
    echo "[+] ZRAM active at priority 100 on $zram_dev"
}

stop_managed_zram() {
    if [[ -L "$ZRAM_DEV_FILE" || ( -e "$ZRAM_DEV_FILE" && ! -f "$ZRAM_DEV_FILE" ) ]]; then
        echo "[-] Refusing ZRAM cleanup: owned-device record is not a regular file" >&2
        return 1
    fi
    [[ -f "$ZRAM_DEV_FILE" ]] || return 0
    local zram_dev
    zram_dev=$(<"$ZRAM_DEV_FILE")
    if [[ ! $zram_dev =~ ^/dev/zram[0-9]+$ ]]; then
        echo "[-] Refusing ZRAM cleanup: invalid owned-device record" >&2
        return 1
    fi
    if ! zram_swap_active "$zram_dev"; then
        echo "[-] Refusing ZRAM cleanup: recorded device is not active; inspect ownership" >&2
        return 1
    fi
    echo "[+] Deactivating managed ZRAM swap $zram_dev..."
    if ! swapoff "$zram_dev" 2>/dev/null; then
        echo "[-] Refusing ZRAM reset: swapoff failed for $zram_dev" >&2
        return 1
    fi
    if zram_swap_active "$zram_dev"; then
        echo "[-] Refusing ZRAM reset: $zram_dev remains active in /proc/swaps" >&2
        return 1
    fi
    if ! zramctl --reset "$zram_dev" 2>/dev/null; then
        echo "[-] Refusing ZRAM record cleanup: reset failed for $zram_dev" >&2
        return 1
    fi
    rm -f "$ZRAM_DEV_FILE"
}

start_tier() {
    echo "[+] Starting RamShared VRAM Tier Service (Protected Architecture)..."
    if ! nbd_swap_absent; then
        echo "[-] Refusing start: NBD swap is active or /proc/swaps is unreadable; use the sealed cascade lifecycle" >&2
        return 1
    fi
    if [[ -e "$PID_FILE" || -L "$PID_FILE" || -e "$SOCK_PATH" || -L "$SOCK_PATH" \
        || -e "$ZRAM_DEV_FILE" || -L "$ZRAM_DEV_FILE" ]]; then
        echo "[-] Refusing start: daemon or ZRAM state already exists; inspect ownership before cleanup" >&2
        return 1
    fi
    if ! command -v pgrep >/dev/null 2>&1; then
        echo "[-] Refusing start: pgrep is unavailable for daemon collision check" >&2
        return 1
    fi
    local pgrep_status=0
    pgrep -x ramsharedd >/dev/null 2>&1 || pgrep_status=$?
    if (( pgrep_status == 0 )); then
        echo "[-] Refusing start: another ramsharedd process is already running" >&2
        return 1
    elif (( pgrep_status != 1 )); then
        echo "[-] Refusing start: daemon collision check failed" >&2
        return 1
    fi
    setup_protected_cgroup

    # 1. Setup ZRAM (Tier 0 - Priority 100) without adopting another owner.
    start_managed_zram || return 1

    # 2. Setup VRAM via GPU (Tier 1 - Priority 50)
    modprobe nbd max_part=8 2>/dev/null || true
    
    local vram_mib
    vram_mib=$(detect_vram_capacity)
    local backend_type="auto"
    local backend_mb="$vram_mib"
    local backend_desc="GPU VRAM"
    if [[ "$vram_mib" -eq 0 ]]; then
        echo "[!] GPU is not accessible (e.g. host NVIDIA driver update in Windows requires a WSL restart: wsl --shutdown)."
        echo "[+] Starting RamShared with native auto-fallback to keep swap alive..."
        backend_type="auto"
        backend_mb="1024"
        backend_desc="native RAM fallback"
    else
        echo "[+] Dynamic VRAM allocation: ${vram_mib} MiB on GPU"
    fi

    if nbd_swap_absent; then
        # Launch ramsharedd inside /ramshared-protected cgroup with memory.swap.max=0 and oom_score_adj=-1000
        bash -c "echo \$\$ > /sys/fs/cgroup/ramshared-protected/cgroup.procs 2>/dev/null || true; echo -1000 > /proc/\$\$/oom_score_adj 2>/dev/null || true; exec /usr/local/bin/ramsharedd --backend '$backend_type' --slices 1 --slice-mb '$backend_mb' --listen-nbd 127.0.0.1:10809 --arbiter-listen 127.0.0.1:9090" > "$LOG_FILE" 2>&1 &
        local daemon_pid=$!
        echo "$daemon_pid" > "$PID_FILE"
        
        # Wait for daemon socket
        for i in {1..20}; do
            if [[ -S "$SOCK_PATH" ]]; then
                break
            fi
            sleep 0.2
        done
        
        if kill -0 "$daemon_pid" 2>/dev/null && [[ -S "$SOCK_PATH" ]]; then
            activate_nbd_tier "$backend_desc" "$backend_mb" || return 1
        else
            echo "[-] Daemon failed to start, check $LOG_FILE"
            return 1
        fi
    else
        echo "[-] Refusing start: NBD swap state changed before daemon launch" >&2
        return 1
    fi

    chmod 0644 /run/ramshared/* 2>/dev/null || true
}

stop_tier() {
    echo "[+] Stopping RamShared VRAM Tier Service (Swapoff-first)..."
    if ! nbd_swap_active && ! nbd_swap_absent; then
        echo "[-] Refusing teardown: NBD swap state is unreadable" >&2
        return 1
    fi
    if [[ -L "$PID_FILE" || ( -e "$PID_FILE" && ! -f "$PID_FILE" ) ]]; then
        echo "[-] Refusing teardown: daemon PID record is not a regular file" >&2
        return 1
    fi
    if [[ ! -e "$PID_FILE" && ! -L "$PID_FILE" ]]; then
        if ! nbd_swap_absent || ! nbd_connection_absent; then
            echo "[-] Refusing teardown: NBD is active or connected without a daemon record" >&2
            return 1
        fi
        if [[ -e "$SOCK_PATH" || -L "$SOCK_PATH" || -e "$SWAP_DEV_FILE" \
            || -L "$SWAP_DEV_FILE" || -e "$CAPACITY_STATUS_FILE" || -L "$CAPACITY_STATUS_FILE" ]]; then
            echo "[-] Refusing no-op stop: unowned service state remains" >&2
            return 1
        fi
        stop_managed_zram || return 1
        echo "[+] RamShared VRAM Tier is already stopped."
        return 0
    fi

    # The PID record is an ownership claim, not proof. Never touch an active
    # swap device when the recorded daemon is missing or belongs to another
    # executable; a stale PID can be recycled by an unrelated process.
    if [[ -f "$PID_FILE" ]]; then
        local pid observed_exe
        pid=$(<"$PID_FILE")
        if [[ ! $pid =~ ^[1-9][0-9]*$ ]] || ! kill -0 "$pid" 2>/dev/null; then
            echo "[-] Refusing teardown: daemon PID record is not live" >&2
            return 1
        fi
        observed_exe=$(readlink -f "/proc/$pid/exe" 2>/dev/null) || {
            echo "[-] Refusing teardown: daemon executable is unreadable" >&2
            return 1
        }
        if [[ $observed_exe != "$DAEMON_BIN" ]]; then
            echo "[-] Refusing teardown: daemon executable identity differs" >&2
            return 1
        fi
    elif nbd_swap_active; then
        echo "[-] Refusing teardown: active NBD swap has no daemon PID record" >&2
        return 1
    fi
    
    # 1. Swapoff VRAM
    if nbd_swap_active; then
        echo "[+] Deactivating swap on $NBD_DEV..."
        if ! swapoff "$NBD_DEV" 2>/dev/null; then
            echo "[-] Refusing NBD disconnect: swapoff failed for $NBD_DEV" >&2
            return 1
        fi
        if ! nbd_swap_absent; then
            echo "[-] Refusing NBD disconnect: $NBD_DEV remains active or /proc/swaps is unreadable" >&2
            return 1
        fi
    fi
    
    # 2. Disconnect only a kernel-confirmed connection. A failed start may
    # leave the owned daemon running without ever attaching NBD.
    if nbd_connection_absent; then
        echo "[+] NBD is already disconnected."
    elif nbd_connection_connected; then
        if ! command -v nbd-client >/dev/null 2>&1; then
            echo "[-] Refusing daemon stop: nbd-client is unavailable" >&2
            return 1
        fi
        if ! nbd-client -d "$NBD_DEV" >/dev/null 2>&1; then
            echo "[-] Refusing daemon stop: NBD disconnect failed" >&2
            return 1
        fi
        if ! nbd_connection_absent; then
            echo "[-] Refusing daemon stop: kernel still reports NBD connected" >&2
            return 1
        fi
    else
        echo "[-] Refusing daemon stop: kernel NBD connection state is unknown" >&2
        return 1
    fi

    # 3. Terminate Daemon
    if [[ -f "$PID_FILE" ]]; then
        local pid
        pid=$(cat "$PID_FILE")
        if kill -0 "$pid" 2>/dev/null; then
            local observed_exe
            observed_exe=$(readlink -f "/proc/$pid/exe" 2>/dev/null) || {
                echo "[-] Refusing daemon stop: executable identity changed" >&2
                return 1
            }
            if [[ $observed_exe != "$DAEMON_BIN" ]]; then
                echo "[-] Refusing daemon stop: executable identity changed" >&2
                return 1
            fi
            if ! nbd_swap_absent || ! nbd_connection_absent; then
                echo "[-] Refusing daemon stop: NBD became active or reconnected" >&2
                return 1
            fi
            echo "[+] Terminating daemon PID $pid..."
            if ! kill -TERM "$pid" 2>/dev/null; then
                echo "[-] Refusing state cleanup: daemon TERM failed" >&2
                return 1
            fi
            for _ in {1..50}; do
                if ! kill -0 "$pid" 2>/dev/null; then
                    break
                fi
                sleep 0.1
            done
            if kill -0 "$pid" 2>/dev/null; then
                echo "[-] Refusing state cleanup: daemon did not exit after TERM" >&2
                return 1
            fi
        fi
        rm -f "$PID_FILE" "$SOCK_PATH" "$SWAP_DEV_FILE" "$CAPACITY_STATUS_FILE"
    fi

    # 4. Only the ZRAM device recorded by this service may be reset.
    stop_managed_zram || return 1

    echo "[+] RamShared VRAM Tier deactivated cleanly."
}

case "${1:-status}" in
    start)
        start_tier
        ;;
    stop)
        stop_tier
        ;;
    restart)
        stop_tier
        sleep 1
        start_tier
        ;;
    status)
        echo "=== Active Memory & Swap Priorities ==="
        swapon --show || true
        echo ""
        echo "=== GPU Memory Status ==="
        /usr/lib/wsl/lib/nvidia-smi --query-gpu=name,memory.total,memory.free,memory.used --format=csv,noheader 2>/dev/null || true
        ;;
    *)
        echo "Usage: $0 {start|stop|restart|status}"
        exit 1
        ;;
esac
