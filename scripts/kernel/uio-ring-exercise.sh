#!/usr/bin/env bash
# Bounded UIO ring-mmap exercise with forced hv_netvsc restore.
set -u

# The instance id is host-assigned and differs on every VM. Discover it from
# the hv_netvsc binding; the first argument is an override only.
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

NIC="${1:-$(discover_nic || true)}"
if [ -z "${NIC:-}" ]; then
	echo "REFUSE: no synthetic NIC bound to hv_netvsc" >&2
	exit 2
fi
CLS='{f8615163-df3e-46c5-913f-f2d2f965ed0e}'
LOG="${2:-/var/tmp/uio-ring-exercise.log}"
: >"$LOG"
echo "NIC=$NIC (discovered from hv_netvsc binding)" | tee -a "$LOG"

restore() {
  echo "RESTORE begin" | tee -a "$LOG"
  echo "$NIC" >/sys/bus/vmbus/drivers/uio_hv_generic/unbind 2>>"$LOG" || true
  echo "$CLS" >/sys/bus/vmbus/drivers/uio_hv_generic/remove_id 2>>"$LOG" || true
  echo "$NIC" >/sys/bus/vmbus/drivers/hv_netvsc/bind 2>>"$LOG" || true
  echo "RESTORE driver=$(readlink -f /sys/bus/vmbus/devices/$NIC/driver 2>/dev/null)" | tee -a "$LOG"
}
trap restore EXIT

echo "STEP1 unbind netvsc" | tee -a "$LOG"
echo "$NIC" >/sys/bus/vmbus/drivers/hv_netvsc/unbind && echo unbind_ok | tee -a "$LOG" || echo unbind_fail | tee -a "$LOG"
sleep 1
echo "STEP2 new_id" | tee -a "$LOG"
echo "$CLS" >/sys/bus/vmbus/drivers/uio_hv_generic/new_id && echo new_id_ok | tee -a "$LOG" || echo new_id_fail | tee -a "$LOG"
echo "STEP3 bind uio" | tee -a "$LOG"
echo "$NIC" >/sys/bus/vmbus/drivers/uio_hv_generic/bind && echo bind_ok | tee -a "$LOG" || echo bind_fail | tee -a "$LOG"
sleep 1
echo "STEP4 uio class" | tee -a "$LOG"
ls /sys/class/uio/ | tee -a "$LOG"
for u in /sys/class/uio/uio*; do
  [ -d "$u" ] || continue
  echo "UIO $u name=$(cat "$u/name" 2>/dev/null)" | tee -a "$LOG"
  ls "$u/maps" 2>/dev/null | tee -a "$LOG"
done
echo "STEP5 ring sysfs" | tee -a "$LOG"
find /sys/devices -path "*$NIC*" -name 'ring' 2>/dev/null | tee -a "$LOG"
RING=$(find /sys/devices -path "*$NIC*" -name 'ring' 2>/dev/null | head -1)
echo "RING=$RING" | tee -a "$LOG"
if [ -n "$RING" ]; then
  ls -la "$RING" | tee -a "$LOG"
  # open/read the ring binary attribute if present
  if [ -e "$RING/ring" ]; then
    ls -la "$RING/ring" | tee -a "$LOG"
    dd if="$RING/ring" of=/dev/null bs=4096 count=1 status=none && echo ring_read_ok | tee -a "$LOG" || echo ring_read_fail | tee -a "$LOG"
  fi
fi
echo "STEP6 unbind uio (drain+free)" | tee -a "$LOG"
echo "$NIC" >/sys/bus/vmbus/drivers/uio_hv_generic/unbind && echo uio_unbind_ok | tee -a "$LOG" || echo uio_unbind_fail | tee -a "$LOG"
sleep 1
echo "STEP7 splats" | tee -a "$LOG"
dmesg | grep -iE 'BUG:|Oops:|panic:|FORTIFY|uio_hv|vmbus_free' | tail -25 | tee -a "$LOG"
echo DONE | tee -a "$LOG"
