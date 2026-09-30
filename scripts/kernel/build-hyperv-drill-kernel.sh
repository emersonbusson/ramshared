#!/usr/bin/env bash
# Build a single-file, self-booting Hyper-V drill kernel for the VMBus
# ring-buffer upstream candidate.
#
# The image is a mainline bzImage with:
#   - CONFIG_EFI_STUB=y            so it is itself an EFI application
#   - CONFIG_INITRAMFS_SOURCE=...  so the drill userspace is embedded
#   - CONFIG_CMDLINE_FORCE=y       so the console is baked in and no bootloader
#                                  argument can drift between runs
#
# The result is one file. Drop it at \EFI\BOOT\BOOTX64.EFI on a FAT32 ESP and
# a Gen2 Hyper-V VM boots it with no GRUB, no distro, no separate initrd and
# no disk root. That is exactly the disposable-guest contract the runtime
# drills require, and it keeps the GitHub runner's disk usage to one kernel
# build tree.
#
# usage: build-hyperv-drill-kernel.sh <kernel-src> <output-dir>
#   kernel-src   mainline tree already at the candidate (series applied)
#   output-dir   where bzImage / BOOTX64.EFI / initramfs land
#
# Requires: gcc, make, busybox-static (provides /bin/busybox), cpio, gzip.
set -euo pipefail

SRC="${1:?usage: build-hyperv-drill-kernel.sh <kernel-src> <output-dir>}"
OUT="${2:?usage: build-hyperv-drill-kernel.sh <kernel-src> <output-dir>}"
ARCH="${ARCH:-x86_64}"
JOBS="${JOBS:-$(nproc 2>/dev/null || echo 2)}"

HERE="$(cd "$(dirname "$0")" && pwd)"
INITRAMFS_SRC="$HERE/hyperv-drill-initramfs"

[ -d "$SRC" ] || { echo "ERROR: kernel source '$SRC' not found" >&2; exit 2; }
[ -f "$SRC/Makefile" ] || { echo "ERROR: '$SRC' is not a kernel tree" >&2; exit 2; }
[ -f "$INITRAMFS_SRC/init" ] || { echo "ERROR: missing $INITRAMFS_SRC/init" >&2; exit 2; }

mkdir -p "$OUT"
BUILD="$OUT/kernel-build"
ROOT="$OUT/initramfs-root"
CPIO="$OUT/initramfs.cpio.gz"

say() { echo "DRILL-BUILD $*"; }

# --- 1. static drill helper --------------------------------------------------
say "helper: building vmbus_drill_helper"
mkdir -p "$OUT/bin"
cc -O2 -Wall -Wextra -static -s \
	-o "$OUT/bin/vmbus_drill_helper" \
	"$INITRAMFS_SRC/vmbus_drill_helper.c"
file "$OUT/bin/vmbus_drill_helper" | tee "$OUT/helper-file.txt"
# Prove it is static: a dynamic helper would not run in the initramfs.
if ldd "$OUT/bin/vmbus_drill_helper" >/dev/null 2>&1; then
	echo "ERROR: helper is dynamically linked; initramfs would not run it" >&2
	exit 1
fi
say "helper: static ok"

# --- 2. assemble the initramfs ----------------------------------------------
say "initramfs: assembling"
BUSYBOX_SRC="${BUSYBOX:-/bin/busybox}"
if [ ! -x "$BUSYBOX_SRC" ]; then
	echo "ERROR: busybox not found at $BUSYBOX_SRC (install busybox-static)" >&2
	exit 2
fi

rm -rf "$ROOT"
mkdir -p "$ROOT"/{bin,sbin,dev,proc,sys,tmp,run,var/tmp,scripts,usr/bin,usr/sbin}

cp "$BUSYBOX_SRC" "$ROOT/bin/busybox"
chmod 755 "$ROOT/bin/busybox"

# Only the applets the drills actually call. Symlinking everything is noise and
# hides a missing dependency until the guest is already booted.
#
# '[' must be quoted: unquoted it is a shell glob that matches nothing and
# expands to a single literal filename "[ ]", which then does not answer
# `[ ... ]` anywhere in the drills.
APPLETS=(
	sh cat ls echo mount umount mkdir rm cp mv sleep grep sed awk find
	tr head tail wc basename dirname readlink dmesg tee seq dd sync
	poweroff reboot uname sort uniq cut paste ln chmod chown kill ps
	id whoami env printf true false yes test '[' ']' which
)
for a in "${APPLETS[@]}"; do
	ln -sf busybox "$ROOT/bin/$a"
done
ln -sf ../bin/busybox "$ROOT/sbin/poweroff"
ln -sf ../bin/busybox "$ROOT/sbin/reboot"

cp "$OUT/bin/vmbus_drill_helper" "$ROOT/bin/vmbus_drill_helper"
chmod 755 "$ROOT/bin/vmbus_drill_helper"

cp "$INITRAMFS_SRC/init" "$ROOT/init"
chmod 755 "$ROOT/init"

for s in vmbus-lifecycle-drill.sh vmbus-fragmentation-drill.sh uio-ring-exercise.sh; do
	if [ -f "$HERE/$s" ]; then
		cp "$HERE/$s" "$ROOT/scripts/$s"
		chmod 755 "$ROOT/scripts/$s"
	else
		echo "ERROR: drill script $HERE/$s not found" >&2
		exit 2
	fi
done

# The drills call the helper by name; make sure PATH resolves it the same way
# inside the guest as it does here.
if ! grep -q 'vmbus_drill_helper' "$ROOT/scripts/vmbus-lifecycle-drill.sh"; then
	echo "ERROR: lifecycle drill does not reference vmbus_drill_helper" >&2
	echo "       it must not depend on python3 in the drill guest" >&2
	exit 2
fi

(
	cd "$ROOT"
	find . | cpio -o -H newc --owner=0:0 2>/dev/null | gzip -9 >"$CPIO"
)
say "initramfs: $(wc -c <"$CPIO") bytes at $CPIO"

# --- 3. configure the kernel -------------------------------------------------
say "config: $(make -C "$SRC" -s ARCH="$ARCH" kernelversion) at $SRC"
# tinyconfig is the base, not defconfig: a full defconfig build costs 20+
# minutes of compile for subsystems this guest never touches. Every symbol the
# guest needs is listed explicitly and verified after olddefconfig, so a thin
# config is safe where a silent one would not be.
#
# ARCH must be spelled out. tinyconfig without it lands on the 32-bit x86
# defaults and produces a kernel that will not boot as a Gen2 UEFI guest.
make -C "$SRC" O="$BUILD" ARCH="$ARCH" -s tinyconfig

CONFIG_REQUESTED="$OUT/config-drill-requested"
: >"$CONFIG_REQUESTED"

enable() {
	echo "CONFIG_$1=y" >>"$CONFIG_REQUESTED"
	"$SRC/scripts/config" --file "$BUILD/.config" --enable "$1"
}
disable() {
	echo "# CONFIG_$1 is not set" >>"$CONFIG_REQUESTED"
	"$SRC/scripts/config" --file "$BUILD/.config" --disable "$1"
}
setstr() {
	echo "CONFIG_$1=\"$2\"" >>"$CONFIG_REQUESTED"
	"$SRC/scripts/config" --file "$BUILD/.config" --set-str "$1" "$2"
}
setval() {
	echo "CONFIG_$1=$2" >>"$CONFIG_REQUESTED"
	"$SRC/scripts/config" --file "$BUILD/.config" --set-val "$1" "$2"
}

# Hyper-V guest bus and the drivers the drills rebind. HYPERV_UTILS pulls in
# the shutdown/timesync channels so poweroff reaches the hypervisor instead of
# hanging the guest.
enable HYPERV
enable HYPERV_VMBUS
enable HYPERV_NET
enable HYPERV_BALLOON
enable CONNECTOR
enable NLS
enable PTP_1588_CLOCK_OPTIONAL
enable HYPERV_UTILS
enable UIO
enable UIO_HV_GENERIC

# Boot: UEFI stub so the bzImage is itself an EFI application
enable BLOCK
enable EFI
enable EFI_STUB
enable EFI_PARTITION
enable BLK_DEV_INITRD
enable RD_GZIP

# Console: Hyper-V COM1 presents as a standard 16550
enable TTY
enable SERIAL_CORE
enable SERIAL_CORE_CONSOLE
enable SERIAL_8250
enable SERIAL_8250_CONSOLE
setval SERIAL_8250_NR_UARTS 4
setval SERIAL_8250_RUNTIME_UARTS 4

# Filesystems and the introspection the drills read
enable DEVTMPFS
enable DEVTMPFS_MOUNT
enable TMPFS
enable SHMEM
enable PROC_FS
enable SYSFS
enable DEBUG_FS
enable KALLSYMS
enable KALLSYMS_ALL
enable PRINTK
enable PRINTK_TIME
enable BUG
enable MAGIC_SYSRQ
enable PROC_PAGE_MONITOR
enable SYSCTL
enable MULTIUSER
enable FILE_LOCKING
enable FUTEX
enable EPOLL
enable SIGNALFD
enable EVENTFD
enable UNIX
enable INET
enable NET
enable NETDEVICES
enable PCI
enable PCI_MSI
enable ACPI
enable SMP
enable HYPERVISOR_GUEST
enable PARAVIRT
enable X86_X2APIC
enable BINFMT_ELF
enable BINFMT_SCRIPT
enable MMU
enable EXPERT
enable POSIX_TIMERS
enable SYSVIPC

# Determinism and evidence hygiene. MODULES is off so the initramfs needs no
# modules.dep; KASLR is off so a logged failure is reproducible; DEBUG_INFO_BTF
# is off because it needs pahole and the image does not need it.
#
# BPF and PERF_EVENTS are deliberately absent from this list: NET selects BPF
# and arch/x86 selects PERF_EVENTS, so requesting them off would make the
# verification step below fail on symbols the architecture will not release.
disable MODULES
disable KUNIT
disable KUNIT_TEST
disable RANDOMIZE_BASE
disable IKCONFIG
disable DEBUG_INFO
disable DEBUG_INFO_BTF
disable DEBUG_INFO_DWARF_TOOLCHAIN_DEFAULT
disable DEBUG_INFO_DWARF4
disable DYNAMIC_DEBUG
disable KPROBES
disable FTRACE
disable FUNCTION_TRACER
disable KGDB

# Embed the initramfs and freeze the command line.
# CMDLINE_OVERRIDE is the x86 name of the old CMDLINE_FORCE: without it the
# EFI stub's own arguments would win and the console could drift between runs.
setstr INITRAMFS_SOURCE "$CPIO"
setval INITRAMFS_ROOT_UID 0
setval INITRAMFS_ROOT_GID 0

CMDLINE="console=ttyS0,115200 earlyprintk=serial,ttyS0,115200 panic=1 oops=panic nmi_watchdog=0"
setstr CMDLINE "$CMDLINE"
enable CMDLINE_BOOL
enable CMDLINE_OVERRIDE

make -C "$SRC" O="$BUILD" ARCH="$ARCH" -s olddefconfig

# --- 4. verify every requested symbol took ----------------------------------
# olddefconfig silently drops symbols the arch does not offer. That produces a
# kernel that builds and does not boot, which is the worst possible failure
# here. Check the resolved .config against the request, line by line.
say "config: verifying"
CONFIG_RESOLVED="$OUT/config-drill-resolved"
cp "$BUILD/.config" "$CONFIG_RESOLVED"
FAILED=0
while IFS= read -r line; do
	[ -n "$line" ] || continue
	case "$line" in
	'# CONFIG_'*' is not set')
		sym="${line#\# CONFIG_}"
		sym="${sym% is not set}"
		if grep -q "^CONFIG_${sym}=" "$CONFIG_RESOLVED"; then
			echo "ERROR: CONFIG_${sym} is still enabled" >&2
			FAILED=1
		fi
		;;
	CONFIG_*=*)
		# Covers --enable (=y), --set-val and --set-str alike. Exact
		# whole-line fixed-string match so a path or value containing
		# regex metacharacters cannot false-positive.
		if ! grep -Fqx "$line" "$CONFIG_RESOLVED"; then
			echo "ERROR: '$line' did not take" >&2
			FAILED=1
		fi
		;;
	*)
		echo "ERROR: unrecognized request line '$line'" >&2
		FAILED=1
		;;
	esac
done <"$CONFIG_REQUESTED"

if [ "$FAILED" -ne 0 ]; then
	echo "ERROR: drill config verification failed; see $CONFIG_RESOLVED" >&2
	exit 1
fi
say "config: all requested symbols resolved"

# --- 5. build ---------------------------------------------------------------
say "bzImage: building with -j$JOBS"
make -C "$SRC" O="$BUILD" -j"$JOBS" ARCH="$ARCH" bzImage 2>&1 | tee "$OUT/build.log"

IMAGE="$BUILD/arch/x86/boot/bzImage"
[ -f "$IMAGE" ] || { echo "ERROR: $IMAGE not built" >&2; exit 1; }

cp "$IMAGE" "$OUT/bzImage"
cp "$IMAGE" "$OUT/BOOTX64.EFI"
sha256sum "$OUT/bzImage" | tee "$OUT/bzImage.sha256"
say "bzImage: $(wc -c <"$OUT/bzImage") bytes"

# --- 6. prove the image is what the harness thinks it is ---------------------
# A bzImage that builds is not a bzImage that boots as an EFI application with
# the drill userspace inside. Check all three facts before declaring done.
python3 - "$OUT/BOOTX64.EFI" "$BUILD/usr/initramfs_inc_data" "$CPIO" <<'PY' | tee "$OUT/image-audit.txt"
import gzip, sys

image, inc, cpio = sys.argv[1], sys.argv[2], sys.argv[3]
head = open(image, 'rb').read(0x200)
assert head[:2] == b'MZ', 'image is not a PE/COFF EFI application (no MZ)'
pe = int.from_bytes(head[0x3c:0x40], 'little')
assert head[pe:pe + 4] == b'PE\x00\x00', 'image has no PE signature'
machine = int.from_bytes(head[pe + 4:pe + 6], 'little')
assert machine == 0x8664, f'unexpected machine 0x{machine:x}'
subsys = int.from_bytes(head[pe + 0x5c:pe + 0x5e], 'little')
assert subsys == 10, f'subsystem {subsys} is not an EFI application (want 10)'
print('IMAGE efi_stub=ok machine=x86_64 subsystem=10')

raw = open(inc, 'rb').read()
want = open(cpio, 'rb').read()
assert raw == want, 'embedded initramfs differs from the built cpio'
plain = gzip.decompress(want) if want[:2] == b'\x1f\x8b' else want
for sig in (b'HYPERV_DRILL_BOOT', b'vmbus_drill_helper', b'MMAP_HOLD',
            b'vmbus-lifecycle-drill', b'MLOCK_HOG'):
    assert sig in plain, f'{sig.decode()} missing from the initramfs'
print('IMAGE initramfs=ok bytes=%d' % len(want))
PY

# The bracket applet is the one that silently becomes a filename with a space
# in it. Confirm the initramfs really carries a usable '['.
if ! gzip -dc "$CPIO" | cpio -t 2>/dev/null | grep -qx 'bin/\['; then
	echo "ERROR: initramfs has no bin/[ applet; the drills will fail" >&2
	exit 1
fi
say "image audit: ok"
say "done: BOOTX64.EFI ready at $OUT/BOOTX64.EFI"
