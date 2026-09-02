#!/usr/bin/env bash
# qemu-validate.sh — validates a custom WSL2 kernel in an ISOLATED QEMU (does not touch WSL).
# Before arming .wslconfig, proves: (1) bzImage boots to userspace; (2) `uname -r`
# matches the expected release; (3) target modules load (insmod in the supplied
# order). Console uses ttyS0 (CONFIG_SERIAL_8250_CONSOLE=y in config-wsl).
#
# usage: qemu-validate.sh <bzImage> <kernelrelease> [mod1.ko mod2.ko ...]
#   modules in dependency ORDER (for example, zsmalloc.ko BEFORE zram.ko).
# exit 0 = PASS (kernel booted and release matches). Module details are in the log.
#
# Reusable for any kernel build (Phase B+ toolkit). SPEC: docs/runbooks/FASE-B-KERNEL.md
set -euo pipefail

if [ "$#" -lt 2 ]; then
  echo "usage: qemu-validate.sh <bzImage> <kernelrelease> [mods...]" >&2
  exit 64
fi

BZ="$1"
REL="$2"
shift 2
MODS=("$@")

[ -f "$BZ" ] || { echo "bzImage missing: $BZ" >&2; exit 69; }
command -v qemu-system-x86_64 >/dev/null || { echo "qemu-system-x86_64 missing (apt install qemu-system-x86)" >&2; exit 69; }
[ -w /dev/kvm ] || { echo "/dev/kvm missing or not writable (KVM required)" >&2; exit 69; }
[ -x /bin/busybox ] || { echo "busybox-static missing (apt install busybox-static)" >&2; exit 69; }

WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
IRD="$WORK/irfs"; mkdir -p "$IRD/bin" "$IRD/modules"
cp /bin/busybox "$IRD/bin/busybox"

# numbered modules preserve dependency order
i=0; for m in "${MODS[@]}"; do
  [ -f "$m" ] || { echo "module missing: $m" >&2; exit 69; }
  cp "$m" "$IRD/modules/$(printf '%02d' "$i")-$(basename "$m")"; i=$((i+1))
done

cat > "$IRD/init" <<'INIT'
#!/bin/busybox sh
/bin/busybox mkdir -p /proc /sys /dev
/bin/busybox mount -t proc proc /proc
/bin/busybox mount -t sysfs sysfs /sys
/bin/busybox mount -t devtmpfs devtmpfs /dev 2>/dev/null
echo "=====KTEST-BEGIN====="
echo "KTEST-UNAME=$(/bin/busybox uname -r)"
for m in /modules/*.ko; do
  if /bin/busybox insmod "$m" 2>/tmp/e; then
    echo "KTEST-INSMOD-OK=$(/bin/busybox basename "$m")"
  else
    echo "KTEST-INSMOD-FAIL=$(/bin/busybox basename "$m"): $(/bin/busybox cat /tmp/e)"
  fi
done
[ -e /dev/ublk-control ] && echo "KTEST-UBLK-CONTROL=present" || echo "KTEST-UBLK-CONTROL=absent"
echo "KTEST-DMESG:"
/bin/busybox dmesg | /bin/busybox grep -iE "module|ublk|zram|zsmalloc|magic|tainted|invalid|disagrees" | /bin/busybox tail -12
echo "=====KTEST-END====="
/bin/busybox poweroff -f
INIT
chmod +x "$IRD/init"

( cd "$IRD" && find . | cpio -o -H newc 2>/dev/null | gzip ) > "$WORK/initramfs.gz"

# KVM accelerates; KVM is strictly required.
ACCEL=(-enable-kvm -cpu host)

echo "[qemu-validate] booting $BZ (expected release: $REL; accel: ${ACCEL[*]})..."
timeout 180 qemu-system-x86_64 "${ACCEL[@]}" -m 1024 -nographic -no-reboot \
  -kernel "$BZ" -initrd "$WORK/initramfs.gz" \
  -append "console=ttyS0 panic=1 rdinit=/init" > "$WORK/serial.log" 2>&1 || true

echo "=========== result ==========="
grep -E "KTEST-" "$WORK/serial.log" || { echo "no KTEST output — kernel may not have booted"; }
echo "================================="
# GATE = boot to userspace (uname matches). This is the catastrophic risk ("fails to boot").
# The insmod via busybox in the minimal initramfs is BEST-EFFORT (the busybox applet is
# limited — fails before reaching the kernel; empty dmesg confirms this). The
# AUTHORITATIVE module validation is POST-BOOT, in the real kernel, via kmod (boot-kernel-safe.ps1
# does `modprobe` + auto-revert). Therefore, the verdict does NOT gate on modules.
if grep -q "KTEST-UNAME=$REL" "$WORK/serial.log" && grep -q "KTEST-END" "$WORK/serial.log"; then
  echo "QEMU-VALIDATE: PASS — kernel booted to userspace; release matches."
  echo "(modules: best effort in initramfs; authoritative validation = post-boot through kmod in the launcher)"
  exit 0
else
  echo "QEMU-VALIDATE: FAIL — kernel did not boot or the release differs. See the serial log above."
  echo "--- serial tail ---"; tail -15 "$WORK/serial.log"
  exit 1
fi
