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
#
# Depends on vmbus_drill_helper (static, see hyperv-drill-initramfs/) for the
# mlock-hog primitive. The guest has no CPython and this is a Day-0
# dependency, not a shim.
set -euo pipefail

LOG="${2:-/var/tmp/vmbus-fragmentation-drill.log}"
: >"$LOG"

HELPER="${HELPER:-$(command -v vmbus_drill_helper || true)}"

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
if [ -z "$HELPER" ]; then
	say "REFUSE: vmbus_drill_helper not found; cannot fragment without mlock-hog"
	exit 2
fi
# 600s hold: the channel-open exercise and the buddy snapshots below have to
# finish while the pages are still locked.
"$HELPER" mlock-hog "$HOGB" 600 >>"$LOG" 2>&1 &
HOGPID=$!

for _ in $(seq 1 60); do
	grep -q 'MLOCK_HOG ready=1' "$LOG" 2>/dev/null && break
	sleep 1
done
if ! grep -q 'MLOCK_HOG ready=1' "$LOG" 2>/dev/null; then
	say "HOG did not reach ready state"
fi

say "--- BUDDY AFTER HOG ---"
buddy

# --- exercise channel open under fragmentation ------------------------------
say "=== CHANNEL OPEN UNDER FRAGMENTATION ==="

# The instance id is host-assigned and differs on every VM. Discover it from
# the hv_netvsc binding.
discover_nic() {
	local d n
	for d in /sys/bus/vmbus/drivers/hv_netvsc/*; do
		[ -e "$d" ] || continue
		n="$(basename "$d")"
		case "$n" in
		bind | unbind | uevent | module | new_id | remove_id) continue ;;
		esac
		[ -d "$d" ] || continue
		printf '%s\n' "$n"
		return 0
	done
	return 1
}

NIC="${NIC:-$(discover_nic || true)}"
if [ -z "${NIC:-}" ]; then
	say "REFUSE: no synthetic NIC bound to hv_netvsc; cannot force ring realloc"
	kill "$HOGPID" 2>/dev/null || true
	wait "$HOGPID" 2>/dev/null || true
	exit 2
fi
say "NIC=$NIC (discovered from hv_netvsc binding)"
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
say "=== END vmbus-fragmentation-drill ==="
say "log=$LOG"
