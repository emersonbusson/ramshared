#!/usr/bin/env bash
# Guest-side VMBus GPADL/UIO lifecycle drill for the ring-buffer upstream v2 series.
#
# Runs INSIDE a disposable ordinary x86_64 Hyper-V guest that booted the exact
# six-patch candidate. It is never run on the daily WSL2 host: it rebinds the
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
# class_id_show() emits "{%pUl}" WITH braces, but new_id_store() hands the
# buffer to guid_parse() -> uuid_is_valid(), which accepts exactly the 36-char
# canonical form. Braces make guid_parse() return -EINVAL, the dynid is never
# registered, and uio_hv_generic/bind fails every cycle. Read the class id
# from sysfs and strip the braces; do not hardcode a GUID that only holds for
# one device class.
CLS="$(cat "/sys/bus/vmbus/devices/$NIC/class_id" 2>/dev/null || true)"
# Strip exactly one leading { and one trailing }. The braces are escaped so the
# expansion is unambiguous in bash, dash and busybox ash alike; the unescaped
# `${CLS#{}` form parses differently across them and would leave a brace on.
CLS="${CLS#\{}"
CLS="${CLS%\}}"
if [ -z "$CLS" ]; then
	# HV_NIC_GUID, unbraced: the synthetic NIC's offer class.
	CLS='f8615163-df3e-46c5-913f-f2d2f965ed0e'
	say "CLS fallback=$CLS (sysfs class_id unreadable)"
else
	say "CLS=$CLS (from sysfs class_id, braces stripped)"
fi
DRIVER_DIR="/sys/bus/vmbus/drivers"

restore_nic() {
	say "RESTORE begin"
	echo "$NIC" >"$DRIVER_DIR/uio_hv_generic/unbind" 2>>"$LOG" || true
	echo "$CLS" >"$DRIVER_DIR/uio_hv_generic/remove_id" 2>>"$LOG" || true
	echo "$NIC" >"$DRIVER_DIR/hv_netvsc/bind" 2>>"$LOG" || true
	say "RESTORE driver=$(readlink -f "/sys/bus/vmbus/devices/$NIC/driver" 2>/dev/null || echo none)"
}

# The BUG-3 holds run in the background so their mappings are alive while
# restore_nic frees the ring. Reap them on every exit path so a failed cycle
# cannot leave a helper holding /dev/uio0 after the script is gone.
HOLD_PIDS=""
cleanup() {
	restore_nic
	for hp in $HOLD_PIDS; do
		wait "$hp" 2>/dev/null || true
	done
}
trap cleanup EXIT

# --- phase 1: lifecycle balance (open/close) --------------------------------
say "=== PHASE 1: $CYCLES bind/unbind cycles ==="
say "PHASE1-BEFORE $(vmbus_maps || echo 'MAPS unavailable')"

# A cycle that cannot bind is not a quiet log line: it means the exercise never
# ran. Count them and fail at the end, otherwise thirty consecutive bind_fail
# still exits 0 and init scores the drill as passed.
CYCLE_FAILS=0

for i in $(seq 1 "$CYCLES"); do
	echo "$NIC" >"$DRIVER_DIR/hv_netvsc/unbind" 2>>"$LOG" ||
		{ say "cycle$i unbind_fail"; CYCLE_FAILS=$((CYCLE_FAILS + 1)); }
	# new_id must succeed: without a registered dynid, uio_hv_generic has
	# id_table = NULL and will never bind. A silent || true here hid exactly
	# that failure for thirty cycles and left the BUG-3 path unexercised.
	#
	# vmbus_add_dynid() ends in driver_attach(), so new_id binds the
	# matching device by itself. A following explicit bind then hits
	# __driver_probe_device() with dev->driver already set and returns
	# -EBUSY, which counted a correct attach as bind_fail thirty times.
	# Verify the binding instead of requiring the redundant write.
	echo "$CLS" >"$DRIVER_DIR/uio_hv_generic/new_id" 2>>"$LOG" ||
		{ say "cycle$i newid_fail"; CYCLE_FAILS=$((CYCLE_FAILS + 1)); }
	bound="$(readlink -f "/sys/bus/vmbus/devices/$NIC/driver" 2>/dev/null || echo none)"
	case "$bound" in
	*/uio_hv_generic) ;;
	*)
		echo "$NIC" >"$DRIVER_DIR/uio_hv_generic/bind" 2>>"$LOG" ||
			{ say "cycle$i bind_fail"; CYCLE_FAILS=$((CYCLE_FAILS + 1)); }
		;;
	esac
	echo "$NIC" >"$DRIVER_DIR/uio_hv_generic/unbind" 2>>"$LOG" || true
	echo "$CLS" >"$DRIVER_DIR/uio_hv_generic/remove_id" 2>>"$LOG" || true
	echo "$NIC" >"$DRIVER_DIR/hv_netvsc/bind" 2>>"$LOG" ||
		{ say "cycle$i rebind_fail"; CYCLE_FAILS=$((CYCLE_FAILS + 1)); }
	if [ $((i % 10)) -eq 0 ]; then
		say "cycle$i $(vmbus_maps || echo 'MAPS unavailable')"
	fi
done
say "PHASE1 cycle_fails=$CYCLE_FAILS / $((CYCLES * 4)) steps"

say "PHASE1-AFTER $(vmbus_maps || echo 'MAPS unavailable')"

# --- phase 2: UIO mmap + hold-in-mmap (BUG-3 candidate repro) ---------------
say "=== PHASE 2: UIO mmap + hold-in-mmap ==="
echo "$NIC" >"$DRIVER_DIR/hv_netvsc/unbind" 2>>"$LOG" || true
# new_id's driver_attach() is what binds the device; no separate bind.
echo "$CLS" >"$DRIVER_DIR/uio_hv_generic/new_id" 2>>"$LOG" || true
sleep 1

UIO_DEV=""
for u in /sys/class/uio/uio*; do
	[ -e "$u/name" ] || continue
	UIO_DEV="/dev/$(basename "$u")"
	say "UIO $(basename "$u") name=$(cat "$u/name" 2>/dev/null) maps=$(ls "$u/maps" 2>/dev/null | tr '\n' ' ')"
done

# mmap-hold sleeps `hold` seconds before releasing, so it has to run
# CONCURRENTLY with the teardown. Running it in the foreground mapped, held,
# released, and only then let restore_nic run -- the mapping was already gone
# when the ring was freed, so the BUG-3 window was never open and a green run
# proved nothing about mmap-versus-release. Background it, give it a second to
# place the mappings, then tear down underneath them.
if [ -n "$UIO_DEV" ] && [ -e "$UIO_DEV" ] && [ -n "$HELPER" ]; then
	say "PHASE2 mmap all maps of $UIO_DEV (held across teardown)"
	# UIO map N lives at offset N * pagesize. 8 maps of 4 KiB covers every
	# map the driver advertises.
	"$HELPER" mmap-hold "$UIO_DEV" 4096 8 8 >>"$LOG" 2>&1 &
	HOLD_PIDS="$HOLD_PIDS $!"
else
	say "PHASE2 no UIO device or no vmbus_drill_helper; skipping UIO mmap"
	say "PHASE2 note UIO hold-in-mmap did NOT run"
fi

RING="$(find /sys/devices -path "*$NIC*" -name 'ring' 2>/dev/null | head -1 || true)"
if [ -n "$RING" ]; then
	say "PHASE2 sysfs ring present: ${RING#/sys}"
	if [ -n "$HELPER" ]; then
		# hv_uio_new_channel() opens subchannels with ring_bytes = SZ_2M,
		# and VMBUS_RING_SIZE(SZ_2M) is 512 pages. hv_uio_ring_mmap_prepare()
		# treats pgoff as a page offset into that ring, so mapping 4 MiB at
		# pgoff 0 asked for 1024 pages of a 512-page ring and
		# hv_uio_mmap_range_valid() correctly rejected it. Map the ring at
		# its real size. chan_attr_ring_buffer has no .size, so the length
		# is not discoverable from stat(); it is fixed at SZ_2M by the
		# driver.
		say "PHASE2 ring mmap 2097152 bytes = SZ_2M subchannel ring (held across teardown)"
		"$HELPER" mmap-hold "$RING" 2097152 8 1 >>"$LOG" 2>&1 &
		HOLD_PIDS="$HOLD_PIDS $!"
	else
		say "PHASE2 no vmbus_drill_helper; skipping ring mmap"
	fi
else
	say "PHASE2 no sysfs ring found"
fi

# Give the helpers time to open their mappings before the teardown races them.
sleep 2

say "=== PHASE 2 teardown while maps held (BUG-3 window) ==="
say "PHASE2-BEFORE-TEARDOWN $(vmbus_maps || echo 'MAPS unavailable')"
restore_nic
say "PHASE2-AFTER-TEARDOWN $(vmbus_maps || echo 'MAPS unavailable')"

# Collect the hold results. A mapping that was alive when restore_nic freed
# the ring is the BUG-3 window; maps=0 means it never opened. The MMAP_HOLD
# lines go to the console so the uploaded artifact carries the numbers (and
# the errno when a mmap fails).
for p in $HOLD_PIDS; do
	wait "$p" 2>/dev/null || true
done
say "--- MMAP_HOLD evidence ---"
grep 'MMAP_HOLD\|HELPER mmap\|HELPER open' "$LOG" | tee -a "$LOG" || true
if grep -qE 'MMAP_HOLD path=.* maps=[1-9]' "$LOG"; then
	PHASE2_RAN=yes
	say "PHASE2 hold-in-mmap window OPEN (mapping alive across restore_nic)"
else
	say "PHASE2 hold-in-mmap window did NOT open (no mapping succeeded)"
fi

# --- phase 3: reconciliation -------------------------------------------------
say "=== PHASE 3: reconciliation ==="
say "FINAL $(vmbus_maps || echo 'MAPS unavailable')"
say "BASELINE $BASE_MAPS"
say "devices_final=$(ls /sys/bus/vmbus/devices 2>/dev/null | wc -l)"

say "--- dmesg fault scan ---"
if dmesg 2>/dev/null | tail -400 | grep -E 'BUG:|Oops:|WARNING:|hung task|accept4 failed|page allocation failure' >>"$LOG"; then
	say "FAULTS_PRESENT see log"
else
	say "FAULTS_NONE"
fi

# Scoring. A green exit means the exercise actually ran:
#   - every bind/unbind step succeeded, and
#   - at least one mapping was alive while restore_nic freed the ring
#     (MMAP_HOLD ... maps>0 observed before teardown returned), so the
#     BUG-3 window was open rather than merely prepared.
# Reporting success after a silent skip -- or after a mapping that was
# already released -- is how a broken dynid registration looked green for
# thirty cycles and how a closed window looked like a hold-in-mmap pass.
if [ "${CYCLE_FAILS:-0}" -gt 0 ]; then
	say "LIFECYCLE_VERDICT=FAIL cycle_fails=$CYCLE_FAILS"
	say "=== END vmbus-lifecycle-drill ==="
	say "log=$LOG"
	exit 1
fi
if [ "${PHASE2_RAN:-no}" != yes ]; then
	say "LIFECYCLE_VERDICT=FAIL no mapping survived into the teardown (BUG-3 window not open)"
	say "=== END vmbus-lifecycle-drill ==="
	say "log=$LOG"
	exit 1
fi

say "LIFECYCLE_VERDICT=PASS cycles=$CYCLES phase2=$PHASE2_RAN"
say "=== END vmbus-lifecycle-drill ==="
say "log=$LOG"
exit 0
