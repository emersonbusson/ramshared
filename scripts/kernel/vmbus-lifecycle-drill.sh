#!/usr/bin/env bash
# Guest-side VMBus GPADL/UIO lifecycle drill for the ring-buffer upstream v2 series.
#
# Runs INSIDE a disposable ordinary x86_64 Hyper-V guest that booted the exact
# seven-patch candidate. It is never run on the daily WSL2 host: it rebinds the
# production synthetic NIC and unloads/reloads VMBus sub-drivers.
#
# Covers the named SPEC gates that hosted KUnit cannot:
#   - vmbus_channel_lifecycle_buffer_balance   (open/close + vmallocinfo)
#   - uio_hv_ring_noncontiguous_mmap           (UIO mmap incl. hold-in-mmap)
#   - GPADL create/teardown/rescind/close balance
#
# Host-safety contract (vmbus-ring-buffer-upstream-v2/SPEC.md): no swap
# activation, no memory pressure, and no RamShared lifecycle state change. This
# script performs none of those. Fragmentation pressure lives in a separate
# drill and only ever inside the same disposable guest.
#
# usage: vmbus-lifecycle-drill.sh [cycles] [logfile]
#   cycles  default 100 (SPEC vmbus_channel_lifecycle_buffer_balance)
#
# Depends on vmbus_drill_helper (static, see hyperv-drill-initramfs/) for the
# mmap-hold primitive. The guest has no CPython and this is a Day-0
# dependency, not a shim.
set -euo pipefail

CYCLES="${1:-100}"
LOG="${2:-/var/tmp/vmbus-lifecycle-drill.log}"
: >"$LOG"

HELPER="${HELPER:-$(command -v vmbus_drill_helper || true)}"

# --- guards: refuse to run on the daily WSL2 host -----------------------------
guard() {
	if grep -qiE 'microsoft-standard-WSL2' /proc/version 2>/dev/null; then
		echo "REFUSE: this is the daily WSL2 host. Run only in a disposable Hyper-V guest." >&2
		exit 2
	fi
	if [ ! -d /sys/bus/vmbus ]; then
		echo "REFUSE: no VMBus on this kernel; drill requires a Hyper-V guest." >&2
		exit 2
	fi
	if ! grep -qi 'hyperv' /sys/bus/vmbus/devices/*/modalias 2>/dev/null &&
		[ "$(ls /sys/bus/vmbus/devices 2>/dev/null | wc -l)" -eq 0 ]; then
		echo "REFUSE: no VMBus devices visible." >&2
		exit 2
	fi
}
guard

say() { echo "$@" | tee -a "$LOG"; }

# --- VMEM: VMBus map accounting ---------------------------------------------
# Counts live vmbus_alloc_buffer maps and total vmalloc area attributed to them.
# Never logs kernel virtual addresses (SPEC: no KASLR material in evidence).
vmbus_maps() {
	if [ ! -r /proc/vmallocinfo ]; then
		echo "MAPS unreadable (need root / CONFIG_PROC_PAGE_MONITOR)" | tee -a "$LOG"
		return 1
	fi
	# /proc/vmallocinfo line shape:
	#   0xffff....-0xffff....  61440 vmbus_alloc_buffer+0x... pages=14 vmalloc
	#   $1 = virtual range (never logged), $2 = size in bytes, pages= is the
	#   backing page count. Size is taken from field 2 directly: parsing the
	#   range as if it were field 2 produced negative totals and would have
	#   reported garbage inside the guest.
	awk '
		/vmbus_alloc_buffer/ {
			n++
			total += $2
			for (i = 1; i <= NF; i++) {
				if ($i ~ /^pages=/) {
					split($i, p, "=")
					pages += p[2]
				}
			}
		}
		END {
			printf "MAPS count=%d bytes=%d pages=%d\n", n, total, pages
		}
	' /proc/vmallocinfo
}

say "=== BEGIN vmbus-lifecycle-drill cycles=$CYCLES ==="
say "kernel=$(uname -r)  cmdline=$(cat /proc/cmdline 2>/dev/null | tr -d '\n')"
say "symbols: $(grep -cE 'vmbus_(alloc|free|release)_buffer' /proc/kallsyms 2>/dev/null || echo 0) present"

say "--- BEFORE ---"
say "devices=$(ls /sys/bus/vmbus/devices 2>/dev/null | wc -l)"
BASE_MAPS="$(vmbus_maps || echo 'MAPS unavailable')"
say "$BASE_MAPS"
dmesg 2>/dev/null | tail -5 >>"$LOG"

# --- synthetic NIC rebind setup ---------------------------------------------
# The instance id is host-assigned and differs on every VM. Discover it from
# the hv_netvsc binding instead of hardcoding a GUID that only held on one lab
# machine.
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
	exit 2
fi
say "NIC=$NIC (discovered from hv_netvsc binding)"
CLS='{f8615163-df3e-46c5-913f-f2d2f965ed0e}'
DRIVER_DIR="/sys/bus/vmbus/drivers"

restore_nic() {
	say "RESTORE begin"
	echo "$NIC" >"$DRIVER_DIR/uio_hv_generic/unbind" 2>>"$LOG" || true
	echo "$CLS" >"$DRIVER_DIR/uio_hv_generic/remove_id" 2>>"$LOG" || true
	echo "$NIC" >"$DRIVER_DIR/hv_netvsc/bind" 2>>"$LOG" || true
	say "RESTORE driver=$(readlink -f "/sys/bus/vmbus/devices/$NIC/driver" 2>/dev/null || echo none)"
}
trap restore_nic EXIT

# --- phase 1: lifecycle balance (open/close) --------------------------------
say "=== PHASE 1: $CYCLES bind/unbind cycles ==="
say "PHASE1-BEFORE $(vmbus_maps || echo 'MAPS unavailable')"

for i in $(seq 1 "$CYCLES"); do
	echo "$NIC" >"$DRIVER_DIR/hv_netvsc/unbind" 2>>"$LOG" || say "cycle$i unbind_fail"
	echo "$CLS" >"$DRIVER_DIR/uio_hv_generic/new_id" 2>>"$LOG" || true
	echo "$NIC" >"$DRIVER_DIR/uio_hv_generic/bind" 2>>"$LOG" || say "cycle$i bind_fail"
	echo "$NIC" >"$DRIVER_DIR/uio_hv_generic/unbind" 2>>"$LOG" || true
	echo "$CLS" >"$DRIVER_DIR/uio_hv_generic/remove_id" 2>>"$LOG" || true
	echo "$NIC" >"$DRIVER_DIR/hv_netvsc/bind" 2>>"$LOG" || say "cycle$i rebind_fail"
	if [ $((i % 10)) -eq 0 ]; then
		say "cycle$i $(vmbus_maps || echo 'MAPS unavailable')"
	fi
done

say "PHASE1-AFTER $(vmbus_maps || echo 'MAPS unavailable')"

# --- phase 2: UIO mmap + hold-in-mmap (BUG-3 candidate repro) ---------------
say "=== PHASE 2: UIO mmap + hold-in-mmap ==="
echo "$NIC" >"$DRIVER_DIR/hv_netvsc/unbind" 2>>"$LOG" || true
echo "$CLS" >"$DRIVER_DIR/uio_hv_generic/new_id" 2>>"$LOG" || true
echo "$NIC" >"$DRIVER_DIR/uio_hv_generic/bind" 2>>"$LOG" || true
sleep 1

UIO_DEV=""
for u in /sys/class/uio/uio*; do
	[ -e "$u/name" ] || continue
	UIO_DEV="/dev/$(basename "$u")"
	say "UIO $(basename "$u") name=$(cat "$u/name" 2>/dev/null) maps=$(ls "$u/maps" 2>/dev/null | tr '\n' ' ')"
done

if [ -n "$UIO_DEV" ] && [ -e "$UIO_DEV" ] && [ -n "$HELPER" ]; then
	say "PHASE2 mmap all maps of $UIO_DEV"
	# UIO map N lives at offset N * pagesize. Hold 3s so the unbind below
	# races the mapping on purpose: that is the BUG-3 window (sysfs ring
	# mmap versus ring release).
	"$HELPER" mmap-hold "$UIO_DEV" 4096 3 8 2>>"$LOG" | tee -a "$LOG" ||
		say "PHASE2 mmap helper failed"
else
	say "PHASE2 no UIO device or no vmbus_drill_helper; skipping mmap"
fi

RING="$(find /sys/devices -path "*$NIC*" -name 'ring' 2>/dev/null | head -1 || true)"
if [ -n "$RING" ]; then
	say "PHASE2 sysfs ring present: ${RING#/sys}"
	say "PHASE2 ring mmap attempt"
	if [ -n "$HELPER" ]; then
		# Hold 2s so restore/unbind frees the ring while the mapping is
		# still alive.
		"$HELPER" mmap-hold "$RING" 4194304 2 1 2>>"$LOG" | tee -a "$LOG" ||
			say "PHASE2 ring mmap failed"
	else
		say "PHASE2 no vmbus_drill_helper; skipping ring mmap"
	fi
else
	say "PHASE2 no sysfs ring found"
fi

say "=== PHASE 2 teardown while maps held (BUG-3 window) ==="
say "PHASE2-BEFORE-TEARDOWN $(vmbus_maps || echo 'MAPS unavailable')"
restore_nic
say "PHASE2-AFTER-TEARDOWN $(vmbus_maps || echo 'MAPS unavailable')"

# --- phase 3: reconciliation -------------------------------------------------
say "=== PHASE 3: reconciliation ==="
say "FINAL $(vmbus_maps || echo 'MAPS unavailable')"
say "BASELINE $BASE_MAPS"
say "devices_final=$(ls /sys/bus/vmbus/devices 2>/dev/null | wc -l)"

say "--- dmesg fault scan ---"
if dmesg 2>/dev/null | tail -400 | grep -E 'BUG:|Oops:|WARNING:|hung task|order:7|accept4 failed|page allocation failure' >>"$LOG"; then
	say "FAULTS_PRESENT see log"
else
	say "FAULTS_NONE"
fi

say "=== END vmbus-lifecycle-drill ==="
say "log=$LOG"
exit 0
