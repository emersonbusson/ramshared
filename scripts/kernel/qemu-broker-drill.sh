#!/usr/bin/env bash
# qemu-broker-drill.sh — end-to-end Memory Broker (P1) drill in an ISOLATED QEMU, without a GPU.
# Proves the complete broker↔agent↔swap-over-NBD path without risk to the host:
#   PHASE 1 (bring-up): insmod nbd; the daemon starts in broker mode (--backend ram, 2 slices) and
#                       listens for the arbiter; the agent registers (Register→Registered).
#   PHASE 2 (arbitration): the arbiter assigns free slices to the tenant (round-robin) → the agent
#                          attaches each export (nbd-client → mkswap → swapon) → /proc/swaps shows them.
#   PHASE 3 (teardown): swapoff + nbd-client -d (clean order while the daemon still serves) → SIGTERM
#                       to the daemon → the worker terminates (DT-28) and exits 0; no orphaned swap.
#
# WHY QEMU (session rule): running the swap daemon + nbd-client + swapon directly in WSL2 can
# freeze the host (swapoff over a dead NBD -> I/O in D-state). In a VM, any stall is contained
# by `timeout` — the host remains unaffected. RAM backend: Cuda::load() is not called (no libcuda).
#
# usage: qemu-broker-drill.sh [bzImage] [daemon_bin] [agent_bin] [nbd.ko]
# exit 0 = PASS (swap active through the broker and clean teardown).
# SPEC: docs/memory-broker/SPECv2.md ITEM-11.
set -euo pipefail

BZ="${1:-$HOME/WSL2-Linux-Kernel/arch/x86/boot/bzImage}"
DAEMON="${2:-$(dirname "$0")/../../target/debug/ramsharedd}"
AGENT="${3:-$(dirname "$0")/../../target/debug/ramshared-agent}"
NBD_KO="${4:-$HOME/WSL2-Linux-Kernel/drivers/block/nbd.ko}"

SLICES=2
SLICE_MB=32
ARBITER=127.0.0.1:7000
SOCK=/tmp/broker.sock

for f in "$BZ" "$DAEMON" "$AGENT" "$NBD_KO"; do
  [ -f "$f" ] || { echo "missing file: $f" >&2; exit 2; }
done
command -v qemu-system-x86_64 >/dev/null || { echo "qemu-system-x86_64 missing" >&2; exit 2; }
command -v nbd-client >/dev/null || { echo "nbd-client missing (install nbd-client)" >&2; exit 2; }
[ -x /bin/busybox ] || { echo "busybox-static missing" >&2; exit 2; }

WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
IRD="$WORK/irfs"; mkdir -p "$IRD/bin" "$IRD/modules"
cp /bin/busybox "$IRD/bin/busybox"
cp "$DAEMON" "$IRD/ramsharedd"
cp "$AGENT" "$IRD/ramshared-agent"
cp "$(command -v nbd-client)" "$IRD/bin/nbd-client"
cp "$NBD_KO" "$IRD/modules/nbd.ko"
sha256sum "$DAEMON" | awk '{print $1}' > "$IRD/expected-ramsharedd.sha256"
sha256sum "$AGENT" | awk '{print $1}' > "$IRD/expected-agent.sha256"

# Dynamic libraries for the 3 binaries (dynamic glibc; no CUDA in the RAM path). Preserves absolute
# paths (including /lib64/ld-linux).
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
# loopback UP: the agent communicates with the arbiter on $ARBITER (127.0.0.1). Without this,
# connect() returns ENETUNREACH ("Network is unreachable").
\$BB ip link set lo up 2>/dev/null || \$BB ifconfig lo 127.0.0.1 up 2>/dev/null || true
# busybox swap applets that the agent invokes by name (mkswap/swapon/swapoff)
for a in mkswap swapon swapoff sleep cat kill; do \$BB ln -sf /bin/busybox /bin/\$a; done
echo "=====KTEST-BEGIN====="
echo "KTEST-UNAME=\$(\$BB uname -r)"
DAEMON_EXPECTED="\$(\$BB cat /expected-ramsharedd.sha256 2>/dev/null)"
DAEMON_ACTUAL="\$(\$BB sha256sum /ramsharedd 2>/dev/null)"
DAEMON_ACTUAL="\${DAEMON_ACTUAL%% *}"
AGENT_EXPECTED="\$(\$BB cat /expected-agent.sha256 2>/dev/null)"
AGENT_ACTUAL="\$(\$BB sha256sum /ramshared-agent 2>/dev/null)"
AGENT_ACTUAL="\${AGENT_ACTUAL%% *}"
echo "KTEST-DAEMON-SHA-EXPECTED=\$DAEMON_EXPECTED"
echo "KTEST-DAEMON-SHA-ACTUAL=\$DAEMON_ACTUAL"
echo "KTEST-AGENT-SHA-EXPECTED=\$AGENT_EXPECTED"
echo "KTEST-AGENT-SHA-ACTUAL=\$AGENT_ACTUAL"
[ -n "\$DAEMON_EXPECTED" ] && [ "\$DAEMON_EXPECTED" = "\$DAEMON_ACTUAL" ] && echo "KTEST-DAEMON-BINARY-MATCH=ok" || echo "KTEST-DAEMON-BINARY-MATCH=fail"
[ -n "\$AGENT_EXPECTED" ] && [ "\$AGENT_EXPECTED" = "\$AGENT_ACTUAL" ] && echo "KTEST-AGENT-BINARY-MATCH=ok" || echo "KTEST-AGENT-BINARY-MATCH=fail"

# --- PHASE 1: bring-up ---
if \$BB insmod /modules/nbd.ko nbds_max=8 2>/tmp/e; then
  echo "KTEST-NBD=ok"
else
  echo "KTEST-NBD=fail: \$(\$BB cat /tmp/e)"
fi

# daemon in RAM broker mode (without a GPU): N slices, Unix socket, and TCP arbiter.
/ramsharedd --transport nbd --backend ram \\
  --slices $SLICES --slice-mb $SLICE_MB --sock $SOCK --arbiter-listen $ARBITER \\
  --telemetry-jsonl /tmp/telem.jsonl \\
  >/tmp/daemon.log 2>&1 &
DPID=\$!
echo "KTEST-DAEMON-PID=\$DPID"
\$BB sleep 1   # arbiter starts and the Unix socket becomes ready

# agent (local tenant: Unix transport → endpoint = the daemon's Unix socket).
/ramshared-agent --broker $ARBITER --tenant vm --transport unix \\
  --nbd-base /dev/nbd --watchdog-secs 120 >/tmp/agent.log 2>&1 &
APID=\$!
echo "KTEST-AGENT-PID=\$APID"

# --- PHASE 2: arbitration → active swap. Waits for /proc/swaps to show at least one nbd (bounded ~25s)
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

# --- Telemetry (RF-5): the broker emits one JSONL line per tick in /tmp/telem.jsonl. Proves the
# live daemon's write-to-file (isolated in QEMU). RAM → vram_*=null (expected sentinel).
TLINES=\$(\$BB grep -c '"flag"' /tmp/telem.jsonl 2>/dev/null); [ -z "\$TLINES" ] && TLINES=0
echo "KTEST-TELEMETRY-LINES=\$TLINES"
[ "\$TLINES" -ge 1 ] && echo "KTEST-TELEMETRY=ok" || echo "KTEST-TELEMETRY=fail"
\$BB sleep 2.5  # lets >=1 tick (2s) emit a complete sample
echo "KTEST-TELEMETRY-SAMPLE:"; \$BB cat /tmp/telem.jsonl 2>/dev/null

# --- PHASE 3: clean teardown. swapoff + disconnect NBD WHILE the daemon is still serving, then
# SIGTERM to the daemon (DT-28: worker terminates during shutdown). This order prevents swapoff over a dead NBD.
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

echo "[qemu-broker-drill] booting (accel: ${ACCEL[*]})..."
timeout 240 qemu-system-x86_64 "${ACCEL[@]}" -m 512 -smp 2 -nographic -no-reboot \
  -kernel "$BZ" -initrd "$WORK/initramfs.gz" \
  -append "console=ttyS0 panic=1 rdinit=/init" > "$WORK/serial.log" 2>&1 || true

echo "=========== result ==========="
grep -E "KTEST-" "$WORK/serial.log" || echo "no KTEST output — kernel may not have booted"
echo "================================="
if grep -q "KTEST-SWAP-ACTIVE=ok" "$WORK/serial.log" \
   && grep -q "KTEST-DAEMON-BINARY-MATCH=ok" "$WORK/serial.log" \
   && grep -q "KTEST-AGENT-BINARY-MATCH=ok" "$WORK/serial.log" \
   && grep -q "KTEST-SWAPOFF=ok" "$WORK/serial.log" \
   && grep -q "KTEST-DAEMON-TERMINATED=ok" "$WORK/serial.log" \
   && grep -q "KTEST-TELEMETRY=ok" "$WORK/serial.log"; then
  echo "QEMU-BROKER-DRILL: PASS — broker assigned slices, swap active through NBD, JSONL telemetry, clean teardown."
  exit 0
else
  echo "QEMU-BROKER-DRILL: FAIL/INCONCLUSIVE — see the KTEST output above and the serial log."
  echo "--- tail serial ---"; tail -30 "$WORK/serial.log"
  exit 1
fi
