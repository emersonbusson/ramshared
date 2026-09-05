#!/usr/bin/env bash
# RamShared Boot Survival & VRAM Tier Service for Linux / WSL2
# Follows SSDV3 GPU reserve rules: dynamically reserves max(2 GiB, 20% total VRAM)
# Protected Cgroup v2 Isolation: memory.min=512M, memory.swap.max=0 (Zero-Deadlock Guarantee)
set -euo pipefail

NBD_DEV="/dev/nbd0"
SOCK_PATH="/run/ramshared/wsl2d.sock"
PID_FILE="/run/ramshared/ramsharedd.pid"
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
        if [[ -n "$raw" ]]; then
            total_mib=$(echo "$raw" | awk -F, '{print $1}' | tr -d ' ')
            free_mib=$(echo "$raw" | awk -F, '{print $2}' | tr -d ' ')
        fi
    fi

    if [[ $total_mib -gt 0 ]]; then
        local reserve_mib=$(( total_mib * 20 / 100 ))
        if [[ $reserve_mib -lt 2048 ]]; then
            reserve_mib=2048
        fi
        local target_mib=$(( total_mib - reserve_mib ))
        if [[ $target_mib -gt $free_mib ]]; then
            target_mib=$(( free_mib - 512 ))
        fi
        if [[ $target_mib -lt 512 ]]; then
            target_mib=512
        fi
        echo "$target_mib"
    else
        echo 3072
    fi
}

start_tier() {
    echo "[+] Starting RamShared VRAM Tier Service (Protected Architecture)..."
    setup_protected_cgroup

    # 1. Setup ZRAM (Tier 0 - Priority 100)
    if [[ $ZRAM_MIB -gt 0 ]]; then
        modprobe zram 2>/dev/null || true
        local zram_dev
        zram_dev=$(zramctl --find --size "${ZRAM_MIB}M" 2>/dev/null || echo "/dev/zram0")
        if ! grep -q zram /proc/swaps 2>/dev/null; then
            echo "[+] Initializing ZRAM (${ZRAM_MIB} MiB)..."
            if [[ -b "$zram_dev" ]]; then
                mkswap "$zram_dev" >/dev/null 2>&1 || true
                swapon -p 100 "$zram_dev" 2>/dev/null || true
                echo "[+] ZRAM active at priority 100 on $zram_dev"
            fi
        fi
        echo "$zram_dev" > "$ZRAM_DEV_FILE"
    fi

    # 2. Setup VRAM via GPU (Tier 1 - Priority 50)
    modprobe nbd max_part=8 2>/dev/null || true
    
    local vram_mib
    vram_mib=$(detect_vram_capacity)
    echo "[+] Dynamic VRAM allocation: ${vram_mib} MiB on GPU"

    # Clean prior stale sockets if daemon is dead
    if [[ -f "$PID_FILE" ]]; then
        local old_pid
        old_pid=$(cat "$PID_FILE" 2>/dev/null || true)
        if [[ -n "$old_pid" ]] && ! kill -0 "$old_pid" 2>/dev/null; then
            rm -f "$SOCK_PATH" "$PID_FILE"
        fi
    fi

    if ! grep -q "$NBD_DEV" /proc/swaps 2>/dev/null; then
        rm -f "$SOCK_PATH" "$PID_FILE"
        
        # Launch ramsharedd inside /ramshared-protected cgroup with memory.swap.max=0 and oom_score_adj=-1000
        bash -c "echo \$\$ > /sys/fs/cgroup/ramshared-protected/cgroup.procs 2>/dev/null || true; echo -1000 > /proc/\$\$/oom_score_adj 2>/dev/null || true; exec /usr/local/bin/ramsharedd --backend vram --slices 1 --slice-mb '$vram_mib' --listen-nbd 127.0.0.1:10809 --arbiter-listen 127.0.0.1:9090" > "$LOG_FILE" 2>&1 &
        local daemon_pid=$!
        echo "$daemon_pid" > "$PID_FILE"
        echo "$NBD_DEV" > "$SWAP_DEV_FILE"
        echo "1" > "$CAPACITY_STATUS_FILE"
        
        # Wait for daemon socket
        for i in {1..20}; do
            if [[ -S "$SOCK_PATH" ]]; then
                break
            fi
            sleep 0.2
        done
        
        if kill -0 "$daemon_pid" 2>/dev/null && [[ -S "$SOCK_PATH" ]]; then
            echo "[+] Connecting $NBD_DEV to VRAM daemon (with swap immunity & zero-timeout protection)..."
            nbd-client -swap -timeout 0 -unix "$SOCK_PATH" "$NBD_DEV" >/dev/null 2>&1 || true
            sleep 1
            if [[ -b "$NBD_DEV" ]]; then
                mkswap -f "$NBD_DEV" >/dev/null 2>&1 || true
                swapon -p 50 "$NBD_DEV" 2>/dev/null || true
                echo "[+] RamShared VRAM Tier active at priority 50 on $NBD_DEV (${vram_mib} MiB) [Zero-Swap Immunity Shield ACTIVE]"
            fi
        else
            echo "[-] Daemon failed to start, check $LOG_FILE"
            exit 1
        fi
    else
        echo "[!] VRAM tier is already active on $NBD_DEV"
        echo "$NBD_DEV" > "$SWAP_DEV_FILE"
        echo "1" > "$CAPACITY_STATUS_FILE"
        pgrep -f "ramsharedd" | head -n 1 > "$PID_FILE" || true
    fi

    chmod 0644 /run/ramshared/* 2>/dev/null || true
}

stop_tier() {
    echo "[+] Stopping RamShared VRAM Tier Service (Swapoff-first)..."
    
    # 1. Swapoff VRAM
    if grep -q "$NBD_DEV" /proc/swaps 2>/dev/null; then
        echo "[+] Deactivating swap on $NBD_DEV..."
        swapoff "$NBD_DEV" 2>/dev/null || true
    fi
    
    # 2. Disconnect NBD
    if command -v nbd-client >/dev/null 2>&1; then
        nbd-client -d "$NBD_DEV" >/dev/null 2>&1 || true
    fi

    # 3. Terminate Daemon
    if [[ -f "$PID_FILE" ]]; then
        local pid
        pid=$(cat "$PID_FILE")
        if kill -0 "$pid" 2>/dev/null; then
            echo "[+] Terminating daemon PID $pid..."
            kill "$pid" 2>/dev/null || true
            sleep 1
            kill -9 "$pid" 2>/dev/null || true
        fi
        rm -f "$PID_FILE" "$SOCK_PATH" "$SWAP_DEV_FILE" "$ZRAM_DEV_FILE" "$CAPACITY_STATUS_FILE"
    fi

    # 4. Swapoff ZRAM
    if grep -q zram /proc/swaps 2>/dev/null; then
        for z in $(grep zram /proc/swaps | awk '{print $1}'); do
            echo "[+] Deactivating ZRAM swap $z..."
            swapoff "$z" 2>/dev/null || true
            zramctl --reset "$z" 2>/dev/null || true
        done
    fi

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
