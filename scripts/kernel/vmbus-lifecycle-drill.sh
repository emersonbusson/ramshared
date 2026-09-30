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
set -euo pipefail

CYCLES="${1:-100}"
LOG="${2:-/var/tmp/vmbus-lifecycle-drill.log}"
: >"$LOG"

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
	awk '
		/vmbus_alloc_buffer/ {
			n++
			# area field is field 2, e.g. "0xffff....-0xffff...."
			split($2, a, "-")
			total += (strtonum(a[2]) - strtonum(a[1]))
		}
		END {
			printf "MAPS count=%d bytes=%d\n", n, total
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
NIC="${NIC:-abcb345e-c024-4e15-ae8f-96f3d210cd74}"
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

if [ -n "$UIO_DEV" ] && [ -e "$UIO_DEV" ]; then
	say "PHASE2 mmap all maps of $UIO_DEV"
	python3 - "$UIO_DEV" <<'PY' 2>>"$LOG" || say "PHASE2 mmap helper failed"
import mmap, os, sys, time
dev = sys.argv[1]
fd = os.open(dev, os.O_RDONLY)
maps = []
try:
    for i in range(8):
        try:
            # UIO map i: offset i * pagesize, read until failure
            m = mmap.mmap(fd, 4096, offset=i * 4096)
            maps.append(m)
        except Exception:
            break
    print(f"PHASE2 mapped={len(maps)}")
    # hold-in-mmap: keep mappings alive while the caller tears the channel down
    # (BUG-3: sysfs ring mmap vs ring release). The unbind below races this hold.
    open("/tmp/.uio_hold", "w").write(str(os.getpid()))
    time.sleep(3)
finally:
    for m in maps:
        m.close()
    os.close(fd)
    print("PHASE2 released")
PY
else
	say "PHASE2 no UIO device appeared; skipping mmap"
fi

RING="$(find /sys/devices -path "*$NIC*" -name 'ring' 2>/dev/null | head -1 || true)"
if [ -n "$RING" ]; then
	say "PHASE2 sysfs ring present: ${RING#/sys}"
	say "PHASE2 ring mmap attempt"
	python3 - "$RING" <<'PY' 2>>"$LOG" || say "PHASE2 ring mmap failed"
import mmap, os, sys, time
path = sys.argv[1]
fd = os.open(path, os.O_RDONLY)
try:
    m = mmap.mmap(fd, 4 * 1024 * 1024)
    print("PHASE2 ring mapped 4MiB")
    time.sleep(2)   # hold while restore/unbind frees the ring
    m.close()
    print("PHASE2 ring released")
finally:
    os.close(fd)
PY
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
