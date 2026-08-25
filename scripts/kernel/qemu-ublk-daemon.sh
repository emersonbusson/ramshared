#!/usr/bin/env bash
# qemu-ublk-daemon.sh — validates the ublk daemon LIFECYCLE (`--backend ram`) in an
# ISOLATED QEMU. Proves without risk to the host: (1) insmod ublk_drv; (2) the daemon starts and
# creates /dev/ublkbN; (3) serves I/O (dd write+read); (4) SIGTERM -> ordered teardown
# (STOP_DEV -> join -> DEL_DEV) -> device removed and daemon exits 0.
#
# WHY QEMU: running this daemon in WSL2 FROZE the host (orphaned device on teardown ->
# I/O in D-state). In a VM, any stall is contained by `timeout` — the host remains intact.
# RAM backend (no GPU): `Cuda::load()` is only called in the VRAM path, so the binary
# (CUDA via dlopen) runs without libcuda. The teardown bug is independent of the backend.
#
# usage: qemu-ublk-daemon.sh [bzImage] [daemon_bin] [ublk_drv.ko]
# exit 0 = PASS (serve plus clean teardown). SPEC: docs/ublk-daemon-integration/IMPL.md F2.
set -euo pipefail

BZ="${1:-$HOME/WSL2-Linux-Kernel/arch/x86/boot/bzImage}"
DAEMON="${2:-$(dirname "$0")/../../target/debug/ramsharedd}"
UBLK_KO="${3:-$HOME/WSL2-Linux-Kernel/drivers/block/ublk_drv.ko}"

for f in "$BZ" "$DAEMON" "$UBLK_KO"; do
  [ -f "$f" ] || { echo "missing file: $f" >&2; exit 2; }
done
command -v qemu-system-x86_64 >/dev/null || { echo "qemu-system-x86_64 missing" >&2; exit 2; }
[ -x /bin/busybox ] || { echo "busybox-static missing" >&2; exit 2; }

WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
IRD="$WORK/irfs"; mkdir -p "$IRD/bin" "$IRD/modules"
cp /bin/busybox "$IRD/bin/busybox"
cp "$DAEMON" "$IRD/ramsharedd"
cp "$UBLK_KO" "$IRD/modules/ublk_drv.ko"
sha256sum "$DAEMON" | awk '{print $1}' > "$IRD/expected-ramsharedd.sha256"

# Copies the daemon's dynamic libraries while preserving absolute paths (the binary is
# glibc-dynamic; no CUDA on load — see ldd). The /lib64/ld-linux linker is included as well.
for lib in $(ldd "$DAEMON" | grep -oE '/[^ ]+\.so[^ ]*'); do
  mkdir -p "$IRD$(dirname "$lib")"
  cp "$lib" "$IRD$lib"
done

cat > "$IRD/init" <<'INIT'
#!/bin/busybox sh
BB=/bin/busybox
$BB mkdir -p /proc /sys /dev /tmp
$BB mount -t proc proc /proc
$BB mount -t sysfs sysfs /sys
$BB mount -t devtmpfs devtmpfs /dev 2>/dev/null
echo "=====KTEST-BEGIN====="
echo "KTEST-UNAME=$($BB uname -r)"
EXPECTED="$($BB cat /expected-ramsharedd.sha256 2>/dev/null)"
ACTUAL="$($BB sha256sum /ramsharedd 2>/dev/null)"
ACTUAL="${ACTUAL%% *}"
echo "KTEST-BINARY-SHA-EXPECTED=$EXPECTED"
echo "KTEST-BINARY-SHA-ACTUAL=$ACTUAL"
[ -n "$EXPECTED" ] && [ "$EXPECTED" = "$ACTUAL" ] && echo "KTEST-BINARY-MATCH=ok" || echo "KTEST-BINARY-MATCH=fail"

# 1) loads the ublk driver
if $BB insmod /modules/ublk_drv.ko 2>/tmp/e; then
  echo "KTEST-INSMOD=ok"
else
  echo "KTEST-INSMOD=fail: $($BB cat /tmp/e)"
fi
[ -e /dev/ublk-control ] && echo "KTEST-UBLK-CONTROL=present" || echo "KTEST-UBLK-CONTROL=absent"

# 2) starts the daemon in the isolated generic-Linux QEMU guest (--force for best-effort mlockall)
/ramsharedd --transport ublk --backend ram \
  --size 8 --queue-depth 1 --force >/tmp/daemon.log 2>&1 &
DPID=$!
echo "KTEST-DAEMON-PID=$DPID"

# 3) waits for /dev/ublkb0 to appear (bounded ~15s)
DEV=""
i=0
while [ $i -lt 150 ]; do
  for n in /dev/ublkb0 /dev/ublkb1; do [ -b "$n" ] && DEV="$n"; done
  [ -n "$DEV" ] && break
  $BB kill -0 "$DPID" 2>/dev/null || { echo "KTEST-DAEMON-DIED-EARLY=1"; break; }
  $BB sleep 0.1; i=$((i+1))
done
if [ -n "$DEV" ]; then
  echo "KTEST-DEVICE=$DEV"
  # 4) serves I/O: 4 KB write and read
  if $BB dd if=/dev/zero of="$DEV" bs=4096 count=1 conv=fsync 2>/dev/null \
     && $BB dd if="$DEV" of=/dev/null bs=4096 count=1 2>/dev/null; then
    echo "KTEST-SERVED=ok"
  else
    echo "KTEST-SERVED=fail"
  fi
  # 5) SIGTERM -> ordered teardown; waits for the daemon to exit (bounded ~12s)
  $BB kill -TERM "$DPID"
  j=0; GONE=0
  while [ $j -lt 120 ]; do
    $BB kill -0 "$DPID" 2>/dev/null || { GONE=1; break; }
    $BB sleep 0.1; j=$((j+1))
  done
  if [ $GONE -eq 1 ]; then
    $BB wait "$DPID" 2>/dev/null; echo "KTEST-TERMINATED=ok"
  else
    echo "KTEST-TERMINATED=timeout"
  fi
  # 6) device removed by teardown?
  [ -b "$DEV" ] && echo "KTEST-DEVICE-REMOVED=no" || echo "KTEST-DEVICE-REMOVED=ok"
else
  echo "KTEST-DEVICE=absent"
fi

echo "KTEST-DAEMON-LOG:"
$BB tail -8 /tmp/daemon.log 2>/dev/null
echo "=====KTEST-END====="
$BB poweroff -f
INIT
chmod +x "$IRD/init"

( cd "$IRD" && find . | cpio -o -H newc 2>/dev/null | gzip ) > "$WORK/initramfs.gz"

ACCEL=(-machine accel=tcg)
[ -w /dev/kvm ] && ACCEL=(-enable-kvm -cpu host)

echo "[qemu-ublk-daemon] booting (accel: ${ACCEL[*]})..."
timeout 180 qemu-system-x86_64 "${ACCEL[@]}" -m 512 -smp 2 -nographic -no-reboot \
  -kernel "$BZ" -initrd "$WORK/initramfs.gz" \
  -append "console=ttyS0 panic=1 rdinit=/init" > "$WORK/serial.log" 2>&1 || true

echo "=========== result ==========="
grep -E "KTEST-" "$WORK/serial.log" || echo "no KTEST output — kernel may not have booted"
echo "================================="
if grep -q "KTEST-SERVED=ok" "$WORK/serial.log" \
   && grep -q "KTEST-BINARY-MATCH=ok" "$WORK/serial.log" \
   && grep -q "KTEST-TERMINATED=ok" "$WORK/serial.log" \
   && grep -q "KTEST-DEVICE-REMOVED=ok" "$WORK/serial.log"; then
  echo "QEMU-UBLK-DAEMON: PASS — daemon served I/O and completed clean teardown (SIGTERM)."
  exit 0
else
  echo "QEMU-UBLK-DAEMON: FAIL/INCONCLUSIVE — see the KTEST output above and the serial log."
  echo "--- tail serial ---"; tail -25 "$WORK/serial.log"
  exit 1
fi
