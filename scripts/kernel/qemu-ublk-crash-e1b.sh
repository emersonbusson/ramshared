#!/usr/bin/env bash
# qemu-ublk-crash-e1b.sh — Experiment E1b (E1 isolation control).
#
# CONTEXT: E1 (qemu-ublk-crash-e1.sh) found 2/3 kernel panics when the ublk daemon
# receives SIGKILL with the device set up as swap under pressure. BUT the E1 VM is minimalist:
# PID 1 (the busybox /init itself) competes for the SAME pressurized memory, so ANY
# anonymous shell page that falls into swap and gets lost triggers "Attempted to kill init!" —
# standard Linux behavior when PID 1 dies, NOT a special effect of ublk.
#
# ISOLATING QUESTION: when a COMMON process (non-PID-1, disposable) rereads a page
# whose swap device died, what happens to IT? Clean and contained SIGBUS (only it
# dies, rest of system continues) or is there a systemic effect (cascade / freeze)?
#
# DESIGN (isolates the victim from PID 1):
#   - Static C victim: mmaps its own PRIVATE ANONYMOUS region A, fills it, and calls
#     madvise(MADV_PAGEOUT) — pushes THOSE pages to /dev/ublkbN. Surgical target.
#   - EVICT swapcache of A: without this, rereading would be served from RAM (swap_cache_get_
#     folio, do_swap_page:3807) and would never touch the device. The victim creates its OWN pressure
#     (region B) until MemAvailable drops below a target -> the clean swapcache of A (the cheapest
#     reclaim target) is discarded -> A remains ONLY on the device. Moderate and
#     self-limiting pressure (stops at the target, not OOM), unlike E1 (300 MiB on a 256 MiB VM).
#   - PID 1 (/init) does NOT allocate anything risky; it stays warm in a loop (pages do not cool →
#     they do not become reclaim targets) and is protected from OOM (oom_score_adj=-1000). Rootfs and ramfs
#     are unevictable. The victim receives oom_score_adj=+1000: if OOM occurs, it dies (not
#     PID 1) and the experiment marks itself inconclusive — it never brings down init through OOM.
#   - A trivial BYSTANDER (busybox loop, heartbeat in /tmp) witnesses containment.
#   - Sequence: arm swap → victim pages out A and evicts swapcache → SIGKILL the daemon →
#     wait for the device to disappear → victim REREADS A (dead device) → device read fails →
#     Read-error on swap-device → SIGBUS in the VICTIM. Observes its exit status (42 = handler
#     caught SIGBUS; 0 = reread ok/NO-FAULT; 137 = OOM) and whether init+bystander survived.
#
# Does NOT run on host (real WSL2) — only in transient qemu, RAM-only, without -hda, same
# non-destructive pattern as qemu-ublk-daemon.sh (DT-29, .claude/rules/benchmarks.md:23). No sudo.
#
# usage: qemu-ublk-crash-e1b.sh [bzImage] [daemon_bin] [ublk_drv.ko]
# exit 0 = experiment produced a verdict; 1 = inconclusive (setup did not complete).
set -euo pipefail

BZ="${1:-$HOME/WSL2-Linux-Kernel/arch/x86/boot/bzImage}"
DAEMON="${2:-$(dirname "$0")/../../target/debug/ramsharedd}"
UBLK_KO="${3:-$HOME/WSL2-Linux-Kernel/drivers/block/ublk_drv.ko}"
# The victim runs inside a cgroup v2 with low memory.max. This forces reclaim to
# DISCARD the clean swapcache of A (otherwise the reread is served from RAM, never touching the
# dead device). The memcg reclaim is surgical: it squeezes ONLY the victim, while PID 1 remains untouched
# by design. (Global pressure in E1 leaked to PID 1 -> panic; here it does not.)
VICTIM_A_MB="${VICTIM_A_MB:-24}"          # anonymous canary (fits in 64 MiB of swap)
MEMCG_MAX_MB="${MEMCG_MAX_MB:-32}"        # victim cgroup limit (< A + B -> reclaim)
PRESS_CAP_MB="${PRESS_CAP_MB:-48}"        # region B: exceeds memory.max -> evicts swapcache
PRESS_TARGET_KB="${PRESS_TARGET_KB:-0}"   # 0 = ignores global MemAvailable (memcg handles it)
SWAP_THRESHOLD_KB="${SWAP_THRESHOLD_KB:-16384}"  # 16 MiB in swap = effective pageout

for f in "$BZ" "$DAEMON" "$UBLK_KO"; do
  [ -f "$f" ] || { echo "missing file: $f" >&2; exit 2; }
done
command -v qemu-system-x86_64 >/dev/null || { echo "qemu-system-x86_64 missing" >&2; exit 2; }
command -v gcc >/dev/null || { echo "gcc missing (needed to compile the static victim)" >&2; exit 2; }
[ -x /bin/busybox ] || { echo "busybox-static missing" >&2; exit 2; }

WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
IRD="$WORK/irfs"; mkdir -p "$IRD/bin" "$IRD/modules"

# --- C victim: mmap A -> MADV_PAGEOUT -> pressure B evicts swapcache -> waits for 'go' -> rereads A ---
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

/* KB for THIS process in swap (through /proc/self/smaps_rollup): per-process evidence. */
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

/* Async-signal-safe handler: proves SIGBUS reached THIS process and remained contained
 * (it decides to exit itself, code 42). Does NOT return (otherwise the faulting instruction refaults). */
static void on_sigbus(int sig) {
    (void)sig;
    static const char m[] = "VICTIM-CAUGHT-SIGBUS\n";
    write(1, m, sizeof(m) - 1);
    _exit(42);
}

static int exists(const char *p) { struct stat st; return stat(p, &st) == 0; }

int main(int argc, char **argv) {
    size_t a_mb      = (argc > 1) ? strtoul(argv[1], NULL, 10) : 48;
    long target_av   = (argc > 2) ? strtol(argv[2], NULL, 10) : 0;   /* KB, 0 = ignore */
    size_t cap_mb    = (argc > 3) ? strtoul(argv[3], NULL, 10) : 48;
    const char *cg   = (argc > 4) ? argv[4] : NULL;
    size_t alen = a_mb * 1024UL * 1024UL;

    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_handler = on_sigbus;
    sigaction(SIGBUS, &sa, NULL);

    /* Automatically joins cgroup v2 BEFORE allocating: A/B count against memory.max and
     * memcg reclaim evicts A's clean swapcache (without touching PID 1). */
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

    /* A = canary: region to reread after device death. */
    char *A = mmap(NULL, alen, PROT_READ | PROT_WRITE,
                   MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (A == MAP_FAILED) { perror("mmap A"); return 2; }
    for (size_t i = 0; i < alen; i += 4096)
        A[i] = (char)0x5a;
    printf("VICTIM-PID=%d\nVICTIM-A-MB=%zu\n", (int)getpid(), a_mb);
    fflush(stdout);

    /* Pushes A to swap (ublk device). */
    if (madvise(A, alen, MADV_PAGEOUT) != 0)
        perror("madvise(A,MADV_PAGEOUT)");
    usleep(300000);
    printf("VICTIM-A-SWAP-KB-AFTER-PAGEOUT=%ld\n", self_swap_kb());
    printf("VICTIM-MEMAVAIL-AFTER-PAGEOUT-KB=%ld\n", meminfo_kb("MemAvailable:"));
    fflush(stdout);

    /* B = pressure to EVICT A's clean swapcache. Grows until MemAvailable < target
     * (or cap). Without this, rereading A is served from RAM and never touches the device. */
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

    /* Waits for init to kill the daemon and confirm the device is dead (bounded 60s). */
    int w = 0;
    while (!exists("/tmp/victim-go")) {
        usleep(100000);
        if (++w > 600) { printf("VICTIM-GO-TIMEOUT\n"); fflush(stdout); return 3; }
    }

    /* Releases B: RAM is available, so rereading A is a CLEAN device read (without competing
     * with reclaim). A exists only on the dead device -> swap-in fails -> SIGBUS in this victim. */
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
# PID 1 must NEVER become an OOM victim (otherwise a panic masks the result).
echo -1000 > /proc/1/oom_score_adj 2>/dev/null
# high swappiness -> reclaim is more willing to scan anonymous LRU and discard swapcache.
echo 100 > /proc/sys/vm/swappiness 2>/dev/null
# cgroup v2 to squeeze ONLY the victim (low memory.max forces reclaim of A's swapcache,
# without touching PID 1). Mounted at /tmp/cg (ramfs) — /sys is read-only for mkdir.
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

# 1) ublk driver (identical to E1)
if $BB insmod /modules/ublk_drv.ko 2>/tmp/e; then
  echo "KTEST-INSMOD=ok"
else
  echo "KTEST-INSMOD=fail: $($BB cat /tmp/e)"
fi

# 2) RAM-backend daemon in the isolated generic-Linux QEMU guest, 64 MiB (identical to E1)
/ramsharedd --transport ublk --backend ram \
  --size 64 --queue-depth 4 --force >/tmp/daemon.log 2>&1 &
DPID=$!
echo "KTEST-DAEMON-PID=$DPID"

# 3) waits for /dev/ublkbN (bounded ~15s) (identical to E1)
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

# 4) arms it as swap (identical to E1)
if $BB mkswap "$DEV" >/tmp/mkswap.log 2>&1 && $BB swapon "$DEV" >/tmp/swapon.log 2>&1; then
  echo "KTEST-SWAPON=ok"
else
  echo "KTEST-SWAPON=fail: $($BB cat /tmp/mkswap.log /tmp/swapon.log 2>/dev/null)"
  echo "=====KTEST-E1B-END====="; $BB poweroff -f; exit 0
fi

# 5) BYSTANDER: containment witness (minimal resident memory, only /tmp=ramfs).
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

# 6) isolated VICTIM: allocates A, pushes it to swap, and creates its OWN pressure to evict
#    A's swapcache. oom_score_adj=+1000 -> if OOM occurs, it dies (never PID 1).
/victim "$VICTIM_A_MB" "$PRESS_TARGET_KB" "$PRESS_CAP_MB" "$CG" >/tmp/victim.log 2>&1 &
VPID=$!
echo "KTEST-VICTIM-PID=$VPID"
echo 1000 > /proc/$VPID/oom_score_adj 2>/dev/null

# 7) waits for the victim to signal that A is in swap and swapcache has been evicted (bounded ~60s)
READY=0
r=0
while [ $r -lt 600 ]; do
  [ -f /tmp/victim-ready ] && { READY=1; break; }
  $BB kill -0 "$VPID" 2>/dev/null || { echo "KTEST-VICTIM-DIED-BEFORE-READY=1"; break; }
  $BB sleep 0.1; r=$((r+1))
done
echo "KTEST-VICTIM-READY=$READY (iter=$r)"

# 8) confirms that swap has pages (Used >= threshold)
USED=$($BB awk 'NR==2{print $4+0}' /proc/swaps 2>/dev/null); [ -z "$USED" ] && USED=0
echo "KTEST-SWAP-USED-KB=$USED (threshold=${SWAP_THRESHOLD_KB})"
$BB cat /proc/swaps | while read -r l; do echo "KTEST-PROC-SWAPS: $l"; done
$BB cat /proc/meminfo | $BB grep -E "^(MemFree|MemAvailable|SwapFree|SwapTotal):" | while read -r l; do echo "KTEST-MEMINFO: $l"; done
HB_PRE=$($BB cat /tmp/hb 2>/dev/null); echo "KTEST-BYSTANDER-HB-PRE=$HB_PRE"

# 9) DECISIVE MOMENT part 1: SIGKILL the daemon (not SIGTERM).
T0_MS=$($BB awk '{print int($1*1000)}' /proc/uptime)
echo "KTEST-KILL-T0-MS=$T0_MS"
echo "KTEST-VICTIM-ALIVE-AT-KILL=$($BB kill -0 "$VPID" 2>/dev/null && echo 1 || echo 0)"
$BB kill -KILL "$DPID"
echo "KTEST-SIGKILL-SENT=1"

# 10) waits for the device to disappear on its own (kernel monitor_work), bounded ~40s
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

# 11) DECISIVE MOMENT part 2: tells the victim to REREAD (device already dead).
echo "KTEST-SENDING-GO=1"
$BB touch /tmp/victim-go

# 12) waits for the victim to finish and captures its exit status (bounded ~40s).
#     42 = handler caught SIGBUS (contained); 0 = reread ok (NO-FAULT);
#     137 = 128+9 SIGKILL/OOM; 135 = 128+7 SIGBUS without handler.
d=0
while [ $d -lt 400 ]; do
  $BB kill -0 "$VPID" 2>/dev/null || break
  $BB sleep 0.1; d=$((d+1))
done
wait "$VPID"; VST=$?
echo "KTEST-VICTIM-EXIT=$VST"

# 13) PROOF OF LIFE / CONTAINMENT: PID 1 reached this point = not in global D-state.
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

# 14) VERDICT
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

echo "[qemu-ublk-crash-e1b] booting (accel: ${ACCEL[*]}, A=${VICTIM_A_MB}MiB, press_target=${PRESS_TARGET_KB}KB)..."
timeout 240 qemu-system-x86_64 "${ACCEL[@]}" -m 256 -smp 2 -nographic -no-reboot \
  -kernel "$BZ" -initrd "$WORK/initramfs.gz" \
  -append "console=ttyS0 panic=1 rdinit=/init" > "$WORK/serial.log" 2>&1 || true

echo "=========== COMPLETE SERIAL LOG (kernel + KTEST) ==========="
cat "$WORK/serial.log"
echo "=========== end of serial log ==========="

echo "=========== critical serial-log signals ==========="
for pat in "Kernel panic" "Attempted to kill init" "hung_task" "Read-error on swap-device" "blocked for more than"; do
  c=$(grep -c "$pat" "$WORK/serial.log" 2>/dev/null || true); c=${c:-0}
  printf "  [%s] %s\n" "$c" "$pat"
done
echo "=========== KTEST summary ==========="
grep -E "KTEST-|VICTIM-" "$WORK/serial.log" || echo "no KTEST output — kernel may not have booted"
echo "====================================="

if grep -q "KTEST-E1B-VERDICT=" "$WORK/serial.log"; then
  VERDICT="$(grep -oE 'KTEST-E1B-VERDICT=[A-Z0-9-]+' "$WORK/serial.log" | tail -1)"
  echo "QEMU-UBLK-CRASH-E1B: EXPERIMENT COMPLETE — $VERDICT"
  exit 0
else
  echo "QEMU-UBLK-CRASH-E1B: INCONCLUSIVE — setup did not reach a verdict (init may have died)."
  exit 1
fi
