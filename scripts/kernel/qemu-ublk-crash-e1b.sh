#!/usr/bin/env bash
# qemu-ublk-crash-e1b.sh — Experimento E1b (controle de isolamento do E1).
#
# CONTEXTO: o E1 (qemu-ublk-crash-e1.sh) achou 2/3 kernel panics quando o daemon ublk
# leva SIGKILL com o device armado como swap sob pressao. MAS a VM do E1 e minimalista:
# o PID 1 (o proprio /init busybox) disputa a MESMA memoria pressionada, entao QUALQUER
# pagina anonima do shell que caia no swap e se perca vira "Attempted to kill init!" —
# comportamento PADRAO do Linux p/ morte do PID 1, NAO um efeito especial do ublk.
#
# PERGUNTA QUE ISOLA: quando um processo COMUM (nao-PID-1, descartavel) rele uma pagina
# cujo device de swap morreu, o que acontece com ELE? SIGBUS limpo e contido (so ele
# morre, resto do sistema segue) ou ha efeito sistemico (cascata / freeze)?
#
# DESENHO (isola a vitima do PID 1):
#   - Vitima em C estatico: mmapeia regiao A ANONIMA PRIVADA propria, preenche, e chama
#     madvise(MADV_PAGEOUT) — empurra ESSAS paginas p/ o /dev/ublkbN. Alvo cirurgico.
#   - EXPULSA o swapcache de A: sem isso a releitura seria servida da RAM (swap_cache_get_
#     folio, do_swap_page:3807) e nunca tocaria o device. A vitima cria pressao PROPRIA
#     (regiao B) ate MemAvailable cair abaixo de um alvo → o swapcache limpo de A (o alvo
#     de reclaim mais barato) e descartado → A fica SO no device. Pressao moderada e
#     auto-limitada (para no alvo, nao no OOM), ao contrario do E1 (300 MiB num VM de 256).
#   - PID 1 (/init) NAO aloca nada arriscado; fica quente em loop (paginas nao esfriam →
#     nao viram alvo de reclaim) e protegido de OOM (oom_score_adj=-1000). Rootfs e ramfs
#     (unevictable). A vitima recebe oom_score_adj=+1000: se houver OOM, ela morre (nao o
#     PID 1) e o experimento marca inconclusivo — nunca derruba o init por OOM.
#   - BYSTANDER trivial (loop busybox, heartbeat em /tmp) testemunha o containment.
#   - Sequencia: arma swap → vitima paga-out A + expulsa swapcache → SIGKILL no daemon →
#     espera o device sumir → vitima RELE A (device morto) → device-read falha →
#     Read-error on swap-device → SIGBUS na VITIMA. Observa exit status dela (42 = handler
#     pegou SIGBUS; 0 = releu ok/NO-FAULT; 137 = OOM) e se init+bystander sobreviveram.
#
# NAO roda no host (WSL2 real) — so no qemu efemero, RAM-only, sem -hda, mesmo padrao
# nao-destrutivo do qemu-ublk-daemon.sh (DT-29, .claude/rules/benchmarks.md:23). Sem sudo.
#
# uso: qemu-ublk-crash-e1b.sh [bzImage] [daemon_bin] [ublk_drv.ko]
# saida 0 = experimento produziu veredito; 1 = inconclusivo (setup nao completou).
set -euo pipefail

BZ="${1:-/home/emdev/WSL2-Linux-Kernel/arch/x86/boot/bzImage}"
DAEMON="${2:-$(dirname "$0")/../../target/debug/ramsharedd}"
UBLK_KO="${3:-/home/emdev/WSL2-Linux-Kernel/drivers/block/ublk_drv.ko}"
# A vitima roda dentro de um cgroup v2 com memory.max baixo. Isso forca o reclaim a
# DESCARTAR o swapcache limpo de A (senao a releitura e servida da RAM, nunca tocando o
# device morto). O reclaim de memcg e cirurgico: espreme SO a vitima, o PID 1 fica intacto
# por construcao. (Pressao global do E1 vazava p/ o PID 1 -> panic; aqui nao.)
VICTIM_A_MB="${VICTIM_A_MB:-24}"          # canary anon (cabe nos 64 MiB do swap)
MEMCG_MAX_MB="${MEMCG_MAX_MB:-32}"        # limite do cgroup da vitima (< A + B -> reclaim)
PRESS_CAP_MB="${PRESS_CAP_MB:-48}"        # regiao B: excede memory.max -> expulsa swapcache
PRESS_TARGET_KB="${PRESS_TARGET_KB:-0}"   # 0 = ignora MemAvailable global (memcg cuida)
SWAP_THRESHOLD_KB="${SWAP_THRESHOLD_KB:-16384}"  # 16 MiB no swap = pageout efetivo

for f in "$BZ" "$DAEMON" "$UBLK_KO"; do
  [ -f "$f" ] || { echo "arquivo inexistente: $f" >&2; exit 2; }
done
command -v qemu-system-x86_64 >/dev/null || { echo "qemu-system-x86_64 ausente" >&2; exit 2; }
command -v gcc >/dev/null || { echo "gcc ausente (preciso p/ compilar a vitima estatica)" >&2; exit 2; }
[ -x /bin/busybox ] || { echo "busybox-static ausente" >&2; exit 2; }

WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
IRD="$WORK/irfs"; mkdir -p "$IRD/bin" "$IRD/modules"

# --- vitima em C: mmap A -> MADV_PAGEOUT -> pressao B expulsa swapcache -> espera 'go' -> rele A ---
cat > "$WORK/victim.c" <<'VICTIMC'
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <signal.h>
#include <sys/mman.h>
#include <sys/stat.h>

#ifndef MADV_PAGEOUT
#define MADV_PAGEOUT 21
#endif

static long meminfo_kb(const char *key) {
    int fd = open("/proc/meminfo", O_RDONLY);
    if (fd < 0) return -1;
    char buf[4096];
    ssize_t n = read(fd, buf, sizeof buf - 1);
    close(fd);
    if (n <= 0) return -1;
    buf[n] = 0;
    char *p = strstr(buf, key);
    if (!p) return -1;
    return strtol(p + strlen(key), NULL, 10);
}

/* KB DESTE processo no swap (via /proc/self/smaps_rollup): evidencia por-processo. */
static long self_swap_kb(void) {
    int fd = open("/proc/self/smaps_rollup", O_RDONLY);
    if (fd < 0) return -1;
    char buf[8192];
    ssize_t n = read(fd, buf, sizeof buf - 1);
    close(fd);
    if (n <= 0) return -1;
    buf[n] = 0;
    char *p = strstr(buf, "Swap:");
    if (!p) return -1;
    return strtol(p + 5, NULL, 10);
}

/* Handler async-signal-safe: prova que o SIGBUS chegou a ESTE processo e ficou contido
 * (ele mesmo decide sair, codigo 42). NAO retorna (senao a instrucao faltosa re-fauta). */
static void on_sigbus(int sig) {
    (void)sig;
    static const char m[] = "VICTIM-CAUGHT-SIGBUS\n";
    write(1, m, sizeof(m) - 1);
    _exit(42);
}

static int exists(const char *p) { struct stat st; return stat(p, &st) == 0; }

int main(int argc, char **argv) {
    size_t a_mb      = (argc > 1) ? strtoul(argv[1], NULL, 10) : 48;
    long target_av   = (argc > 2) ? strtol(argv[2], NULL, 10) : 0;   /* KB, 0 = ignora */
    size_t cap_mb    = (argc > 3) ? strtoul(argv[3], NULL, 10) : 48;
    const char *cg   = (argc > 4) ? argv[4] : NULL;
    size_t alen = a_mb * 1024UL * 1024UL;

    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_handler = on_sigbus;
    sigaction(SIGBUS, &sa, NULL);

    /* Auto-entra no cgroup v2 ANTES de alocar: assim A/B contam contra memory.max e o
     * reclaim de memcg expulsa o swapcache limpo de A (sem tocar o PID 1). */
    if (cg && cg[0]) {
        char path[512];
        snprintf(path, sizeof path, "%s/cgroup.procs", cg);
        int cfd = open(path, O_WRONLY);
        if (cfd >= 0) {
            char pid[32];
            int len = snprintf(pid, sizeof pid, "%d\n", (int)getpid());
            ssize_t wr = write(cfd, pid, len);
            close(cfd);
            printf("VICTIM-CGROUP-JOIN=%s\n", wr > 0 ? "ok" : "fail");
        } else {
            printf("VICTIM-CGROUP-JOIN=open-fail\n");
        }
        fflush(stdout);
    }

    /* A = canary: regiao que sera relida apos a morte do device. */
    char *A = mmap(NULL, alen, PROT_READ | PROT_WRITE,
                   MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (A == MAP_FAILED) { perror("mmap A"); return 2; }
    for (size_t i = 0; i < alen; i += 4096)
        A[i] = (char)0x5a;
    printf("VICTIM-PID=%d\nVICTIM-A-MB=%zu\n", (int)getpid(), a_mb);
    fflush(stdout);

    /* Empurra A p/ o swap (device ublk). */
    if (madvise(A, alen, MADV_PAGEOUT) != 0)
        perror("madvise(A,MADV_PAGEOUT)");
    usleep(300000);
    printf("VICTIM-A-SWAP-KB-AFTER-PAGEOUT=%ld\n", self_swap_kb());
    printf("VICTIM-MEMAVAIL-AFTER-PAGEOUT-KB=%ld\n", meminfo_kb("MemAvailable:"));
    fflush(stdout);

    /* B = pressao p/ EXPULSAR o swapcache limpo de A. Cresce ate MemAvailable < target
     * (ou cap). Sem isto, a releitura de A e servida da RAM e nunca toca o device. */
    size_t cap = cap_mb * 1024UL * 1024UL;
    char *B = mmap(NULL, cap, PROT_READ | PROT_WRITE,
                   MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    size_t touched = 0;
    if (B == MAP_FAILED) {
        perror("mmap B");
    } else {
        while (touched < cap) {
            size_t step_end = touched + (8UL << 20);
            if (step_end > cap) step_end = cap;
            for (; touched < step_end; touched += 4096)
                B[touched] = (char)0xa5;
            long av = meminfo_kb("MemAvailable:");
            if (av >= 0 && av < target_av) break;
        }
    }
    printf("VICTIM-B-TOUCHED-MB=%zu\n", touched >> 20);
    printf("VICTIM-A-SWAP-KB-AFTER-PRESSURE=%ld\n", self_swap_kb());
    printf("VICTIM-MEMAVAIL-AFTER-PRESSURE-KB=%ld\n", meminfo_kb("MemAvailable:"));
    fflush(stdout);

    int fd = open("/tmp/victim-ready", O_CREAT | O_WRONLY | O_TRUNC, 0644);
    if (fd >= 0) close(fd);
    printf("VICTIM-READY\n");
    fflush(stdout);

    /* Espera o init matar o daemon + confirmar o device morto (bounded 60s). */
    int w = 0;
    while (!exists("/tmp/victim-go")) {
        usleep(100000);
        if (++w > 600) { printf("VICTIM-GO-TIMEOUT\n"); fflush(stdout); return 3; }
    }

    /* Libera B: RAM sobra, entao a releitura de A e um device-read LIMPO (sem competir
     * com reclaim). A esta so no device morto -> swap-in falha -> SIGBUS nesta vitima. */
    if (B != MAP_FAILED) munmap(B, cap);

    printf("VICTIM-REREAD-START\n");
    fflush(stdout);
    volatile unsigned char *vp = (volatile unsigned char *)A;
    unsigned long sum = 0;
    for (size_t i = 0; i < alen; i += 4096)
        sum += vp[i];
    printf("VICTIM-REREAD-OK sum=%lu\n", sum);
    printf("VICTIM-A-SWAP-KB-AFTER-REREAD=%ld\n", self_swap_kb());
    fflush(stdout);
    return 0;
}
VICTIMC
gcc -static -O2 -o "$WORK/victim" "$WORK/victim.c"

cp /bin/busybox "$IRD/bin/busybox"
cp "$DAEMON" "$IRD/ramsharedd"
cp "$UBLK_KO" "$IRD/modules/ublk_drv.ko"
cp "$WORK/victim" "$IRD/victim"

for lib in $(ldd "$DAEMON" | grep -oE '/[^ ]+\.so[^ ]*'); do
  mkdir -p "$IRD$(dirname "$lib")"
  cp "$lib" "$IRD$lib"
done

cat > "$IRD/init" <<INIT
#!/bin/busybox sh
BB=/bin/busybox
VICTIM_A_MB=${VICTIM_A_MB}
PRESS_TARGET_KB=${PRESS_TARGET_KB}
PRESS_CAP_MB=${PRESS_CAP_MB}
MEMCG_MAX_MB=${MEMCG_MAX_MB}
SWAP_THRESHOLD_KB=${SWAP_THRESHOLD_KB}
INIT
cat >> "$IRD/init" <<'INIT'
$BB mkdir -p /proc /sys /dev /tmp
$BB mount -t proc proc /proc
$BB mount -t sysfs sysfs /sys
$BB mount -t devtmpfs devtmpfs /dev 2>/dev/null
# PID 1 NUNCA deve ser vitima de OOM (senao panic mascara o resultado).
echo -1000 > /proc/1/oom_score_adj 2>/dev/null
# swappiness alto -> reclaim mais disposto a scanear a LRU anon e descartar o swapcache.
echo 100 > /proc/sys/vm/swappiness 2>/dev/null
# cgroup v2 p/ espremer SO a vitima (memory.max baixo forca o reclaim do swapcache de A,
# sem tocar o PID 1). Montado em /tmp/cg (ramfs) — /sys e read-only p/ mkdir.
CG=""
$BB mkdir -p /tmp/cg
if $BB mount -t cgroup2 none /tmp/cg 2>/tmp/cgerr; then
  echo "+memory" > /tmp/cg/cgroup.subtree_control 2>/dev/null
  $BB mkdir -p /tmp/cg/victim
  MEMCG_MAX_BYTES=$((MEMCG_MAX_MB * 1024 * 1024))
  echo "$MEMCG_MAX_BYTES" > /tmp/cg/victim/memory.max 2>/dev/null
  echo "max" > /tmp/cg/victim/memory.swap.max 2>/dev/null
  CG="/tmp/cg/victim"
  echo "KTEST-CGROUP=ok (memory.max=${MEMCG_MAX_MB}MiB)"
else
  echo "KTEST-CGROUP=fail: $($BB cat /tmp/cgerr 2>/dev/null)"
fi
echo "=====KTEST-E1B-BEGIN====="
echo "KTEST-UNAME=$($BB uname -r)"

# 1) driver ublk (identico ao E1)
if $BB insmod /modules/ublk_drv.ko 2>/tmp/e; then
  echo "KTEST-INSMOD=ok"
else
  echo "KTEST-INSMOD=fail: $($BB cat /tmp/e)"
fi

# 2) daemon backend RAM, device 64 MiB (identico ao E1)
RAMSHARED_ALLOW_UBLK_ON_WSL2=1 /ramsharedd --transport ublk --backend ram \
  --size 64 --queue-depth 4 --force >/tmp/daemon.log 2>&1 &
DPID=$!
echo "KTEST-DAEMON-PID=$DPID"

# 3) espera /dev/ublkbN (bounded ~15s) (identico ao E1)
DEV=""
i=0
while [ $i -lt 150 ]; do
  for n in /dev/ublkb0 /dev/ublkb1; do [ -b "$n" ] && DEV="$n"; done
  [ -n "$DEV" ] && break
  $BB kill -0 "$DPID" 2>/dev/null || { echo "KTEST-DAEMON-DIED-EARLY=1"; break; }
  $BB sleep 0.1; i=$((i+1))
done
[ -n "$DEV" ] || { echo "KTEST-DEVICE=absent"; echo "=====KTEST-E1B-END====="; $BB poweroff -f; exit 0; }
echo "KTEST-DEVICE=$DEV"

# 4) arma como swap (identico ao E1)
if $BB mkswap "$DEV" >/tmp/mkswap.log 2>&1 && $BB swapon "$DEV" >/tmp/swapon.log 2>&1; then
  echo "KTEST-SWAPON=ok"
else
  echo "KTEST-SWAPON=fail: $($BB cat /tmp/mkswap.log /tmp/swapon.log 2>/dev/null)"
  echo "=====KTEST-E1B-END====="; $BB poweroff -f; exit 0
fi

# 5) BYSTANDER: testemunha de containment (memoria residente minima, so /tmp=ramfs).
(
  n=0
  while [ $n -lt 100000 ]; do
    echo "$n" > /tmp/hb
    n=$((n+1))
    $BB sleep 0.2
  done
) &
BPID=$!
echo "KTEST-BYSTANDER-PID=$BPID"
echo -500 > /proc/$BPID/oom_score_adj 2>/dev/null

# 6) VITIMA isolada: aloca A, empurra p/ swap, e cria pressao PROPRIA p/ expulsar o
#    swapcache de A. oom_score_adj=+1000 -> se houver OOM, ela morre (nunca o PID 1).
/victim "$VICTIM_A_MB" "$PRESS_TARGET_KB" "$PRESS_CAP_MB" "$CG" >/tmp/victim.log 2>&1 &
VPID=$!
echo "KTEST-VICTIM-PID=$VPID"
echo 1000 > /proc/$VPID/oom_score_adj 2>/dev/null

# 7) espera a vitima sinalizar que A esta no swap e o swapcache foi expulso (bounded ~60s)
READY=0
r=0
while [ $r -lt 600 ]; do
  [ -f /tmp/victim-ready ] && { READY=1; break; }
  $BB kill -0 "$VPID" 2>/dev/null || { echo "KTEST-VICTIM-DIED-BEFORE-READY=1"; break; }
  $BB sleep 0.1; r=$((r+1))
done
echo "KTEST-VICTIM-READY=$READY (iter=$r)"

# 8) confirma que o swap tem paginas (Used >= threshold)
USED=$($BB awk 'NR==2{print $4+0}' /proc/swaps 2>/dev/null); [ -z "$USED" ] && USED=0
echo "KTEST-SWAP-USED-KB=$USED (threshold=${SWAP_THRESHOLD_KB})"
$BB cat /proc/swaps | while read -r l; do echo "KTEST-PROC-SWAPS: $l"; done
$BB cat /proc/meminfo | $BB grep -E "^(MemFree|MemAvailable|SwapFree|SwapTotal):" | while read -r l; do echo "KTEST-MEMINFO: $l"; done
HB_PRE=$($BB cat /tmp/hb 2>/dev/null); echo "KTEST-BYSTANDER-HB-PRE=$HB_PRE"

# 9) MOMENTO DECISIVO parte 1: SIGKILL no daemon (nao SIGTERM).
T0_MS=$($BB awk '{print int($1*1000)}' /proc/uptime)
echo "KTEST-KILL-T0-MS=$T0_MS"
echo "KTEST-VICTIM-ALIVE-AT-KILL=$($BB kill -0 "$VPID" 2>/dev/null && echo 1 || echo 0)"
$BB kill -KILL "$DPID"
echo "KTEST-SIGKILL-SENT=1"

# 10) espera o device sumir sozinho (monitor_work do kernel), bounded ~40s
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

# 11) MOMENTO DECISIVO parte 2: manda a vitima RELER (device ja morto).
echo "KTEST-SENDING-GO=1"
$BB touch /tmp/victim-go

# 12) espera a vitima terminar e captura o exit status (bounded ~40s).
#     42 = handler pegou SIGBUS (contido); 0 = releu ok (NO-FAULT);
#     137 = 128+9 SIGKILL/OOM; 135 = 128+7 SIGBUS sem handler.
d=0
while [ $d -lt 400 ]; do
  $BB kill -0 "$VPID" 2>/dev/null || break
  $BB sleep 0.1; d=$((d+1))
done
wait "$VPID"; VST=$?
echo "KTEST-VICTIM-EXIT=$VST"

# 13) PROVA DE VIDA / CONTAINMENT: o PID 1 chegou aqui = nao esta em D-state global.
echo "KTEST-VM-RESPONSIVE-AFTER-KILL=1"
echo "KTEST-PID1-ALIVE=1"
$BB sleep 0.5
HB_POST=$($BB cat /tmp/hb 2>/dev/null); echo "KTEST-BYSTANDER-HB-POST=$HB_POST"
BYS_ALIVE=$($BB kill -0 "$BPID" 2>/dev/null && echo 1 || echo 0)
echo "KTEST-BYSTANDER-ALIVE=$BYS_ALIVE"
if [ -n "$HB_PRE" ] && [ -n "$HB_POST" ] && [ "$HB_POST" -gt "$HB_PRE" ] 2>/dev/null; then
  echo "KTEST-BYSTANDER-PROGRESSED=1"
else
  echo "KTEST-BYSTANDER-PROGRESSED=0"
fi
$BB cat /proc/swaps | while read -r l; do echo "KTEST-PROC-SWAPS-POST: $l"; done
$BB cat /proc/loadavg | while read -r l; do echo "KTEST-LOADAVG-POST: $l"; done

echo "KTEST-VICTIM-LOG:"
$BB cat /tmp/victim.log 2>/dev/null | while read -r l; do echo "  $l"; done

# 14) VEREDITO
if [ "$VST" = "42" ] || [ "$VST" = "135" ]; then
  if [ "$BYS_ALIVE" = "1" ]; then
    echo "KTEST-E1B-VERDICT=CONTAINED-SIGBUS"
  else
    echo "KTEST-E1B-VERDICT=SIGBUS-BUT-BYSTANDER-DIED"
  fi
elif [ "$VST" = "0" ]; then
  echo "KTEST-E1B-VERDICT=NO-FAULT-PAGES-SURVIVED"
elif [ "$VST" = "137" ]; then
  echo "KTEST-E1B-VERDICT=VICTIM-OOM-KILLED-INCONCLUSIVE"
else
  echo "KTEST-E1B-VERDICT=UNEXPECTED-EXIT-$VST"
fi

echo "KTEST-DAEMON-LOG:"
$BB tail -6 /tmp/daemon.log 2>/dev/null
echo "=====KTEST-E1B-END====="
$BB poweroff -f
INIT
chmod +x "$IRD/init"

( cd "$IRD" && find . | cpio -o -H newc 2>/dev/null | gzip ) > "$WORK/initramfs.gz"

ACCEL=(-machine accel=tcg)
[ -w /dev/kvm ] && ACCEL=(-enable-kvm -cpu host)

echo "[qemu-ublk-crash-e1b] bootando (accel: ${ACCEL[*]}, A=${VICTIM_A_MB}MiB, press_target=${PRESS_TARGET_KB}KB)..."
timeout 240 qemu-system-x86_64 "${ACCEL[@]}" -m 256 -smp 2 -nographic -no-reboot \
  -kernel "$BZ" -initrd "$WORK/initramfs.gz" \
  -append "console=ttyS0 panic=1 rdinit=/init" > "$WORK/serial.log" 2>&1 || true

echo "=========== SERIAL COMPLETO (kernel + KTEST) ==========="
cat "$WORK/serial.log"
echo "=========== fim do serial ==========="

echo "=========== sinais criticos no serial ==========="
for pat in "Kernel panic" "Attempted to kill init" "hung_task" "Read-error on swap-device" "blocked for more than"; do
  c=$(grep -c "$pat" "$WORK/serial.log" 2>/dev/null || true); c=${c:-0}
  printf "  [%s] %s\n" "$c" "$pat"
done
echo "=========== resumo KTEST ==========="
grep -E "KTEST-|VICTIM-" "$WORK/serial.log" || echo "sem output KTEST — kernel pode nao ter bootado"
echo "====================================="

if grep -q "KTEST-E1B-VERDICT=" "$WORK/serial.log"; then
  VERDICT="$(grep -oE 'KTEST-E1B-VERDICT=[A-Z0-9-]+' "$WORK/serial.log" | tail -1)"
  echo "QEMU-UBLK-CRASH-E1B: EXPERIMENTO COMPLETO — $VERDICT"
  exit 0
else
  echo "QEMU-UBLK-CRASH-E1B: INCONCLUSIVO — setup nao chegou ao veredito (init pode ter morrido)."
  exit 1
fi
