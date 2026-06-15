#!/usr/bin/env bash
# qemu-broker-drill.sh — drill end-to-end do Memory Broker (P1) num QEMU ISOLADO, sem GPU.
# Prova, sem risco pro host, o caminho completo broker↔agente↔swap-sobre-NBD:
#   FASE 1 (bring-up): insmod nbd; o daemon sobe em modo broker (--backend ram, 2 slices) e
#                      escuta o árbitro; o agente registra (Register→Registered).
#   FASE 2 (arbitragem): o árbitro assina as slices livres ao tenant (round-robin) → o agente
#                        anexa cada export (nbd-client → mkswap → swapon) → /proc/swaps as mostra.
#   FASE 3 (teardown): swapoff + nbd-client -d (ordem limpa, daemon ainda servindo) → SIGTERM no
#                      daemon → o worker encerra (DT-28) e sai 0; nenhum swap órfão.
#
# POR QUE QEMU (regra de sessão): rodar o daemon de swap + nbd-client + swapon no WSL2 pode
# congelar o host (swapoff sobre NBD morto → I/O em D-state). Na VM, um stall é contido pelo
# `timeout` — o host fica intacto. Backend RAM: Cuda::load() não é chamado (sem libcuda).
#
# uso: qemu-broker-drill.sh [bzImage] [daemon_bin] [agent_bin] [nbd.ko]
# saída 0 = PASS (swap ativo via broker + teardown limpo).
# SPEC: docs/memory-broker/SPECv2.md ITEM-11.
set -euo pipefail

BZ="${1:-/home/emdev/WSL2-Linux-Kernel/arch/x86/boot/bzImage}"
DAEMON="${2:-$(dirname "$0")/../../target/debug/ramsharedd}"
AGENT="${3:-$(dirname "$0")/../../target/debug/ramshared-agent}"
NBD_KO="${4:-/home/emdev/WSL2-Linux-Kernel/drivers/block/nbd.ko}"

SLICES=2
SLICE_MB=32
ARBITER=127.0.0.1:7000
SOCK=/tmp/broker.sock

for f in "$BZ" "$DAEMON" "$AGENT" "$NBD_KO"; do
  [ -f "$f" ] || { echo "arquivo inexistente: $f" >&2; exit 2; }
done
command -v qemu-system-x86_64 >/dev/null || { echo "qemu-system-x86_64 ausente" >&2; exit 2; }
command -v nbd-client >/dev/null || { echo "nbd-client ausente (instale nbd-client)" >&2; exit 2; }
[ -x /bin/busybox ] || { echo "busybox-static ausente" >&2; exit 2; }

WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
IRD="$WORK/irfs"; mkdir -p "$IRD/bin" "$IRD/modules"
cp /bin/busybox "$IRD/bin/busybox"
cp "$DAEMON" "$IRD/ramsharedd"
cp "$AGENT" "$IRD/ramshared-agent"
cp "$(command -v nbd-client)" "$IRD/bin/nbd-client"
cp "$NBD_KO" "$IRD/modules/nbd.ko"

# Libs dinâmicas dos 3 binários (glibc-dinâmicos; sem CUDA no caminho RAM). Preserva caminhos
# absolutos (o /lib64/ld-linux entra junto).
for bin in "$DAEMON" "$AGENT" "$(command -v nbd-client)"; do
  for lib in $(ldd "$bin" 2>/dev/null | grep -oE '/[^ ]+\.so[^ ]*'); do
    mkdir -p "$IRD$(dirname "$lib")"
    cp -n "$lib" "$IRD$lib" 2>/dev/null || true
  done
done

cat > "$IRD/init" <<INIT
#!/bin/busybox sh
BB=/bin/busybox
export PATH=/bin
\$BB mkdir -p /proc /sys /dev /tmp
\$BB mount -t proc proc /proc
\$BB mount -t sysfs sysfs /sys
\$BB mount -t devtmpfs devtmpfs /dev 2>/dev/null
# loopback UP: o agente fala com o árbitro em $ARBITER (127.0.0.1). Sem isto, connect()
# devolve ENETUNREACH ("Network is unreachable").
\$BB ip link set lo up 2>/dev/null || \$BB ifconfig lo 127.0.0.1 up 2>/dev/null || true
# applets de swap do busybox que o agente invoca por nome (mkswap/swapon/swapoff)
for a in mkswap swapon swapoff sleep cat kill; do \$BB ln -sf /bin/busybox /bin/\$a; done
echo "=====KTEST-BEGIN====="
echo "KTEST-UNAME=\$(\$BB uname -r)"

# --- FASE 1: bring-up ---
if \$BB insmod /modules/nbd.ko nbds_max=8 2>/tmp/e; then
  echo "KTEST-NBD=ok"
else
  echo "KTEST-NBD=fail: \$(\$BB cat /tmp/e)"
fi

# daemon em modo broker RAM (sem GPU): N slices, socket Unix + árbitro TCP.
/ramsharedd --transport nbd --backend ram \\
  --slices $SLICES --slice-mb $SLICE_MB --sock $SOCK --arbiter-listen $ARBITER \\
  >/tmp/daemon.log 2>&1 &
DPID=\$!
echo "KTEST-DAEMON-PID=\$DPID"
\$BB sleep 1   # árbitro liga + socket Unix pronto

# agente (tenant local: transporte unix → endpoint = socket Unix do daemon).
/ramshared-agent --broker $ARBITER --tenant vm --transport unix \\
  --nbd-base /dev/nbd --watchdog-secs 120 >/tmp/agent.log 2>&1 &
APID=\$!
echo "KTEST-AGENT-PID=\$APID"

# --- FASE 2: arbitragem → swap ativo. Espera /proc/swaps mostrar pelo menos 1 nbd (bounded ~25s)
N=0; i=0
while [ \$i -lt 250 ]; do
  N=\$(\$BB grep -c '/dev/nbd' /proc/swaps 2>/dev/null); [ -z "\$N" ] && N=0
  [ "\$N" -ge 1 ] && break
  \$BB kill -0 \$DPID 2>/dev/null || { echo "KTEST-DAEMON-DIED-EARLY=1"; break; }
  \$BB sleep 0.1; i=\$((i+1))
done
echo "KTEST-SWAPS=\$N"
echo "KTEST-SWAPS-DUMP:"; \$BB cat /proc/swaps
if [ "\$N" -ge 1 ]; then
  echo "KTEST-SWAP-ACTIVE=ok"
else
  echo "KTEST-SWAP-ACTIVE=fail"
fi

# --- FASE 3: teardown limpo. swapoff + desconecta NBD ENQUANTO o daemon ainda serve, depois
# SIGTERM no daemon (DT-28: o worker encerra no shutdown). Ordem evita swapoff sobre NBD morto.
for dev in /dev/nbd0 /dev/nbd1 /dev/nbd2 /dev/nbd3; do
  if \$BB grep -q "\$dev " /proc/swaps 2>/dev/null; then
    \$BB swapoff "\$dev" 2>/dev/null && nbd-client -d "\$dev" 2>/dev/null || true
  fi
done
LEFT=\$(\$BB grep -c '/dev/nbd' /proc/swaps 2>/dev/null); [ -z "\$LEFT" ] && LEFT=0
echo "KTEST-SWAPOFF-LEFT=\$LEFT"
[ "\$LEFT" -eq 0 ] && echo "KTEST-SWAPOFF=ok" || echo "KTEST-SWAPOFF=fail"

\$BB kill -TERM \$APID 2>/dev/null || true
\$BB kill -TERM \$DPID 2>/dev/null || true
j=0; GONE=0
while [ \$j -lt 120 ]; do
  \$BB kill -0 \$DPID 2>/dev/null || { GONE=1; break; }
  \$BB sleep 0.1; j=\$((j+1))
done
[ \$GONE -eq 1 ] && echo "KTEST-DAEMON-TERMINATED=ok" || echo "KTEST-DAEMON-TERMINATED=timeout"

echo "KTEST-DAEMON-LOG:"; \$BB tail -10 /tmp/daemon.log 2>/dev/null
echo "KTEST-AGENT-LOG:"; \$BB tail -10 /tmp/agent.log 2>/dev/null
echo "=====KTEST-END====="
\$BB poweroff -f
INIT
chmod +x "$IRD/init"

( cd "$IRD" && find . | cpio -o -H newc 2>/dev/null | gzip ) > "$WORK/initramfs.gz"

ACCEL=(-machine accel=tcg)
[ -w /dev/kvm ] && ACCEL=(-enable-kvm -cpu host)

echo "[qemu-broker-drill] bootando (accel: ${ACCEL[*]})..."
timeout 240 qemu-system-x86_64 "${ACCEL[@]}" -m 512 -smp 2 -nographic -no-reboot \
  -kernel "$BZ" -initrd "$WORK/initramfs.gz" \
  -append "console=ttyS0 panic=1 rdinit=/init" > "$WORK/serial.log" 2>&1 || true

echo "=========== resultado ==========="
grep -E "KTEST-" "$WORK/serial.log" || echo "sem output KTEST — kernel pode nao ter bootado"
echo "================================="
if grep -q "KTEST-SWAP-ACTIVE=ok" "$WORK/serial.log" \
   && grep -q "KTEST-SWAPOFF=ok" "$WORK/serial.log" \
   && grep -q "KTEST-DAEMON-TERMINATED=ok" "$WORK/serial.log"; then
  echo "QEMU-BROKER-DRILL: PASS — broker assinou slices, swap ativo via NBD, teardown limpo."
  exit 0
else
  echo "QEMU-BROKER-DRILL: FAIL/INCONCLUSIVO — veja os KTEST acima e o serial."
  echo "--- tail serial ---"; tail -30 "$WORK/serial.log"
  exit 1
fi
