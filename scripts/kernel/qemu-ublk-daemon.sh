#!/usr/bin/env bash
# qemu-ublk-daemon.sh — valida o CICLO DE VIDA do daemon ublk (`--backend ram`) num
# QEMU ISOLADO. Prova, sem risco pro host: (1) insmod ublk_drv; (2) o daemon sobe e
# cria /dev/ublkbN; (3) serve I/O (dd write+read); (4) SIGTERM -> teardown ordenado
# (STOP_DEV -> join -> DEL_DEV) -> device removido + daemon sai 0.
#
# POR QUE QEMU: rodar esse daemon no WSL2 CONGELOU o host (device orfao no teardown ->
# I/O em D-state). Numa VM, um stall e contido pelo `timeout` — o host fica intacto.
# Backend RAM (sem GPU): `Cuda::load()` so e chamado no caminho VRAM, entao o binario
# (CUDA via dlopen) roda sem libcuda. O bug de teardown e independente do backend.
#
# uso: qemu-ublk-daemon.sh [bzImage] [daemon_bin] [ublk_drv.ko]
# saida 0 = PASS (serve + teardown limpo). SPEC: docs/ublk-daemon-integration/IMPL.md F2.
set -euo pipefail

BZ="${1:-/home/emdev/WSL2-Linux-Kernel/arch/x86/boot/bzImage}"
DAEMON="${2:-$(dirname "$0")/../../target/debug/ramshared-wsl2d}"
UBLK_KO="${3:-/home/emdev/WSL2-Linux-Kernel/drivers/block/ublk_drv.ko}"

for f in "$BZ" "$DAEMON" "$UBLK_KO"; do
  [ -f "$f" ] || { echo "arquivo inexistente: $f" >&2; exit 2; }
done
command -v qemu-system-x86_64 >/dev/null || { echo "qemu-system-x86_64 ausente" >&2; exit 2; }
[ -x /bin/busybox ] || { echo "busybox-static ausente" >&2; exit 2; }

WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
IRD="$WORK/irfs"; mkdir -p "$IRD/bin" "$IRD/modules"
cp /bin/busybox "$IRD/bin/busybox"
cp "$DAEMON" "$IRD/ramshared-wsl2d"
cp "$UBLK_KO" "$IRD/modules/ublk_drv.ko"

# Copia as libs dinamicas do daemon preservando os caminhos absolutos (o binario e
# glibc-dinamico; sem CUDA no load — ver ldd). O linker /lib64/ld-linux entra junto.
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

# 1) carrega o driver ublk
if $BB insmod /modules/ublk_drv.ko 2>/tmp/e; then
  echo "KTEST-INSMOD=ok"
else
  echo "KTEST-INSMOD=fail: $($BB cat /tmp/e)"
fi
[ -e /dev/ublk-control ] && echo "KTEST-UBLK-CONTROL=present" || echo "KTEST-UBLK-CONTROL=absent"

# 2) sobe o daemon (backend RAM, override da trava de WSL2, --force p/ mlockall best-effort)
RAMSHARED_ALLOW_UBLK_ON_WSL2=1 /ramshared-wsl2d --transport ublk --backend ram \
  --size 8 --queue-depth 1 --force >/tmp/daemon.log 2>&1 &
DPID=$!
echo "KTEST-DAEMON-PID=$DPID"

# 3) espera o /dev/ublkb0 surgir (bounded ~15s)
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
  # 4) serve I/O: write + read de 4KB
  if $BB dd if=/dev/zero of="$DEV" bs=4096 count=1 conv=fsync 2>/dev/null \
     && $BB dd if="$DEV" of=/dev/null bs=4096 count=1 2>/dev/null; then
    echo "KTEST-SERVED=ok"
  else
    echo "KTEST-SERVED=fail"
  fi
  # 5) SIGTERM -> teardown ordenado; espera o daemon sair (bounded ~12s)
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
  # 6) device removido pelo teardown?
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

echo "[qemu-ublk-daemon] bootando (accel: ${ACCEL[*]})..."
timeout 180 qemu-system-x86_64 "${ACCEL[@]}" -m 512 -smp 2 -nographic -no-reboot \
  -kernel "$BZ" -initrd "$WORK/initramfs.gz" \
  -append "console=ttyS0 panic=1 rdinit=/init" > "$WORK/serial.log" 2>&1 || true

echo "=========== resultado ==========="
grep -E "KTEST-" "$WORK/serial.log" || echo "sem output KTEST — kernel pode nao ter bootado"
echo "================================="
if grep -q "KTEST-SERVED=ok" "$WORK/serial.log" \
   && grep -q "KTEST-TERMINATED=ok" "$WORK/serial.log" \
   && grep -q "KTEST-DEVICE-REMOVED=ok" "$WORK/serial.log"; then
  echo "QEMU-UBLK-DAEMON: PASS — daemon serviu I/O e fez teardown limpo (SIGTERM)."
  exit 0
else
  echo "QEMU-UBLK-DAEMON: FAIL/INCONCLUSIVO — veja os KTEST acima e o serial."
  echo "--- tail serial ---"; tail -25 "$WORK/serial.log"
  exit 1
fi
