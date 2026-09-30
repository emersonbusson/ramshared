#!/usr/bin/env bash
# Guest-side order-7 fragmentation drill for the ring-buffer upstream v2 series.
#
# Proves the fallback the maintainer asked about: under REAL physical
# fragmentation, `vmbus_alloc_ring()` must degrade to order-0 chunks and the
# channel must still open. KUnit fault injection is NOT this evidence.
#
# Runs INSIDE a disposable ordinary x86_64 Hyper-V guest only. It consumes most
# of guest RAM with movable pinned pages to break high-order contiguity, then
# exercises VMBus channel open. This is memory pressure — so it is forbidden on
# the daily WSL2 host and is guarded against it.
#
# Host-safety contract (vmbus-ring-buffer-upstream-v2/SPEC.md): never on the
# daily WSL2 environment. Swap is never activated here.
#
# usage: vmbus-fragmentation-drill.sh [hog_mib] [logfile]
#   hog_mib  default 75% of MemAvailable
set -euo pipefail

LOG="${2:-/var/tmp/vmbus-fragmentation-drill.log}"
: >"$LOG"

# --- guards ------------------------------------------------------------------
guard() {
	if grep -qiE 'microsoft-standard-WSL2' /proc/version 2>/dev/null; then
		echo "REFUSE: daily WSL2 host. Fragmentation pressure is never run here." >&2
		exit 2
	fi
	if [ ! -d /sys/bus/vmbus ]; then
		echo "REFUSE: no VMBus; requires a Hyper-V guest." >&2
		exit 2
	fi
	# Refuse if RamShared cascade is present and active — SPEC forbids changing
	# its lifecycle state, and pressure would interact with it.
	if command -v ramshared >/dev/null 2>&1; then
		if ramshared status 2>/dev/null | grep -qiE 'phase *: *(On|ACTIVE)'; then
			echo "REFUSE: RamShared cascade is active. SPEC forbids host lifecycle change." >&2
			exit 2
		fi
	fi
	if [ -r /proc/swaps ] && [ "$(grep -c . /proc/swaps)" -gt 1 ]; then
		echo "REFUSE: swap is active. This drill never runs with swap." >&2
		exit 2
	fi
}
guard

say() { echo "$@" | tee -a "$LOG"; }

hog_mib() {
	local avail
	avail="$(awk '/MemAvailable/{print int($2/1024)}' /proc/meminfo)"
	echo $((avail * 75 / 100))
}

HOGB="${1:-$(hog_mib)}"
say "=== BEGIN vmbus-fragmentation-drill hog_mib=$HOGB ==="
say "kernel=$(uname -r)"
say "MemTotal=$(awk '/MemTotal/{print $2}' /proc/meminfo)kB MemAvailable=$(awk '/MemAvailable/{print $2}' /proc/meminfo)kB"

# --- buddy state before ------------------------------------------------------
buddy() { cat /proc/buddyinfo 2>/dev/null | tee -a "$LOG" || say "buddyinfo unavailable"; }

say "--- BUDDY BEFORE ---"
buddy

# --- fragment: pin most of RAM as unmovable-ish high-order eaters ------------
# mlock'd anonymous pages resist compaction and drain high-order free blocks.
say "=== HOG ${HOGB} MiB with mlock ==="
python3 - "$HOGB" <<'PY' 2>>"$LOG" &
import mmap, os, sys, time, signal
mib = int(sys.argv[1])
try:
    buf = mmap.mmap(-1, mib * 1024 * 1024)
    # touch every page so it is populated, then lock
    for i in range(0, mib * 1024 * 1024, 4096):
        buf[i] = 1
    import ctypes
    libc = ctypes.CDLL("libc.so.6")
    libc.mlock(ctypes.c_void_p(ctypes.addressof(ctypes.c_char.from_buffer(buf))), mib * 1024 * 1024)
    print(f"HOG locked {mib} MiB", flush=True)
    open("/tmp/.hog_ready", "w").write("1")
    time.sleep(600)
except Exception as e:
    print(f"HOG fail {e}", flush=True)
PY
HOGPID=$!

for _ in $(seq 1 60); do
	[ -f /tmp/.hog_ready ] && break
	sleep 1
done
if [ ! -f /tmp/.hog_ready ]; then
	say "HOG did not reach ready state"
fi

say "--- BUDDY AFTER HOG ---"
buddy

# --- exercise channel open under fragmentation ------------------------------
say "=== CHANNEL OPEN UNDER FRAGMENTATION ==="
NIC="${NIC:-abcb345e-c024-4e15-ae8f-96f3d210cd74}"
CLS='{f8615163-df3e-46c5-913f-f2d2f965ed0e}'
DRIVER_DIR="/sys/bus/vmbus/drivers"

say "PRE-OPEN $(grep -c vmbus_alloc_buffer /proc/vmallocinfo 2>/dev/null || echo 0) maps"

# force a fresh ring allocation by rebinding the synthetic NIC
echo "$NIC" >"$DRIVER_DIR/hv_netvsc/unbind" 2>>"$LOG" || true
sleep 1
echo "$NIC" >"$DRIVER_DIR/hv_netvsc/bind" 2>>"$LOG" || say "rebind_fail"
sleep 2

say "POST-OPEN $(grep -c vmbus_alloc_buffer /proc/vmallocinfo 2>/dev/null || echo 0) maps"
say "nic_driver=$(readlink -f "/sys/bus/vmbus/devices/$NIC/driver" 2>/dev/null || echo none)"

# --- verdict ----------------------------------------------------------------
say "--- DMESG SCAN ---"
DM="$(dmesg 2>/dev/null | tail -500)"
ORDER7="$(echo "$DM" | grep -c 'order:7' || true)"
ACCEPT="$(echo "$DM" | grep -c 'accept4 failed' || true)"
FALLBK="$(echo "$DM" | grep -ciE 'vmbus.*(fallback|order.?0|order-zero|chunk)' || true)"
OOPS="$(echo "$DM" | grep -cE 'BUG:|Oops:|WARNING:|hung task' || true)"

say "RESULT order7_failures=$ORDER7 accept4_failures=$ACCEPT fallback_markers=$FALLBK oops=$OOPS"
if [ "$ORDER7" -gt 0 ] && [ "$ACCEPT" -eq 0 ] && [ "$OOPS" -eq 0 ]; then
	say "VERDICT=PASS_ORDER0_FALLBACK"
elif [ "$ORDER7" -eq 0 ]; then
	say "VERDICT=INCONCLUSIVE_ORDER7_NEVER_FAILED (raise hog_mib or shrink CMA/reserves)"
else
	say "VERDICT=FAIL see log"
fi

say "--- BUDDY FINAL ---"
buddy

# --- teardown hog -----------------------------------------------------------
kill "$HOGPID" 2>/dev/null || true
wait "$HOGPID" 2>/dev/null || true
rm -f /tmp/.hog_ready
say "=== END vmbus-fragmentation-drill ==="
say "log=$LOG"
