#!/usr/bin/env bash
# qemu-ublk-crash-e1.sh — Experimento E1 (decisao de arquitetura crash-safety do ublk,
# docs/ublk-teardown-crash-safety/). Testa o cenario que o smoke original
# (qemu-ublk-daemon.sh) NAO cobre: device armado como SWAP sob pressao real de memoria,
# daemon morto por SIGKILL (nao SIGTERM) com I/O possivelmente em voo.
#
# Pergunta que decide o ramo (ver docs/ublk-teardown-crash-safety/PRD.md):
#   Ramo A: /dev/ublkbN some em <=~10s, VM responsiva -> o `ublk_daemon_monitor_work`
#           do kernel (poll 5s, ublk_drv.c:1486-1516) e suficiente; risco residual e so
#           a janela + perda das paginas em voo (SIGBUS nos processos donos).
#   Ramo B: device persiste / VM trava >60s / hung_task no dmesg -> reproduz isolado o
#           freeze historico do WSL2 (MEMORY.md:883-901); reaper userspace NAO ajudaria
#           (DEL_DEV passa pelo mesmo del_gendisk que o monitor ja aciona sozinho).
#
# v2: corrige 2 problemas da 1a rodada (n=1, inconclusiva por regra propria do projeto,
# .claude/rules/benchmarks.md "1 amostra mente"): (a) `dmesg` do busybox voltou vazio —
# trocado por imprimir o serial.log CRU e completo (kernel manda printk pro ttyS0
# diretamente, entao ja esta la sem depender do applet); (b) o gatilho "Used>0" pegava
# um blip transitorio (1 pagina) — agora exige um piso mais alto (KB_THRESHOLD) e
# reamostra antes de decidir, alem de logar se o `dd` de pressao ainda esta vivo no
# instante do kill (evidencia de I/O em voo).
#
# Nao roda no host (WSL2 real) de jeito nenhum — so dentro do qemu efemero, RAM-only,
# sem -hda, mesmo padrao nao-destrutivo do qemu-ublk-daemon.sh (DT-29,
# .claude/rules/benchmarks.md:23). Sem sudo: qemu roda como usuario normal (TCG se
# /dev/kvm nao for gravavel).
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

# 5) pressao real de memoria via tmpfs (RAM da VM e pequena de proposito: -m 256).
#    tmpfs e swappable; sob pressao o kernel empurra paginas p/ o /dev/ublkbN. Escreve
#    em pedacos de 1 MiB, MEDINDO /proc/swaps a cada pedaco (sem depender so de um
#    watcher externo) p/ garantir que a pressao e continua, nao um blip.
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

# 6) espera o swap ATIVAR de forma NAO-trivial (Used >= 4096 KB, nao so >0 -- evita
#    contar 1 pagina isolada como "ativado"), bounded ~40s. So entao matamos o daemon.
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

# 7) O MOMENTO DECISIVO: SIGKILL (nao SIGTERM) no daemon, com o `dd` de pressao ainda
#    rodando (checado acima) -- maior chance real de I/O de swap em voo no ublk.
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

# 9) prova de vida: se este shell chegou ate aqui e roda mais comandos, a VM nao esta
#    travada em D-state global (um freeze real pararia o script tambem, e o `timeout`
#    externo do host e quem detectaria isso).
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
