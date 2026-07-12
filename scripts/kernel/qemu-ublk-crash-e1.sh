#!/usr/bin/env bash
# qemu-ublk-crash-e1.sh — Experiment E1 (ublk crash-safety architecture decision,
# docs/ublk-teardown-crash-safety/). Tests the scenario that the original smoke test
# (qemu-ublk-daemon.sh) does NOT cover: device set up as SWAP under real memory pressure,
# daemon killed by SIGKILL (not SIGTERM) with I/O potentially in flight.
#
# Question that decides the branch (see docs/ublk-teardown-crash-safety/PRD.md):
#   Branch A: /dev/ublkbN disappears in <=~10s, VM remains responsive -> kernel's
#             `ublk_daemon_monitor_work` (5s poll, ublk_drv.c:1486-1516) is sufficient;
#             residual risk is only the window + loss of in-flight pages (SIGBUS in owner processes).
#   Branch B: device persists / VM freezes >60s / hung_task in dmesg -> reproduces the isolated
#             historical WSL2 freeze (MEMORY.md:883-901); userspace reaper would NOT help
#             (DEL_DEV goes through the same del_gendisk that the monitor already triggers on its own).
#
# v2: fixes 2 issues from the 1st run (n=1, inconclusive by project's own rule,
# .claude/rules/benchmarks.md "1 sample lies"): (a) busybox `dmesg` was empty —
# replaced by printing the raw, complete serial.log (kernel sends printk to ttyS0
# directly, so it is already there without depending on the applet); (b) "Used>0" trigger caught
# a transient blip (1 page) — now requires a higher floor (KB_THRESHOLD) and
# resamples before deciding, in addition to logging if the pressure `dd` is still alive at the
# moment of the kill (evidence of I/O in flight).
#
# Does not run on host (real WSL2) under any circumstances — only inside transient qemu, RAM-only,
# without -hda, same non-destructive pattern as qemu-ublk-daemon.sh (DT-29,
# .claude/rules/benchmarks.md:23). No sudo: qemu runs as a normal user (TCG if
# /dev/kvm is not writable).
#
# uso: qemu-ublk-crash-e1.sh [bzImage] [daemon_bin] [ublk_drv.ko]
# saida 0 = experimento rodou e produziu um veredito; saida 1 = inconclusivo (setup nao
# completou). O veredito (Ramo A/B) NAO decide o exit code — leia KTEST-E1-VERDICT e o
# serial completo (impresso sempre, nao so as linhas KTEST-).
set -euo pipefail

BZ="${1:-$HOME/WSL2-Linux-Kernel/arch/x86/boot/bzImage}"
DAEMON="${2:-$(dirname "$0")/../../target/debug/ramsharedd}"
UBLK_KO="${3:-$HOME/WSL2-Linux-Kernel/drivers/block/ublk_drv.ko}"

for f in "$BZ" "$DAEMON" "$UBLK_KO"; do
  [ -f "$f" ] || { echo "arquivo inexistente: $f" >&2; exit 2; }
done
command -v qemu-system-x86_64 >/dev/null || { echo "qemu-system-x86_64 ausente" >&2; exit 2; }
[ -x /bin/busybox ] || { echo "busybox-static ausente" >&2; exit 2; }

WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
IRD="$WORK/irfs"; mkdir -p "$IRD/bin" "$IRD/modules"
cp /bin/busybox "$IRD/bin/busybox"
cp "$DAEMON" "$IRD/ramsharedd"
cp "$UBLK_KO" "$IRD/modules/ublk_drv.ko"

for lib in $(ldd "$DAEMON" | grep -oE '/[^ ]+\.so[^ ]*'); do
  mkdir -p "$IRD$(dirname "$lib")"
  cp "$lib" "$IRD$lib"
done

cat > "$IRD/init" <<'INIT'
#!/bin/busybox sh
BB=/bin/busybox
$BB mkdir -p /proc /sys /dev /tmp /mnt/hog
$BB mount -t proc proc /proc
$BB mount -t sysfs sysfs /sys
$BB mount -t devtmpfs devtmpfs /dev 2>/dev/null
echo "=====KTEST-E1-BEGIN====="
echo "KTEST-UNAME=$($BB uname -r)"

# 1) driver ublk
if $BB insmod /modules/ublk_drv.ko 2>/tmp/e; then
  echo "KTEST-INSMOD=ok"
else
  echo "KTEST-INSMOD=fail: $($BB cat /tmp/e)"
fi

# 2) daemon (backend RAM, device maior p/ ser um alvo de swap com sentido: 64 MiB)
RAMSHARED_ALLOW_UBLK_ON_WSL2=1 /ramsharedd --transport ublk --backend ram \
  --size 64 --queue-depth 4 --force >/tmp/daemon.log 2>&1 &
DPID=$!
echo "KTEST-DAEMON-PID=$DPID"

# 3) espera /dev/ublkbN (bounded ~15s)
DEV=""
i=0
while [ $i -lt 150 ]; do
  for n in /dev/ublkb0 /dev/ublkb1; do [ -b "$n" ] && DEV="$n"; done
  [ -n "$DEV" ] && break
  $BB kill -0 "$DPID" 2>/dev/null || { echo "KTEST-DAEMON-DIED-EARLY=1"; break; }
  $BB sleep 0.1; i=$((i+1))
done
[ -n "$DEV" ] || { echo "KTEST-DEVICE=absent"; echo "=====KTEST-E1-END====="; $BB poweroff -f; exit 0; }
echo "KTEST-DEVICE=$DEV"

# 4) arma como swap
if $BB mkswap "$DEV" >/tmp/mkswap.log 2>&1 && $BB swapon "$DEV" >/tmp/swapon.log 2>&1; then
  echo "KTEST-SWAPON=ok"
else
  echo "KTEST-SWAPON=fail: $($BB cat /tmp/mkswap.log /tmp/swapon.log 2>/dev/null)"
  echo "=====KTEST-E1-END====="; $BB poweroff -f; exit 0
fi

# 5) real memory pressure via tmpfs (VM RAM is intentionally small: -m 256).
#    tmpfs is swappable; under pressure the kernel pushes pages to /dev/ublkbN. Writes
#    in 1 MiB chunks, measuring /proc/swaps at each chunk (without relying only on an
#    external watcher) to guarantee that pressure is continuous, not a blip.
$BB mount -t tmpfs -o size=300m tmpfs /mnt/hog
$BB cat /proc/mounts | $BB grep hog | while read -r l; do echo "KTEST-HOG-MOUNT: $l"; done
(
  n=0
  while [ $n -lt 300 ]; do
    $BB dd if=/dev/zero of=/mnt/hog/f seek=$n bs=1M count=1 conv=notrunc 2>>/tmp/dd.log \
      || { echo "KTEST-DD-BROKE-AT-CHUNK=$n" >>/tmp/dd.log; break; }
    n=$((n+1))
  done
  echo "KTEST-DD-CHUNKS-WRITTEN=$n" >>/tmp/dd.log
) &
DDPID=$!

# 6) waits for swap to ACTIVATE non-trivially (Used >= 4096 KB, not just >0 — avoids
#    counting a single isolated page as "activated"), bounded ~40s. Only then we kill the daemon.
KB_THRESHOLD=4096
SWAPPED=0
k=0
while [ $k -lt 400 ]; do
  # /proc/swaps: Filename Type Size Used Priority -> Used e a coluna $4, nao $3
  # (bug da 1a/2a rodada: $3 e Size, sempre constante -> disparava no iter=0).
  USED=$($BB awk 'NR==2{print $4+0}' /proc/swaps 2>/dev/null)
  [ -z "$USED" ] && USED=0
  if [ "$USED" -ge "$KB_THRESHOLD" ]; then SWAPPED=1; break; fi
  $BB kill -0 "$DDPID" 2>/dev/null || { echo "KTEST-DD-DIED-BEFORE-THRESHOLD=1"; break; }
  $BB sleep 0.1; k=$((k+1))
done
echo "KTEST-SWAP-ACTIVATED=$SWAPPED (threshold=${KB_THRESHOLD}KB, iter=$k)"
echo "KTEST-DD-ALIVE-AT-DECISION=$($BB kill -0 "$DDPID" 2>/dev/null && echo 1 || echo 0)"
$BB cat /proc/swaps | while read -r l; do echo "KTEST-PROC-SWAPS: $l"; done
$BB cat /proc/meminfo | $BB grep -E "^(MemFree|MemAvailable|SwapFree|SwapTotal):" | while read -r l; do echo "KTEST-MEMINFO: $l"; done

# 7) DECISIVE MOMENT: SIGKILL (not SIGTERM) to the daemon, with the pressure `dd` still
#    running (checked above) — higher real chance of swap I/O in flight in ublk.
T0_MS=$($BB awk '{print int($1*1000)}' /proc/uptime)
echo "KTEST-KILL-T0-MS=$T0_MS"
$BB kill -KILL "$DPID"
echo "KTEST-SIGKILL-SENT=1"

# 8) observa se o device some sozinho (monitor_work do kernel), bounded ~40s.
GONE=0
m=0
while [ $m -lt 400 ]; do
  [ -b "$DEV" ] || { GONE=1; break; }
  $BB sleep 0.1; m=$((m+1))
done
T1_MS=$($BB awk '{print int($1*1000)}' /proc/uptime)
ELAPSED_MS=$((T1_MS - T0_MS))
echo "KTEST-DEVICE-GONE=$GONE"
echo "KTEST-ELAPSED-MS=$ELAPSED_MS"

# 9) proof of life: if this shell reached this point and runs more commands, the VM is not
#    stuck in a global D-state (a real freeze would also stop the script, and the host's
#    external `timeout` would detect it).
echo "KTEST-VM-RESPONSIVE-AFTER-KILL=1"
$BB cat /proc/swaps | while read -r l; do echo "KTEST-PROC-SWAPS-POST: $l"; done
$BB cat /tmp/dd.log 2>/dev/null | $BB tail -10 | while read -r l; do echo "KTEST-DD-LOG: $l"; done
$BB df /mnt/hog 2>/dev/null | while read -r l; do echo "KTEST-HOG-DF: $l"; done

if [ $GONE -eq 1 ] && [ $ELAPSED_MS -le 15000 ]; then
  echo "KTEST-E1-VERDICT=RAMO-A"
elif [ $GONE -eq 0 ]; then
  echo "KTEST-E1-VERDICT=RAMO-B-DEVICE-PERSISTIU"
else
  echo "KTEST-E1-VERDICT=AMBIGUO-LENTO"
fi

echo "KTEST-DAEMON-LOG:"
$BB tail -8 /tmp/daemon.log 2>/dev/null
echo "=====KTEST-E1-END====="
$BB poweroff -f
INIT
chmod +x "$IRD/init"

( cd "$IRD" && find . | cpio -o -H newc 2>/dev/null | gzip ) > "$WORK/initramfs.gz"

ACCEL=(-machine accel=tcg)
[ -w /dev/kvm ] && ACCEL=(-enable-kvm -cpu host)

echo "[qemu-ublk-crash-e1] bootando (accel: ${ACCEL[*]})..."
timeout 240 qemu-system-x86_64 "${ACCEL[@]}" -m 256 -smp 2 -nographic -no-reboot \
  -kernel "$BZ" -initrd "$WORK/initramfs.gz" \
  -append "console=ttyS0 panic=1 rdinit=/init" > "$WORK/serial.log" 2>&1 || true

echo "=========== SERIAL COMPLETO (kernel + KTEST) ==========="
cat "$WORK/serial.log"
echo "=========== fim do serial ==========="

echo "=========== resumo KTEST ==========="
grep -E "KTEST-" "$WORK/serial.log" || echo "sem output KTEST — kernel pode nao ter bootado"
echo "====================================="

if grep -q "KTEST-E1-VERDICT=" "$WORK/serial.log"; then
  VERDICT="$(grep -oE 'KTEST-E1-VERDICT=[A-Z-]+' "$WORK/serial.log" | tail -1)"
  echo "QEMU-UBLK-CRASH-E1: EXPERIMENTO COMPLETO — $VERDICT"
  exit 0
else
  echo "QEMU-UBLK-CRASH-E1: INCONCLUSIVO — setup nao chegou ao veredito."
  exit 1
fi
