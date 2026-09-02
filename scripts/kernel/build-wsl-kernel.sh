#!/usr/bin/env bash
# build-wsl-kernel.sh — reusable custom WSL2 kernel build from the official
# Microsoft base plus extra configs. It incorporates the toolkit lessons:
#   - starts from Microsoft/config-wsl (all WSL2 boot configs guaranteed);
#   - applies extra configs and VERIFIES that they took effect (catches bool vs. --module);
#   - runs modules_install (otherwise .ko files do not load at boot).
#
# usage: build-wsl-kernel.sh [CONFIG=y|m|n ...]
#   default: CONFIG_BLK_DEV_UBLK=m CONFIG_ZRAM_WRITEBACK=y CONFIG_IO_URING=y (Phase B)
# env: KTAG (branch/tag, default linux-msft-wsl-6.6.y), KSRC (source directory), JOBS.
#
# Output: prints bzImage and kernelrelease (pass them to qemu-validate.sh / boot-kernel-safe.ps1).
set -euo pipefail

# SPEC wsl2-custom-kernel-p1: default 6.18.y; override with KTAG=
KTAG="${KTAG:-linux-msft-wsl-6.18.y}"
KSRC="${KSRC:-$HOME/src/WSL2-Linux-Kernel}"
JOBS="${JOBS:-2}"

if ! [[ "$JOBS" =~ ^[0-9]+$ ]]; then
  echo "Error: JOBS must be a positive integer." >&2
  exit 64 # EX_USAGE
fi
if [ "$JOBS" -lt 1 ]; then
  JOBS=1
fi
MAX_JOBS=$(nproc 2>/dev/null || echo 128)
if [ "$JOBS" -gt "$MAX_JOBS" ]; then
  echo "Error: JOBS ($JOBS) exceeds available CPU cores ($MAX_JOBS)." >&2
  exit 64 # EX_USAGE
fi

CONFIGS=("$@"); [ ${#CONFIGS[@]} -eq 0 ] && CONFIGS=(CONFIG_BLK_DEV_UBLK=m CONFIG_ZRAM_WRITEBACK=y CONFIG_IO_URING=y)

echo "[build] deps..."
sudo apt-get install -y -q build-essential flex bison libelf-dev libssl-dev bc dwarves cpio python3 >/dev/null

for cmd in make gcc flex bison; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "Error: Required build tool '$cmd' is missing." >&2
    exit 69 # EX_UNAVAILABLE
  fi
done

if [ ! -d "$KSRC/.git" ]; then
  echo "[build] cloning $KTAG -> $KSRC"
  git clone --depth 1 --branch "$KTAG" https://github.com/microsoft/WSL2-Linux-Kernel.git "$KSRC"
fi
cd "$KSRC" || { echo "Error: Failed to enter $KSRC" >&2; exit 74; }

if [ ! -f "Makefile" ] || [ ! -f "Kconfig" ]; then
  echo "Error: '$KSRC' is not a valid kernel source tree." >&2
  exit 74 # EX_IOERR
fi

echo "[build] base = Microsoft/config-wsl + extra configs"
if [ ! -f "Microsoft/config-wsl" ]; then
  echo "Error: Microsoft/config-wsl not found." >&2
  exit 78 # EX_CONFIG
fi
cp Microsoft/config-wsl .config

if [ ! -f ".config" ]; then
  echo "Error: .config was not created." >&2
  exit 78 # EX_CONFIG
fi

for kv in "${CONFIGS[@]}"; do
  name="${kv%%=*}"; val="${kv##*=}"
  case "$val" in
    y) ./scripts/config --file .config --enable  "$name" ;;
    m) ./scripts/config --file .config --module  "$name" ;;
    n) ./scripts/config --file .config --disable "$name" ;;
    *) echo "[build] invalid value in $kv (use y|m|n)"; exit 64 ;;
  esac
done
make olddefconfig >/dev/null

# VERIFY that each config took effect (olddefconfig reverts invalid ones — e.g. bool requested as --module).
fail=0
for kv in "${CONFIGS[@]}"; do
  name="${kv%%=*}"; val="${kv##*=}"
  got="$(grep -E "^${name}=" .config | cut -d= -f2 || true)"; [ -z "$got" ] && got="(unset)"
  if { [ "$val" = "y" ] && [ "$got" = "y" ]; } || { [ "$val" = "m" ] && [ "$got" = "m" ]; } || { [ "$val" = "n" ] && [ "$got" = "(unset)" ]; }; then
    echo "[build]  OK  $name=$got"
  else
    echo "[build] !!!! $name requested=$val but resolved=$got — probable missing dependency (for example, a bool requires --enable, not --module; or depends on another CONFIG)."
    fail=1
  fi
done
[ "$fail" = 1 ] && { echo "[build] configs did not apply; aborting."; exit 78; }

echo "[build] make -j$JOBS (resource-intensive; limited -j avoids stalling WSL2)..."
make -j"$JOBS"
echo "[build] modules_install..."
sudo make modules_install >/dev/null

REL="$(make -s kernelrelease)"
echo "=============================================="
echo "[build] OK"
echo "  bzImage : $KSRC/arch/x86/boot/bzImage"
echo "  release : $REL"
echo "  validate : sudo bash scripts/kernel/qemu-validate.sh $KSRC/arch/x86/boot/bzImage \"$REL\" \\"
echo "              $KSRC/drivers/block/ublk_drv.ko $KSRC/mm/zsmalloc.ko $KSRC/drivers/block/zram/zram.ko"
echo "=============================================="
