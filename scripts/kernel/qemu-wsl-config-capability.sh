#!/usr/bin/env bash
# Boot the candidate WSL kernel in QEMU and validate only config capabilities.
set -euo pipefail

BZIMAGE="${1:?usage: $0 <bzImage> <kernelrelease> <zsmalloc.ko> <zram.ko> <ublk_drv.ko>}"
KERNEL_RELEASE="${2:?missing kernel release}"
ZSMALLOC_MODULE="${3:?missing zsmalloc module}"
ZRAM_MODULE="${4:?missing zram module}"
UBLK_MODULE="${5:?missing ublk module}"
INIT_SOURCE="$(dirname "$0")/qemu-wsl-config-capability-init.sh"

for file in "$BZIMAGE" "$ZSMALLOC_MODULE" "$ZRAM_MODULE" "$UBLK_MODULE" "$INIT_SOURCE"; do
  test -f "$file" || { printf 'missing input: %s\n' "$file" >&2; exit 2; }
done
for command_name in busybox cpio gzip qemu-system-x86_64 sfdisk; do
  command -v "$command_name" >/dev/null || {
    printf 'missing command: %s\n' "$command_name" >&2
    exit 2
  }
done

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT
INITRAMFS_ROOT="$WORK_DIR/initramfs"
SERIAL_LOG="$WORK_DIR/serial.log"
BACKING_DISK="$WORK_DIR/backing.raw"
mkdir -p "$INITRAMFS_ROOT/bin" "$INITRAMFS_ROOT/modules"
cp "$(command -v busybox)" "$INITRAMFS_ROOT/bin/busybox"
cp "$INIT_SOURCE" "$INITRAMFS_ROOT/init"
cp "$ZSMALLOC_MODULE" "$INITRAMFS_ROOT/modules/zsmalloc.ko"
cp "$ZRAM_MODULE" "$INITRAMFS_ROOT/modules/zram.ko"
cp "$UBLK_MODULE" "$INITRAMFS_ROOT/modules/ublk_drv.ko"
chmod +x "$INITRAMFS_ROOT/init"

truncate -s 128M "$BACKING_DISK"
printf 'label: dos\nstart=2048, type=83\n' | sfdisk "$BACKING_DISK" >/dev/null
(
  cd "$INITRAMFS_ROOT"
  find . -print0 | cpio --null -o -H newc 2>/dev/null | gzip
) > "$WORK_DIR/initramfs.gz"

accel=(-machine accel=tcg)
if test -w /dev/kvm; then
  accel=(-enable-kvm -cpu host)
fi

set +e
timeout --foreground --kill-after=5s 180s qemu-system-x86_64 \
  "${accel[@]}" -m 768 -nographic -no-reboot \
  -kernel "$BZIMAGE" -initrd "$WORK_DIR/initramfs.gz" \
  -append 'console=ttyS0 panic=1 rdinit=/init' \
  -device virtio-scsi-pci,id=scsi0 \
  -drive "file=$BACKING_DISK,format=raw,if=none,id=backing0" \
  -device scsi-hd,drive=backing0,bus=scsi0.0 >"$SERIAL_LOG" 2>&1
qemu_rc=$?
set -e

if test -n "${RAMSHARED_QEMU_EVIDENCE_LOG:-}"; then
  cp "$SERIAL_LOG" "$RAMSHARED_QEMU_EVIDENCE_LOG"
fi

grep -F "KTEST-UNAME=$KERNEL_RELEASE" "$SERIAL_LOG" >/dev/null || qemu_rc=1
grep -F 'KTEST-PRELOAD=inactive' "$SERIAL_LOG" >/dev/null || qemu_rc=1
grep -F 'KTEST-UBLK-CONTROL=present' "$SERIAL_LOG" >/dev/null || qemu_rc=1
grep -F 'KTEST-ZRAM-WRITEBACK-INTERFACE=present' "$SERIAL_LOG" >/dev/null || qemu_rc=1
grep -F 'KTEST-ZRAM-WRITEBACK-IO=pass' "$SERIAL_LOG" >/dev/null || qemu_rc=1
grep -F 'KTEST-CLEANUP=pass' "$SERIAL_LOG" >/dev/null || qemu_rc=1
grep -F 'KTEST-END' "$SERIAL_LOG" >/dev/null || qemu_rc=1

grep -F 'KTEST-' "$SERIAL_LOG" || true
if test "$qemu_rc" -ne 0; then
  printf 'QEMU_WSL_CONFIG_CAPABILITY=FAIL rc=%s\n' "$qemu_rc" >&2
  tail -40 "$SERIAL_LOG" >&2
  exit 1
fi
printf 'QEMU_WSL_CONFIG_CAPABILITY=PASS\n'
